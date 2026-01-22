//! Init Process (PID 1) for Orbital OS
//!
//! The init process is the first user-space process spawned by the kernel.
//! In the refactored architecture, init has a minimal role:
//!
//! - **Bootstrap**: Spawn PermissionManager (PID 2) and initial apps
//! - **Service Registry**: Maintain name → endpoint mapping for service discovery
//! - **Idle**: After bootstrap, enter minimal loop
//!
//! Permission management has been delegated to PermissionManager (PID 2).
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

use orbital_process::{self as syscall};

// =============================================================================
// Service Protocol Constants
// =============================================================================

/// Register a service: data = [name_len: u8, name: [u8], endpoint_id_low: u32, endpoint_id_high: u32]
pub const MSG_REGISTER_SERVICE: u32 = 0x1000;

/// Lookup a service: data = [name_len: u8, name: [u8]]
pub const MSG_LOOKUP_SERVICE: u32 = 0x1001;

/// Lookup response: data = [found: u8, endpoint_id_low: u32, endpoint_id_high: u32]
pub const MSG_LOOKUP_RESPONSE: u32 = 0x1002;

/// Request spawn: data = [name_len: u8, name: [u8]]
pub const MSG_SPAWN_SERVICE: u32 = 0x1003;

/// Spawn response: data = [success: u8, pid: u32]
pub const MSG_SPAWN_RESPONSE: u32 = 0x1004;

/// Service ready notification (service → init after registration complete)
pub const MSG_SERVICE_READY: u32 = 0x1005;

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
struct ServiceInfo {
    /// Process ID of the service
    pid: u32,
    /// Endpoint ID for communicating with the service
    endpoint_id: u64,
    /// Whether the service has signaled it's ready
    ready: bool,
}

/// Init process state
struct Init {
    /// Service registry: name → info
    services: BTreeMap<String, ServiceInfo>,
    /// Our endpoint slot for receiving messages
    endpoint_slot: u32,
    /// Boot sequence complete
    boot_complete: bool,
}

impl Init {
    fn new() -> Self {
        Self {
            services: BTreeMap::new(),
            endpoint_slot: INIT_ENDPOINT_SLOT,
            boot_complete: false,
        }
    }

    /// Print to console via SYS_CONSOLE_WRITE syscall
    fn log(&self, msg: &str) {
        syscall::console_write(&format!("[init] {}\n", msg));
    }

    /// Run the init process
    fn run(&mut self) {
        self.log("Orbital OS Init Process starting (PID 1)");
        self.log("Service registry initialized");

        // Boot sequence: spawn core services
        self.boot_sequence();

        self.log("Entering idle loop...");

        // Minimal loop: handle service messages
        loop {
            if let Some(msg) = syscall::receive(self.endpoint_slot) {
                self.handle_message(&msg);
            }
            syscall::yield_now();
        }
    }

    /// Boot sequence - spawn PermissionManager and initial apps
    fn boot_sequence(&mut self) {
        self.log("Starting boot sequence...");

        // 1. Spawn PermissionManager (PID 2) - the capability authority
        self.log("Spawning PermissionManager (PID 2)...");
        syscall::debug("INIT:SPAWN:permission_manager");

        // NOTE: Terminal is no longer auto-spawned here.
        // Each terminal window is spawned by the Desktop component via launchTerminal(),
        // which creates a process and links it to a window for proper lifecycle management.
        // This enables process isolation (each window has its own terminal process).

        self.boot_complete = true;
        self.log("Boot sequence complete");
        self.log("  PermissionManager: handles capability requests");
        self.log("  Terminal: spawned per-window by Desktop");
        self.log("Init entering minimal idle state");
    }

    /// Handle an incoming IPC message
    fn handle_message(&mut self, msg: &syscall::ReceivedMessage) {
        match msg.tag {
            MSG_REGISTER_SERVICE => self.handle_register(msg),
            MSG_LOOKUP_SERVICE => self.handle_lookup(msg),
            MSG_SERVICE_READY => self.handle_ready(msg),
            MSG_SPAWN_SERVICE => self.handle_spawn_request(msg),
            _ => {
                self.log(&format!(
                    "Unknown message tag: 0x{:x} from PID {}",
                    msg.tag, msg.from_pid
                ));
            }
        }
    }

    /// Handle service registration
    fn handle_register(&mut self, msg: &syscall::ReceivedMessage) {
        // Parse: [name_len: u8, name: [u8; name_len], endpoint_id_low: u32, endpoint_id_high: u32]
        if msg.data.len() < 9 {
            self.log("Register: invalid message (too short)");
            return;
        }

        let name_len = msg.data[0] as usize;
        if msg.data.len() < 1 + name_len + 8 {
            self.log("Register: invalid message (name truncated)");
            return;
        }

        let name = match core::str::from_utf8(&msg.data[1..1 + name_len]) {
            Ok(s) => String::from(s),
            Err(_) => {
                self.log("Register: invalid UTF-8 in name");
                return;
            }
        };

        let endpoint_id_low = u32::from_le_bytes([
            msg.data[1 + name_len],
            msg.data[2 + name_len],
            msg.data[3 + name_len],
            msg.data[4 + name_len],
        ]);
        let endpoint_id_high = u32::from_le_bytes([
            msg.data[5 + name_len],
            msg.data[6 + name_len],
            msg.data[7 + name_len],
            msg.data[8 + name_len],
        ]);
        let endpoint_id = ((endpoint_id_high as u64) << 32) | (endpoint_id_low as u64);

        let info = ServiceInfo {
            pid: msg.from_pid,
            endpoint_id,
            ready: false,
        };

        self.log(&format!(
            "Service '{}' registered by PID {} (endpoint {})",
            name, msg.from_pid, endpoint_id
        ));

        self.services.insert(name, info);
    }

    /// Handle service lookup
    fn handle_lookup(&mut self, msg: &syscall::ReceivedMessage) {
        // Parse: [name_len: u8, name: [u8; name_len]]
        if msg.data.is_empty() {
            self.log("Lookup: invalid message (empty)");
            return;
        }

        let name_len = msg.data[0] as usize;
        if msg.data.len() < 1 + name_len {
            self.log("Lookup: invalid message (name truncated)");
            return;
        }

        let name = match core::str::from_utf8(&msg.data[1..1 + name_len]) {
            Ok(s) => s,
            Err(_) => {
                self.log("Lookup: invalid UTF-8 in name");
                return;
            }
        };

        let (found, endpoint_id) = match self.services.get(name) {
            Some(info) => (1u8, info.endpoint_id),
            None => (0u8, 0u64),
        };

        self.log(&format!(
            "Lookup '{}' from PID {}: found={}",
            name,
            msg.from_pid,
            found != 0
        ));

        // Send response via debug channel
        let response_msg = format!(
            "INIT:LOOKUP_RESPONSE:{}:{}:{}",
            msg.from_pid, found, endpoint_id
        );
        syscall::debug(&response_msg);
    }

    /// Handle service ready notification
    fn handle_ready(&mut self, msg: &syscall::ReceivedMessage) {
        // Find service by PID and mark ready
        let mut found_name: Option<String> = None;
        for (name, info) in self.services.iter_mut() {
            if info.pid == msg.from_pid {
                info.ready = true;
                found_name = Some(name.clone());
                break;
            }
        }

        match found_name {
            Some(name) => self.log(&format!(
                "Service '{}' (PID {}) is ready",
                name, msg.from_pid
            )),
            None => self.log(&format!("Ready signal from unknown PID {}", msg.from_pid)),
        }
    }

    /// Handle spawn request
    fn handle_spawn_request(&mut self, msg: &syscall::ReceivedMessage) {
        // Parse: [name_len: u8, name: [u8; name_len]]
        if msg.data.is_empty() {
            self.log("Spawn: invalid message (empty)");
            return;
        }

        let name_len = msg.data[0] as usize;
        if msg.data.len() < 1 + name_len {
            self.log("Spawn: invalid message (name truncated)");
            return;
        }

        let name = match core::str::from_utf8(&msg.data[1..1 + name_len]) {
            Ok(s) => s,
            Err(_) => {
                self.log("Spawn: invalid UTF-8 in name");
                return;
            }
        };

        self.log(&format!(
            "Spawn request for '{}' from PID {}",
            name, msg.from_pid
        ));

        // Request supervisor to spawn
        syscall::debug(&format!("INIT:SPAWN:{}", name));
    }

    /// List all registered services (for debugging)
    #[allow(dead_code)]
    fn list_services(&self) {
        self.log("Registered services:");
        for (name, info) in &self.services {
            self.log(&format!(
                "  {} -> PID {} endpoint {} ready={}",
                name, info.pid, info.endpoint_id, info.ready
            ));
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
    use alloc::string::ToString;
    let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
        format!("init PANIC: {}", s)
    } else {
        "init PANIC: unknown".to_string()
    };
    syscall::debug(&msg);
    syscall::exit(1);
}

// =============================================================================
// Allocator (required for alloc in no_std on WASM)
// =============================================================================

#[cfg(target_arch = "wasm32")]
mod allocator {
    use core::alloc::{GlobalAlloc, Layout};

    struct BumpAllocator {
        head: core::sync::atomic::AtomicUsize,
    }

    #[global_allocator]
    static ALLOCATOR: BumpAllocator = BumpAllocator {
        head: core::sync::atomic::AtomicUsize::new(0),
    };

    const HEAP_START: usize = 0x10000; // 64KB offset
    const HEAP_SIZE: usize = 1024 * 1024; // 1MB heap

    unsafe impl GlobalAlloc for BumpAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let size = layout.size();
            let align = layout.align();

            loop {
                let head = self.head.load(core::sync::atomic::Ordering::Relaxed);
                let aligned = (HEAP_START + head + align - 1) & !(align - 1);
                let new_head = aligned - HEAP_START + size;

                if new_head > HEAP_SIZE {
                    return core::ptr::null_mut();
                }

                if self
                    .head
                    .compare_exchange_weak(
                        head,
                        new_head,
                        core::sync::atomic::Ordering::SeqCst,
                        core::sync::atomic::Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    return aligned as *mut u8;
                }
            }
        }

        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
            // Bump allocator doesn't deallocate
        }
    }
}
