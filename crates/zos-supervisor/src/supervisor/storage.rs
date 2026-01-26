//! Storage System Integration
//!
//! This module handles the integration between IndexedDB storage (JavaScript)
//! and the WASM processes. The supervisor receives notifications from JavaScript
//! when IndexedDB operations complete and delivers the results to the requesting
//! processes via IPC through Init.
//!
//! # Safety Invariants
//!
//! ## Success Criteria
//! - Storage result delivered to requesting process via Init-routed IPC
//! - Request ID correctly correlated with original requesting PID
//! - Payload format matches MSG_STORAGE_RESULT specification
//!
//! ## Acceptable Partial Failures
//! - Unknown request_id: Logged as error, no result delivered (orphaned response)
//! - Process terminated before result: Logged, IPC delivery may fail gracefully
//! - Init not available: Logged, result delivery fails but system continues
//!
//! ## Forbidden States
//! - Result delivered to wrong PID (request_id correlation must be exact)
//! - Silent failures without logging (all failures must be logged)
//! - Payload corruption (data must match what JavaScript provided)

use zos_hal::HAL;

use crate::constants::SERVICE_INPUT_SLOT;
use crate::util::log;

// =============================================================================
// Storage Constants (matching zos-process::storage_result)
// =============================================================================

/// Storage result types for MSG_STORAGE_RESULT IPC
pub(super) mod storage_const {
    // Storage result type constants from zos-ipc
    pub const STORAGE_READ_OK: u8 = zos_ipc::storage::result::READ_OK;
    pub const STORAGE_WRITE_OK: u8 = zos_ipc::storage::result::WRITE_OK;
    pub const STORAGE_NOT_FOUND: u8 = zos_ipc::storage::result::NOT_FOUND;
    pub const STORAGE_ERROR: u8 = zos_ipc::storage::result::ERROR;
    pub const STORAGE_LIST_OK: u8 = zos_ipc::storage::result::LIST_OK;
    pub const STORAGE_EXISTS_OK: u8 = zos_ipc::storage::result::EXISTS_OK;

    /// MSG_STORAGE_RESULT tag from zos-ipc (the single source of truth)
    pub const MSG_STORAGE_RESULT: u32 = zos_ipc::storage::MSG_STORAGE_RESULT;
}

impl super::Supervisor {
    /// Internal handler for storage read complete.
    pub(super) fn notify_storage_read_complete_internal(&mut self, request_id: u32, data: &[u8]) {
        log(&format!(
            "[supervisor] notify_storage_read_complete: request_id={}, len={}",
            request_id,
            data.len()
        ));

        // Look up which PID requested this
        let pid = match self.system.hal().take_storage_request_pid(request_id) {
            Some(p) => p,
            None => {
                log(&format!(
                    "[supervisor] Unknown storage request_id: {}",
                    request_id
                ));
                return;
            }
        };

        // Build MSG_STORAGE_RESULT payload
        // Format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
        let mut payload = Vec::with_capacity(9 + data.len());
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_READ_OK);
        payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
        payload.extend_from_slice(data);

        // Deliver to requesting process via Init
        self.deliver_storage_result(pid, &payload);
    }

    /// Internal handler for storage not found.
    pub(super) fn notify_storage_not_found_internal(&mut self, request_id: u32) {
        log(&format!(
            "[supervisor] notify_storage_not_found: request_id={}",
            request_id
        ));

        let pid = match self.system.hal().take_storage_request_pid(request_id) {
            Some(p) => p,
            None => {
                log(&format!(
                    "[supervisor] ERROR: Unknown storage request_id {} in not_found handler (orphaned response)",
                    request_id
                ));
                return;
            }
        };

        // Build MSG_STORAGE_RESULT payload for NOT_FOUND
        let mut payload = Vec::with_capacity(9);
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_NOT_FOUND);
        payload.extend_from_slice(&0u32.to_le_bytes());

        self.deliver_storage_result(pid, &payload);
    }

    /// Internal handler for storage write complete.
    pub(super) fn notify_storage_write_complete_internal(&mut self, request_id: u32) {
        log(&format!(
            "[supervisor] notify_storage_write_complete: request_id={}",
            request_id
        ));

        let pid = match self.system.hal().take_storage_request_pid(request_id) {
            Some(p) => p,
            None => {
                log(&format!(
                    "[supervisor] ERROR: Unknown storage request_id {} in write_complete handler (orphaned response)",
                    request_id
                ));
                return;
            }
        };

        // Build MSG_STORAGE_RESULT payload for WRITE_OK
        let mut payload = Vec::with_capacity(9);
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_WRITE_OK);
        payload.extend_from_slice(&0u32.to_le_bytes());

        self.deliver_storage_result(pid, &payload);
    }

    /// Internal handler for storage list complete.
    pub(super) fn notify_storage_list_complete_internal(
        &mut self,
        request_id: u32,
        keys_json: &str,
    ) {
        log(&format!(
            "[supervisor] notify_storage_list_complete: request_id={}, len={}",
            request_id,
            keys_json.len()
        ));

        let pid = match self.system.hal().take_storage_request_pid(request_id) {
            Some(p) => p,
            None => {
                log(&format!(
                    "[supervisor] ERROR: Unknown storage request_id {} in list_complete handler (orphaned response)",
                    request_id
                ));
                return;
            }
        };

        let data = keys_json.as_bytes();
        let mut payload = Vec::with_capacity(9 + data.len());
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_LIST_OK);
        payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
        payload.extend_from_slice(data);

        self.deliver_storage_result(pid, &payload);
    }

    /// Internal handler for storage exists complete.
    pub(super) fn notify_storage_exists_complete_internal(
        &mut self,
        request_id: u32,
        exists: bool,
    ) {
        log(&format!(
            "[supervisor] notify_storage_exists_complete: request_id={}, exists={}",
            request_id, exists
        ));

        let pid = match self.system.hal().take_storage_request_pid(request_id) {
            Some(p) => p,
            None => {
                log(&format!(
                    "[supervisor] ERROR: Unknown storage request_id {} in exists_complete handler (orphaned response)",
                    request_id
                ));
                return;
            }
        };

        let mut payload = Vec::with_capacity(10);
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_EXISTS_OK);
        payload.extend_from_slice(&1u32.to_le_bytes()); // data_len = 1
        payload.push(if exists { 1 } else { 0 });

        self.deliver_storage_result(pid, &payload);
    }

    /// Internal handler for storage error.
    pub(super) fn notify_storage_error_internal(&mut self, request_id: u32, error: &str) {
        log(&format!(
            "[supervisor] notify_storage_error: request_id={}, error={}",
            request_id, error
        ));

        let pid = match self.system.hal().take_storage_request_pid(request_id) {
            Some(p) => p,
            None => {
                log(&format!(
                    "[supervisor] ERROR: Unknown storage request_id {} in error handler (orphaned response, error was: {})",
                    request_id, error
                ));
                return;
            }
        };

        let error_bytes = error.as_bytes();
        let mut payload = Vec::with_capacity(9 + error_bytes.len());
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_ERROR);
        payload.extend_from_slice(&(error_bytes.len() as u32).to_le_bytes());
        payload.extend_from_slice(error_bytes);

        self.deliver_storage_result(pid, &payload);
    }

    /// Deliver a storage result to a process via IPC through Init.
    pub(super) fn deliver_storage_result(&mut self, pid: u64, payload: &[u8]) {
        // Route through Init for capability-checked delivery
        // Use SERVICE_INPUT_SLOT for storage results to services.
        // Services like IdentityService and VfsService use storage syscalls and
        // receive all IPC on slot 1 via the app_main! framework.
        // Note: VFS_RESPONSE_SLOT (4) is for VFS *client* responses, not storage syscalls.
        self.route_ipc_via_init(
            pid,
            SERVICE_INPUT_SLOT,
            storage_const::MSG_STORAGE_RESULT,
            payload,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::storage_const;

    // =========================================================================
    // Payload Building Helpers (test-only)
    // =========================================================================

    /// Build a storage read result payload.
    ///
    /// Format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
    fn build_storage_read_payload(request_id: u32, data: &[u8]) -> Vec<u8> {
        let mut payload = Vec::with_capacity(9 + data.len());
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_READ_OK);
        payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
        payload.extend_from_slice(data);
        payload
    }

    /// Build a storage write result payload.
    ///
    /// Format: [request_id: u32, result_type: u8, data_len: u32]
    fn build_storage_write_payload(request_id: u32) -> Vec<u8> {
        let mut payload = Vec::with_capacity(9);
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_WRITE_OK);
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload
    }

    /// Build a storage not found result payload.
    ///
    /// Format: [request_id: u32, result_type: u8, data_len: u32]
    fn build_storage_not_found_payload(request_id: u32) -> Vec<u8> {
        let mut payload = Vec::with_capacity(9);
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_NOT_FOUND);
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload
    }

    /// Build a storage list result payload.
    ///
    /// Format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
    fn build_storage_list_payload(request_id: u32, keys_json: &str) -> Vec<u8> {
        let data = keys_json.as_bytes();
        let mut payload = Vec::with_capacity(9 + data.len());
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_LIST_OK);
        payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
        payload.extend_from_slice(data);
        payload
    }

    /// Build a storage exists result payload.
    ///
    /// Format: [request_id: u32, result_type: u8, data_len: u32, exists: u8]
    fn build_storage_exists_payload(request_id: u32, exists: bool) -> Vec<u8> {
        let mut payload = Vec::with_capacity(10);
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_EXISTS_OK);
        payload.extend_from_slice(&1u32.to_le_bytes()); // data_len = 1
        payload.push(if exists { 1 } else { 0 });
        payload
    }

    /// Build a storage error result payload.
    ///
    /// Format: [request_id: u32, result_type: u8, data_len: u32, error: [u8]]
    fn build_storage_error_payload(request_id: u32, error: &str) -> Vec<u8> {
        let error_bytes = error.as_bytes();
        let mut payload = Vec::with_capacity(9 + error_bytes.len());
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_ERROR);
        payload.extend_from_slice(&(error_bytes.len() as u32).to_le_bytes());
        payload.extend_from_slice(error_bytes);
        payload
    }

    /// Parse a storage result payload header.
    ///
    /// Returns (request_id, result_type, data_len) or None if invalid.
    fn parse_storage_result_header(payload: &[u8]) -> Option<(u32, u8, u32)> {
        if payload.len() < 9 {
            return None;
        }
        let request_id = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let result_type = payload[4];
        let data_len = u32::from_le_bytes([payload[5], payload[6], payload[7], payload[8]]);
        Some((request_id, result_type, data_len))
    }

    #[test]
    fn test_storage_read_payload_format() {
        let payload = build_storage_read_payload(42, b"hello world");
        
        // Header: 4 bytes request_id + 1 byte type + 4 bytes data_len = 9 bytes
        assert_eq!(payload.len(), 9 + 11); // 9 header + 11 data bytes
        
        let (req_id, result_type, data_len) = parse_storage_result_header(&payload).unwrap();
        assert_eq!(req_id, 42);
        assert_eq!(result_type, storage_const::STORAGE_READ_OK);
        assert_eq!(data_len, 11);
        assert_eq!(&payload[9..], b"hello world");
    }

    #[test]
    fn test_storage_write_payload_format() {
        let payload = build_storage_write_payload(123);
        
        assert_eq!(payload.len(), 9);
        
        let (req_id, result_type, data_len) = parse_storage_result_header(&payload).unwrap();
        assert_eq!(req_id, 123);
        assert_eq!(result_type, storage_const::STORAGE_WRITE_OK);
        assert_eq!(data_len, 0);
    }

    #[test]
    fn test_storage_not_found_payload_format() {
        let payload = build_storage_not_found_payload(999);
        
        assert_eq!(payload.len(), 9);
        
        let (req_id, result_type, data_len) = parse_storage_result_header(&payload).unwrap();
        assert_eq!(req_id, 999);
        assert_eq!(result_type, storage_const::STORAGE_NOT_FOUND);
        assert_eq!(data_len, 0);
    }

    #[test]
    fn test_storage_list_payload_format() {
        let keys = r#"["key1","key2","key3"]"#;
        let payload = build_storage_list_payload(55, keys);
        
        let (req_id, result_type, data_len) = parse_storage_result_header(&payload).unwrap();
        assert_eq!(req_id, 55);
        assert_eq!(result_type, storage_const::STORAGE_LIST_OK);
        assert_eq!(data_len as usize, keys.len());
        assert_eq!(&payload[9..], keys.as_bytes());
    }

    #[test]
    fn test_storage_exists_payload_format() {
        let payload_true = build_storage_exists_payload(1, true);
        let payload_false = build_storage_exists_payload(2, false);
        
        assert_eq!(payload_true.len(), 10);
        assert_eq!(payload_false.len(), 10);
        
        let (req_id, result_type, data_len) = parse_storage_result_header(&payload_true).unwrap();
        assert_eq!(req_id, 1);
        assert_eq!(result_type, storage_const::STORAGE_EXISTS_OK);
        assert_eq!(data_len, 1);
        assert_eq!(payload_true[9], 1); // true = 1
        
        let (req_id, result_type, data_len) = parse_storage_result_header(&payload_false).unwrap();
        assert_eq!(req_id, 2);
        assert_eq!(result_type, storage_const::STORAGE_EXISTS_OK);
        assert_eq!(data_len, 1);
        assert_eq!(payload_false[9], 0); // false = 0
    }

    #[test]
    fn test_storage_error_payload_format() {
        let error = "File not found";
        let payload = build_storage_error_payload(77, error);
        
        let (req_id, result_type, data_len) = parse_storage_result_header(&payload).unwrap();
        assert_eq!(req_id, 77);
        assert_eq!(result_type, storage_const::STORAGE_ERROR);
        assert_eq!(data_len as usize, error.len());
        assert_eq!(&payload[9..], error.as_bytes());
    }

    #[test]
    fn test_parse_invalid_payload() {
        // Too short
        assert!(parse_storage_result_header(&[]).is_none());
        assert!(parse_storage_result_header(&[1, 2, 3, 4, 5, 6, 7, 8]).is_none());
        
        // Exactly minimum valid length
        assert!(parse_storage_result_header(&[0, 0, 0, 0, 0, 0, 0, 0, 0]).is_some());
    }

    #[test]
    fn test_storage_read_empty_data() {
        let payload = build_storage_read_payload(100, &[]);
        
        let (req_id, result_type, data_len) = parse_storage_result_header(&payload).unwrap();
        assert_eq!(req_id, 100);
        assert_eq!(result_type, storage_const::STORAGE_READ_OK);
        assert_eq!(data_len, 0);
        assert_eq!(payload.len(), 9); // Just the header
    }

    #[test]
    fn test_storage_constants_match_zos_ipc() {
        // Verify our constants match the canonical zos-ipc values
        assert_eq!(storage_const::STORAGE_READ_OK, zos_ipc::storage::result::READ_OK);
        assert_eq!(storage_const::STORAGE_WRITE_OK, zos_ipc::storage::result::WRITE_OK);
        assert_eq!(storage_const::STORAGE_NOT_FOUND, zos_ipc::storage::result::NOT_FOUND);
        assert_eq!(storage_const::STORAGE_ERROR, zos_ipc::storage::result::ERROR);
        assert_eq!(storage_const::STORAGE_LIST_OK, zos_ipc::storage::result::LIST_OK);
        assert_eq!(storage_const::STORAGE_EXISTS_OK, zos_ipc::storage::result::EXISTS_OK);
        assert_eq!(storage_const::MSG_STORAGE_RESULT, zos_ipc::storage::MSG_STORAGE_RESULT);
    }
}
