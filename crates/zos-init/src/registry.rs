//! Service registry handlers
//!
//! Manages the service name → endpoint mapping for service discovery.

#[cfg(target_arch = "wasm32")]
use alloc::format;
#[cfg(target_arch = "wasm32")]
use alloc::string::String;

#[cfg(not(target_arch = "wasm32"))]
use std::format;
#[cfg(not(target_arch = "wasm32"))]
use std::string::String;

use crate::Init;
use zos_process as syscall;

impl Init {
    /// Handle service registration
    pub fn handle_register(&mut self, msg: &syscall::ReceivedMessage) {
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

        let info = crate::ServiceInfo {
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
    pub fn handle_lookup(&mut self, msg: &syscall::ReceivedMessage) {
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
    pub fn handle_ready(&mut self, msg: &syscall::ReceivedMessage) {
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

    /// List all registered services (for debugging)
    #[allow(dead_code)]
    pub fn list_services(&self) {
        self.log("Registered services:");
        for (name, info) in &self.services {
            self.log(&format!(
                "  {} -> PID {} endpoint {} ready={}",
                name, info.pid, info.endpoint_id, info.ready
            ));
        }
    }
}
