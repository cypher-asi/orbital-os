//! Init Process (PID 1) for Zero OS
//!
//! The init process is the first user-space process spawned by the kernel.
//! In the refactored architecture, init has a minimal role:
//!
//! - **Bootstrap**: Spawn PermissionService (PID 2) and initial apps
//! - **Service Registry**: Maintain name → endpoint mapping for service discovery
//! - **Idle**: After bootstrap, enter minimal loop
//!
//! Permission management has been delegated to PermissionService (PID 2).
//!
//! # Service Protocol
//!
//! Services communicate with init using IPC messages:
//!
//! - `MSG_REGISTER_SERVICE (0x1000)`: Register a service name with an endpoint
//! - `MSG_LOOKUP_SERVICE (0x1001)`: Look up a service by name
//! - `MSG_LOOKUP_RESPONSE (0x1002)`: Response to a lookup request
//! - `MSG_SPAWN_SERVICE (0x1003)`: Request init to spawn a new service

#![cfg_attr(target_arch = "wasm32", no_std)]

// Initialize bump allocator with 1MB heap
zos_allocator::init!(1024 * 1024);

#[cfg(target_arch = "wasm32")]
extern crate alloc;

#[cfg(target_arch = "wasm32")]
use alloc::collections::BTreeMap;
#[cfg(target_arch = "wasm32")]
use alloc::format;
#[cfg(target_arch = "wasm32")]
use alloc::string::String;

#[cfg(not(target_arch = "wasm32"))]
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::format;
#[cfg(not(target_arch = "wasm32"))]
use std::string::String;

use zos_process::{self as syscall};

// =============================================================================
// Module Organization
// =============================================================================

mod bootstrap;
mod handlers;
mod registry;

// =============================================================================
// Service Protocol Constants
// =============================================================================
// All constants are re-exported from zos-ipc via zos-process for consistency.

pub use zos_process::{
    MSG_LOOKUP_RESPONSE, MSG_LOOKUP_SERVICE, MSG_REGISTER_SERVICE, MSG_SERVICE_READY,
    MSG_SPAWN_RESPONSE, MSG_SPAWN_SERVICE, MSG_SUPERVISOR_CONSOLE_INPUT,
    MSG_SUPERVISOR_IPC_DELIVERY, MSG_SUPERVISOR_KILL_PROCESS,
};

// Additional Init-specific constants from zos-ipc
pub use zos_process::init::{MSG_SERVICE_CAP_GRANTED, MSG_VFS_RESPONSE_CAP_GRANTED};

// Spawn protocol messages for Init-driven spawn
pub use zos_process::supervisor::{
    MSG_SUPERVISOR_CAP_RESPONSE, MSG_SUPERVISOR_CREATE_ENDPOINT, MSG_SUPERVISOR_ENDPOINT_RESPONSE,
    MSG_SUPERVISOR_GRANT_CAP, MSG_SUPERVISOR_SPAWN_PROCESS, MSG_SUPERVISOR_SPAWN_RESPONSE,
};

// =============================================================================
// Well-known Capability Slots
// =============================================================================

/// Init's main endpoint for receiving service messages (slot 0)
const INIT_ENDPOINT_SLOT: u32 = 0;

// Note: Console output now uses SYS_CONSOLE_WRITE syscall (no slot needed)

// =============================================================================
// Service Registry
// =============================================================================

/// Service registration info
#[derive(Clone, Debug)]
pub struct ServiceInfo {
    /// Process ID of the service
    pub pid: u32,
    /// Endpoint ID for communicating with the service
    pub endpoint_id: u64,
    /// Whether the service has signaled it's ready
    pub ready: bool,
}


/// Init process state
pub struct Init {
    /// Service registry: name → info
    pub services: BTreeMap<String, ServiceInfo>,
    /// Service input capability slots: service_pid → capability slot in Init's CSpace
    /// Used for delivering IPC messages to services' input endpoint (slot 1)
    pub service_cap_slots: BTreeMap<u32, u32>,
    /// Service VFS response capability slots: service_pid → capability slot in Init's CSpace
    /// Used for delivering VFS responses to services' VFS response endpoint (slot 4)
    pub service_vfs_slots: BTreeMap<u32, u32>,
    /// Our endpoint slot for receiving messages
    pub endpoint_slot: u32,
    /// Boot sequence complete
    pub boot_complete: bool,
}

impl Init {
    fn new() -> Self {
        Self {
            services: BTreeMap::new(),
            service_cap_slots: BTreeMap::new(),
            service_vfs_slots: BTreeMap::new(),
            endpoint_slot: INIT_ENDPOINT_SLOT,
            boot_complete: false,
        }
    }

    /// Print to console via SYS_CONSOLE_WRITE syscall
    pub fn log(&self, msg: &str) {
        syscall::console_write(&format!("[init] {}\n", msg));
    }

    /// Run the init process
    fn run(&mut self) {
        self.log("Zero OS Init Process starting (PID 1)");
        self.log("Service registry initialized");

        // Boot sequence: spawn core services
        self.boot_sequence();

        self.log("Entering idle loop...");

        // Minimal loop: handle service messages
        loop {
            match syscall::receive(self.endpoint_slot) {
                Ok(msg) => {
                    self.log(&format!("AGENT_LOG:receive_returned_message:tag=0x{:x}:from_pid={}:len={}", msg.tag, msg.from_pid, msg.data.len()));
                    self.handle_message(&msg);
                }
                Err(syscall::RecvError::NoMessage) => {
                    // No message available - this is normal
                }
                Err(e) => {
                    self.log(&format!("AGENT_LOG:receive_error:{:?}", e));
                }
            }
            syscall::yield_now();
        }
    }

    /// Handle an incoming IPC message
    fn handle_message(&mut self, msg: &syscall::ReceivedMessage) {
        self.log(&format!(
            "AGENT_LOG:handle_message:tag=0x{:x}:from_pid={}:len={}",
            msg.tag, msg.from_pid, msg.data.len()
        ));
        
        match msg.tag {
            // Service registry protocol
            MSG_REGISTER_SERVICE => self.handle_register(msg),
            MSG_LOOKUP_SERVICE => self.handle_lookup(msg),
            MSG_SERVICE_READY => self.handle_ready(msg),
            MSG_SPAWN_SERVICE => self.handle_spawn_request(msg),

            // Supervisor → Init protocol
            MSG_SUPERVISOR_CONSOLE_INPUT => self.handle_supervisor_console_input(msg),
            MSG_SUPERVISOR_KILL_PROCESS => self.handle_supervisor_kill_process(msg),
            MSG_SUPERVISOR_IPC_DELIVERY => {
                self.log(&format!("AGENT_LOG:dispatching_to_ipc_delivery_handler:tag=0x{:x}", msg.tag));
                self.handle_supervisor_ipc_delivery(msg);
            }
            MSG_SERVICE_CAP_GRANTED => {
                self.log("AGENT_LOG:dispatching_to_cap_granted_handler");
                self.handle_service_cap_granted(msg);
            }
            MSG_VFS_RESPONSE_CAP_GRANTED => self.handle_vfs_response_cap_granted(msg),

            // Init-driven spawn protocol (supervisor → Init)
            MSG_SUPERVISOR_SPAWN_PROCESS => self.handle_supervisor_spawn_process(msg),
            MSG_SUPERVISOR_CREATE_ENDPOINT => self.handle_supervisor_create_endpoint(msg),
            MSG_SUPERVISOR_GRANT_CAP => self.handle_supervisor_grant_cap(msg),

            _ => {
                self.log(&format!(
                    "Unknown message tag: 0x{:x} from PID {}",
                    msg.tag, msg.from_pid
                ));
            }
        }
    }
}

// =============================================================================
// WASM Entry Point
// =============================================================================

/// Process entry point - called by the Web Worker
#[no_mangle]
pub extern "C" fn _start() {
    let mut init = Init::new();
    init.run();
}

// =============================================================================
// Panic Handler (required for no_std on WASM)
// =============================================================================

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let msg = format!("init PANIC: {}", info.message());
    syscall::debug(&msg);
    syscall::exit(1);
}

