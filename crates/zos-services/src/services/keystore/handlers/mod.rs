//! Keystore Service request handlers
//!
//! # Safety Properties
//!
//! - **Success**: storage operation completed, response sent
//! - **Acceptable partial failure**: None (operations are atomic)
//! - **Forbidden**: Returning success before storage commit

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use zos_apps::syscall;
use zos_apps::{AppContext, AppError, Message};
use zos_process::keystore_result;
use zos_ipc::keystore_svc;

use super::types::{
    KeystoreDeleteRequest, KeystoreDeleteResponse, KeystoreError, KeystoreExistsRequest,
    KeystoreExistsResponse, KeystoreListRequest, KeystoreListResponse, KeystoreReadRequest,
    KeystoreReadResponse, KeystoreWriteRequest, KeystoreWriteResponse,
};
use super::{
    validate_key, result_type_name, ClientContext, KeystoreService, PendingOp, MAX_CONTENT_SIZE,
};

impl KeystoreService {
    // =========================================================================
    // Request handlers (start async operations)
    // =========================================================================

    /// Handle MSG_KEYSTORE_READ - read key data
    pub fn handle_read(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: KeystoreReadRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = KeystoreReadResponse {
                    result: Err(KeystoreError::InvalidRequest(format!(
                        "Failed to parse request: {}",
                        e
                    ))),
                };
                return self.send_response_via_debug(
                    msg.from_pid,
                    keystore_svc::MSG_KEYSTORE_READ_RESPONSE,
                    &response,
                );
            }
        };

        // Validate key
        if let Err(error) = validate_key(&request.key) {
            let response = KeystoreReadResponse {
                result: Err(error),
            };
            return self.send_response_via_debug(
                msg.from_pid,
                keystore_svc::MSG_KEYSTORE_READ_RESPONSE,
                &response,
            );
        }

        syscall::debug(&format!("KeystoreService: read {}", request.key));

        let client_ctx = ClientContext::from_message(msg);
        let key = request.key.clone();

        self.start_keystore_read(
            &request.key,
            PendingOp::Read {
                ctx: client_ctx,
                key,
            },
        )
    }

    /// Handle MSG_KEYSTORE_WRITE - write key data
    pub fn handle_write(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: KeystoreWriteRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = KeystoreWriteResponse {
                    result: Err(KeystoreError::InvalidRequest(format!(
                        "Failed to parse request: {}",
                        e
                    ))),
                };
                return self.send_response_via_debug(
                    msg.from_pid,
                    keystore_svc::MSG_KEYSTORE_WRITE_RESPONSE,
                    &response,
                );
            }
        };

        // Validate key
        if let Err(error) = validate_key(&request.key) {
            let response = KeystoreWriteResponse {
                result: Err(error),
            };
            return self.send_response_via_debug(
                msg.from_pid,
                keystore_svc::MSG_KEYSTORE_WRITE_RESPONSE,
                &response,
            );
        }

        // Rule 11: Enforce content size limit
        if request.value.len() > MAX_CONTENT_SIZE {
            let response = KeystoreWriteResponse {
                result: Err(KeystoreError::InvalidRequest(format!(
                    "Value too large: {} bytes exceeds limit of {} bytes",
                    request.value.len(),
                    MAX_CONTENT_SIZE
                ))),
            };
            return self.send_response_via_debug(
                msg.from_pid,
                keystore_svc::MSG_KEYSTORE_WRITE_RESPONSE,
                &response,
            );
        }

        syscall::debug(&format!(
            "KeystoreService: write {} ({} bytes)",
            request.key,
            request.value.len()
        ));

        let client_ctx = ClientContext::from_message(msg);
        let key = request.key.clone();

        self.start_keystore_write(
            &request.key,
            &request.value,
            PendingOp::Write {
                ctx: client_ctx,
                key,
            },
        )
    }

    /// Handle MSG_KEYSTORE_DELETE - delete key
    pub fn handle_delete(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: KeystoreDeleteRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = KeystoreDeleteResponse {
                    result: Err(KeystoreError::InvalidRequest(format!(
                        "Failed to parse request: {}",
                        e
                    ))),
                };
                return self.send_response_via_debug(
                    msg.from_pid,
                    keystore_svc::MSG_KEYSTORE_DELETE_RESPONSE,
                    &response,
                );
            }
        };

        // Validate key
        if let Err(error) = validate_key(&request.key) {
            let response = KeystoreDeleteResponse {
                result: Err(error),
            };
            return self.send_response_via_debug(
                msg.from_pid,
                keystore_svc::MSG_KEYSTORE_DELETE_RESPONSE,
                &response,
            );
        }

        syscall::debug(&format!("KeystoreService: delete {}", request.key));

        let client_ctx = ClientContext::from_message(msg);
        let key = request.key.clone();

        self.start_keystore_delete(
            &request.key,
            PendingOp::Delete {
                ctx: client_ctx,
                key,
            },
        )
    }

    /// Handle MSG_KEYSTORE_EXISTS - check if key exists
    pub fn handle_exists(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: KeystoreExistsRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = KeystoreExistsResponse {
                    result: Err(KeystoreError::InvalidRequest(format!(
                        "Failed to parse request: {}",
                        e
                    ))),
                };
                return self.send_response_via_debug(
                    msg.from_pid,
                    keystore_svc::MSG_KEYSTORE_EXISTS_RESPONSE,
                    &response,
                );
            }
        };

        // Validate key
        if let Err(error) = validate_key(&request.key) {
            let response = KeystoreExistsResponse {
                result: Err(error),
            };
            return self.send_response_via_debug(
                msg.from_pid,
                keystore_svc::MSG_KEYSTORE_EXISTS_RESPONSE,
                &response,
            );
        }

        syscall::debug(&format!("KeystoreService: exists {}", request.key));

        let client_ctx = ClientContext::from_message(msg);
        let key = request.key.clone();

        self.start_keystore_exists(
            &request.key,
            PendingOp::Exists {
                ctx: client_ctx,
                key,
            },
        )
    }

    /// Handle MSG_KEYSTORE_LIST - list keys with prefix
    pub fn handle_list(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: KeystoreListRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = KeystoreListResponse {
                    result: Err(KeystoreError::InvalidRequest(format!(
                        "Failed to parse request: {}",
                        e
                    ))),
                };
                return self.send_response_via_debug(
                    msg.from_pid,
                    keystore_svc::MSG_KEYSTORE_LIST_RESPONSE,
                    &response,
                );
            }
        };

        // Validate prefix (must be under /keys/)
        if !request.prefix.starts_with("/keys/") && request.prefix != "/keys" {
            let response = KeystoreListResponse {
                result: Err(KeystoreError::InvalidKey(
                    "Prefix must start with /keys/".into(),
                )),
            };
            return self.send_response_via_debug(
                msg.from_pid,
                keystore_svc::MSG_KEYSTORE_LIST_RESPONSE,
                &response,
            );
        }

        syscall::debug(&format!("KeystoreService: list {}", request.prefix));

        let client_ctx = ClientContext::from_message(msg);
        let prefix = request.prefix.clone();

        self.start_keystore_list(
            &request.prefix,
            PendingOp::List {
                ctx: client_ctx,
                prefix,
            },
        )
    }

    // =========================================================================
    // Result handlers
    // =========================================================================

    /// Handle read operation result
    pub fn handle_read_result(
        &self,
        ctx: &ClientContext,
        key: &str,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        let response = match result_type {
            keystore_result::READ_OK => KeystoreReadResponse {
                result: Ok(data.to_vec()),
            },
            keystore_result::NOT_FOUND => KeystoreReadResponse {
                result: Err(KeystoreError::NotFound),
            },
            _ => {
                syscall::debug(&format!(
                    "KeystoreService: read {} failed with unexpected result: {} ({})",
                    key,
                    result_type,
                    result_type_name(result_type)
                ));
                KeystoreReadResponse {
                    result: Err(KeystoreError::StorageError(format!(
                        "Read failed: {} ({})",
                        result_type,
                        result_type_name(result_type)
                    ))),
                }
            }
        };
        self.send_response(ctx, keystore_svc::MSG_KEYSTORE_READ_RESPONSE, &response)
    }

    /// Handle write operation result
    pub fn handle_write_result(
        &self,
        ctx: &ClientContext,
        key: &str,
        result_type: u8,
    ) -> Result<(), AppError> {
        let response = match result_type {
            keystore_result::WRITE_OK => {
                syscall::debug(&format!("KeystoreService: write {} completed", key));
                KeystoreWriteResponse { result: Ok(()) }
            }
            _ => {
                syscall::debug(&format!(
                    "KeystoreService: write {} failed with unexpected result: {} ({})",
                    key,
                    result_type,
                    result_type_name(result_type)
                ));
                KeystoreWriteResponse {
                    result: Err(KeystoreError::StorageError(format!(
                        "Write failed: {} ({})",
                        result_type,
                        result_type_name(result_type)
                    ))),
                }
            }
        };
        self.send_response(ctx, keystore_svc::MSG_KEYSTORE_WRITE_RESPONSE, &response)
    }

    /// Handle delete operation result
    pub fn handle_delete_result(
        &self,
        ctx: &ClientContext,
        key: &str,
        result_type: u8,
    ) -> Result<(), AppError> {
        let response = match result_type {
            keystore_result::WRITE_OK => {
                syscall::debug(&format!("KeystoreService: delete {} completed", key));
                KeystoreDeleteResponse { result: Ok(()) }
            }
            keystore_result::NOT_FOUND => {
                // Delete of non-existent key is still success
                syscall::debug(&format!(
                    "KeystoreService: delete {} - key not found (OK)",
                    key
                ));
                KeystoreDeleteResponse { result: Ok(()) }
            }
            _ => {
                syscall::debug(&format!(
                    "KeystoreService: delete {} failed with unexpected result: {} ({})",
                    key,
                    result_type,
                    result_type_name(result_type)
                ));
                KeystoreDeleteResponse {
                    result: Err(KeystoreError::StorageError(format!(
                        "Delete failed: {} ({})",
                        result_type,
                        result_type_name(result_type)
                    ))),
                }
            }
        };
        self.send_response(ctx, keystore_svc::MSG_KEYSTORE_DELETE_RESPONSE, &response)
    }

    /// Handle exists operation result
    pub fn handle_exists_result(
        &self,
        ctx: &ClientContext,
        key: &str,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        let response = match result_type {
            keystore_result::EXISTS_OK => {
                let exists = !data.is_empty() && data[0] == 1;
                syscall::debug(&format!(
                    "KeystoreService: exists {} = {}",
                    key, exists
                ));
                KeystoreExistsResponse {
                    result: Ok(exists),
                }
            }
            keystore_result::NOT_FOUND => KeystoreExistsResponse {
                result: Ok(false),
            },
            _ => {
                syscall::debug(&format!(
                    "KeystoreService: exists {} failed with unexpected result: {} ({})",
                    key,
                    result_type,
                    result_type_name(result_type)
                ));
                KeystoreExistsResponse {
                    result: Err(KeystoreError::StorageError(format!(
                        "Exists check failed: {} ({})",
                        result_type,
                        result_type_name(result_type)
                    ))),
                }
            }
        };
        self.send_response(ctx, keystore_svc::MSG_KEYSTORE_EXISTS_RESPONSE, &response)
    }

    /// Handle list operation result
    pub fn handle_list_result(
        &self,
        ctx: &ClientContext,
        prefix: &str,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        let response = match result_type {
            keystore_result::LIST_OK => {
                // Data is JSON array of keys
                match serde_json::from_slice::<Vec<String>>(data) {
                    Ok(keys) => {
                        syscall::debug(&format!(
                            "KeystoreService: list {} returned {} keys",
                            prefix,
                            keys.len()
                        ));
                        KeystoreListResponse { result: Ok(keys) }
                    }
                    Err(e) => {
                        syscall::debug(&format!(
                            "KeystoreService: list {} failed to parse keys: {}",
                            prefix, e
                        ));
                        KeystoreListResponse {
                            result: Err(KeystoreError::StorageError(format!(
                                "Failed to parse key list: {}",
                                e
                            ))),
                        }
                    }
                }
            }
            keystore_result::NOT_FOUND => {
                // No keys found with this prefix
                KeystoreListResponse {
                    result: Ok(Vec::new()),
                }
            }
            _ => {
                syscall::debug(&format!(
                    "KeystoreService: list {} failed with unexpected result: {} ({})",
                    prefix,
                    result_type,
                    result_type_name(result_type)
                ));
                KeystoreListResponse {
                    result: Err(KeystoreError::StorageError(format!(
                        "List failed: {} ({})",
                        result_type,
                        result_type_name(result_type)
                    ))),
                }
            }
        };
        self.send_response(ctx, keystore_svc::MSG_KEYSTORE_LIST_RESPONSE, &response)
    }
}
