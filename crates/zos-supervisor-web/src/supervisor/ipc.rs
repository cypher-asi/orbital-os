//! IPC Routing and Delivery
//!
//! This module handles IPC message routing through the Init process for
//! capability-checked delivery. The supervisor uses capability-checked IPC
//! instead of privileged kernel APIs (Invariant 16 compliance).

use wasm_bindgen::prelude::*;
use zos_kernel::ProcessId;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

/// Decode a hex string to bytes
fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, &'static str> {
    if !hex.len().is_multiple_of(2) {
        return Err("Invalid hex length");
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| "Invalid hex character"))
        .collect()
}

impl super::Supervisor {
    /// Route console input through Init (fallback when supervisor lacks capability)
    pub(super) fn route_console_input_via_init(&mut self, target_pid: u64, input: &str) {
        let init_slot = match self.init_endpoint_slot {
            Some(slot) => slot,
            None => {
                log("[supervisor] Cannot route console input: no Init capability");
                return;
            }
        };

        // Build message for Init: [target_pid: u32, endpoint_slot: u32, data_len: u16, data: [u8]]
        let mut payload = Vec::with_capacity(10 + input.len());
        payload.extend_from_slice(&(target_pid as u32).to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes()); // Terminal input slot
        payload.extend_from_slice(&(input.len() as u16).to_le_bytes());
        payload.extend_from_slice(input.as_bytes());

        let supervisor_pid = ProcessId(0);
        use zos_ipc::supervisor::MSG_SUPERVISOR_CONSOLE_INPUT;

        match self.system.ipc_send(
            supervisor_pid,
            init_slot,
            MSG_SUPERVISOR_CONSOLE_INPUT,
            payload,
        ) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Routed {} bytes to PID {} via Init",
                    input.len(),
                    target_pid
                ));
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to route console input via Init: {:?}",
                    e
                ));
            }
        }
    }

    /// Route an IPC message through Init for capability-checked delivery
    pub(super) fn route_ipc_via_init(
        &mut self,
        target_pid: u64,
        endpoint_slot: u32,
        tag: u32,
        data: &[u8],
    ) {
        let init_slot = match self.init_endpoint_slot {
            Some(slot) => slot,
            None => {
                log("[supervisor] Cannot route IPC: no Init capability");
                return;
            }
        };

        use zos_ipc::supervisor::MSG_SUPERVISOR_IPC_DELIVERY;

        // Build message for Init: [target_pid: u32, endpoint_slot: u32, tag: u32, data_len: u16, data: [u8]]
        let mut payload = Vec::with_capacity(14 + data.len());
        payload.extend_from_slice(&(target_pid as u32).to_le_bytes());
        payload.extend_from_slice(&endpoint_slot.to_le_bytes());
        payload.extend_from_slice(&tag.to_le_bytes());
        payload.extend_from_slice(&(data.len() as u16).to_le_bytes());
        payload.extend_from_slice(data);

        let supervisor_pid = ProcessId(0);

        match self.system.ipc_send(
            supervisor_pid,
            init_slot,
            MSG_SUPERVISOR_IPC_DELIVERY,
            payload,
        ) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Routed IPC to PID {} endpoint {} tag 0x{:x} via Init",
                    target_pid, endpoint_slot, tag
                ));
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to route IPC via Init: {:?}",
                    e
                ));
            }
        }
    }

    /// Handle VFS:RESPONSE: debug message.
    ///
    /// Format: {to_pid}:{tag_hex}:{hex_data}
    /// Example: "4:00008013:7b22..."
    /// Routes VFS responses back to the requesting process via Init.
    pub(super) fn handle_debug_vfs_response(&mut self, rest: &str) {
        let parts: Vec<&str> = rest.splitn(3, ':').collect();
        if parts.len() != 3 {
            log(&format!("[supervisor] Malformed VFS:RESPONSE: {}", rest));
            return;
        }

        let to_pid = match parts[0].parse::<u32>() {
            Ok(p) => p,
            Err(_) => {
                log(&format!(
                    "[supervisor] VFS:RESPONSE invalid PID: {}",
                    parts[0]
                ));
                return;
            }
        };

        let tag = match u32::from_str_radix(parts[1], 16) {
            Ok(t) => t,
            Err(_) => {
                log(&format!(
                    "[supervisor] VFS:RESPONSE invalid tag: {}",
                    parts[1]
                ));
                return;
            }
        };

        let data = match hex_to_bytes(parts[2]) {
            Ok(d) => d,
            Err(_) => {
                log("[supervisor] VFS:RESPONSE invalid hex data");
                return;
            }
        };

        // Route response through Init for capability-checked delivery
        // Use slot 4 (VFS_RESPONSE_SLOT) instead of slot 1 to avoid race conditions
        // where the VFS client's blocking receive could consume other IPC messages.
        const VFS_RESPONSE_SLOT: u32 = 4;
        self.route_ipc_via_init(to_pid as u64, VFS_RESPONSE_SLOT, tag, &data);
    }

    /// Handle SERVICE:RESPONSE: debug message.
    ///
    /// Format: {to_pid}:{tag_hex}:{hex_data}
    /// Example: "0:00007055:7b22..."
    pub(super) fn handle_debug_service_response(&self, rest: &str) {
        let parts: Vec<&str> = rest.splitn(3, ':').collect();
        if parts.len() != 3 {
            log(&format!(
                "[supervisor] Malformed SERVICE:RESPONSE: {}",
                rest
            ));
            return;
        }

        let request_id = parts[1]; // tag hex is the request_id
        let hex_data = parts[2];

        // Invoke JS callback if registered
        let Some(ref callback) = self.ipc_response_callback else {
            log(&format!(
                "[supervisor] SERVICE:RESPONSE received but no callback registered (request_id={})",
                request_id
            ));
            return;
        };

        let bytes = match hex_to_bytes(hex_data) {
            Ok(b) => b,
            Err(_) => {
                log(&format!(
                    "[supervisor] SERVICE:RESPONSE invalid hex for request_id={}",
                    request_id
                ));
                return;
            }
        };

        let json = match String::from_utf8(bytes) {
            Ok(j) => j,
            Err(_) => {
                log(&format!(
                    "[supervisor] SERVICE:RESPONSE invalid UTF-8 for request_id={}",
                    request_id
                ));
                return;
            }
        };

        let this = JsValue::null();
        let id_arg = JsValue::from_str(request_id);
        let data_arg = JsValue::from_str(&json);
        let _ = callback.call2(&this, &id_arg, &data_arg);
        log(&format!(
            "[supervisor] Invoked IPC callback for request_id={}",
            request_id
        ));
    }

    /// Find a service process by name.
    ///
    /// Looks up "{service_name}_service" in the process list.
    pub(super) fn find_service_pid(&self, service_name: &str) -> Option<ProcessId> {
        let expected_name = format!("{}_service", service_name);
        for (pid, proc) in self.system.list_processes() {
            if proc.name == expected_name {
                return Some(pid);
            }
        }
        None
    }
}
