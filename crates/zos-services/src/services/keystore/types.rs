//! Keystore IPC request/response types.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

// ============================================================================
// Error Types
// ============================================================================

/// Keystore operation error.
#[derive(Clone, Debug, Serialize, Deserialize)]
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

// ============================================================================
// Request Types
// ============================================================================

/// Read key request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeystoreReadRequest {
    /// Key path to read (e.g., "/keys/123/identity/public_keys.json")
    pub key: String,
}

/// Write key request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeystoreWriteRequest {
    /// Key path to write
    pub key: String,
    /// Value to store
    pub value: Vec<u8>,
}

/// Delete key request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeystoreDeleteRequest {
    /// Key path to delete
    pub key: String,
}

/// Check if key exists request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeystoreExistsRequest {
    /// Key path to check
    pub key: String,
}

/// List keys with prefix request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeystoreListRequest {
    /// Prefix to match (e.g., "/keys/123/identity/machine")
    pub prefix: String,
}

// ============================================================================
// Response Types
// ============================================================================

/// Read key response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeystoreReadResponse {
    /// Result containing key data or error
    pub result: Result<Vec<u8>, KeystoreError>,
}

/// Write key response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeystoreWriteResponse {
    /// Result of operation
    pub result: Result<(), KeystoreError>,
}

/// Delete key response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeystoreDeleteResponse {
    /// Result of operation
    pub result: Result<(), KeystoreError>,
}

/// Check if key exists response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeystoreExistsResponse {
    /// Result containing whether the key exists
    pub result: Result<bool, KeystoreError>,
}

/// List keys response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeystoreListResponse {
    /// Result containing matching keys
    pub result: Result<Vec<String>, KeystoreError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = KeystoreReadRequest {
            key: String::from("/keys/123/identity/public_keys.json"),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: KeystoreReadRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.key, req.key);
    }

    #[test]
    fn test_response_serialization() {
        let resp = KeystoreReadResponse {
            result: Ok(vec![1, 2, 3, 4]),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: KeystoreReadResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.result.unwrap(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_error_serialization() {
        let resp = KeystoreReadResponse {
            result: Err(KeystoreError::NotFound),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: KeystoreReadResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed.result, Err(KeystoreError::NotFound)));
    }
}
