//! VFS Result Dispatch
//!
//! Handles MSG_VFS_*_RESPONSE messages and routes to appropriate handlers
//! based on the pending operation type.

use alloc::format;

use super::handlers::{credentials, keys, session};
use super::pending::{ExpectedVfsResponse, PendingKeystoreOp, PendingStorageOp, RequestContext};
use super::{response, IdentityService};
use zos_apps::syscall;
use zos_apps::{AppError, Message};
use zos_identity::error::CredentialError;
use zos_identity::keystore::{CredentialStore, LocalKeyStore};
use zos_identity::KeyError;
use zos_vfs::async_client;
use zos_vfs::ipc::vfs_msg;

impl IdentityService {
    // =========================================================================
    // VFS result handler (dispatches to per-response-type handlers)
    // =========================================================================

    /// Handle VFS IPC responses. Routes to specific handlers based on response type.
    ///
    /// VFS IPC doesn't use request IDs, so we process pending operations in FIFO order.
    pub fn handle_vfs_result(&mut self, msg: &Message) -> Result<(), AppError> {
        syscall::debug(&format!(
            "IdentityService: handle_vfs_result tag=0x{:x}, data_len={}",
            msg.tag,
            msg.data.len()
        ));

        match msg.tag {
            vfs_msg::MSG_VFS_READ_RESPONSE => self.handle_vfs_read_response(msg),
            vfs_msg::MSG_VFS_WRITE_RESPONSE => self.handle_vfs_write_response(msg),
            vfs_msg::MSG_VFS_EXISTS_RESPONSE => self.handle_vfs_exists_response(msg),
            vfs_msg::MSG_VFS_MKDIR_RESPONSE => self.handle_vfs_mkdir_response(msg),
            vfs_msg::MSG_VFS_READDIR_RESPONSE => self.handle_vfs_readdir_response(msg),
            vfs_msg::MSG_VFS_UNLINK_RESPONSE => self.handle_vfs_unlink_response(msg),
            _ => {
                syscall::debug(&format!(
                    "IdentityService: Unhandled VFS response tag 0x{:x}",
                    msg.tag
                ));
                Ok(())
            }
        }
    }

    /// Take the next pending VFS operation matching the expected response type.
    ///
    /// Instead of FIFO ordering, this searches for an operation that expects
    /// the given response type. This is necessary because VFS responses can
    /// arrive out of order when multiple operations of different types are
    /// in flight concurrently.
    ///
    /// Returns None if no matching operations are pending.
    fn take_pending_vfs_op_for(&mut self, expected: ExpectedVfsResponse) -> Option<PendingStorageOp> {
        // Find the first operation that expects this response type
        let key = self.pending_vfs_ops
            .iter()
            .find(|(_, op)| op.expected_response() == expected)
            .map(|(k, _)| *k)?;
        self.pending_vfs_ops.remove(&key)
    }

    /// Handle VFS read response (MSG_VFS_READ_RESPONSE)
    fn handle_vfs_read_response(&mut self, msg: &Message) -> Result<(), AppError> {
        let pending_op = match self.take_pending_vfs_op_for(ExpectedVfsResponse::Read) {
            Some(op) => op,
            None => {
                syscall::debug("IdentityService: VFS read response but no pending read operation");
                return Ok(());
            }
        };

        // Parse VFS response
        let result = async_client::parse_read_response(&msg.data);

        // Dispatch based on operation type
        self.dispatch_vfs_read_result(pending_op, result)
    }

    /// Handle VFS write response (MSG_VFS_WRITE_RESPONSE)
    fn handle_vfs_write_response(&mut self, msg: &Message) -> Result<(), AppError> {
        let pending_op = match self.take_pending_vfs_op_for(ExpectedVfsResponse::Write) {
            Some(op) => op,
            None => {
                syscall::debug("IdentityService: VFS write response but no pending write operation");
                return Ok(());
            }
        };

        // Parse VFS response
        let result = async_client::parse_write_response(&msg.data);

        // Dispatch based on operation type
        self.dispatch_vfs_write_result(pending_op, result)
    }

    /// Handle VFS exists response (MSG_VFS_EXISTS_RESPONSE)
    fn handle_vfs_exists_response(&mut self, msg: &Message) -> Result<(), AppError> {
        let pending_op = match self.take_pending_vfs_op_for(ExpectedVfsResponse::Exists) {
            Some(op) => op,
            None => {
                syscall::debug("IdentityService: VFS exists response but no pending exists operation");
                return Ok(());
            }
        };

        // Parse VFS response
        let result = async_client::parse_exists_response(&msg.data);

        // Dispatch based on operation type
        self.dispatch_vfs_exists_result(pending_op, result)
    }

    /// Handle VFS mkdir response (MSG_VFS_MKDIR_RESPONSE)
    fn handle_vfs_mkdir_response(&mut self, msg: &Message) -> Result<(), AppError> {
        let pending_op = match self.take_pending_vfs_op_for(ExpectedVfsResponse::Mkdir) {
            Some(op) => op,
            None => {
                syscall::debug("IdentityService: VFS mkdir response but no pending mkdir operation");
                return Ok(());
            }
        };

        // Parse VFS response
        let result = async_client::parse_mkdir_response(&msg.data);

        // Dispatch based on operation type
        self.dispatch_vfs_mkdir_result(pending_op, result)
    }

    /// Handle VFS readdir response (MSG_VFS_READDIR_RESPONSE)
    fn handle_vfs_readdir_response(&mut self, msg: &Message) -> Result<(), AppError> {
        let pending_op = match self.take_pending_vfs_op_for(ExpectedVfsResponse::Readdir) {
            Some(op) => op,
            None => {
                syscall::debug("IdentityService: VFS readdir response but no pending readdir operation");
                return Ok(());
            }
        };

        // Parse VFS response
        let result = async_client::parse_readdir_response(&msg.data);

        // Dispatch based on operation type
        self.dispatch_vfs_readdir_result(pending_op, result)
    }

    /// Handle VFS unlink response (MSG_VFS_UNLINK_RESPONSE)
    fn handle_vfs_unlink_response(&mut self, msg: &Message) -> Result<(), AppError> {
        let pending_op = match self.take_pending_vfs_op_for(ExpectedVfsResponse::Unlink) {
            Some(op) => op,
            None => {
                syscall::debug("IdentityService: VFS unlink response but no pending unlink operation");
                return Ok(());
            }
        };

        // Parse VFS response
        let result = async_client::parse_unlink_response(&msg.data);

        // Dispatch based on operation type
        self.dispatch_vfs_unlink_result(pending_op, result)
    }

    // =========================================================================
    // VFS result dispatchers
    // =========================================================================
    //
    // VFS operations are single-step (no content/inode split), so handlers
    // complete with a single response after the VFS operation succeeds.

    /// Dispatch VFS read result to appropriate handler based on pending operation type.
    fn dispatch_vfs_read_result(
        &mut self,
        op: PendingStorageOp,
        result: Result<alloc::vec::Vec<u8>, alloc::string::String>,
    ) -> Result<(), AppError> {
        match op {
            PendingStorageOp::GetIdentityKey { ctx } => match result {
                Ok(data) => match serde_json::from_slice::<LocalKeyStore>(&data) {
                    Ok(key_store) => response::send_get_identity_key_success(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        Some(key_store),
                    ),
                    Err(e) => {
                        syscall::debug(&format!(
                            "IdentityService: Failed to parse stored keys: {}",
                            e
                        ));
                        response::send_get_identity_key_error(
                            ctx.client_pid,
                            &ctx.cap_slots,
                            KeyError::StorageError(format!("Parse failed: {}", e)),
                        )
                    }
                },
                Err(_) => {
                    // Key not found
                    response::send_get_identity_key_success(ctx.client_pid, &ctx.cap_slots, None)
                }
            },
            PendingStorageOp::ReadIdentityForRecovery {
                ctx,
                user_id,
                zid_shards,
            } => match result {
                Ok(data) if !data.is_empty() => {
                    match serde_json::from_slice::<LocalKeyStore>(&data) {
                        Ok(key_store) => {
                            // CRITICAL: Two different user_id concepts:
                            // - derivation_user_id (key_store.user_id) - ORIGINAL user_id for crypto
                            // - storage_user_id (user_id from request) - derived_user_id for paths
                            let derivation_user_id = key_store.user_id;
                            let storage_user_id = user_id;
                            keys::continue_recover_after_identity_read(
                                self,
                                ctx.client_pid,
                                derivation_user_id,
                                storage_user_id,
                                zid_shards,
                                key_store.identity_signing_public_key,
                                ctx.cap_slots,
                            )
                        }
                        Err(e) => {
                            syscall::debug(&format!(
                                "IdentityService: Failed to parse LocalKeyStore for recovery: {}",
                                e
                            ));
                            response::send_recover_key_error(
                                ctx.client_pid,
                                &ctx.cap_slots,
                                KeyError::StorageError("Corrupted identity key store".into()),
                            )
                        }
                    }
                }
                _ => {
                    syscall::debug("IdentityService: Identity read for recovery failed");
                    response::send_recover_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::IdentityKeyRequired,
                    )
                }
            },
            PendingStorageOp::ReadIdentityForMachine { ctx, request } => match result {
                Ok(data) if !data.is_empty() => {
                    match serde_json::from_slice::<LocalKeyStore>(&data) {
                        Ok(key_store) => keys::continue_create_machine_after_identity_read(
                            self,
                            ctx.client_pid,
                            request,
                            key_store.identity_signing_public_key,
                            ctx.cap_slots,
                        ),
                        Err(e) => {
                            syscall::debug(&format!(
                                "IdentityService: Failed to parse LocalKeyStore: {}",
                                e
                            ));
                            response::send_create_machine_key_error(
                                ctx.client_pid,
                                &ctx.cap_slots,
                                KeyError::StorageError("Corrupted identity key store".into()),
                            )
                        }
                    }
                }
                _ => {
                    syscall::debug("IdentityService: Identity read failed");
                    response::send_create_machine_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::IdentityKeyRequired,
                    )
                }
            },
            PendingStorageOp::ReadMachineKey {
                ctx,
                user_id,
                mut remaining_paths,
                mut records,
            } => {
                // Process this machine key result
                if let Ok(data) = result {
                    if let Ok(record) =
                        serde_json::from_slice::<zos_identity::keystore::MachineKeyRecord>(&data)
                    {
                        records.push(record);
                    }
                }

                // Continue reading remaining paths or send response
                if remaining_paths.is_empty() {
                    response::send_list_machine_keys(ctx.client_pid, &ctx.cap_slots, records)
                } else {
                    let next_path = remaining_paths.remove(0);
                    self.start_vfs_read(
                        &next_path,
                        PendingStorageOp::ReadMachineKey {
                            ctx: RequestContext::new(ctx.client_pid, ctx.cap_slots),
                            user_id,
                            remaining_paths,
                            records,
                        },
                    )
                }
            }
            PendingStorageOp::ReadSingleMachineKey { ctx } => match result {
                Ok(data) => {
                    match serde_json::from_slice::<zos_identity::keystore::MachineKeyRecord>(&data) {
                        Ok(record) => response::send_get_machine_key_success(
                            ctx.client_pid,
                            &ctx.cap_slots,
                            Some(record),
                        ),
                        Err(_) => {
                            response::send_get_machine_key_success(ctx.client_pid, &ctx.cap_slots, None)
                        }
                    }
                }
                Err(_) => response::send_get_machine_key_success(ctx.client_pid, &ctx.cap_slots, None),
            },
            PendingStorageOp::ReadMachineForRotate {
                ctx,
                user_id,
                machine_id,
            } => match result {
                Ok(data) => keys::continue_rotate_after_read(
                    self,
                    ctx.client_pid,
                    user_id,
                    machine_id,
                    &data,
                    ctx.cap_slots,
                ),
                Err(_) => response::send_rotate_machine_key_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    KeyError::MachineKeyNotFound,
                ),
            },
            PendingStorageOp::ReadCredentialsForAttach {
                ctx,
                user_id,
                email,
            } => {
                let existing_store = result
                    .ok()
                    .and_then(|data| serde_json::from_slice::<CredentialStore>(&data).ok());
                credentials::continue_attach_email_after_read(
                    self,
                    ctx.client_pid,
                    user_id,
                    email,
                    existing_store,
                    ctx.cap_slots,
                )
            }
            PendingStorageOp::GetCredentials { ctx } => {
                let credentials = result
                    .ok()
                    .and_then(|data| serde_json::from_slice::<CredentialStore>(&data).ok())
                    .map(|store| store.credentials)
                    .unwrap_or_default();
                response::send_get_credentials(ctx.client_pid, &ctx.cap_slots, credentials)
            }
            PendingStorageOp::ReadCredentialsForUnlink {
                ctx,
                user_id,
                credential_type,
            } => match result {
                Ok(data) if !data.is_empty() => credentials::continue_unlink_credential_after_read(
                    self,
                    ctx.client_pid,
                    user_id,
                    credential_type,
                    &data,
                    ctx.cap_slots,
                ),
                _ => response::send_unlink_credential_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    CredentialError::NotFound,
                ),
            },
            PendingStorageOp::ReadMachineKeyForZidLogin {
                ctx,
                user_id,
                zid_endpoint,
            } => match result {
                Ok(data) => session::continue_zid_login_after_read(
                    self,
                    ctx.client_pid,
                    user_id,
                    zid_endpoint,
                    &data,
                    ctx.cap_slots,
                ),
                Err(_) => response::send_zid_login_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    zos_identity::error::ZidError::MachineKeyNotFound,
                ),
            },
            PendingStorageOp::ReadMachineKeyForZidEnroll {
                ctx,
                user_id,
                zid_endpoint,
            } => match result {
                Ok(data) => session::continue_zid_enroll_after_read(
                    self,
                    ctx.client_pid,
                    user_id,
                    zid_endpoint,
                    &data,
                    ctx.cap_slots,
                ),
                Err(_) => response::send_zid_enroll_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    zos_identity::error::ZidError::MachineKeyNotFound,
                ),
            },
            PendingStorageOp::ReadIdentityPreferences { ctx, user_id: _ } => {
                let preferences = result
                    .ok()
                    .and_then(|data| {
                        serde_json::from_slice::<zos_identity::ipc::IdentityPreferences>(&data).ok()
                    })
                    .unwrap_or_default();
                let resp = zos_identity::ipc::GetIdentityPreferencesResponse { preferences };
                response::send_get_identity_preferences_response(ctx.client_pid, &ctx.cap_slots, resp)
            }
            PendingStorageOp::ReadPreferencesForUpdate {
                ctx,
                user_id,
                new_key_scheme,
            } => {
                let mut preferences = result
                    .ok()
                    .and_then(|data| {
                        serde_json::from_slice::<zos_identity::ipc::IdentityPreferences>(&data).ok()
                    })
                    .unwrap_or_default();

                preferences.default_key_scheme = new_key_scheme;

                match serde_json::to_vec(&preferences) {
                    Ok(json_bytes) => {
                        let prefs_path =
                            zos_identity::ipc::IdentityPreferences::storage_path(user_id);
                        self.start_vfs_write(
                            &prefs_path,
                            &json_bytes,
                            PendingStorageOp::WritePreferences {
                                ctx: RequestContext::new(ctx.client_pid, ctx.cap_slots),
                                user_id,
                                json_bytes: json_bytes.clone(),
                            },
                        )
                    }
                    Err(_) => response::send_set_default_key_scheme_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::StorageError("Serialization failed".into()),
                    ),
                }
            }
            PendingStorageOp::ReadPreferencesForDefaultMachine {
                ctx,
                user_id,
                new_default_machine_id,
            } => {
                let mut preferences = result
                    .ok()
                    .and_then(|data| {
                        serde_json::from_slice::<zos_identity::ipc::IdentityPreferences>(&data).ok()
                    })
                    .unwrap_or_default();

                preferences.default_machine_id = Some(new_default_machine_id);

                match serde_json::to_vec(&preferences) {
                    Ok(json_bytes) => {
                        let prefs_path =
                            zos_identity::ipc::IdentityPreferences::storage_path(user_id);
                        self.start_vfs_write(
                            &prefs_path,
                            &json_bytes,
                            PendingStorageOp::WritePreferencesForDefaultMachine {
                                ctx: RequestContext::new(ctx.client_pid, ctx.cap_slots),
                                user_id,
                                json_bytes: json_bytes.clone(),
                            },
                        )
                    }
                    Err(_) => response::send_set_default_machine_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::StorageError("Serialization failed".into()),
                    ),
                }
            }
            PendingStorageOp::ReadPreferencesForZidLogin {
                ctx,
                user_id,
                zid_endpoint,
            } => {
                // Parse preferences to get default_machine_id (defaults to None if not found)
                let preferences = result
                    .ok()
                    .and_then(|data| {
                        serde_json::from_slice::<zos_identity::ipc::IdentityPreferences>(&data).ok()
                    })
                    .unwrap_or_default();

                // Continue ZID login with the default_machine_id from preferences
                session::continue_zid_login_after_preferences(
                    self,
                    ctx.client_pid,
                    user_id,
                    zid_endpoint,
                    preferences.default_machine_id,
                    ctx.cap_slots,
                )
            }
            PendingStorageOp::ReadZidSessionForRefresh {
                ctx,
                user_id,
                zid_endpoint,
                refresh_token_override,
            } => match result {
                Ok(data) => session::continue_zid_refresh_after_session_read(
                    self,
                    ctx.client_pid,
                    user_id,
                    zid_endpoint,
                    &data,
                    refresh_token_override,
                    ctx.cap_slots,
                ),
                Err(_) => response::send_zid_refresh_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    zos_identity::error::ZidError::InvalidRequest("No session found".into()),
                ),
            }
            // Rule 5: Explicitly enumerate all remaining pending operation types
            // These operations don't expect a read response - if we get here, it's a logic error
            PendingStorageOp::CheckIdentityDirectory { ctx, .. } |
            PendingStorageOp::CreateIdentityDirectory { ctx, .. } |
            PendingStorageOp::CreateIdentityDirectoryComplete { ctx, .. } |
            PendingStorageOp::CheckKeyExists { ctx, .. } |
            PendingStorageOp::WriteKeyStore { ctx, .. } |
            PendingStorageOp::WriteRecoveredKeyStore { ctx, .. } |
            PendingStorageOp::WriteMachineKey { ctx, .. } |
            PendingStorageOp::ListMachineKeys { ctx, .. } |
            PendingStorageOp::DeleteMachineKey { ctx, .. } |
            PendingStorageOp::WriteRotatedMachineKey { ctx, .. } |
            PendingStorageOp::WriteUnlinkedCredential { ctx, .. } |
            PendingStorageOp::WriteEmailCredential { ctx, .. } |
            PendingStorageOp::WriteZidSession { ctx, .. } |
            PendingStorageOp::WriteZidEnrollSession { ctx, .. } |
            PendingStorageOp::WritePreferences { ctx, .. } |
            PendingStorageOp::WritePreferencesForDefaultMachine { ctx, .. } |
            PendingStorageOp::WritePreferencesForDefaultMachineRetry { ctx, .. } |
            PendingStorageOp::WriteRefreshedZidSession { ctx, .. } |
            PendingStorageOp::WriteZidEmailLoginSession { ctx, .. } |
            PendingStorageOp::CreateCredentialsDirectory { ctx, .. } |
            PendingStorageOp::CreateIdentityDirForPreferences { ctx, .. } |
            PendingStorageOp::WriteEmailCredentialRetry { ctx, .. } |
            PendingStorageOp::CreateDerivedUserDirectory { ctx, .. } |
            PendingStorageOp::DeleteZidSession { ctx } => {
                // Rule 5: These operations should NOT receive a read response
                // This indicates a state machine bug - report it clearly
                syscall::debug(&format!(
                    "IdentityService: STATE_MACHINE_ERROR - unexpected VFS read result for non-read op, client_pid={}",
                    ctx.client_pid
                ));
                Err(AppError::Internal(
                    "State machine error: unexpected VFS read result for non-read operation".into()
                ))
            }
        }
    }

    /// Dispatch VFS write result to appropriate handler based on pending operation type.
    fn dispatch_vfs_write_result(
        &mut self,
        op: PendingStorageOp,
        result: Result<(), alloc::string::String>,
    ) -> Result<(), AppError> {
        match op {
            PendingStorageOp::WriteKeyStore {
                ctx, result: key_result, ..
            } => {
                match result {
                    Ok(()) => {
                        syscall::debug("IdentityService: Neural key stored successfully via VFS");
                        response::send_neural_key_success(ctx.client_pid, &ctx.cap_slots, key_result)
                    }
                    Err(e) => {
                        // Rule 9: Include operation and result type in error message
                        syscall::debug(&format!(
                            "IdentityService: WriteKeyStore failed - op=write_neural_key, error={}",
                            e
                        ));
                        response::send_neural_key_error(
                            ctx.client_pid,
                            &ctx.cap_slots,
                            KeyError::StorageError(format!("VFS write failed for neural key: {}", e)),
                        )
                    }
                }
            }
            PendingStorageOp::WriteRecoveredKeyStore {
                ctx, result: key_result, ..
            } => {
                match result {
                    Ok(()) => {
                        syscall::debug("IdentityService: Recovered key stored successfully via VFS");
                        response::send_recover_key_success(ctx.client_pid, &ctx.cap_slots, key_result)
                    }
                    Err(e) => {
                        // Rule 9: Include operation and result type in error message
                        syscall::debug(&format!(
                            "IdentityService: WriteRecoveredKeyStore failed - op=recover_neural_key, error={}",
                            e
                        ));
                        response::send_recover_key_error(
                            ctx.client_pid,
                            &ctx.cap_slots,
                            KeyError::StorageError(format!("VFS write failed for recovered key: {}", e)),
                        )
                    }
                }
            }
            PendingStorageOp::WriteMachineKey { ctx, record, .. } => {
                match result {
                    Ok(()) => {
                        syscall::debug(&format!(
                            "IdentityService: Machine key {:032x} stored successfully via VFS",
                            record.machine_id
                        ));
                        response::send_create_machine_key_success(ctx.client_pid, &ctx.cap_slots, record)
                    }
                    Err(e) => {
                        // Rule 9: Include operation and result type in error message
                        syscall::debug(&format!(
                            "IdentityService: WriteMachineKey failed - op=create_machine_key, machine_id={:032x}, error={}",
                            record.machine_id, e
                        ));
                        response::send_create_machine_key_error(
                            ctx.client_pid,
                            &ctx.cap_slots,
                            KeyError::StorageError(format!("VFS write failed for machine key: {}", e)),
                        )
                    }
                }
            }
            PendingStorageOp::WriteRotatedMachineKey { ctx, record, .. } => {
                match result {
                    Ok(()) => {
                        syscall::debug(&format!(
                            "IdentityService: Rotated machine key {:032x} stored successfully via VFS",
                            record.machine_id
                        ));
                        response::send_rotate_machine_key_success(ctx.client_pid, &ctx.cap_slots, record)
                    }
                    Err(e) => {
                        // Rule 9: Include operation and result type in error message
                        syscall::debug(&format!(
                            "IdentityService: WriteRotatedMachineKey failed - op=rotate_machine_key, machine_id={:032x}, error={}",
                            record.machine_id, e
                        ));
                        response::send_rotate_machine_key_error(
                            ctx.client_pid,
                            &ctx.cap_slots,
                            KeyError::StorageError(format!("VFS write failed for rotated key: {}", e)),
                        )
                    }
                }
            }
            PendingStorageOp::WriteEmailCredential { ctx, user_id, json_bytes } => {
                match result {
                    Ok(()) => {
                        syscall::debug("IdentityService: Email credential stored successfully via VFS");
                        response::send_attach_email_success(ctx.client_pid, &ctx.cap_slots)
                    }
                    Err(e) => {
                        // VFS write failed - likely directory doesn't exist for existing users
                        // Try to create the credentials directory on-demand
                        syscall::debug(&format!(
                            "IdentityService: WriteEmailCredential failed ({}), creating credentials directory on-demand",
                            e
                        ));
                        // IMPORTANT: Use {} format (decimal) to match canonical user home path format
                        // Use create_parents=true to create all parent directories if they don't exist
                        let cred_dir = alloc::format!("/home/{}/.zos/credentials", user_id);
                        self.start_vfs_mkdir(
                            &cred_dir,
                            true, // create_parents = true - creates /home/{user}/.zos if needed
                            PendingStorageOp::CreateCredentialsDirectory {
                                ctx,
                                user_id,
                                json_bytes,
                            },
                        )
                    }
                }
            }
            PendingStorageOp::WriteEmailCredentialRetry { ctx, .. } => {
                match result {
                    Ok(()) => {
                        syscall::debug("IdentityService: Email credential stored successfully via VFS (after directory creation)");
                        response::send_attach_email_success(ctx.client_pid, &ctx.cap_slots)
                    }
                    Err(e) => {
                        syscall::debug(&format!(
                            "IdentityService: WriteEmailCredentialRetry still failed: {}",
                            e
                        ));
                        response::send_attach_email_error(
                            ctx.client_pid,
                            &ctx.cap_slots,
                            CredentialError::StorageError(alloc::format!("VFS write failed: {}", e)),
                        )
                    }
                }
            }
            PendingStorageOp::WriteUnlinkedCredential { ctx, .. } => {
                match result {
                    Ok(()) => {
                        syscall::debug("IdentityService: Credential unlinked successfully via VFS");
                        response::send_unlink_credential_success(ctx.client_pid, &ctx.cap_slots)
                    }
                    Err(e) => {
                        // Rule 9: Include operation and result type in error message
                        syscall::debug(&format!(
                            "IdentityService: WriteUnlinkedCredential failed - op=unlink_credential, error={}",
                            e
                        ));
                        response::send_unlink_credential_error(
                            ctx.client_pid,
                            &ctx.cap_slots,
                            CredentialError::StorageError(format!("VFS write failed for unlink: {}", e)),
                        )
                    }
                }
            }
            PendingStorageOp::WriteZidSession { ctx, tokens, .. } => {
                match result {
                    Ok(()) => {
                        syscall::debug("IdentityService: ZID session stored successfully via VFS");
                        response::send_zid_login_success(ctx.client_pid, &ctx.cap_slots, tokens)
                    }
                    Err(e) => {
                        // Rule 9: Include operation and result type in error message
                        syscall::debug(&format!(
                            "IdentityService: WriteZidSession failed - op=zid_login, error={}",
                            e
                        ));
                        // Still return success with tokens since authentication succeeded,
                        // only session persistence failed (acceptable partial failure per Rule 0)
                        syscall::debug("IdentityService: Session write failed but auth succeeded - returning tokens anyway");
                        response::send_zid_login_success(ctx.client_pid, &ctx.cap_slots, tokens)
                    }
                }
            }
            PendingStorageOp::WriteZidEnrollSession { ctx, tokens, .. } => {
                match result {
                    Ok(()) => {
                        syscall::debug("IdentityService: ZID enroll session stored successfully via VFS");
                        response::send_zid_enroll_success(ctx.client_pid, &ctx.cap_slots, tokens)
                    }
                    Err(e) => {
                        // Rule 9: Include operation and result type in error message
                        syscall::debug(&format!(
                            "IdentityService: WriteZidEnrollSession failed - op=zid_enroll, error={}",
                            e
                        ));
                        // Still return success with tokens since enrollment/auth succeeded,
                        // only session persistence failed (acceptable partial failure per Rule 0)
                        syscall::debug("IdentityService: Enroll session write failed but auth succeeded - returning tokens anyway");
                        response::send_zid_enroll_success(ctx.client_pid, &ctx.cap_slots, tokens)
                    }
                }
            }
            PendingStorageOp::WritePreferences { ctx, .. } => {
                match result {
                    Ok(()) => {
                        syscall::debug("IdentityService: Preferences stored successfully via VFS");
                        let resp = zos_identity::ipc::SetDefaultKeySchemeResponse { result: Ok(()) };
                        response::send_set_default_key_scheme_response(ctx.client_pid, &ctx.cap_slots, resp)
                    }
                    Err(e) => {
                        // Rule 9: Include operation and result type in error message
                        syscall::debug(&format!(
                            "IdentityService: WritePreferences failed - op=set_default_key_scheme, error={}",
                            e
                        ));
                        response::send_set_default_key_scheme_error(
                            ctx.client_pid,
                            &ctx.cap_slots,
                            KeyError::StorageError(format!("VFS write failed for preferences: {}", e)),
                        )
                    }
                }
            }
            PendingStorageOp::WritePreferencesForDefaultMachine { ctx, user_id, json_bytes } => {
                match result {
                    Ok(()) => {
                        syscall::debug("IdentityService: Default machine key preference stored successfully via VFS");
                        let resp = zos_identity::ipc::SetDefaultMachineKeyResponse { result: Ok(()) };
                        response::send_set_default_machine_key_response(ctx.client_pid, &ctx.cap_slots, resp)
                    }
                    Err(e) => {
                        // On failure (likely NotFound for parent directory), create directory and retry
                        syscall::debug(&format!(
                            "IdentityService: WritePreferencesForDefaultMachine failed ({}), creating identity directory on-demand",
                            e
                        ));
                        // Use create_parents=true to create all parent directories if they don't exist
                        let identity_dir = format!("/home/{}/.zos/identity", user_id);
                        self.start_vfs_mkdir(
                            &identity_dir,
                            true, // create_parents = true - creates /home/{user}/.zos if needed
                            PendingStorageOp::CreateIdentityDirForPreferences {
                                ctx,
                                user_id,
                                json_bytes,
                            },
                        )
                    }
                }
            }
            PendingStorageOp::WritePreferencesForDefaultMachineRetry { ctx, .. } => {
                match result {
                    Ok(()) => {
                        syscall::debug("IdentityService: Default machine key preference stored successfully via VFS (after directory creation)");
                        let resp = zos_identity::ipc::SetDefaultMachineKeyResponse { result: Ok(()) };
                        response::send_set_default_machine_key_response(ctx.client_pid, &ctx.cap_slots, resp)
                    }
                    Err(e) => {
                        // Rule 9: Include operation and result type in error message
                        syscall::debug(&format!(
                            "IdentityService: WritePreferencesForDefaultMachineRetry still failed - op=set_default_machine_key, error={}",
                            e
                        ));
                        response::send_set_default_machine_key_error(
                            ctx.client_pid,
                            &ctx.cap_slots,
                            KeyError::StorageError(format!("VFS write failed for default machine key: {}", e)),
                        )
                    }
                }
            }
            PendingStorageOp::WriteRefreshedZidSession { ctx, tokens, .. } => {
                match result {
                    Ok(()) => {
                        syscall::debug("IdentityService: Refreshed ZID session stored successfully via VFS");
                        response::send_zid_refresh_success(ctx.client_pid, &ctx.cap_slots, tokens)
                    }
                    Err(e) => {
                        // Rule 9: Include operation and result type in error message
                        syscall::debug(&format!(
                            "IdentityService: WriteRefreshedZidSession failed - op=zid_refresh, error={}",
                            e
                        ));
                        // Still return success with tokens since refresh succeeded,
                        // only session persistence failed (acceptable partial failure per Rule 0)
                        syscall::debug("IdentityService: Session write failed but refresh succeeded - returning tokens anyway");
                        response::send_zid_refresh_success(ctx.client_pid, &ctx.cap_slots, tokens)
                    }
                }
            }
            PendingStorageOp::WriteZidEmailLoginSession { ctx, tokens, .. } => {
                match result {
                    Ok(()) => {
                        syscall::debug("IdentityService: ZID email login session stored successfully via VFS");
                        response::send_zid_email_login_success(ctx.client_pid, &ctx.cap_slots, tokens)
                    }
                    Err(e) => {
                        // Rule 9: Include operation and result type in error message
                        syscall::debug(&format!(
                            "IdentityService: WriteZidEmailLoginSession failed - op=zid_email_login, error={}",
                            e
                        ));
                        // Still return success with tokens since email login succeeded,
                        // only session persistence failed (acceptable partial failure per Rule 0)
                        syscall::debug("IdentityService: Session write failed but email login succeeded - returning tokens anyway");
                        response::send_zid_email_login_success(ctx.client_pid, &ctx.cap_slots, tokens)
                    }
                }
            }
            // Rule 5: Explicitly enumerate all remaining pending operation types
            // These operations don't expect a write response - if we get here, it's a logic error
            PendingStorageOp::CheckIdentityDirectory { ctx, .. } |
            PendingStorageOp::CreateIdentityDirectory { ctx, .. } |
            PendingStorageOp::CheckKeyExists { ctx, .. } |
            PendingStorageOp::GetIdentityKey { ctx } |
            PendingStorageOp::ReadIdentityForRecovery { ctx, .. } |
            PendingStorageOp::ReadIdentityForMachine { ctx, .. } |
            PendingStorageOp::ListMachineKeys { ctx, .. } |
            PendingStorageOp::ReadMachineKey { ctx, .. } |
            PendingStorageOp::DeleteMachineKey { ctx, .. } |
            PendingStorageOp::ReadMachineForRotate { ctx, .. } |
            PendingStorageOp::ReadSingleMachineKey { ctx } |
            PendingStorageOp::ReadCredentialsForAttach { ctx, .. } |
            PendingStorageOp::GetCredentials { ctx } |
            PendingStorageOp::ReadCredentialsForUnlink { ctx, .. } |
            PendingStorageOp::ReadMachineKeyForZidLogin { ctx, .. } |
            PendingStorageOp::ReadMachineKeyForZidEnroll { ctx, .. } |
            PendingStorageOp::ReadIdentityPreferences { ctx, .. } |
            PendingStorageOp::ReadPreferencesForUpdate { ctx, .. } |
            PendingStorageOp::ReadPreferencesForDefaultMachine { ctx, .. } |
            PendingStorageOp::ReadPreferencesForZidLogin { ctx, .. } |
            PendingStorageOp::ReadZidSessionForRefresh { ctx, .. } |
            PendingStorageOp::CreateCredentialsDirectory { ctx, .. } |
            PendingStorageOp::CreateIdentityDirForPreferences { ctx, .. } |
            PendingStorageOp::CreateIdentityDirectoryComplete { ctx, .. } |
            PendingStorageOp::CreateDerivedUserDirectory { ctx, .. } |
            PendingStorageOp::DeleteZidSession { ctx } => {
                // Rule 5: These operations should NOT receive a write response
                // This indicates a state machine bug - report it clearly
                syscall::debug(&format!(
                    "IdentityService: STATE_MACHINE_ERROR - unexpected VFS write result for non-write op, client_pid={}",
                    ctx.client_pid
                ));
                Err(AppError::Internal(
                    "State machine error: unexpected VFS write result for non-write operation".into()
                ))
            }
        }
    }

    /// Dispatch VFS exists result to appropriate handler based on pending operation type.
    ///
    /// # Rule 5 Compliance
    /// Errors are handled explicitly - we do NOT use unwrap_or to silently swallow errors.
    fn dispatch_vfs_exists_result(
        &mut self,
        op: PendingStorageOp,
        result: Result<bool, alloc::string::String>,
    ) -> Result<(), AppError> {
        match op {
            PendingStorageOp::CheckIdentityDirectory { ctx, user_id, password } => {
                // Rule 5: Handle errors explicitly, don't swallow them
                match result {
                    Ok(exists) => keys::continue_generate_after_directory_check(
                        self,
                        ctx.client_pid,
                        user_id,
                        exists,
                        password,
                        ctx.cap_slots,
                    ),
                    Err(e) => {
                        syscall::debug(&format!(
                            "IdentityService: VFS exists check failed for identity directory: {}",
                            e
                        ));
                        response::send_neural_key_error(
                            ctx.client_pid,
                            &ctx.cap_slots,
                            KeyError::StorageError(format!("Directory check failed: {}", e)),
                        )
                    }
                }
            }
            PendingStorageOp::CheckKeyExists { ctx, user_id, password } => {
                // Rule 5: Handle errors explicitly, don't swallow them
                match result {
                    Ok(exists) => keys::continue_generate_after_exists_check(
                        self,
                        ctx.client_pid,
                        user_id,
                        exists,
                        password,
                        ctx.cap_slots,
                    ),
                    Err(e) => {
                        syscall::debug(&format!(
                            "IdentityService: VFS exists check failed for key file: {}",
                            e
                        ));
                        response::send_neural_key_error(
                            ctx.client_pid,
                            &ctx.cap_slots,
                            KeyError::StorageError(format!("Key exists check failed: {}", e)),
                        )
                    }
                }
            }
            // Rule 5: Explicitly enumerate all remaining pending operation types
            // These operations don't expect an exists response - if we get here, it's a logic error
            PendingStorageOp::CreateIdentityDirectory { ctx, .. } |
            PendingStorageOp::WriteKeyStore { ctx, .. } |
            PendingStorageOp::GetIdentityKey { ctx } |
            PendingStorageOp::ReadIdentityForRecovery { ctx, .. } |
            PendingStorageOp::WriteRecoveredKeyStore { ctx, .. } |
            PendingStorageOp::ReadIdentityForMachine { ctx, .. } |
            PendingStorageOp::WriteMachineKey { ctx, .. } |
            PendingStorageOp::ListMachineKeys { ctx, .. } |
            PendingStorageOp::ReadMachineKey { ctx, .. } |
            PendingStorageOp::DeleteMachineKey { ctx, .. } |
            PendingStorageOp::ReadMachineForRotate { ctx, .. } |
            PendingStorageOp::WriteRotatedMachineKey { ctx, .. } |
            PendingStorageOp::ReadSingleMachineKey { ctx } |
            PendingStorageOp::ReadCredentialsForAttach { ctx, .. } |
            PendingStorageOp::GetCredentials { ctx } |
            PendingStorageOp::ReadCredentialsForUnlink { ctx, .. } |
            PendingStorageOp::WriteUnlinkedCredential { ctx, .. } |
            PendingStorageOp::WriteEmailCredential { ctx, .. } |
            PendingStorageOp::ReadMachineKeyForZidLogin { ctx, .. } |
            PendingStorageOp::WriteZidSession { ctx, .. } |
            PendingStorageOp::ReadMachineKeyForZidEnroll { ctx, .. } |
            PendingStorageOp::WriteZidEnrollSession { ctx, .. } |
            PendingStorageOp::ReadIdentityPreferences { ctx, .. } |
            PendingStorageOp::ReadPreferencesForUpdate { ctx, .. } |
            PendingStorageOp::ReadPreferencesForDefaultMachine { ctx, .. } |
            PendingStorageOp::ReadPreferencesForZidLogin { ctx, .. } |
            PendingStorageOp::ReadZidSessionForRefresh { ctx, .. } |
            PendingStorageOp::WritePreferences { ctx, .. } |
            PendingStorageOp::WritePreferencesForDefaultMachine { ctx, .. } |
            PendingStorageOp::WritePreferencesForDefaultMachineRetry { ctx, .. } |
            PendingStorageOp::WriteRefreshedZidSession { ctx, .. } |
            PendingStorageOp::WriteZidEmailLoginSession { ctx, .. } |
            PendingStorageOp::CreateCredentialsDirectory { ctx, .. } |
            PendingStorageOp::CreateIdentityDirForPreferences { ctx, .. } |
            PendingStorageOp::CreateIdentityDirectoryComplete { ctx, .. } |
            PendingStorageOp::CreateDerivedUserDirectory { ctx, .. } |
            PendingStorageOp::WriteEmailCredentialRetry { ctx, .. } |
            PendingStorageOp::DeleteZidSession { ctx } => {
                // Rule 5: These operations should NOT receive an exists response
                // This indicates a state machine bug - report it clearly
                syscall::debug(&format!(
                    "IdentityService: STATE_MACHINE_ERROR - unexpected VFS exists result for non-exists op, client_pid={}",
                    ctx.client_pid
                ));
                Err(AppError::Internal(
                    "State machine error: unexpected VFS exists result for non-exists operation".into()
                ))
            }
        }
    }

    /// Dispatch VFS mkdir result to appropriate handler based on pending operation type.
    fn dispatch_vfs_mkdir_result(
        &mut self,
        op: PendingStorageOp,
        result: Result<(), alloc::string::String>,
    ) -> Result<(), AppError> {
        match op {
            PendingStorageOp::CreateIdentityDirectory {
                ctx,
                user_id,
                directories,
                password,
            } => {
                // Treat "already exists" as success - we just need the directory to exist
                let is_ok = result.is_ok() || result.as_ref().err().map_or(false, |e| e.contains("AlreadyExists"));
                
                if is_ok {
                    // Continue creating remaining directories
                    keys::continue_create_directories(
                        self,
                        ctx.client_pid,
                        user_id,
                        directories,
                        password,
                        ctx.cap_slots,
                    )
                } else {
                    syscall::debug(&format!(
                        "IdentityService: Failed to create identity directory: {:?}",
                        result
                    ));
                    response::send_neural_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::StorageError("Failed to create identity directory".into()),
                    )
                }
            }
            // New efficient path: create_parents=true creates all directories in one VFS call
            PendingStorageOp::CreateIdentityDirectoryComplete {
                ctx,
                user_id,
                password,
            } => {
                // Treat "already exists" as success - we just need the directory to exist
                let is_ok = result.is_ok() || result.as_ref().err().map_or(false, |e| e.contains("AlreadyExists"));
                
                if is_ok {
                    syscall::debug(&format!(
                        "IdentityService: Identity directory structure created for user {}",
                        user_id
                    ));
                    // Proceed to check if key already exists (via Keystore)
                    let key_path = LocalKeyStore::storage_path(user_id);
                    self.start_keystore_exists(
                        &key_path,
                        PendingKeystoreOp::CheckKeyExists { ctx, user_id, password },
                    )
                } else {
                    syscall::debug(&format!(
                        "IdentityService: Failed to create identity directory: {:?}",
                        result
                    ));
                    response::send_neural_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::StorageError("Failed to create identity directory".into()),
                    )
                }
            }
            // Create VFS directory for derived user_id after neural key generation
            PendingStorageOp::CreateDerivedUserDirectory {
                ctx,
                derived_user_id,
                result: key_result,
            } => {
                // Treat "already exists" as success - we just need the directory to exist
                let is_ok = result.is_ok() || result.as_ref().err().map_or(false, |e| e.contains("AlreadyExists"));
                
                if is_ok {
                    syscall::debug(&format!(
                        "IdentityService: VFS directory created for derived user {}, sending success response",
                        derived_user_id
                    ));
                    // Now we can send the success response
                    response::send_neural_key_success(ctx.client_pid, &ctx.cap_slots, key_result)
                } else {
                    // Directory creation failed - log but still return success for neural key
                    // since the keys are already stored in keystore. The VFS directory will
                    // be created on-demand when needed (e.g., first preferences write).
                    syscall::debug(&format!(
                        "IdentityService: Warning - VFS directory creation failed for derived user {}: {:?}. Keys are stored, continuing.",
                        derived_user_id, result
                    ));
                    response::send_neural_key_success(ctx.client_pid, &ctx.cap_slots, key_result)
                }
            }
            // On-demand credentials directory creation for existing users
            PendingStorageOp::CreateCredentialsDirectory { ctx, user_id, json_bytes } => {
                // Treat "already exists" as success - we just need the directory to exist
                let is_ok = result.is_ok() || result.as_ref().err().map_or(false, |e| e.contains("AlreadyExists"));
                
                if is_ok {
                    syscall::debug("IdentityService: Credentials directory created, retrying write");
                    let cred_path = zos_identity::keystore::CredentialStore::storage_path(user_id);
                    self.start_vfs_write(
                        &cred_path,
                        &json_bytes,
                        PendingStorageOp::WriteEmailCredentialRetry {
                            ctx,
                            user_id,
                            json_bytes: json_bytes.clone(),
                        },
                    )
                } else {
                    syscall::debug(&format!(
                        "IdentityService: Failed to create credentials directory: {:?}",
                        result
                    ));
                    response::send_attach_email_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        CredentialError::StorageError("Failed to create credentials directory".into()),
                    )
                }
            }
            // On-demand identity directory creation for preferences write
            PendingStorageOp::CreateIdentityDirForPreferences { ctx, user_id, json_bytes } => {
                // Treat "already exists" as success - we just need the directory to exist
                let is_ok = result.is_ok() || result.as_ref().err().map_or(false, |e| e.contains("AlreadyExists"));
                
                if is_ok {
                    syscall::debug("IdentityService: Identity directory created, retrying preferences write");
                    let prefs_path = zos_identity::ipc::IdentityPreferences::storage_path(user_id);
                    self.start_vfs_write(
                        &prefs_path,
                        &json_bytes,
                        PendingStorageOp::WritePreferencesForDefaultMachineRetry {
                            ctx,
                            user_id,
                            json_bytes: json_bytes.clone(),
                        },
                    )
                } else {
                    syscall::debug(&format!(
                        "IdentityService: Failed to create identity directory: {:?}",
                        result
                    ));
                    response::send_set_default_machine_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::StorageError("Failed to create identity directory".into()),
                    )
                }
            }
            // Rule 5: Explicitly enumerate all remaining pending operation types
            // These operations don't expect a mkdir response - if we get here, it's a logic error
            PendingStorageOp::CheckIdentityDirectory { ctx, .. } |
            PendingStorageOp::CheckKeyExists { ctx, .. } |
            PendingStorageOp::WriteKeyStore { ctx, .. } |
            PendingStorageOp::GetIdentityKey { ctx } |
            PendingStorageOp::ReadIdentityForRecovery { ctx, .. } |
            PendingStorageOp::WriteRecoveredKeyStore { ctx, .. } |
            PendingStorageOp::ReadIdentityForMachine { ctx, .. } |
            PendingStorageOp::WriteMachineKey { ctx, .. } |
            PendingStorageOp::ListMachineKeys { ctx, .. } |
            PendingStorageOp::ReadMachineKey { ctx, .. } |
            PendingStorageOp::DeleteMachineKey { ctx, .. } |
            PendingStorageOp::ReadMachineForRotate { ctx, .. } |
            PendingStorageOp::WriteRotatedMachineKey { ctx, .. } |
            PendingStorageOp::ReadSingleMachineKey { ctx } |
            PendingStorageOp::ReadCredentialsForAttach { ctx, .. } |
            PendingStorageOp::GetCredentials { ctx } |
            PendingStorageOp::ReadCredentialsForUnlink { ctx, .. } |
            PendingStorageOp::WriteUnlinkedCredential { ctx, .. } |
            PendingStorageOp::WriteEmailCredential { ctx, .. } |
            PendingStorageOp::WriteEmailCredentialRetry { ctx, .. } |
            PendingStorageOp::ReadMachineKeyForZidLogin { ctx, .. } |
            PendingStorageOp::WriteZidSession { ctx, .. } |
            PendingStorageOp::ReadMachineKeyForZidEnroll { ctx, .. } |
            PendingStorageOp::WriteZidEnrollSession { ctx, .. } |
            PendingStorageOp::ReadIdentityPreferences { ctx, .. } |
            PendingStorageOp::ReadPreferencesForUpdate { ctx, .. } |
            PendingStorageOp::ReadPreferencesForDefaultMachine { ctx, .. } |
            PendingStorageOp::ReadPreferencesForZidLogin { ctx, .. } |
            PendingStorageOp::ReadZidSessionForRefresh { ctx, .. } |
            PendingStorageOp::WritePreferences { ctx, .. } |
            PendingStorageOp::WritePreferencesForDefaultMachine { ctx, .. } |
            PendingStorageOp::WritePreferencesForDefaultMachineRetry { ctx, .. } |
            PendingStorageOp::WriteRefreshedZidSession { ctx, .. } |
            PendingStorageOp::WriteZidEmailLoginSession { ctx, .. } |
            PendingStorageOp::DeleteZidSession { ctx } => {
                // Rule 5: These operations should NOT receive a mkdir response
                // This indicates a state machine bug - report it clearly
                syscall::debug(&format!(
                    "IdentityService: STATE_MACHINE_ERROR - unexpected VFS mkdir result for non-mkdir op, client_pid={}",
                    ctx.client_pid
                ));
                Err(AppError::Internal(
                    "State machine error: unexpected VFS mkdir result for non-mkdir operation".into()
                ))
            }
        }
    }

    /// Dispatch VFS readdir result to appropriate handler based on pending operation type.
    fn dispatch_vfs_readdir_result(
        &mut self,
        op: PendingStorageOp,
        result: Result<alloc::vec::Vec<zos_vfs::DirEntry>, alloc::string::String>,
    ) -> Result<(), AppError> {
        match op {
            PendingStorageOp::ListMachineKeys { ctx, user_id } => match result {
                Ok(entries) => {
                    // Convert directory entries to file paths and start reading machine keys
                    let paths: alloc::vec::Vec<alloc::string::String> = entries
                        .iter()
                        .filter(|e| e.name.ends_with(".json"))
                        .map(|e| format!("/home/{}/.zos/identity/machine/{}", user_id, e.name))
                        .collect();

                    if paths.is_empty() {
                        response::send_list_machine_keys(ctx.client_pid, &ctx.cap_slots, alloc::vec![])
                    } else {
                        let mut remaining_paths = paths;
                        let first_path = remaining_paths.remove(0);
                        self.start_vfs_read(
                            &first_path,
                            PendingStorageOp::ReadMachineKey {
                                ctx: RequestContext::new(ctx.client_pid, ctx.cap_slots),
                                user_id,
                                remaining_paths,
                                records: alloc::vec![],
                            },
                        )
                    }
                }
                Err(_) => {
                    // No machine keys directory or error - return empty list
                    response::send_list_machine_keys(ctx.client_pid, &ctx.cap_slots, alloc::vec![])
                }
            },
            PendingStorageOp::ReadMachineKeyForZidLogin {
                ctx,
                user_id,
                zid_endpoint,
            } => match result {
                Ok(entries) => {
                    let paths: alloc::vec::Vec<alloc::string::String> = entries
                        .iter()
                        .filter(|e| e.name.ends_with(".json"))
                        .map(|e| format!("/home/{}/.zos/identity/machine/{}", user_id, e.name))
                        .collect();
                    session::continue_zid_login_after_list(
                        self,
                        ctx.client_pid,
                        user_id,
                        zid_endpoint,
                        paths,
                        ctx.cap_slots,
                    )
                }
                Err(_) => response::send_zid_login_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    zos_identity::error::ZidError::MachineKeyNotFound,
                ),
            },
            PendingStorageOp::ReadMachineKeyForZidEnroll {
                ctx,
                user_id,
                zid_endpoint,
            } => match result {
                Ok(entries) => {
                    let paths: alloc::vec::Vec<alloc::string::String> = entries
                        .iter()
                        .filter(|e| e.name.ends_with(".json"))
                        .map(|e| format!("/home/{}/.zos/identity/machine/{}", user_id, e.name))
                        .collect();
                    session::continue_zid_enroll_after_list(
                        self,
                        ctx.client_pid,
                        user_id,
                        zid_endpoint,
                        paths,
                        ctx.cap_slots,
                    )
                }
                Err(_) => response::send_zid_enroll_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    zos_identity::error::ZidError::MachineKeyNotFound,
                ),
            },
            // Rule 5: Explicitly enumerate all remaining pending operation types
            // These operations don't expect a readdir response - if we get here, it's a logic error
            PendingStorageOp::CheckIdentityDirectory { ctx, .. } |
            PendingStorageOp::CreateIdentityDirectory { ctx, .. } |
            PendingStorageOp::CreateIdentityDirectoryComplete { ctx, .. } |
            PendingStorageOp::CheckKeyExists { ctx, .. } |
            PendingStorageOp::WriteKeyStore { ctx, .. } |
            PendingStorageOp::GetIdentityKey { ctx } |
            PendingStorageOp::ReadIdentityForRecovery { ctx, .. } |
            PendingStorageOp::WriteRecoveredKeyStore { ctx, .. } |
            PendingStorageOp::ReadIdentityForMachine { ctx, .. } |
            PendingStorageOp::WriteMachineKey { ctx, .. } |
            PendingStorageOp::ReadMachineKey { ctx, .. } |
            PendingStorageOp::DeleteMachineKey { ctx, .. } |
            PendingStorageOp::ReadMachineForRotate { ctx, .. } |
            PendingStorageOp::WriteRotatedMachineKey { ctx, .. } |
            PendingStorageOp::ReadSingleMachineKey { ctx } |
            PendingStorageOp::ReadCredentialsForAttach { ctx, .. } |
            PendingStorageOp::GetCredentials { ctx } |
            PendingStorageOp::ReadCredentialsForUnlink { ctx, .. } |
            PendingStorageOp::WriteUnlinkedCredential { ctx, .. } |
            PendingStorageOp::WriteEmailCredential { ctx, .. } |
            PendingStorageOp::WriteZidSession { ctx, .. } |
            PendingStorageOp::WriteZidEnrollSession { ctx, .. } |
            PendingStorageOp::ReadIdentityPreferences { ctx, .. } |
            PendingStorageOp::ReadPreferencesForUpdate { ctx, .. } |
            PendingStorageOp::ReadPreferencesForDefaultMachine { ctx, .. } |
            PendingStorageOp::ReadPreferencesForZidLogin { ctx, .. } |
            PendingStorageOp::ReadZidSessionForRefresh { ctx, .. } |
            PendingStorageOp::WritePreferences { ctx, .. } |
            PendingStorageOp::WritePreferencesForDefaultMachine { ctx, .. } |
            PendingStorageOp::WritePreferencesForDefaultMachineRetry { ctx, .. } |
            PendingStorageOp::WriteRefreshedZidSession { ctx, .. } |
            PendingStorageOp::WriteZidEmailLoginSession { ctx, .. } |
            PendingStorageOp::CreateCredentialsDirectory { ctx, .. } |
            PendingStorageOp::CreateIdentityDirForPreferences { ctx, .. } |
            PendingStorageOp::CreateDerivedUserDirectory { ctx, .. } |
            PendingStorageOp::WriteEmailCredentialRetry { ctx, .. } |
            PendingStorageOp::DeleteZidSession { ctx } => {
                // Rule 5: These operations should NOT receive a readdir response
                // This indicates a state machine bug - report it clearly
                syscall::debug(&format!(
                    "IdentityService: STATE_MACHINE_ERROR - unexpected VFS readdir result for non-readdir op, client_pid={}",
                    ctx.client_pid
                ));
                Err(AppError::Internal(
                    "State machine error: unexpected VFS readdir result for non-readdir operation".into()
                ))
            }
        }
    }

    /// Dispatch VFS unlink result to appropriate handler based on pending operation type.
    fn dispatch_vfs_unlink_result(
        &mut self,
        op: PendingStorageOp,
        result: Result<(), alloc::string::String>,
    ) -> Result<(), AppError> {
        match op {
            PendingStorageOp::DeleteMachineKey { ctx, .. } => {
                if result.is_ok() {
                    syscall::debug("IdentityService: Machine key deleted successfully via VFS");
                    response::send_revoke_machine_key_success(ctx.client_pid, &ctx.cap_slots)
                } else {
                    response::send_revoke_machine_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::MachineKeyNotFound,
                    )
                }
            }
            PendingStorageOp::DeleteZidSession { ctx } => {
                // Session delete - success even if file didn't exist (already logged out)
                if result.is_ok() {
                    syscall::debug("IdentityService: ZID session deleted successfully via VFS");
                } else {
                    // Log but don't fail - session file might not exist
                    syscall::debug("IdentityService: ZID session delete - file may not exist, treating as success");
                }
                response::send_zid_logout_success(ctx.client_pid, &ctx.cap_slots)
            }
            // Rule 5: Explicitly enumerate all remaining pending operation types
            // These operations don't expect an unlink response - if we get here, it's a logic error
            PendingStorageOp::CheckIdentityDirectory { ctx, .. } |
            PendingStorageOp::CreateIdentityDirectory { ctx, .. } |
            PendingStorageOp::CreateIdentityDirectoryComplete { ctx, .. } |
            PendingStorageOp::CreateDerivedUserDirectory { ctx, .. } |
            PendingStorageOp::CheckKeyExists { ctx, .. } |
            PendingStorageOp::WriteKeyStore { ctx, .. } |
            PendingStorageOp::GetIdentityKey { ctx } |
            PendingStorageOp::ReadIdentityForRecovery { ctx, .. } |
            PendingStorageOp::WriteRecoveredKeyStore { ctx, .. } |
            PendingStorageOp::ReadIdentityForMachine { ctx, .. } |
            PendingStorageOp::WriteMachineKey { ctx, .. } |
            PendingStorageOp::ListMachineKeys { ctx, .. } |
            PendingStorageOp::ReadMachineKey { ctx, .. } |
            PendingStorageOp::ReadMachineForRotate { ctx, .. } |
            PendingStorageOp::WriteRotatedMachineKey { ctx, .. } |
            PendingStorageOp::ReadSingleMachineKey { ctx } |
            PendingStorageOp::ReadCredentialsForAttach { ctx, .. } |
            PendingStorageOp::GetCredentials { ctx } |
            PendingStorageOp::ReadCredentialsForUnlink { ctx, .. } |
            PendingStorageOp::WriteUnlinkedCredential { ctx, .. } |
            PendingStorageOp::WriteEmailCredential { ctx, .. } |
            PendingStorageOp::ReadMachineKeyForZidLogin { ctx, .. } |
            PendingStorageOp::WriteZidSession { ctx, .. } |
            PendingStorageOp::ReadMachineKeyForZidEnroll { ctx, .. } |
            PendingStorageOp::WriteZidEnrollSession { ctx, .. } |
            PendingStorageOp::ReadIdentityPreferences { ctx, .. } |
            PendingStorageOp::ReadPreferencesForUpdate { ctx, .. } |
            PendingStorageOp::ReadPreferencesForDefaultMachine { ctx, .. } |
            PendingStorageOp::ReadPreferencesForZidLogin { ctx, .. } |
            PendingStorageOp::ReadZidSessionForRefresh { ctx, .. } |
            PendingStorageOp::WritePreferences { ctx, .. } |
            PendingStorageOp::WritePreferencesForDefaultMachine { ctx, .. } |
            PendingStorageOp::WritePreferencesForDefaultMachineRetry { ctx, .. } |
            PendingStorageOp::WriteRefreshedZidSession { ctx, .. } |
            PendingStorageOp::WriteZidEmailLoginSession { ctx, .. } |
            PendingStorageOp::CreateCredentialsDirectory { ctx, .. } |
            PendingStorageOp::CreateIdentityDirForPreferences { ctx, .. } |
            PendingStorageOp::WriteEmailCredentialRetry { ctx, .. } => {
                // Rule 5: These operations should NOT receive an unlink response
                // This indicates a state machine bug - report it clearly
                syscall::debug(&format!(
                    "IdentityService: STATE_MACHINE_ERROR - unexpected VFS unlink result for non-unlink op, client_pid={}",
                    ctx.client_pid
                ));
                Err(AppError::Internal(
                    "State machine error: unexpected VFS unlink result for non-unlink operation".into()
                ))
            }
        }
    }
}
