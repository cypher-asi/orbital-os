//! Service protocol handlers
//!
//! Handles MSG_SPAWN_SERVICE and capability granted notifications.

#[cfg(target_arch = "wasm32")]
use alloc::format;

#[cfg(not(target_arch = "wasm32"))]
use std::format;

use crate::Init;
use zos_process as syscall;

impl Init {
    /// Handle spawn request
    pub fn handle_spawn_request(&mut self, msg: &syscall::ReceivedMessage) {
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

    /// Handle service capability granted notification from supervisor.
    ///
    /// The supervisor notifies Init when it grants Init a capability to a
    /// service's input endpoint. Init stores this mapping so it can deliver
    /// IPC messages to services via capability-checked syscall::send().
    ///
    /// Payload: [service_pid: u32, cap_slot: u32]
    pub fn handle_service_cap_granted(&mut self, msg: &syscall::ReceivedMessage) {
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
    pub fn handle_vfs_response_cap_granted(&mut self, msg: &syscall::ReceivedMessage) {
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
}
