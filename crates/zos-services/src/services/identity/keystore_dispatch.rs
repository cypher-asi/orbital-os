//! Keystore Result Dispatch
//!
//! Handles keystore IPC responses and dispatches to appropriate handlers.
//! This is the keystore equivalent of vfs_dispatch.rs.
//!
//! # Invariant 32 Compliance
//!
//! All `/keys/` paths are handled via Keystore IPC, keeping cryptographic key
//! material separate from the general filesystem. This module processes responses
//! from KeystoreService (PID 7) and routes them to continuation handlers.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::handlers::keys;
use super::pending::{PendingKeystoreOp, RequestContext};
use super::{response, IdentityService};
use zos_apps::syscall;
use zos_apps::{AppError, Message};
use zos_identity::keystore::{EncryptedShardStore, LocalKeyStore, MachineKeyRecord};
use zos_identity::KeyError;
use zos_ipc::keystore_svc;
use zos_vfs::client::keystore_async;

impl IdentityService {
    /// Handle keystore IPC response messages
    ///
    /// This dispatches keystore responses to the appropriate continuation handlers
    /// based on the pending operation type.
    pub fn handle_keystore_result(&mut self, msg: &Message) -> Result<(), AppError> {
        syscall::debug(&format!(
            "IdentityService: Received keystore result tag=0x{:x} (pending_ops={})",
            msg.tag,
            self.pending_keystore_ops.len()
        ));

        match msg.tag {
            keystore_svc::MSG_KEYSTORE_READ_RESPONSE => self.handle_keystore_read_response(msg),
            keystore_svc::MSG_KEYSTORE_WRITE_RESPONSE => self.handle_keystore_write_response(msg),
            keystore_svc::MSG_KEYSTORE_DELETE_RESPONSE => self.handle_keystore_delete_response(msg),
            keystore_svc::MSG_KEYSTORE_EXISTS_RESPONSE => self.handle_keystore_exists_response(msg),
            keystore_svc::MSG_KEYSTORE_LIST_RESPONSE => self.handle_keystore_list_response(msg),
            _ => {
                syscall::debug(&format!(
                    "IdentityService: Unexpected keystore response tag 0x{:x}",
                    msg.tag
                ));
                Ok(())
            }
        }
    }

    /// Take the next pending keystore operation (FIFO order).
    /// Returns None if no operations are pending.
    fn take_next_pending_keystore_op(&mut self) -> Option<PendingKeystoreOp> {
        // Get the smallest key (oldest operation)
        let key = *self.pending_keystore_ops.keys().next()?;
        self.pending_keystore_ops.remove(&key)
    }

    /// Handle keystore read response
    fn handle_keystore_read_response(&mut self, msg: &Message) -> Result<(), AppError> {
        let pending_op = match self.take_next_pending_keystore_op() {
            Some(op) => op,
            None => {
                syscall::debug("IdentityService: Keystore read response but no pending operation");
                return Ok(());
            }
        };

        // Parse keystore response
        let result = keystore_async::parse_read_response(&msg.data);

        // Dispatch based on operation type
        self.dispatch_keystore_read_result(pending_op, result)
    }

    /// Handle keystore write response
    fn handle_keystore_write_response(&mut self, msg: &Message) -> Result<(), AppError> {
        let pending_op = match self.take_next_pending_keystore_op() {
            Some(op) => op,
            None => {
                syscall::debug("IdentityService: Keystore write response but no pending operation");
                return Ok(());
            }
        };

        // Parse keystore response
        let result = keystore_async::parse_write_response(&msg.data);

        // Dispatch based on operation type
        self.dispatch_keystore_write_result(pending_op, result)
    }

    /// Handle keystore delete response
    fn handle_keystore_delete_response(&mut self, msg: &Message) -> Result<(), AppError> {
        let pending_op = match self.take_next_pending_keystore_op() {
            Some(op) => op,
            None => {
                syscall::debug("IdentityService: Keystore delete response but no pending operation");
                return Ok(());
            }
        };

        // Parse keystore response
        let result = keystore_async::parse_delete_response(&msg.data);

        // Dispatch based on operation type
        self.dispatch_keystore_delete_result(pending_op, result)
    }

    /// Handle keystore exists response
    fn handle_keystore_exists_response(&mut self, msg: &Message) -> Result<(), AppError> {
        let pending_op = match self.take_next_pending_keystore_op() {
            Some(op) => op,
            None => {
                syscall::debug("IdentityService: Keystore exists response but no pending operation");
                return Ok(());
            }
        };

        // Parse keystore response
        let result = keystore_async::parse_exists_response(&msg.data);

        // Dispatch based on operation type
        self.dispatch_keystore_exists_result(pending_op, result)
    }

    /// Handle keystore list response
    fn handle_keystore_list_response(&mut self, msg: &Message) -> Result<(), AppError> {
        let pending_op = match self.take_next_pending_keystore_op() {
            Some(op) => op,
            None => {
                syscall::debug("IdentityService: Keystore list response but no pending operation");
                return Ok(());
            }
        };

        // Parse keystore response
        let result = keystore_async::parse_list_response(&msg.data);

        // Dispatch based on operation type
        self.dispatch_keystore_list_result(pending_op, result)
    }

    // =========================================================================
    // Keystore result dispatchers
    // =========================================================================

    /// Dispatch keystore read result to appropriate handler based on pending operation type.
    fn dispatch_keystore_read_result(
        &mut self,
        op: PendingKeystoreOp,
        result: Result<Vec<u8>, String>,
    ) -> Result<(), AppError> {
        match op {
            PendingKeystoreOp::GetIdentityKey { ctx } => match result {
                Ok(data) => match serde_json::from_slice::<LocalKeyStore>(&data) {
                    Ok(key_store) => response::send_get_identity_key_success(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        Some(key_store),
                    ),
                    Err(e) => {
                        syscall::debug(&format!(
                            "IdentityService: Failed to parse stored keys from keystore: {}",
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
            PendingKeystoreOp::ReadIdentityForRecovery {
                ctx,
                user_id,
                zid_shards,
            } => match result {
                Ok(data) if !data.is_empty() => {
                    match serde_json::from_slice::<LocalKeyStore>(&data) {
                        Ok(key_store) => keys::continue_recover_after_identity_read(
                            self,
                            ctx.client_pid,
                            user_id,
                            zid_shards,
                            key_store.identity_signing_public_key,
                            ctx.cap_slots,
                        ),
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
                    syscall::debug("IdentityService: Identity read for recovery failed (keystore)");
                    response::send_recover_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::IdentityKeyRequired,
                    )
                }
            },
            PendingKeystoreOp::ReadIdentityForMachine { ctx, request } => match result {
                Ok(data) if !data.is_empty() => {
                    match serde_json::from_slice::<LocalKeyStore>(&data) {
                        Ok(key_store) => {
                            // Chain to read encrypted shards
                            let shards_path = EncryptedShardStore::storage_path(request.user_id);
                            syscall::debug(&format!(
                                "IdentityService: Identity read success, now reading encrypted shards from {}",
                                shards_path
                            ));
                            self.start_keystore_read(
                                &shards_path,
                                PendingKeystoreOp::ReadEncryptedShardsForMachine {
                                    ctx,
                                    request,
                                    stored_identity_pubkey: key_store.identity_signing_public_key,
                                },
                            )
                        }
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
                    syscall::debug("IdentityService: Identity read failed (keystore)");
                    response::send_create_machine_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::IdentityKeyRequired,
                    )
                }
            },
            PendingKeystoreOp::ReadEncryptedShardsForMachine {
                ctx,
                request,
                stored_identity_pubkey,
            } => match result {
                Ok(data) if !data.is_empty() => {
                    match serde_json::from_slice::<EncryptedShardStore>(&data) {
                        Ok(encrypted_store) => keys::continue_create_machine_after_shards_read(
                            self,
                            ctx.client_pid,
                            request,
                            stored_identity_pubkey,
                            encrypted_store,
                            ctx.cap_slots,
                        ),
                        Err(e) => {
                            syscall::debug(&format!(
                                "IdentityService: Failed to parse EncryptedShardStore: {}",
                                e
                            ));
                            response::send_create_machine_key_error(
                                ctx.client_pid,
                                &ctx.cap_slots,
                                KeyError::StorageError("Corrupted encrypted shard store".into()),
                            )
                        }
                    }
                }
                _ => {
                    syscall::debug("IdentityService: Encrypted shards not found (keystore)");
                    response::send_create_machine_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::EncryptedShardsNotFound,
                    )
                }
            },
            PendingKeystoreOp::ReadMachineKey {
                ctx,
                user_id,
                mut remaining_paths,
                mut records,
            } => {
                // Process this machine key result
                if let Ok(data) = result {
                    if let Ok(record) = serde_json::from_slice::<MachineKeyRecord>(&data) {
                        records.push(record);
                    }
                }

                // Continue reading remaining paths or send response
                if remaining_paths.is_empty() {
                    response::send_list_machine_keys(ctx.client_pid, &ctx.cap_slots, records)
                } else {
                    let next_path = remaining_paths.remove(0);
                    self.start_keystore_read(
                        &next_path,
                        PendingKeystoreOp::ReadMachineKey {
                            ctx: RequestContext::new(ctx.client_pid, ctx.cap_slots),
                            user_id,
                            remaining_paths,
                            records,
                        },
                    )
                }
            }
            PendingKeystoreOp::ReadSingleMachineKey { ctx } => match result {
                Ok(data) => match serde_json::from_slice::<MachineKeyRecord>(&data) {
                    Ok(record) => response::send_get_machine_key_success(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        Some(record),
                    ),
                    Err(_) => {
                        response::send_get_machine_key_success(ctx.client_pid, &ctx.cap_slots, None)
                    }
                },
                Err(_) => response::send_get_machine_key_success(ctx.client_pid, &ctx.cap_slots, None),
            },
            PendingKeystoreOp::ReadMachineForRotate {
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
            PendingKeystoreOp::ReadMachineKeyForZidLogin {
                ctx,
                user_id: _,
                zid_endpoint: _,
            } => {
                // ZID login reads machine key - for now just log (session handlers will be updated separately)
                syscall::debug("IdentityService: ReadMachineKeyForZidLogin via keystore - not yet migrated");
                response::send_zid_login_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    zos_identity::error::ZidError::MachineKeyNotFound,
                )
            }
            PendingKeystoreOp::ReadMachineKeyForZidEnroll {
                ctx,
                user_id: _,
                zid_endpoint: _,
            } => {
                // ZID enroll reads machine key - for now just log (session handlers will be updated separately)
                syscall::debug("IdentityService: ReadMachineKeyForZidEnroll via keystore - not yet migrated");
                response::send_zid_enroll_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    zos_identity::error::ZidError::MachineKeyNotFound,
                )
            }
            // Operations that should NOT receive a read response
            PendingKeystoreOp::CheckKeyExists { ctx, .. }
            | PendingKeystoreOp::WriteKeyStore { ctx, .. }
            | PendingKeystoreOp::WriteEncryptedShards { ctx, .. }
            | PendingKeystoreOp::WriteRecoveredKeyStore { ctx, .. }
            | PendingKeystoreOp::WriteMachineKey { ctx, .. }
            | PendingKeystoreOp::ListMachineKeys { ctx, .. }
            | PendingKeystoreOp::DeleteMachineKey { ctx, .. }
            | PendingKeystoreOp::DeleteIdentityKeyAfterShardFailure { ctx, .. }
            | PendingKeystoreOp::WriteRotatedMachineKey { ctx, .. } => {
                syscall::debug(&format!(
                    "IdentityService: STATE_MACHINE_ERROR - unexpected keystore read result for non-read op, client_pid={}",
                    ctx.client_pid
                ));
                Err(AppError::Internal(
                    "State machine error: unexpected keystore read result for non-read operation".into(),
                ))
            }
        }
    }

    /// Dispatch keystore write result to appropriate handler based on pending operation type.
    fn dispatch_keystore_write_result(
        &mut self,
        op: PendingKeystoreOp,
        result: Result<(), String>,
    ) -> Result<(), AppError> {
        match op {
            PendingKeystoreOp::WriteKeyStore {
                ctx,
                user_id,
                result: key_result,
                encrypted_shards_json,
                ..
            } => match result {
                Ok(()) => {
                    syscall::debug("IdentityService: Neural key stored successfully via Keystore, now writing encrypted shards");
                    // Chain to write encrypted shards
                    let shards_path = EncryptedShardStore::storage_path(user_id);
                    self.start_keystore_write(
                        &shards_path,
                        &encrypted_shards_json,
                        PendingKeystoreOp::WriteEncryptedShards {
                            ctx,
                            user_id,
                            result: key_result,
                        },
                    )
                }
                Err(e) => {
                    syscall::debug(&format!(
                        "IdentityService: WriteKeyStore failed - op=write_neural_key, error={}",
                        e
                    ));
                    response::send_neural_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::StorageError(format!("Keystore write failed for neural key: {}", e)),
                    )
                }
            },
            PendingKeystoreOp::WriteEncryptedShards {
                ctx,
                user_id,
                result: key_result,
                ..
            } => match result {
                Ok(()) => {
                    syscall::debug("IdentityService: Encrypted shards stored successfully via Keystore");
                    response::send_neural_key_success(ctx.client_pid, &ctx.cap_slots, key_result)
                }
                Err(e) => {
                    syscall::debug(&format!(
                        "IdentityService: WriteEncryptedShards failed - op=write_encrypted_shards, error={}",
                        e
                    ));
                    let key_path = LocalKeyStore::storage_path(user_id);
                    syscall::debug(&format!(
                        "IdentityService: Rolling back identity key store at {}",
                        key_path
                    ));
                    if let Err(err) = self.start_keystore_delete(
                        &key_path,
                        PendingKeystoreOp::DeleteIdentityKeyAfterShardFailure {
                            ctx: ctx.clone(),
                            user_id,
                        },
                    ) {
                        syscall::debug(&format!(
                            "IdentityService: Failed to schedule rollback delete: {:?}",
                            err
                        ));
                    }
                    response::send_neural_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::StorageError(format!("Keystore write failed for encrypted shards: {}", e)),
                    )
                }
            },
            PendingKeystoreOp::WriteRecoveredKeyStore {
                ctx,
                result: key_result,
                ..
            } => match result {
                Ok(()) => {
                    syscall::debug("IdentityService: Recovered key stored successfully via Keystore");
                    response::send_recover_key_success(ctx.client_pid, &ctx.cap_slots, key_result)
                }
                Err(e) => {
                    syscall::debug(&format!(
                        "IdentityService: WriteRecoveredKeyStore failed - op=recover_neural_key, error={}",
                        e
                    ));
                    response::send_recover_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::StorageError(format!("Keystore write failed for recovered key: {}", e)),
                    )
                }
            },
            PendingKeystoreOp::WriteMachineKey { ctx, record, .. } => match result {
                Ok(()) => {
                    syscall::debug(&format!(
                        "IdentityService: Machine key {:032x} stored successfully via Keystore",
                        record.machine_id
                    ));
                    response::send_create_machine_key_success(ctx.client_pid, &ctx.cap_slots, record)
                }
                Err(e) => {
                    syscall::debug(&format!(
                        "IdentityService: WriteMachineKey failed - op=create_machine_key, machine_id={:032x}, error={}",
                        record.machine_id, e
                    ));
                    response::send_create_machine_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::StorageError(format!("Keystore write failed for machine key: {}", e)),
                    )
                }
            },
            PendingKeystoreOp::WriteRotatedMachineKey { ctx, record, .. } => match result {
                Ok(()) => {
                    syscall::debug(&format!(
                        "IdentityService: Rotated machine key {:032x} stored successfully via Keystore",
                        record.machine_id
                    ));
                    response::send_rotate_machine_key_success(ctx.client_pid, &ctx.cap_slots, record)
                }
                Err(e) => {
                    syscall::debug(&format!(
                        "IdentityService: WriteRotatedMachineKey failed - op=rotate_machine_key, machine_id={:032x}, error={}",
                        record.machine_id, e
                    ));
                    response::send_rotate_machine_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::StorageError(format!("Keystore write failed for rotated key: {}", e)),
                    )
                }
            },
            // Operations that should NOT receive a write response
            PendingKeystoreOp::CheckKeyExists { ctx, .. }
            | PendingKeystoreOp::GetIdentityKey { ctx }
            | PendingKeystoreOp::ReadIdentityForRecovery { ctx, .. }
            | PendingKeystoreOp::ReadIdentityForMachine { ctx, .. }
            | PendingKeystoreOp::ReadEncryptedShardsForMachine { ctx, .. }
            | PendingKeystoreOp::ListMachineKeys { ctx, .. }
            | PendingKeystoreOp::ReadMachineKey { ctx, .. }
            | PendingKeystoreOp::DeleteMachineKey { ctx, .. }
            | PendingKeystoreOp::DeleteIdentityKeyAfterShardFailure { ctx, .. }
            | PendingKeystoreOp::ReadMachineForRotate { ctx, .. }
            | PendingKeystoreOp::ReadSingleMachineKey { ctx }
            | PendingKeystoreOp::ReadMachineKeyForZidLogin { ctx, .. }
            | PendingKeystoreOp::ReadMachineKeyForZidEnroll { ctx, .. } => {
                syscall::debug(&format!(
                    "IdentityService: STATE_MACHINE_ERROR - unexpected keystore write result for non-write op, client_pid={}",
                    ctx.client_pid
                ));
                Err(AppError::Internal(
                    "State machine error: unexpected keystore write result for non-write operation".into(),
                ))
            }
        }
    }

    /// Dispatch keystore exists result to appropriate handler based on pending operation type.
    fn dispatch_keystore_exists_result(
        &mut self,
        op: PendingKeystoreOp,
        result: Result<bool, String>,
    ) -> Result<(), AppError> {
        match op {
            PendingKeystoreOp::CheckKeyExists { ctx, user_id, password } => match result {
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
                        "IdentityService: Keystore exists check failed for key file: {}",
                        e
                    ));
                    response::send_neural_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::StorageError(format!("Key exists check failed: {}", e)),
                    )
                }
            },
            // Operations that should NOT receive an exists response
            PendingKeystoreOp::WriteKeyStore { ctx, .. }
            | PendingKeystoreOp::WriteEncryptedShards { ctx, .. }
            | PendingKeystoreOp::GetIdentityKey { ctx }
            | PendingKeystoreOp::ReadIdentityForRecovery { ctx, .. }
            | PendingKeystoreOp::WriteRecoveredKeyStore { ctx, .. }
            | PendingKeystoreOp::ReadIdentityForMachine { ctx, .. }
            | PendingKeystoreOp::ReadEncryptedShardsForMachine { ctx, .. }
            | PendingKeystoreOp::WriteMachineKey { ctx, .. }
            | PendingKeystoreOp::ListMachineKeys { ctx, .. }
            | PendingKeystoreOp::ReadMachineKey { ctx, .. }
            | PendingKeystoreOp::DeleteMachineKey { ctx, .. }
            | PendingKeystoreOp::DeleteIdentityKeyAfterShardFailure { ctx, .. }
            | PendingKeystoreOp::ReadMachineForRotate { ctx, .. }
            | PendingKeystoreOp::WriteRotatedMachineKey { ctx, .. }
            | PendingKeystoreOp::ReadSingleMachineKey { ctx }
            | PendingKeystoreOp::ReadMachineKeyForZidLogin { ctx, .. }
            | PendingKeystoreOp::ReadMachineKeyForZidEnroll { ctx, .. } => {
                syscall::debug(&format!(
                    "IdentityService: STATE_MACHINE_ERROR - unexpected keystore exists result for non-exists op, client_pid={}",
                    ctx.client_pid
                ));
                Err(AppError::Internal(
                    "State machine error: unexpected keystore exists result for non-exists operation".into(),
                ))
            }
        }
    }

    /// Dispatch keystore list result to appropriate handler based on pending operation type.
    fn dispatch_keystore_list_result(
        &mut self,
        op: PendingKeystoreOp,
        result: Result<Vec<String>, String>,
    ) -> Result<(), AppError> {
        match op {
            PendingKeystoreOp::ListMachineKeys { ctx, user_id } => match result {
                Ok(keys) => {
                    // Convert key list to paths and start reading machine keys
                    // Keys are returned as full paths from keystore list
                    let paths: Vec<String> = keys
                        .into_iter()
                        .filter(|k| k.ends_with(".json"))
                        .collect();

                    if paths.is_empty() {
                        response::send_list_machine_keys(ctx.client_pid, &ctx.cap_slots, alloc::vec![])
                    } else {
                        let mut remaining_paths = paths;
                        let first_path = remaining_paths.remove(0);
                        self.start_keystore_read(
                            &first_path,
                            PendingKeystoreOp::ReadMachineKey {
                                ctx: RequestContext::new(ctx.client_pid, ctx.cap_slots),
                                user_id,
                                remaining_paths,
                                records: alloc::vec![],
                            },
                        )
                    }
                }
                Err(_) => {
                    // No machine keys or error - return empty list
                    response::send_list_machine_keys(ctx.client_pid, &ctx.cap_slots, alloc::vec![])
                }
            },
            // Operations that should NOT receive a list response
            PendingKeystoreOp::CheckKeyExists { ctx, .. }
            | PendingKeystoreOp::WriteKeyStore { ctx, .. }
            | PendingKeystoreOp::WriteEncryptedShards { ctx, .. }
            | PendingKeystoreOp::GetIdentityKey { ctx }
            | PendingKeystoreOp::ReadIdentityForRecovery { ctx, .. }
            | PendingKeystoreOp::WriteRecoveredKeyStore { ctx, .. }
            | PendingKeystoreOp::ReadIdentityForMachine { ctx, .. }
            | PendingKeystoreOp::ReadEncryptedShardsForMachine { ctx, .. }
            | PendingKeystoreOp::WriteMachineKey { ctx, .. }
            | PendingKeystoreOp::ReadMachineKey { ctx, .. }
            | PendingKeystoreOp::DeleteMachineKey { ctx, .. }
            | PendingKeystoreOp::DeleteIdentityKeyAfterShardFailure { ctx, .. }
            | PendingKeystoreOp::ReadMachineForRotate { ctx, .. }
            | PendingKeystoreOp::WriteRotatedMachineKey { ctx, .. }
            | PendingKeystoreOp::ReadSingleMachineKey { ctx }
            | PendingKeystoreOp::ReadMachineKeyForZidLogin { ctx, .. }
            | PendingKeystoreOp::ReadMachineKeyForZidEnroll { ctx, .. } => {
                syscall::debug(&format!(
                    "IdentityService: STATE_MACHINE_ERROR - unexpected keystore list result for non-list op, client_pid={}",
                    ctx.client_pid
                ));
                Err(AppError::Internal(
                    "State machine error: unexpected keystore list result for non-list operation".into(),
                ))
            }
        }
    }

    /// Dispatch keystore delete result to appropriate handler based on pending operation type.
    fn dispatch_keystore_delete_result(
        &mut self,
        op: PendingKeystoreOp,
        result: Result<(), String>,
    ) -> Result<(), AppError> {
        match op {
            PendingKeystoreOp::DeleteMachineKey { ctx, .. } => {
                if result.is_ok() {
                    syscall::debug("IdentityService: Machine key deleted successfully via Keystore");
                    response::send_revoke_machine_key_success(ctx.client_pid, &ctx.cap_slots)
                } else {
                    response::send_revoke_machine_key_error(
                        ctx.client_pid,
                        &ctx.cap_slots,
                        KeyError::MachineKeyNotFound,
                    )
                }
            }
            PendingKeystoreOp::DeleteIdentityKeyAfterShardFailure { ctx: _, user_id } => {
                if result.is_ok() {
                    syscall::debug(&format!(
                        "IdentityService: Rolled back identity key store for user {:032x}",
                        user_id
                    ));
                } else {
                    syscall::debug(&format!(
                        "IdentityService: Failed to roll back identity key store for user {:032x}",
                        user_id
                    ));
                }
                Ok(())
            }
            // Operations that should NOT receive a delete response
            PendingKeystoreOp::CheckKeyExists { ctx, .. }
            | PendingKeystoreOp::WriteKeyStore { ctx, .. }
            | PendingKeystoreOp::WriteEncryptedShards { ctx, .. }
            | PendingKeystoreOp::GetIdentityKey { ctx }
            | PendingKeystoreOp::ReadIdentityForRecovery { ctx, .. }
            | PendingKeystoreOp::WriteRecoveredKeyStore { ctx, .. }
            | PendingKeystoreOp::ReadIdentityForMachine { ctx, .. }
            | PendingKeystoreOp::ReadEncryptedShardsForMachine { ctx, .. }
            | PendingKeystoreOp::WriteMachineKey { ctx, .. }
            | PendingKeystoreOp::ListMachineKeys { ctx, .. }
            | PendingKeystoreOp::ReadMachineKey { ctx, .. }
            | PendingKeystoreOp::ReadMachineForRotate { ctx, .. }
            | PendingKeystoreOp::WriteRotatedMachineKey { ctx, .. }
            | PendingKeystoreOp::ReadSingleMachineKey { ctx }
            | PendingKeystoreOp::ReadMachineKeyForZidLogin { ctx, .. }
            | PendingKeystoreOp::ReadMachineKeyForZidEnroll { ctx, .. } => {
                syscall::debug(&format!(
                    "IdentityService: STATE_MACHINE_ERROR - unexpected keystore delete result for non-delete op, client_pid={}",
                    ctx.client_pid
                ));
                Err(AppError::Internal(
                    "State machine error: unexpected keystore delete result for non-delete operation".into(),
                ))
            }
        }
    }
}
