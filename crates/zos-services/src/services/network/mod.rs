//! Network Service (PID 6)
//!
//! The Network Service mediates HTTP requests for Zero OS. It:
//! - Handles MSG_NET_REQUEST IPC messages from processes
//! - Performs HTTP fetch operations via async syscalls (routed through supervisor)
//! - Responds with MSG_NET_RESPONSE messages
//!
//! # Safety Invariants
//!
//! **Success means:**
//! - HTTP request completed (success or HTTP error) AND response delivered to client
//! - Request-response correlation maintained via syscall_request_id
//!
//! **Acceptable partial failure:**
//! - Network timeout → error response to client
//! - HTTP error status → forwarded to client as-is
//!
//! **Forbidden:**
//! - Allowing unauthorized processes to make network requests
//! - Unbounded pending operations (DoS vector)
//! - Orphan pending ops (client response never sent)
//! - Mismatched request-response correlation
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

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::manifests::NETWORK_MANIFEST;
use zos_apps::syscall;
use zos_apps::{AppContext, AppError, AppManifest, ControlFlow, Message, ZeroApp};
use zos_network::result as net_result;
use zos_process::net;

// =============================================================================
// Permission & Limit Constants
// =============================================================================

/// Maximum number of pending network operations (DoS protection per Rule 11)
const MAX_PENDING_OPS: usize = 64;

/// System service PIDs that are always allowed network access.
/// - PID 0: Supervisor
/// - PID 1: Init
/// - PID 7: Identity Service (needs network for auth flows)
const TRUSTED_PIDS_FOR_NETWORK: &[u32] = &[0, 1, 7];

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
    /// Check if caller is authorized to make network requests (Rule 4: fail-closed).
    fn check_network_permission(&self, from_pid: u32) -> bool {
        let allowed = TRUSTED_PIDS_FOR_NETWORK.contains(&from_pid);
        if !allowed {
            syscall::debug(&format!(
                "NetworkService: SECURITY - Network request denied for PID {} (not in trusted list)",
                from_pid
            ));
        }
        allowed
    }

    /// Check and enforce pending operation limits (DoS protection per Rule 11).
    /// Returns true if a new operation can be accepted.
    fn check_pending_limit(&self) -> bool {
        if self.pending_ops.len() >= MAX_PENDING_OPS {
            syscall::debug(&format!(
                "NetworkService: Pending operation limit reached ({}/{})",
                self.pending_ops.len(),
                MAX_PENDING_OPS
            ));
            false
        } else {
            true
        }
    }

    /// Handle MSG_NET_REQUEST - perform HTTP fetch
    fn handle_net_request(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        // Parse the request
        let request_json = &msg.data;

        syscall::debug(&format!(
            "NetworkService: Received request from PID {}, len={}",
            msg.from_pid,
            request_json.len()
        ));

        // Extract client request ID early for error responses
        let client_request_id = self.next_request_id;
        self.next_request_id += 1;

        // Permission check (Rule 4: fail-closed)
        if !self.check_network_permission(msg.from_pid) {
            return self.send_error_response(
                msg.from_pid,
                client_request_id,
                "Permission denied: MSG_NET_REQUEST requires network capability",
            );
        }

        // DoS protection: check pending operation limit (Rule 11)
        if !self.check_pending_limit() {
            return self.send_error_response(
                msg.from_pid,
                client_request_id,
                "Service busy: pending network operation limit reached",
            );
        }

        // Start async network fetch via syscall
        match syscall::network_fetch_async(request_json) {
            Ok(syscall_request_id) => {
                let syscall_request_id = syscall_request_id as u32;
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
                    "NetworkService: network_fetch_async syscall failed: {}",
                    e
                ));
                // Send error response with context (Rule 9)
                self.send_error_response(
                    msg.from_pid,
                    client_request_id,
                    &format!("Network syscall failed: SYS_NETWORK_FETCH returned {}", e),
                )
            }
        }
    }

    /// Handle MSG_NET_RESULT - async network operation completed
    fn handle_net_result(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        // Parse network result
        // Format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
        if msg.data.len() < 9 {
            syscall::debug(&format!(
                "NetworkService: MSG_NET_RESULT payload too short (got {} bytes, need 9)",
                msg.data.len()
            ));
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
        &NETWORK_MANIFEST
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

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Permission check tests (Rule 4: fail-closed)
    // -------------------------------------------------------------------------

    #[test]
    fn test_network_permission_trusted_pids() {
        let service = NetworkService::default();
        // Trusted PIDs should be allowed
        for &pid in TRUSTED_PIDS_FOR_NETWORK {
            assert!(
                service.check_network_permission(pid),
                "PID {} should be trusted for network",
                pid
            );
        }
    }

    #[test]
    fn test_network_permission_denies_untrusted() {
        let service = NetworkService::default();
        // Untrusted PIDs should be denied
        assert!(!service.check_network_permission(100));
        assert!(!service.check_network_permission(2)); // Permission service
        assert!(!service.check_network_permission(3)); // Desktop
        assert!(!service.check_network_permission(9999));
    }

    // -------------------------------------------------------------------------
    // DoS protection tests (Rule 11: pending-op limits)
    // -------------------------------------------------------------------------

    #[test]
    fn test_pending_limit_allows_under_max() {
        let service = NetworkService::default();
        assert!(service.check_pending_limit());
    }

    #[test]
    fn test_pending_limit_denies_at_max() {
        let mut service = NetworkService::default();
        // Fill up to the limit
        for i in 0..MAX_PENDING_OPS {
            service.pending_ops.insert(
                i as u32,
                PendingRequest {
                    client_pid: 1,
                    client_request_id: i as u32,
                },
            );
        }
        assert!(!service.check_pending_limit());
    }

    // -------------------------------------------------------------------------
    // Request ID allocation tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_request_id_increments() {
        let mut service = NetworkService::default();
        let id1 = service.next_request_id;
        service.next_request_id += 1;
        let id2 = service.next_request_id;
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }
}
