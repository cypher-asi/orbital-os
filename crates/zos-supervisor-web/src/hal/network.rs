//! Network operations for WASM HAL
//!
//! This module handles network fetch operations via the JavaScript ZosNetwork API.

use wasm_bindgen::prelude::*;
use zos_hal::{HalError, NetworkRequestId};

use super::WasmHal;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

/// Helper function to start a network fetch via ZosNetwork.startFetch
pub(crate) fn start_network_fetch(request_id: u32, pid: u64, request_json: &str) {
    let window = match web_sys::window() {
        Some(w) => w,
        None => {
            log(&format!(
                "[wasm-hal] start_network_fetch: no window for request_id={}",
                request_id
            ));
            return;
        }
    };

    let zos_network = match js_sys::Reflect::get(&window, &"ZosNetwork".into()) {
        Ok(n) if !n.is_undefined() => n,
        _ => {
            log(&format!(
                "[wasm-hal] start_network_fetch: ZosNetwork not found for request_id={}",
                request_id
            ));
            return;
        }
    };

    let request_obj = match js_sys::JSON::parse(request_json) {
        Ok(obj) => obj,
        Err(e) => {
            log(&format!(
                "[wasm-hal] start_network_fetch: JSON parse error for request_id={}: {:?}",
                request_id, e
            ));
            log(&format!("[wasm-hal] request_json: {}", request_json));
            return;
        }
    };

    let start_fetch_fn = match js_sys::Reflect::get(&zos_network, &"startFetch".into()) {
        Ok(f) => match f.dyn_into::<js_sys::Function>() {
            Ok(func) => func,
            Err(_) => {
                log(&format!("[wasm-hal] start_network_fetch: startFetch is not a function for request_id={}", request_id));
                return;
            }
        },
        Err(_) => {
            log(&format!(
                "[wasm-hal] start_network_fetch: startFetch not found for request_id={}",
                request_id
            ));
            return;
        }
    };

    let args = js_sys::Array::of3(&request_id.into(), &(pid as f64).into(), &request_obj);
    if let Err(e) = js_sys::Reflect::apply(&start_fetch_fn, &zos_network, &args) {
        log(&format!(
            "[wasm-hal] start_network_fetch: apply error for request_id={}: {:?}",
            request_id, e
        ));
    }
}

impl WasmHal {
    /// Start an async network fetch operation
    pub fn do_network_fetch_async(
        &self,
        pid: u64,
        request: &[u8],
    ) -> Result<NetworkRequestId, HalError> {
        let request_id = self.next_network_request_id();
        self.record_pending_network_request(request_id, pid);

        // Convert request bytes to JSON string
        let request_json = match std::str::from_utf8(request) {
            Ok(s) => s,
            Err(_) => {
                log("[wasm-hal] network_fetch_async: invalid UTF-8 in request");
                return Err(HalError::InvalidArgument);
            }
        };

        log(&format!(
            "[wasm-hal] network_fetch_async: request_id={}, pid={}",
            request_id, pid
        ));

        // Call JavaScript to start fetch operation
        start_network_fetch(request_id, pid, request_json);

        Ok(request_id)
    }

    /// Get the PID associated with a network request
    pub fn do_get_network_request_pid(&self, request_id: NetworkRequestId) -> Option<u64> {
        self.pending_network_requests
            .lock()
            .ok()
            .and_then(|pending| pending.get(&request_id).copied())
    }

    /// Take (remove) the PID associated with a network request
    pub fn do_take_network_request_pid(&self, request_id: NetworkRequestId) -> Option<u64> {
        self.pending_network_requests
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&request_id))
    }
}
