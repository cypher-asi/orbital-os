//! Network System Integration
//!
//! This module handles the integration between JavaScript network operations
//! (fetch API) and WASM processes. The supervisor receives notifications from
//! JavaScript when network operations complete and delivers the results to the
//! requesting processes via IPC through Init.

use wasm_bindgen::prelude::*;
use zos_hal::HAL;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

impl super::Supervisor {
    /// Internal handler for network result.
    pub(super) fn on_network_result_internal(
        &mut self,
        request_id: u32,
        pid: u64,
        result: JsValue,
    ) {
        log(&format!(
            "[supervisor] onNetworkResult: request_id={}, pid={}",
            request_id, pid
        ));

        // Verify the PID matches and remove from pending
        let expected_pid = match self.system.hal().take_network_request_pid(request_id) {
            Some(p) => p,
            None => {
                log(&format!(
                    "[supervisor] Unknown network request_id: {}",
                    request_id
                ));
                return;
            }
        };

        if expected_pid != pid {
            log(&format!(
                "[supervisor] Network request PID mismatch: expected {}, got {}",
                expected_pid, pid
            ));
            return;
        }

        // Serialize the result to JSON bytes
        let result_json = match js_sys::JSON::stringify(&result) {
            Ok(s) => s.as_string().unwrap_or_default(),
            Err(_) => {
                log("[supervisor] Failed to stringify network result");
                r#"{"result":{"Err":"Internal error"}}"#.to_string()
            }
        };

        // Build MSG_NET_RESULT payload
        // Format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
        let result_bytes = result_json.as_bytes();
        let mut payload = Vec::with_capacity(9 + result_bytes.len());
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(0); // NET_OK - the actual result status is in the JSON
        payload.extend_from_slice(&(result_bytes.len() as u32).to_le_bytes());
        payload.extend_from_slice(result_bytes);

        // Deliver to requesting process via Init
        self.deliver_network_result(pid, &payload);
    }

    /// Deliver a network result to a process via IPC through Init.
    fn deliver_network_result(&mut self, pid: u64, payload: &[u8]) {
        // Route through Init for capability-checked delivery
        const INPUT_ENDPOINT_SLOT: u32 = 1;
        // MSG_NET_RESULT = 0x9002
        const MSG_NET_RESULT: u32 = 0x9002;

        self.route_ipc_via_init(pid, INPUT_ENDPOINT_SLOT, MSG_NET_RESULT, payload);
    }
}
