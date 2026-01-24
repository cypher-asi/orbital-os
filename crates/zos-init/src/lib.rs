//! Init Process (PID 1) for Zero OS
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

use zos_process::{self as syscall};

// =============================================================================
// Service Protocol Constants
// =============================================================================
// All constants are re-exported from zos-ipc via zos-process for consistency.

pub use zos_process::{
    MSG_REGISTER_SERVICE,
    MSG_LOOKUP_SERVICE,
    MSG_LOOKUP_RESPONSE,
    MSG_SPAWN_SERVICE,
    MSG_SPAWN_RESPONSE,
    MSG_SERVICE_READY,
    MSG_SUPERVISOR_CONSOLE_INPUT,
    MSG_SUPERVISOR_KILL_PROCESS,
    MSG_SUPERVISOR_IPC_DELIVERY,
};

// Additional Init-specific constants from zos-ipc
pub use zos_process::init::{MSG_SERVICE_CAP_GRANTED, MSG_VFS_RESPONSE_CAP_GRANTED};

// Spawn protocol messages for Init-driven spawn
pub use zos_process::supervisor::{
    MSG_SUPERVISOR_SPAWN_PROCESS,
    MSG_SUPERVISOR_SPAWN_RESPONSE,
    MSG_SUPERVISOR_CREATE_ENDPOINT,
    MSG_SUPERVISOR_ENDPOINT_RESPONSE,
    MSG_SUPERVISOR_GRANT_CAP,
    MSG_SUPERVISOR_CAP_RESPONSE,
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
    /// Service input capability slots: service_pid → capability slot in Init's CSpace
    /// Used for delivering IPC messages to services' input endpoint (slot 1)
    service_cap_slots: BTreeMap<u32, u32>,
    /// Service VFS response capability slots: service_pid → capability slot in Init's CSpace
    /// Used for delivering VFS responses to services' VFS response endpoint (slot 4)
    service_vfs_slots: BTreeMap<u32, u32>,
    /// Our endpoint slot for receiving messages
    endpoint_slot: u32,
    /// Boot sequence complete
    boot_complete: bool,
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
    fn log(&self, msg: &str) {
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
            if let Some(msg) = syscall::receive(self.endpoint_slot) {
                self.handle_message(&msg);
            }
            syscall::yield_now();
        }
    }

    /// Boot sequence - spawn PermissionManager, VfsService, IdentityService, and initial apps
    fn boot_sequence(&mut self) {
        self.log("Starting boot sequence...");

        // 1. Spawn PermissionManager (PID 2) - the capability authority
        self.log("Spawning PermissionManager (PID 2)...");
        syscall::debug("INIT:SPAWN:permission_manager");

        // 2. Spawn VfsService (PID 3) - virtual filesystem service
        // NOTE: VFS must be spawned before IdentityService since identity needs VFS
        self.log("Spawning VfsService (PID 3)...");
        syscall::debug("INIT:SPAWN:vfs_service");

        // 3. Spawn IdentityService (PID 4) - user identity and key management
        self.log("Spawning IdentityService (PID 4)...");
        syscall::debug("INIT:SPAWN:identity_service");

        // 4. Spawn TimeService (PID 5) - time settings management
        self.log("Spawning TimeService (PID 5)...");
        syscall::debug("INIT:SPAWN:time_service");

        // NOTE: Terminal is no longer auto-spawned here.
        // Each terminal window is spawned by the Desktop component via launchTerminal(),
        // which creates a process and links it to a window for proper lifecycle management.
        // This enables process isolation (each window has its own terminal process).

        self.boot_complete = true;
        self.log("Boot sequence complete");
        self.log("  PermissionManager: handles capability requests");
        self.log("  VfsService: handles filesystem operations");
        self.log("  IdentityService: handles identity and key management");
        self.log("  TimeService: handles time settings");
        self.log("  Terminal: spawned per-window by Desktop");
        self.log("Init entering minimal idle state");
    }

    /// Handle an incoming IPC message
    fn handle_message(&mut self, msg: &syscall::ReceivedMessage) {
        match msg.tag {
            // Service registry protocol
            MSG_REGISTER_SERVICE => self.handle_register(msg),
            MSG_LOOKUP_SERVICE => self.handle_lookup(msg),
            MSG_SERVICE_READY => self.handle_ready(msg),
            MSG_SPAWN_SERVICE => self.handle_spawn_request(msg),

            // Supervisor → Init protocol
            MSG_SUPERVISOR_CONSOLE_INPUT => self.handle_supervisor_console_input(msg),
            MSG_SUPERVISOR_KILL_PROCESS => self.handle_supervisor_kill_process(msg),
            MSG_SUPERVISOR_IPC_DELIVERY => self.handle_supervisor_ipc_delivery(msg),
            MSG_SERVICE_CAP_GRANTED => self.handle_service_cap_granted(msg),
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

    // =========================================================================
    // Supervisor → Init Message Handlers
    // =========================================================================
    //
    // These handlers process messages from the supervisor that need kernel
    // access. Init (PID 1) has the necessary capabilities to perform these
    // operations via syscalls, while the supervisor does not have direct
    // kernel access.

    /// Handle supervisor request to deliver console input to a terminal.
    ///
    /// The supervisor routes keyboard input here. Init then forwards
    /// to the target terminal process via IPC.
    ///
    /// Payload: [target_pid: u32, endpoint_slot: u32, data_len: u16, data: [u8]]
    fn handle_supervisor_console_input(&mut self, msg: &syscall::ReceivedMessage) {
        // Verify sender is supervisor (PID 0)
        if msg.from_pid != 0 {
            self.log(&format!(
                "SECURITY: Supervisor message from non-supervisor PID {}",
                msg.from_pid
            ));
            return;
        }

        // Parse: [target_pid: u32, endpoint_slot: u32, data_len: u16, data: [u8]]
        if msg.data.len() < 10 {
            self.log("SupervisorConsoleInput: message too short");
            return;
        }

        let target_pid = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);
        let endpoint_slot =
            u32::from_le_bytes([msg.data[4], msg.data[5], msg.data[6], msg.data[7]]);
        let data_len = u16::from_le_bytes([msg.data[8], msg.data[9]]) as usize;

        if msg.data.len() < 10 + data_len {
            self.log("SupervisorConsoleInput: data truncated");
            return;
        }

        let input_data = &msg.data[10..10 + data_len];

        self.log(&format!(
            "Routing console input to PID {} endpoint {} ({} bytes)",
            target_pid, endpoint_slot, data_len
        ));

        // Forward to target process
        // Note: Init needs a capability to the target's endpoint.
        // For now, we use the debug channel to signal the supervisor
        // to do the actual delivery. This will be replaced once Init
        // has proper endpoint capabilities granted during spawn.
        let data_hex: String = input_data.iter().map(|b| format!("{:02x}", b)).collect();
        syscall::debug(&format!(
            "INIT:CONSOLE_INPUT:{}:{}:{}",
            target_pid, endpoint_slot, data_hex
        ));
    }

    /// Handle supervisor request to kill a process.
    ///
    /// The supervisor requests process termination here. Init invokes
    /// the SYS_KILL syscall. Init (PID 1) has implicit permission to
    /// kill any process.
    ///
    /// Payload: [target_pid: u32]
    fn handle_supervisor_kill_process(&mut self, msg: &syscall::ReceivedMessage) {
        // Verify sender is supervisor (PID 0)
        if msg.from_pid != 0 {
            self.log(&format!(
                "SECURITY: Kill request from non-supervisor PID {}",
                msg.from_pid
            ));
            return;
        }

        // Parse: [target_pid: u32]
        if msg.data.len() < 4 {
            self.log("SupervisorKillProcess: message too short");
            return;
        }

        let target_pid = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);

        self.log(&format!(
            "Supervisor requested kill of PID {}",
            target_pid
        ));

        // Invoke the kill syscall
        // Init (PID 1) has implicit permission to kill any process
        match syscall::kill(target_pid) {
            Ok(()) => {
                self.log(&format!("Process {} terminated successfully", target_pid));
                // Notify supervisor of success
                syscall::debug(&format!("INIT:KILL_OK:{}", target_pid));
            }
            Err(e) => {
                self.log(&format!(
                    "Failed to kill process {}: error {}",
                    target_pid, e
                ));
                // Notify supervisor of failure
                syscall::debug(&format!("INIT:KILL_FAIL:{}:{}", target_pid, e));
            }
        }
    }

    /// Handle supervisor request to deliver an IPC message to a process.
    ///
    /// The supervisor routes messages that need capability-checked delivery.
    /// Init performs the IPC send using its capabilities.
    ///
    /// Payload: [target_pid: u32, endpoint_slot: u32, tag: u32, data_len: u16, data: [u8]]
    fn handle_supervisor_ipc_delivery(&mut self, msg: &syscall::ReceivedMessage) {
        // Verify sender is supervisor (PID 0)
        if msg.from_pid != 0 {
            self.log(&format!(
                "SECURITY: IPC delivery request from non-supervisor PID {}",
                msg.from_pid
            ));
            return;
        }

        // Parse: [target_pid: u32, endpoint_slot: u32, tag: u32, data_len: u16, data: [u8]]
        if msg.data.len() < 14 {
            self.log("SupervisorIpcDelivery: message too short");
            return;
        }

        let target_pid = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);
        let endpoint_slot =
            u32::from_le_bytes([msg.data[4], msg.data[5], msg.data[6], msg.data[7]]);
        let tag = u32::from_le_bytes([msg.data[8], msg.data[9], msg.data[10], msg.data[11]]);
        let data_len = u16::from_le_bytes([msg.data[12], msg.data[13]]) as usize;

        if msg.data.len() < 14 + data_len {
            self.log("SupervisorIpcDelivery: data truncated");
            return;
        }

        let ipc_data = &msg.data[14..14 + data_len];

        // Select the correct capability slot based on target endpoint:
        // - Slot 4 (VFS_RESPONSE_SLOT): use service_vfs_slots (VFS response delivery)
        // - Slot 1 (input endpoint): use service_cap_slots (general IPC)
        const VFS_RESPONSE_SLOT: u32 = 4;
        
        let cap_slot = if endpoint_slot == VFS_RESPONSE_SLOT {
            self.service_vfs_slots.get(&target_pid).copied()
        } else {
            self.service_cap_slots.get(&target_pid).copied()
        };
        
        if let Some(cap_slot) = cap_slot {
            self.log(&format!(
                "Delivering IPC to PID {} slot {} via cap slot {} (tag 0x{:x}, {} bytes)",
                target_pid, endpoint_slot, cap_slot, tag, data_len
            ));

            // Deliver via capability-checked IPC
            match syscall::send(cap_slot, tag, ipc_data) {
                Ok(()) => {
                    self.log(&format!("IPC delivered to PID {} slot {}", target_pid, endpoint_slot));
                }
                Err(e) => {
                    self.log(&format!(
                        "IPC delivery to PID {} slot {} failed: error {}",
                        target_pid, endpoint_slot, e
                    ));
                }
            }
        } else {
            self.log(&format!(
                "No capability for PID {} slot {} - cannot deliver IPC (routing via debug fallback)",
                target_pid, endpoint_slot
            ));
            // Fall back to debug channel for supervisor (legacy behavior)
            let data_hex: String = ipc_data.iter().map(|b| format!("{:02x}", b)).collect();
            syscall::debug(&format!(
                "INIT:IPC_DELIVERY:{}:{}:{:x}:{}",
                target_pid, endpoint_slot, tag, data_hex
            ));
        }
    }

    /// Handle service capability granted notification from supervisor.
    ///
    /// The supervisor notifies Init when it grants Init a capability to a
    /// service's input endpoint. Init stores this mapping so it can deliver
    /// IPC messages to services via capability-checked syscall::send().
    ///
    /// Payload: [service_pid: u32, cap_slot: u32]
    fn handle_service_cap_granted(&mut self, msg: &syscall::ReceivedMessage) {
        // Verify sender is supervisor (PID 0)
        if msg.from_pid != 0 {
            self.log(&format!(
                "SECURITY: Service cap notification from non-supervisor PID {}",
                msg.from_pid
            ));
            return;
        }

        // Parse: [service_pid: u32, cap_slot: u32]
        if msg.data.len() < 8 {
            self.log("ServiceCapGranted: message too short");
            return;
        }

        let service_pid = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);
        let cap_slot = u32::from_le_bytes([msg.data[4], msg.data[5], msg.data[6], msg.data[7]]);

        self.log(&format!(
            "Registered capability for service PID {} at slot {}",
            service_pid, cap_slot
        ));

        self.service_cap_slots.insert(service_pid, cap_slot);
    }

    /// Handle VFS response endpoint capability granted notification from supervisor.
    ///
    /// The supervisor notifies Init when it grants Init a capability to a
    /// process's VFS response endpoint (slot 4). Init stores this mapping
    /// so it can deliver VFS responses to the correct endpoint, separate
    /// from the process's input endpoint (slot 1).
    ///
    /// Payload: [service_pid: u32, cap_slot: u32]
    fn handle_vfs_response_cap_granted(&mut self, msg: &syscall::ReceivedMessage) {
        // Verify sender is supervisor (PID 0)
        if msg.from_pid != 0 {
            self.log(&format!(
                "SECURITY: VFS response cap notification from non-supervisor PID {}",
                msg.from_pid
            ));
            return;
        }

        // Parse: [service_pid: u32, cap_slot: u32]
        if msg.data.len() < 8 {
            self.log("VfsResponseCapGranted: message too short");
            return;
        }

        let service_pid = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);
        let cap_slot = u32::from_le_bytes([msg.data[4], msg.data[5], msg.data[6], msg.data[7]]);

        self.log(&format!(
            "Registered VFS response capability for PID {} at slot {}",
            service_pid, cap_slot
        ));

        self.service_vfs_slots.insert(service_pid, cap_slot);
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

    // =========================================================================
    // Init-Driven Spawn Protocol Handlers
    // =========================================================================
    //
    // These handlers implement the Init-driven spawn protocol where all process
    // lifecycle operations flow through Init. This ensures:
    // - All operations are logged via SysLog (Invariant 9)
    // - Supervisor has no direct kernel access (Invariant 16)
    // - Init is the capability authority for process creation

    /// Handle supervisor request to spawn a new process.
    ///
    /// The supervisor sends MSG_SUPERVISOR_SPAWN_PROCESS when it wants to
    /// create a new process. Init performs the actual kernel registration
    /// via SYS_REGISTER_PROCESS and responds with the assigned PID.
    ///
    /// Payload: [name_len: u8, name: [u8]]
    fn handle_supervisor_spawn_process(&mut self, msg: &syscall::ReceivedMessage) {
        // Verify sender is supervisor (PID 0)
        if msg.from_pid != 0 {
            self.log(&format!(
                "SECURITY: Spawn request from non-supervisor PID {}",
                msg.from_pid
            ));
            return;
        }

        // Parse: [name_len: u8, name: [u8]]
        if msg.data.is_empty() {
            self.log("SupervisorSpawnProcess: message too short");
            self.send_spawn_response(0, 0); // failure
            return;
        }

        let name_len = msg.data[0] as usize;
        if msg.data.len() < 1 + name_len {
            self.log("SupervisorSpawnProcess: name truncated");
            self.send_spawn_response(0, 0); // failure
            return;
        }

        let name = match core::str::from_utf8(&msg.data[1..1 + name_len]) {
            Ok(s) => s,
            Err(_) => {
                self.log("SupervisorSpawnProcess: invalid UTF-8 in name");
                self.send_spawn_response(0, 0); // failure
                return;
            }
        };

        self.log(&format!(
            "Spawn request from supervisor: registering '{}'",
            name
        ));

        // Register the process via SYS_REGISTER_PROCESS syscall
        // This syscall is Init-only and logs to SysLog
        match syscall::register_process(name) {
            Ok(pid) => {
                self.log(&format!(
                    "Process '{}' registered with PID {}",
                    name, pid
                ));
                self.send_spawn_response(1, pid); // success
            }
            Err(e) => {
                self.log(&format!(
                    "Failed to register process '{}': error {}",
                    name, e
                ));
                self.send_spawn_response(0, 0); // failure
            }
        }
    }

    /// Send spawn response to supervisor.
    ///
    /// Payload: [success: u8, pid: u32]
    fn send_spawn_response(&self, success: u8, pid: u32) {
        let mut payload = [0u8; 5];
        payload[0] = success;
        payload[1..5].copy_from_slice(&pid.to_le_bytes());

        // Send via debug channel to supervisor (PID 0 doesn't have standard endpoint)
        let hex: String = payload.iter().map(|b| format!("{:02x}", b)).collect();
        syscall::debug(&format!("SPAWN:RESPONSE:{}", hex));
    }

    /// Handle supervisor request to create an endpoint for a process.
    ///
    /// The supervisor sends MSG_SUPERVISOR_CREATE_ENDPOINT to set up
    /// endpoints for a newly spawned process. Init creates the endpoint
    /// via SYS_CREATE_ENDPOINT_FOR and responds with the endpoint info.
    ///
    /// Payload: [target_pid: u32]
    fn handle_supervisor_create_endpoint(&mut self, msg: &syscall::ReceivedMessage) {
        // Verify sender is supervisor (PID 0)
        if msg.from_pid != 0 {
            self.log(&format!(
                "SECURITY: Create endpoint request from non-supervisor PID {}",
                msg.from_pid
            ));
            return;
        }

        // Parse: [target_pid: u32]
        if msg.data.len() < 4 {
            self.log("SupervisorCreateEndpoint: message too short");
            self.send_endpoint_response(0, 0, 0); // failure
            return;
        }

        let target_pid = u32::from_le_bytes([
            msg.data[0],
            msg.data[1],
            msg.data[2],
            msg.data[3],
        ]);

        self.log(&format!(
            "Create endpoint request for PID {}",
            target_pid
        ));

        // Create endpoint via SYS_CREATE_ENDPOINT_FOR syscall
        // This syscall is Init-only and logs to SysLog
        match syscall::create_endpoint_for(target_pid) {
            Ok((endpoint_id, slot)) => {
                self.log(&format!(
                    "Created endpoint {} at slot {} for PID {}",
                    endpoint_id, slot, target_pid
                ));
                self.send_endpoint_response(1, endpoint_id, slot); // success
            }
            Err(e) => {
                self.log(&format!(
                    "Failed to create endpoint for PID {}: error {}",
                    target_pid, e
                ));
                self.send_endpoint_response(0, 0, 0); // failure
            }
        }
    }

    /// Send endpoint response to supervisor.
    ///
    /// Payload: [success: u8, endpoint_id: u64, slot: u32]
    fn send_endpoint_response(&self, success: u8, endpoint_id: u64, slot: u32) {
        let mut payload = [0u8; 13];
        payload[0] = success;
        payload[1..9].copy_from_slice(&endpoint_id.to_le_bytes());
        payload[9..13].copy_from_slice(&slot.to_le_bytes());

        // Send via debug channel to supervisor
        let hex: String = payload.iter().map(|b| format!("{:02x}", b)).collect();
        syscall::debug(&format!("ENDPOINT:RESPONSE:{}", hex));
    }

    /// Handle supervisor request to grant a capability.
    ///
    /// The supervisor sends MSG_SUPERVISOR_GRANT_CAP to set up capabilities
    /// during process spawn. Init performs the grant via SYS_CAP_GRANT.
    ///
    /// Payload: [from_pid: u32, from_slot: u32, to_pid: u32, perms: u8]
    fn handle_supervisor_grant_cap(&mut self, msg: &syscall::ReceivedMessage) {
        // Verify sender is supervisor (PID 0)
        if msg.from_pid != 0 {
            self.log(&format!(
                "SECURITY: Grant cap request from non-supervisor PID {}",
                msg.from_pid
            ));
            return;
        }

        // Parse: [from_pid: u32, from_slot: u32, to_pid: u32, perms: u8]
        if msg.data.len() < 13 {
            self.log("SupervisorGrantCap: message too short");
            self.send_cap_response(0, 0); // failure
            return;
        }

        let from_pid = u32::from_le_bytes([
            msg.data[0],
            msg.data[1],
            msg.data[2],
            msg.data[3],
        ]);
        let from_slot = u32::from_le_bytes([
            msg.data[4],
            msg.data[5],
            msg.data[6],
            msg.data[7],
        ]);
        let to_pid = u32::from_le_bytes([
            msg.data[8],
            msg.data[9],
            msg.data[10],
            msg.data[11],
        ]);
        let perms = msg.data[12];

        self.log(&format!(
            "Grant cap request: from PID {} slot {} to PID {} perms 0x{:02x}",
            from_pid, from_slot, to_pid, perms
        ));

        // Grant capability via SYS_CAP_GRANT syscall
        // Note: Init can grant capabilities because it has grant permission
        match syscall::cap_grant(from_slot, to_pid, syscall::Permissions::from_byte(perms)) {
            Ok(new_slot) => {
                self.log(&format!(
                    "Granted cap to PID {} at slot {}",
                    to_pid, new_slot
                ));
                self.send_cap_response(1, new_slot); // success
            }
            Err(e) => {
                self.log(&format!(
                    "Failed to grant cap to PID {}: error {}",
                    to_pid, e
                ));
                self.send_cap_response(0, 0); // failure
            }
        }
    }

    /// Send capability grant response to supervisor.
    ///
    /// Payload: [success: u8, new_slot: u32]
    fn send_cap_response(&self, success: u8, new_slot: u32) {
        let mut payload = [0u8; 5];
        payload[0] = success;
        payload[1..5].copy_from_slice(&new_slot.to_le_bytes());

        // Send via debug channel to supervisor
        let hex: String = payload.iter().map(|b| format!("{:02x}", b)).collect();
        syscall::debug(&format!("CAP:RESPONSE:{}", hex));
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
