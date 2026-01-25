//! Neural key generation and recovery handlers.

extern crate alloc;

use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;
use zos_identity::ipc::NeuralKeyGenerated;
use zos_identity::keystore::LocalKeyStore;
use zos_identity::KeyError;
use zos_process::storage_result;
use zos_vfs::{parent_path, Inode};

use crate::identity::pending::PendingStorageOp;
use crate::identity::response;
use crate::identity::storage_handlers::StorageHandlerResult;
use crate::syscall;

/// Handle CheckKeyExists result for neural key generation.
pub fn handle_check_key_exists(
    client_pid: u32,
    user_id: u128,
    cap_slots: Vec<u32>,
    result_type: u8,
    data: &[u8],
) -> (bool, u32, u128, Vec<u32>) {
    let exists = if result_type == storage_result::EXISTS_OK {
        !data.is_empty() && data[0] == 1
    } else {
        false
    };
    (exists, client_pid, user_id, cap_slots)
}

/// Handle WriteKeyStoreContent result.
pub fn handle_write_key_store_content(
    client_pid: u32,
    user_id: u128,
    result: NeuralKeyGenerated,
    json_bytes: Vec<u8>,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type != storage_result::WRITE_OK {
        syscall::debug(&format!(
            "IdentityService: Neural key content write failed for user {:032x}, result_type={}",
            user_id, result_type
        ));
        return StorageHandlerResult::Done(response::send_neural_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError(format!(
                "Content write failed (result_type={}). Parent directory may not exist.",
                result_type
            )),
        ));
    }

    // Step 2: Write the inode
    let key_path = LocalKeyStore::storage_path(user_id);
    let now = syscall::get_wallclock();
    let inode = Inode::new_file(
        key_path.clone(),
        parent_path(&key_path).to_string(),
        key_path
            .rsplit('/')
            .next()
            .unwrap_or("keys.json")
            .to_string(),
        Some(user_id),
        json_bytes.len() as u64,
        None,
        now,
    );

    match serde_json::to_vec(&inode) {
        Ok(inode_json) => StorageHandlerResult::ContinueWrite {
            key: format!("inode:{}", key_path),
            value: inode_json,
            next_op: PendingStorageOp::WriteKeyStoreInode {
                client_pid,
                result,
                cap_slots,
            },
        },
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Failed to serialize inode: {}",
                e
            ));
            StorageHandlerResult::Done(response::send_neural_key_error(
                client_pid,
                &cap_slots,
                KeyError::StorageError(format!("Inode serialization failed: {}", e)),
            ))
        }
    }
}

/// Handle WriteKeyStoreInode result (final step).
pub fn handle_write_key_store_inode(
    client_pid: u32,
    result: NeuralKeyGenerated,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type == storage_result::WRITE_OK {
        syscall::debug("IdentityService: Neural key stored successfully (content + inode)");
        StorageHandlerResult::Done(response::send_neural_key_success(
            client_pid, &cap_slots, result,
        ))
    } else {
        syscall::debug(&format!(
            "IdentityService: Neural key inode write failed, result_type={}",
            result_type
        ));
        StorageHandlerResult::Done(response::send_neural_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError(format!("Inode write failed (result_type={})", result_type)),
        ))
    }
}

/// Handle GetIdentityKey result.
pub fn handle_get_identity_key(
    client_pid: u32,
    cap_slots: Vec<u32>,
    result_type: u8,
    data: &[u8],
) -> StorageHandlerResult {
    if result_type == storage_result::READ_OK {
        match serde_json::from_slice::<LocalKeyStore>(data) {
            Ok(key_store) => StorageHandlerResult::Done(response::send_get_identity_key_success(
                client_pid,
                &cap_slots,
                Some(key_store),
            )),
            Err(e) => {
                syscall::debug(&format!(
                    "IdentityService: Failed to parse stored keys: {}",
                    e
                ));
                StorageHandlerResult::Done(response::send_get_identity_key_error(
                    client_pid,
                    &cap_slots,
                    KeyError::StorageError(format!("Parse failed: {}", e)),
                ))
            }
        }
    } else {
        // Key not found
        StorageHandlerResult::Done(response::send_get_identity_key_success(
            client_pid, &cap_slots, None,
        ))
    }
}

/// Handle WriteRecoveredKeyStoreContent result.
pub fn handle_write_recovered_content(
    client_pid: u32,
    user_id: u128,
    result: NeuralKeyGenerated,
    json_bytes: Vec<u8>,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type != storage_result::WRITE_OK {
        return StorageHandlerResult::Done(response::send_recover_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError("Content write failed".into()),
        ));
    }

    let key_path = LocalKeyStore::storage_path(user_id);
    let now = syscall::get_wallclock();
    let inode = Inode::new_file(
        key_path.clone(),
        parent_path(&key_path).to_string(),
        key_path
            .rsplit('/')
            .next()
            .unwrap_or("keys.json")
            .to_string(),
        Some(user_id),
        json_bytes.len() as u64,
        None,
        now,
    );

    match serde_json::to_vec(&inode) {
        Ok(inode_json) => StorageHandlerResult::ContinueWrite {
            key: format!("inode:{}", key_path),
            value: inode_json,
            next_op: PendingStorageOp::WriteRecoveredKeyStoreInode {
                client_pid,
                result,
                cap_slots,
            },
        },
        Err(e) => StorageHandlerResult::Done(response::send_recover_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError(format!("Inode serialization failed: {}", e)),
        )),
    }
}

/// Handle WriteRecoveredKeyStoreInode result (final step).
pub fn handle_write_recovered_inode(
    client_pid: u32,
    result: NeuralKeyGenerated,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type == storage_result::WRITE_OK {
        syscall::debug("IdentityService: Recovered key stored (content + inode)");
        StorageHandlerResult::Done(response::send_recover_key_success(
            client_pid, &cap_slots, result,
        ))
    } else {
        StorageHandlerResult::Done(response::send_recover_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError("Inode write failed".into()),
        ))
    }
}
