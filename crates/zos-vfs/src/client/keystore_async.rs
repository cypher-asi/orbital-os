//! Async Keystore Client for Event-Driven Services
//!
//! This module provides non-blocking Keystore IPC helpers for services that
//! communicate with the KeystoreService (PID 7) for cryptographic key storage.
//!
//! # Usage Pattern
//!
//! ```ignore
//! use zos_vfs::client::keystore_async;
//!
//! struct MyService {
//!     pending_keystore_ops: BTreeMap<u32, KeystorePendingOp>,
//!     next_keystore_request_id: u32,
//! }
//!
//! impl MyService {
//!     fn read_key(&mut self, key: &str) -> Result<(), KeystoreError> {
//!         let request_id = self.next_keystore_request_id;
//!         self.next_keystore_request_id += 1;
//!         
//!         keystore_async::send_read_request(key)?;
//!         self.pending_keystore_ops.insert(request_id, KeystorePendingOp::Read { key: key.into() });
//!         Ok(())
//!     }
//!
//!     fn on_message(&mut self, msg: Message) -> Result<(), Error> {
//!         if keystore_async::is_keystore_response(msg.tag) {
//!             self.handle_keystore_response(msg)
//!         } else {
//!             // Handle other messages
//!         }
//!     }
//! }
//! ```

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::core::VfsError;
use zos_ipc::keystore_svc;

/// Default capability slot for Keystore service endpoint.
/// This is assigned by init when the process starts (after VFS slot 3, VFS response slot 4).
pub const KEYSTORE_ENDPOINT_SLOT: u32 = 5;

// =============================================================================
// Keystore IPC Types (local definitions to avoid circular deps)
// =============================================================================

/// Read key request.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KeystoreReadRequest {
    /// Key path to read
    pub key: String,
}

/// Write key request.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KeystoreWriteRequest {
    /// Key path to write
    pub key: String,
    /// Value to store
    pub value: Vec<u8>,
}

/// Delete key request.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KeystoreDeleteRequest {
    /// Key path to delete
    pub key: String,
}

/// Check if key exists request.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KeystoreExistsRequest {
    /// Key path to check
    pub key: String,
}

/// List keys with prefix request.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KeystoreListRequest {
    /// Prefix to match
    pub prefix: String,
}

/// Keystore operation error.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum KeystoreError {
    /// Invalid key path
    InvalidKey(String),
    /// Invalid request format
    InvalidRequest(String),
    /// Key not found
    NotFound,
    /// Storage operation failed
    StorageError(String),
    /// Too many pending operations
    ResourceExhausted,
}

/// Read key response.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KeystoreReadResponse {
    /// Result containing key data or error
    pub result: Result<Vec<u8>, KeystoreError>,
}

/// Write key response.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KeystoreWriteResponse {
    /// Result of operation
    pub result: Result<(), KeystoreError>,
}

/// Delete key response.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KeystoreDeleteResponse {
    /// Result of operation
    pub result: Result<(), KeystoreError>,
}

/// Check if key exists response.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KeystoreExistsResponse {
    /// Result containing whether the key exists
    pub result: Result<bool, KeystoreError>,
}

/// List keys response.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KeystoreListResponse {
    /// Result containing matching keys
    pub result: Result<Vec<String>, KeystoreError>,
}

// =============================================================================
// Keystore Request Senders (Non-blocking)
// =============================================================================

/// Send a keystore read request (non-blocking).
///
/// The response will arrive as a message with tag `MSG_KEYSTORE_READ_RESPONSE`.
pub fn send_read_request(key: &str) -> Result<(), VfsError> {
    let request = KeystoreReadRequest {
        key: String::from(key),
    };
    send_keystore_request(keystore_svc::MSG_KEYSTORE_READ, &request)
}

/// Send a keystore write request (non-blocking).
///
/// The response will arrive as a message with tag `MSG_KEYSTORE_WRITE_RESPONSE`.
pub fn send_write_request(key: &str, value: &[u8]) -> Result<(), VfsError> {
    let request = KeystoreWriteRequest {
        key: String::from(key),
        value: value.to_vec(),
    };
    send_keystore_request(keystore_svc::MSG_KEYSTORE_WRITE, &request)
}

/// Send a keystore delete request (non-blocking).
///
/// The response will arrive as a message with tag `MSG_KEYSTORE_DELETE_RESPONSE`.
pub fn send_delete_request(key: &str) -> Result<(), VfsError> {
    let request = KeystoreDeleteRequest {
        key: String::from(key),
    };
    send_keystore_request(keystore_svc::MSG_KEYSTORE_DELETE, &request)
}

/// Send a keystore exists request (non-blocking).
///
/// The response will arrive as a message with tag `MSG_KEYSTORE_EXISTS_RESPONSE`.
pub fn send_exists_request(key: &str) -> Result<(), VfsError> {
    let request = KeystoreExistsRequest {
        key: String::from(key),
    };
    send_keystore_request(keystore_svc::MSG_KEYSTORE_EXISTS, &request)
}

/// Send a keystore list request (non-blocking).
///
/// The response will arrive as a message with tag `MSG_KEYSTORE_LIST_RESPONSE`.
pub fn send_list_request(prefix: &str) -> Result<(), VfsError> {
    let request = KeystoreListRequest {
        prefix: String::from(prefix),
    };
    send_keystore_request(keystore_svc::MSG_KEYSTORE_LIST, &request)
}

// =============================================================================
// Keystore Response Helpers
// =============================================================================

/// Check if a message tag is a keystore response.
pub fn is_keystore_response(tag: u32) -> bool {
    matches!(
        tag,
        keystore_svc::MSG_KEYSTORE_READ_RESPONSE
            | keystore_svc::MSG_KEYSTORE_WRITE_RESPONSE
            | keystore_svc::MSG_KEYSTORE_DELETE_RESPONSE
            | keystore_svc::MSG_KEYSTORE_EXISTS_RESPONSE
            | keystore_svc::MSG_KEYSTORE_LIST_RESPONSE
    )
}

/// Parse a keystore read response.
///
/// Returns `Ok(data)` on success, `Err(error_message)` on failure.
pub fn parse_read_response(data: &[u8]) -> Result<Vec<u8>, String> {
    match serde_json::from_slice::<KeystoreReadResponse>(data) {
        Ok(response) => response.result.map_err(|e| format!("{:?}", e)),
        Err(e) => Err(format!("Parse error: {}", e)),
    }
}

/// Parse a keystore write response.
///
/// Returns `Ok(())` on success, `Err(error_message)` on failure.
pub fn parse_write_response(data: &[u8]) -> Result<(), String> {
    match serde_json::from_slice::<KeystoreWriteResponse>(data) {
        Ok(response) => response.result.map_err(|e| format!("{:?}", e)),
        Err(e) => Err(format!("Parse error: {}", e)),
    }
}

/// Parse a keystore delete response.
///
/// Returns `Ok(())` on success, `Err(error_message)` on failure.
pub fn parse_delete_response(data: &[u8]) -> Result<(), String> {
    match serde_json::from_slice::<KeystoreDeleteResponse>(data) {
        Ok(response) => response.result.map_err(|e| format!("{:?}", e)),
        Err(e) => Err(format!("Parse error: {}", e)),
    }
}

/// Parse a keystore exists response.
///
/// Returns `Ok(exists)` where `exists` is true if key exists.
pub fn parse_exists_response(data: &[u8]) -> Result<bool, String> {
    match serde_json::from_slice::<KeystoreExistsResponse>(data) {
        Ok(response) => response.result.map_err(|e| format!("{:?}", e)),
        Err(e) => Err(format!("Parse error: {}", e)),
    }
}

/// Parse a keystore list response.
///
/// Returns `Ok(keys)` on success, `Err(error_message)` on failure.
pub fn parse_list_response(data: &[u8]) -> Result<Vec<String>, String> {
    match serde_json::from_slice::<KeystoreListResponse>(data) {
        Ok(response) => response.result.map_err(|e| format!("{:?}", e)),
        Err(e) => Err(format!("Parse error: {}", e)),
    }
}

// =============================================================================
// Internal Helpers
// =============================================================================

/// Send a keystore request via IPC (non-blocking).
#[cfg(target_arch = "wasm32")]
fn send_keystore_request<T: serde::Serialize>(tag: u32, request: &T) -> Result<(), VfsError> {
    let data = serde_json::to_vec(request)
        .map_err(|e| VfsError::StorageError(format!("Serialize error: {}", e)))?;

    zos_process::send(KEYSTORE_ENDPOINT_SLOT, tag, &data)
        .map_err(|e| VfsError::StorageError(format!("Send error: {}", e)))
}

#[cfg(not(target_arch = "wasm32"))]
fn send_keystore_request<T: serde::Serialize>(_tag: u32, _request: &T) -> Result<(), VfsError> {
    // No-op outside WASM - allows tests to compile
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_keystore_response() {
        assert!(is_keystore_response(keystore_svc::MSG_KEYSTORE_READ_RESPONSE));
        assert!(is_keystore_response(keystore_svc::MSG_KEYSTORE_WRITE_RESPONSE));
        assert!(is_keystore_response(keystore_svc::MSG_KEYSTORE_EXISTS_RESPONSE));

        // Not a keystore response
        assert!(!is_keystore_response(keystore_svc::MSG_KEYSTORE_READ)); // Request, not response
        assert!(!is_keystore_response(0x8000)); // VFS message
    }
}
