//! Storage System Integration
//!
//! This module handles the integration between IndexedDB storage (JavaScript)
//! and the WASM processes. The supervisor receives notifications from JavaScript
//! when IndexedDB operations complete and delivers the results to the requesting
//! processes via IPC through Init.

use wasm_bindgen::prelude::*;
use zos_hal::HAL;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

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
            None => return,
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
            None => return,
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
            None => return,
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
            None => return,
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
            None => return,
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
        // Use slot 1 (INPUT_ENDPOINT_SLOT) for storage results to services.
        // Services like IdentityService and VfsService use storage syscalls and
        // receive all IPC on slot 1 via the app_main! framework.
        // Note: VFS_RESPONSE_SLOT (4) is for VFS *client* responses, not storage syscalls.
        const INPUT_ENDPOINT_SLOT: u32 = 1;

        self.route_ipc_via_init(
            pid,
            INPUT_ENDPOINT_SLOT,
            storage_const::MSG_STORAGE_RESULT,
            payload,
        );
    }
}
