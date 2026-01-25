//! Identity Service core implementation
//!
//! Contains the IdentityService struct, storage/network syscall helpers,
//! and result dispatchers for async operations.

use alloc::collections::BTreeMap;
use alloc::format;

use zos_apps::identity::network_handlers::{self, NetworkHandlerResult};
use zos_apps::identity::pending::{PendingNetworkOp, PendingStorageOp};
use zos_apps::identity::response;
use zos_apps::identity::storage_handlers::{self, StorageHandlerResult};
use zos_apps::syscall;
use zos_apps::{AppError, Message};
use zos_identity::error::CredentialError;
use zos_identity::keystore::CredentialStore;
use zos_network::{HttpRequest, HttpResponse, NetworkError};
use zos_process::storage_result;

/// IdentityService - manages user cryptographic identities
#[derive(Default)]
pub struct IdentityService {
    /// Whether we have registered with init
    pub registered: bool,
    /// Pending storage operations: request_id -> operation context
    pub pending_ops: BTreeMap<u32, PendingStorageOp>,
    /// Pending network operations: request_id -> operation context
    pub pending_net_ops: BTreeMap<u32, PendingNetworkOp>,
}

impl IdentityService {
    // =========================================================================
    // Storage syscall helpers (async, non-blocking)
    // =========================================================================

    pub fn start_storage_read(&mut self, key: &str, pending_op: PendingStorageOp) -> Result<(), AppError> {
        match syscall::storage_read_async(key) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "IdentityService: storage_read_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("IdentityService: storage_read_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage read failed: {}", e)))
            }
        }
    }

    pub fn start_storage_write(&mut self, key: &str, value: &[u8], pending_op: PendingStorageOp) -> Result<(), AppError> {
        match syscall::storage_write_async(key, value) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "IdentityService: storage_write_async({}, {} bytes) -> request_id={}",
                    key, value.len(), request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("IdentityService: storage_write_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage write failed: {}", e)))
            }
        }
    }

    pub fn start_storage_delete(&mut self, key: &str, pending_op: PendingStorageOp) -> Result<(), AppError> {
        match syscall::storage_delete_async(key) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "IdentityService: storage_delete_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("IdentityService: storage_delete_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage delete failed: {}", e)))
            }
        }
    }

    pub fn start_storage_exists(&mut self, key: &str, pending_op: PendingStorageOp) -> Result<(), AppError> {
        match syscall::storage_exists_async(key) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "IdentityService: storage_exists_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("IdentityService: storage_exists_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage exists failed: {}", e)))
            }
        }
    }

    pub fn start_storage_list(&mut self, prefix: &str, pending_op: PendingStorageOp) -> Result<(), AppError> {
        match syscall::storage_list_async(prefix) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "IdentityService: storage_list_async({}) -> request_id={}",
                    prefix, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("IdentityService: storage_list_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage list failed: {}", e)))
            }
        }
    }

    // =========================================================================
    // Network syscall helpers (async, non-blocking)
    // =========================================================================

    pub fn start_network_fetch(&mut self, request: &HttpRequest, pending_op: PendingNetworkOp) -> Result<(), AppError> {
        let request_json = match serde_json::to_vec(request) {
            Ok(json) => json,
            Err(e) => {
                syscall::debug(&format!("IdentityService: Failed to serialize HTTP request: {}", e));
                return Err(AppError::IpcError(format!("Request serialization failed: {}", e)));
            }
        };

        match syscall::network_fetch_async(&request_json) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "IdentityService: network_fetch_async({} {}) -> request_id={}",
                    request.method.as_str(), request.url, request_id
                ));
                self.pending_net_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("IdentityService: network_fetch_async failed: {}", e));
                Err(AppError::IpcError(format!("Network fetch failed: {}", e)))
            }
        }
    }

    // =========================================================================
    // Storage result handler (dispatches to storage_handlers module)
    // =========================================================================

    pub fn handle_storage_result(&mut self, msg: &Message) -> Result<(), AppError> {
        if msg.data.len() < 9 {
            return Ok(());
        }

        let request_id = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);
        let result_type = msg.data[4];
        let data_len = u32::from_le_bytes([msg.data[5], msg.data[6], msg.data[7], msg.data[8]]) as usize;
        let data = if data_len > 0 && msg.data.len() >= 9 + data_len { &msg.data[9..9 + data_len] } else { &[] };

        let pending_op = match self.pending_ops.remove(&request_id) {
            Some(op) => op,
            None => return Ok(()),
        };

        self.dispatch_storage_result(pending_op, result_type, data)
    }

    fn dispatch_storage_result(&mut self, op: PendingStorageOp, result_type: u8, data: &[u8]) -> Result<(), AppError> {
        use crate::handlers::{credentials, keys, session};

        match op {
            PendingStorageOp::CheckKeyExists { client_pid, user_id, cap_slots } => {
                let exists = result_type == storage_result::EXISTS_OK && !data.is_empty() && data[0] == 1;
                keys::continue_generate_after_exists_check(self, client_pid, user_id, exists, cap_slots)
            }
            PendingStorageOp::WriteKeyStoreContent { client_pid, user_id, result, json_bytes, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_key_store_content(client_pid, user_id, result, json_bytes, cap_slots, result_type))
            }
            PendingStorageOp::WriteKeyStoreInode { client_pid, result, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_key_store_inode(client_pid, result, cap_slots, result_type))
            }
            PendingStorageOp::GetIdentityKey { client_pid, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_get_identity_key(client_pid, cap_slots, result_type, data))
            }
            PendingStorageOp::WriteRecoveredKeyStoreContent { client_pid, user_id, result, json_bytes, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_recovered_content(client_pid, user_id, result, json_bytes, cap_slots, result_type))
            }
            PendingStorageOp::WriteRecoveredKeyStoreInode { client_pid, result, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_recovered_inode(client_pid, result, cap_slots, result_type))
            }
            PendingStorageOp::CheckIdentityForMachine { client_pid, request, cap_slots } => {
                let exists = result_type == storage_result::EXISTS_OK && !data.is_empty() && data[0] == 1;
                keys::continue_create_machine_after_identity_check(self, client_pid, request, exists, cap_slots)
            }
            PendingStorageOp::WriteMachineKeyContent { client_pid, user_id, record, json_bytes, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_machine_key_content(client_pid, user_id, record, json_bytes, cap_slots, result_type))
            }
            PendingStorageOp::WriteMachineKeyInode { client_pid, record, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_machine_key_inode(client_pid, record, cap_slots, result_type))
            }
            PendingStorageOp::ListMachineKeys { client_pid, user_id, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_list_machine_keys(client_pid, user_id, cap_slots, result_type, data))
            }
            PendingStorageOp::ReadMachineKey { client_pid, user_id, remaining_paths, records, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_read_machine_key(client_pid, user_id, remaining_paths, records, cap_slots, result_type, data))
            }
            PendingStorageOp::DeleteMachineKey { client_pid, user_id, machine_id, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_delete_machine_key(client_pid, user_id, machine_id, cap_slots, result_type))
            }
            PendingStorageOp::DeleteMachineKeyInode { client_pid, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_delete_machine_key_inode(client_pid, cap_slots, result_type))
            }
            PendingStorageOp::ReadMachineForRotate { client_pid, user_id, machine_id, cap_slots } => {
                if result_type == storage_result::READ_OK {
                    keys::continue_rotate_after_read(self, client_pid, user_id, machine_id, data, cap_slots)
                } else {
                    response::send_rotate_machine_key_error(client_pid, &cap_slots, zos_identity::KeyError::MachineKeyNotFound)
                }
            }
            PendingStorageOp::WriteRotatedMachineKeyContent { client_pid, user_id, record, json_bytes, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_rotated_content(client_pid, user_id, record, json_bytes, cap_slots, result_type))
            }
            PendingStorageOp::WriteRotatedMachineKeyInode { client_pid, record, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_rotated_inode(client_pid, record, cap_slots, result_type))
            }
            PendingStorageOp::ReadSingleMachineKey { client_pid, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_read_single_machine_key(client_pid, cap_slots, result_type, data))
            }
            PendingStorageOp::ReadCredentialsForAttach { client_pid, user_id, email, cap_slots } => {
                let existing_store = if result_type == storage_result::READ_OK && !data.is_empty() {
                    serde_json::from_slice::<CredentialStore>(data).ok()
                } else { None };
                credentials::continue_attach_email_after_read(self, client_pid, user_id, email, existing_store, cap_slots)
            }
            PendingStorageOp::GetCredentials { client_pid, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_get_credentials(client_pid, cap_slots, result_type, data))
            }
            PendingStorageOp::ReadCredentialsForUnlink { client_pid, user_id, credential_type, cap_slots } => {
                if result_type == storage_result::READ_OK && !data.is_empty() {
                    credentials::continue_unlink_credential_after_read(self, client_pid, user_id, credential_type, data, cap_slots)
                } else {
                    response::send_unlink_credential_error(client_pid, &cap_slots, CredentialError::NotFound)
                }
            }
            PendingStorageOp::WriteUnlinkedCredentialContent { client_pid, user_id, json_bytes, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_unlinked_content(client_pid, user_id, json_bytes, cap_slots, result_type))
            }
            PendingStorageOp::WriteUnlinkedCredentialInode { client_pid, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_unlinked_inode(client_pid, cap_slots, result_type))
            }
            PendingStorageOp::WriteEmailCredentialContent { client_pid, user_id, json_bytes, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_email_cred_content(client_pid, user_id, json_bytes, cap_slots, result_type))
            }
            PendingStorageOp::WriteEmailCredentialInode { client_pid, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_email_cred_inode(client_pid, cap_slots, result_type))
            }
            PendingStorageOp::ReadMachineKeyForZidLogin { client_pid, user_id, zid_endpoint, cap_slots } => {
                match storage_handlers::handle_read_machine_for_zid_login(client_pid, user_id, zid_endpoint, cap_slots, result_type, data) {
                    Ok(storage_handlers::ZidLoginReadResult::PathList { paths, client_pid, user_id, zid_endpoint, cap_slots }) => {
                        session::continue_zid_login_after_list(self, client_pid, user_id, zid_endpoint, paths, cap_slots)
                    }
                    Ok(storage_handlers::ZidLoginReadResult::MachineKeyData { data, client_pid, user_id, zid_endpoint, cap_slots }) => {
                        session::continue_zid_login_after_read(self, client_pid, user_id, zid_endpoint, &data, cap_slots)
                    }
                    Err(result) => self.handle_storage_handler_result(*result),
                }
            }
            PendingStorageOp::WriteZidSessionContent { client_pid, user_id, tokens, json_bytes, cap_slots } => {
                session::continue_zid_login_after_write_content(self, client_pid, user_id, tokens, json_bytes, cap_slots, result_type)
            }
            PendingStorageOp::WriteZidSessionInode { client_pid, tokens, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_zid_session_inode(client_pid, tokens, cap_slots, result_type))
            }
            PendingStorageOp::ReadMachineKeyForZidEnroll { client_pid, user_id, zid_endpoint, cap_slots } => {
                match storage_handlers::handle_read_machine_for_zid_enroll(client_pid, user_id, zid_endpoint, cap_slots, result_type, data) {
                    Ok(storage_handlers::ZidEnrollReadResult::PathList { paths, client_pid, user_id, zid_endpoint, cap_slots }) => {
                        session::continue_zid_enroll_after_list(self, client_pid, user_id, zid_endpoint, paths, cap_slots)
                    }
                    Ok(storage_handlers::ZidEnrollReadResult::MachineKeyData { data, client_pid, user_id, zid_endpoint, cap_slots }) => {
                        session::continue_zid_enroll_after_read(self, client_pid, user_id, zid_endpoint, &data, cap_slots)
                    }
                    Err(result) => self.handle_storage_handler_result(*result),
                }
            }
            PendingStorageOp::WriteZidEnrollSessionContent { client_pid, user_id, tokens, json_bytes, cap_slots } => {
                if result_type == storage_result::WRITE_OK {
                    let session_path = format!("/home/{:032x}/.zos/identity/zid_session.json", user_id);
                    self.start_storage_write(&format!("inode:{}", session_path), &json_bytes, PendingStorageOp::WriteZidEnrollSessionInode { client_pid, tokens, cap_slots })
                } else {
                    response::send_zid_enroll_error(client_pid, &cap_slots, zos_identity::error::ZidError::EnrollmentFailed("Session write failed".into()))
                }
            }
            PendingStorageOp::WriteZidEnrollSessionInode { client_pid, tokens, cap_slots } => {
                self.handle_storage_handler_result(storage_handlers::handle_write_zid_enroll_session_inode(client_pid, tokens, cap_slots, result_type))
            }
        }
    }

    fn handle_storage_handler_result(&mut self, result: StorageHandlerResult) -> Result<(), AppError> {
        match result {
            StorageHandlerResult::Done(r) => r,
            StorageHandlerResult::ContinueWrite { key, value, next_op } => {
                self.start_storage_write(&key, &value, next_op)
            }
            StorageHandlerResult::ContinueRead { key, next_op } => {
                self.start_storage_read(&key, next_op)
            }
            StorageHandlerResult::ContinueDelete { key, next_op } => {
                self.start_storage_delete(&key, next_op)
            }
        }
    }

    // =========================================================================
    // Network result handler
    // =========================================================================

    pub fn handle_net_result(&mut self, msg: &Message) -> Result<(), AppError> {
        if msg.data.len() < 9 { return Ok(()); }

        let request_id = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);
        let result_type = msg.data[4];
        let data_len = u32::from_le_bytes([msg.data[5], msg.data[6], msg.data[7], msg.data[8]]) as usize;
        let data = if data_len > 0 && msg.data.len() >= 9 + data_len { &msg.data[9..9 + data_len] } else { &[] };

        let pending_op = match self.pending_net_ops.remove(&request_id) {
            Some(op) => op,
            None => return Ok(()),
        };

        let http_response: HttpResponse = if result_type == 0 && !data.is_empty() {
            serde_json::from_slice(data).unwrap_or_else(|_| HttpResponse::err(NetworkError::Other("Parse error".into())))
        } else {
            HttpResponse::err(NetworkError::Other("Network error".into()))
        };

        self.dispatch_network_result(pending_op, http_response)
    }

    fn dispatch_network_result(&mut self, op: PendingNetworkOp, http_response: HttpResponse) -> Result<(), AppError> {
        use crate::handlers::{credentials, session};

        match op {
            PendingNetworkOp::RequestZidChallenge { client_pid, user_id, zid_endpoint, machine_key, cap_slots } => {
                match network_handlers::handle_zid_challenge_result(client_pid, user_id, zid_endpoint, *machine_key, cap_slots, http_response) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueZidLoginWithChallenge { client_pid, user_id, zid_endpoint, machine_key, challenge_response, cap_slots } => {
                        session::continue_zid_login_after_challenge(self, client_pid, user_id, zid_endpoint, *machine_key, challenge_response, cap_slots)
                    }
                    _ => Ok(()),
                }
            }
            PendingNetworkOp::SubmitZidLogin { client_pid, user_id, zid_endpoint, cap_slots } => {
                match network_handlers::handle_zid_login_result(client_pid, user_id, zid_endpoint, cap_slots, http_response) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueZidLoginWithTokens { client_pid, user_id, zid_endpoint, login_response, cap_slots } => {
                        session::continue_zid_login_after_login(self, client_pid, user_id, zid_endpoint, login_response, cap_slots)
                    }
                    _ => Ok(()),
                }
            }
            PendingNetworkOp::SubmitEmailToZid { client_pid, user_id, email, cap_slots } => {
                match network_handlers::handle_email_to_zid_result(client_pid, user_id, email, cap_slots, http_response) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueAttachEmail { client_pid, user_id, email, cap_slots } => {
                        credentials::continue_attach_email_after_zid(self, client_pid, user_id, email, cap_slots)
                    }
                    _ => Ok(()),
                }
            }
            PendingNetworkOp::SubmitZidEnroll { client_pid, user_id, zid_endpoint, cap_slots } => {
                match network_handlers::handle_zid_enroll_result(client_pid, user_id, zid_endpoint, cap_slots, http_response) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueZidEnroll { client_pid, user_id, zid_endpoint, enroll_response, cap_slots } => {
                        session::continue_zid_enroll_after_submit(self, client_pid, user_id, zid_endpoint, enroll_response, cap_slots)
                    }
                    _ => Ok(()),
                }
            }
        }
    }
}
