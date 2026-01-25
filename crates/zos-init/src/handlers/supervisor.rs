//! Supervisor protocol handlers
//!
//! These handlers process messages from the supervisor that need kernel
//! access. Init (PID 1) has the necessary capabilities to perform these
//! operations via syscalls, while the supervisor does not have direct
//! kernel access.

#[cfg(target_arch = "wasm32")]
use alloc::{format, string::String};

#[cfg(not(target_arch = "wasm32"))]
use std::{format, string::String};

use crate::Init;
use zos_process as syscall;

impl Init {
    /// Handle supervisor request to deliver console input to a terminal.
    ///
    /// The supervisor routes keyboard input here. Init then forwards
    /// to the target terminal process via IPC.
    ///
    /// Payload: [target_pid: u32, endpoint_slot: u32, data_len: u16, data: [u8]]
    pub fn handle_supervisor_console_input(&mut self, msg: &syscall::ReceivedMessage) {
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
    pub fn handle_supervisor_kill_process(&mut self, msg: &syscall::ReceivedMessage) {
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
    pub fn handle_supervisor_ipc_delivery(&mut self, msg: &syscall::ReceivedMessage) {
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
    pub fn handle_supervisor_spawn_process(&mut self, msg: &syscall::ReceivedMessage) {
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
    pub fn send_spawn_response(&self, success: u8, pid: u32) {
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
    pub fn handle_supervisor_create_endpoint(&mut self, msg: &syscall::ReceivedMessage) {
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
    pub fn send_endpoint_response(&self, success: u8, endpoint_id: u64, slot: u32) {
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
    pub fn handle_supervisor_grant_cap(&mut self, msg: &syscall::ReceivedMessage) {
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
    pub fn send_cap_response(&self, success: u8, new_slot: u32) {
        let mut payload = [0u8; 5];
        payload[0] = success;
        payload[1..5].copy_from_slice(&new_slot.to_le_bytes());

        // Send via debug channel to supervisor
        let hex: String = payload.iter().map(|b| format!("{:02x}", b)).collect();
        syscall::debug(&format!("CAP:RESPONSE:{}", hex));
    }
}
