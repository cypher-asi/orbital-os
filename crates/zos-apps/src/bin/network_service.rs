//! Network Service (PID 6)
//!
//! The Network Service mediates HTTP requests for Zero OS. It:
//! - Handles MSG_NET_REQUEST IPC messages from processes
//! - Performs HTTP fetch operations via async syscalls (routed through supervisor)
//! - Responds with MSG_NET_RESPONSE messages
//!
//! # Architecture
//!
//! Network operations are event-driven using push-based async network:
//!
//! ```text
//! Client Process (e.g. Identity Service)
//!        │
//!        │ IPC (MSG_NET_REQUEST)
//!        ▼
//! ┌─────────────────┐
//! │ Network Service │  ◄── This service
//! │   (Process)     │
//! └────────┬────────┘
//!          │
//!          │ SYS_NETWORK_FETCH syscall (returns request_id immediately)
//!          ▼
//! ┌─────────────────┐
//! │  Kernel/Axiom   │
//! └────────┬────────┘
//!          │
//!          │ HAL async network
//!          ▼
//! ┌─────────────────┐
//! │   Supervisor    │  ◄── Main thread
//! └────────┬────────┘
//!          │
//!          │ ZosNetwork.startFetch()
//!          ▼
//! ┌─────────────────┐
//! │  Browser fetch  │  ◄── Actual HTTP request
//! └────────┬────────┘
//!          │
//!          │ Promise resolves
//!          ▼
//! ┌─────────────────┐
//! │   Supervisor    │  ◄── onNetworkResult()
//! └────────┬────────┘
//!          │
//!          │ IPC (MSG_NET_RESULT)
//!          ▼
//! ┌─────────────────┐
//! │ Network Service │  ◄── Matches request_id, sends response to client
//! └─────────────────┘
//! ```
//!
//! # Protocol
//!
//! Processes communicate with NetworkService via IPC:
//!
//! - `MSG_NET_REQUEST (0x9000)`: HTTP request
//! - `MSG_NET_RESPONSE (0x9001)`: HTTP response
//! - `MSG_NET_RESULT (0x9002)`: Internal result from HAL

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use zos_apps::manifest::NETWORK_SERVICE_MANIFEST;
use zos_apps::syscall;
use zos_apps::{app_main, AppContext, AppError, AppManifest, ControlFlow, Message, ZeroApp};
use zos_process::net;

// =============================================================================
// Network Result Types (matches zos-ipc::net)
// =============================================================================

mod net_result {
    /// Request succeeded, response body follows
    pub const NET_OK: u8 = 0;
    /// Request failed with error
    #[allow(dead_code)]
    pub const NET_ERROR: u8 = 1;
}

// =============================================================================
// Pending Network Operations
// =============================================================================

/// Tracks pending network operations awaiting results
#[derive(Clone)]
struct PendingRequest {
    /// Client PID that made the request
    client_pid: u32,
    /// Original client request ID (from NetRequest)
    client_request_id: u32,
}

// =============================================================================
// NetworkService Application
// =============================================================================

/// Network Service - mediates HTTP requests
pub struct NetworkService {
    /// Whether we have registered with init
    registered: bool,
    /// Pending network operations: syscall_request_id -> pending context
    pending_ops: BTreeMap<u32, PendingRequest>,
    /// Next client request ID (for internal tracking)
    next_request_id: u32,
}

impl Default for NetworkService {
    fn default() -> Self {
        Self {
            registered: false,
            pending_ops: BTreeMap::new(),
            next_request_id: 1,
        }
    }
}

impl NetworkService {
    /// Handle MSG_NET_REQUEST - perform HTTP fetch
    fn handle_net_request(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        // Parse the request
        let request_json = &msg.data;

        syscall::debug(&format!(
            "NetworkService: Received request from PID {}, len={}",
            msg.from_pid,
            request_json.len()
        ));

        // Extract client request ID from the request if present
        // For simplicity, we generate our own tracking ID
        let client_request_id = self.next_request_id;
        self.next_request_id += 1;

        // Start async network fetch via syscall
        match syscall::network_fetch_async(request_json) {
            Ok(syscall_request_id) => {
                syscall::debug(&format!(
                    "NetworkService: network_fetch_async -> syscall_request_id={}",
                    syscall_request_id
                ));

                // Track this pending request
                self.pending_ops.insert(
                    syscall_request_id,
                    PendingRequest {
                        client_pid: msg.from_pid,
                        client_request_id,
                    },
                );

                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!(
                    "NetworkService: network_fetch_async failed: {}",
                    e
                ));
                // Send error response immediately
                self.send_error_response(
                    msg.from_pid,
                    client_request_id,
                    "Failed to start network operation",
                )
            }
        }
    }

    /// Handle MSG_NET_RESULT - async network operation completed
    fn handle_net_result(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        // Parse network result
        // Format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
        if msg.data.len() < 9 {
            syscall::debug("NetworkService: net result too short");
            return Ok(());
        }

        let request_id = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);
        let result_type = msg.data[4];
        let data_len =
            u32::from_le_bytes([msg.data[5], msg.data[6], msg.data[7], msg.data[8]]) as usize;
        let data = if data_len > 0 && msg.data.len() >= 9 + data_len {
            &msg.data[9..9 + data_len]
        } else {
            &[]
        };

        syscall::debug(&format!(
            "NetworkService: net result request_id={}, type={}, data_len={}",
            request_id, result_type, data_len
        ));

        // Look up pending operation
        let pending = match self.pending_ops.remove(&request_id) {
            Some(p) => p,
            None => {
                syscall::debug(&format!(
                    "NetworkService: unknown request_id {}",
                    request_id
                ));
                return Ok(());
            }
        };

        // Forward result to client
        if result_type == net_result::NET_OK {
            // Success - forward the response data
            self.send_response(pending.client_pid, pending.client_request_id, data)
        } else {
            // Error - parse error and forward
            let error_msg = if !data.is_empty() {
                String::from_utf8_lossy(data).to_string()
            } else {
                "Network error".into()
            };
            self.send_error_response(pending.client_pid, pending.client_request_id, &error_msg)
        }
    }

    /// Send successful response to client
    fn send_response(
        &self,
        to_pid: u32,
        request_id: u32,
        response_data: &[u8],
    ) -> Result<(), AppError> {
        // Build response message
        // Format: [request_id: u32, response_data: [u8]]
        let mut data = Vec::with_capacity(4 + response_data.len());
        data.extend_from_slice(&request_id.to_le_bytes());
        data.extend_from_slice(response_data);

        // Send via debug message for supervisor to route via IPC
        let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
        syscall::debug(&format!(
            "NET:RESPONSE:{}:{:08x}:{}",
            to_pid,
            net::MSG_NET_RESPONSE,
            hex
        ));

        Ok(())
    }

    /// Send error response to client
    fn send_error_response(
        &self,
        to_pid: u32,
        request_id: u32,
        error_msg: &str,
    ) -> Result<(), AppError> {
        // Build error response JSON
        let error_json = format!(
            r#"{{"result":{{"Err":{{"Other":"{}"}}}}}}"#,
            error_msg.replace('"', "\\\"")
        );
        let error_bytes = error_json.as_bytes();

        // Build response message
        let mut data = Vec::with_capacity(4 + error_bytes.len());
        data.extend_from_slice(&request_id.to_le_bytes());
        data.extend_from_slice(error_bytes);

        // Send via debug message for supervisor to route via IPC
        let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
        syscall::debug(&format!(
            "NET:RESPONSE:{}:{:08x}:{}",
            to_pid,
            net::MSG_NET_RESPONSE,
            hex
        ));

        Ok(())
    }
}

impl ZeroApp for NetworkService {
    fn manifest() -> &'static AppManifest {
        &NETWORK_SERVICE_MANIFEST
    }

    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError> {
        syscall::debug(&format!("NetworkService starting (PID {})", ctx.pid));

        // Register with init as "network" service
        let service_name = "network";
        let name_bytes = service_name.as_bytes();
        let mut data = Vec::with_capacity(1 + name_bytes.len() + 8);
        data.push(name_bytes.len() as u8);
        data.extend_from_slice(name_bytes);
        // Endpoint ID (placeholder)
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());

        let _ = syscall::send(
            syscall::INIT_ENDPOINT_SLOT,
            syscall::MSG_REGISTER_SERVICE,
            &data,
        );
        self.registered = true;

        syscall::debug("NetworkService: Registered with init");

        Ok(())
    }

    fn update(&mut self, _ctx: &AppContext) -> ControlFlow {
        ControlFlow::Yield
    }

    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError> {
        syscall::debug(&format!(
            "NetworkService: Received message tag 0x{:x} from PID {}",
            msg.tag, msg.from_pid
        ));

        match msg.tag {
            net::MSG_NET_REQUEST => self.handle_net_request(ctx, &msg),
            net::MSG_NET_RESULT => self.handle_net_result(ctx, &msg),
            _ => {
                syscall::debug(&format!(
                    "NetworkService: Unknown message tag 0x{:x}",
                    msg.tag
                ));
                Ok(())
            }
        }
    }

    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("NetworkService: shutting down");
    }
}

// Entry point
app_main!(NetworkService);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("NetworkService is meant to run as WASM in Zero OS");
}
