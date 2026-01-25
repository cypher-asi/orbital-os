//! Machine key storage handlers.

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use zos_identity::keystore::MachineKeyRecord;
use zos_identity::KeyError;
use zos_process::storage_result;
use zos_vfs::{parent_path, Inode};

use crate::identity::pending::PendingStorageOp;
use crate::identity::response;
use crate::identity::storage_handlers::StorageHandlerResult;
use crate::syscall;

/// Handle WriteMachineKeyContent result.
pub fn handle_write_machine_key_content(
    client_pid: u32,
    user_id: u128,
    record: MachineKeyRecord,
    json_bytes: Vec<u8>,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type != storage_result::WRITE_OK {
        return StorageHandlerResult::Done(response::send_create_machine_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError("Content write failed".into()),
        ));
    }

    let machine_path = MachineKeyRecord::storage_path(user_id, record.machine_id);
    let now = syscall::get_wallclock();
    let inode = Inode::new_file(
        machine_path.clone(),
        parent_path(&machine_path).to_string(),
        machine_path
            .rsplit('/')
            .next()
            .unwrap_or("machine.json")
            .to_string(),
        Some(user_id),
        json_bytes.len() as u64,
        None,
        now,
    );

    match serde_json::to_vec(&inode) {
        Ok(inode_json) => StorageHandlerResult::ContinueWrite {
            key: format!("inode:{}", machine_path),
            value: inode_json,
            next_op: PendingStorageOp::WriteMachineKeyInode {
                client_pid,
                record,
                cap_slots,
            },
        },
        Err(e) => StorageHandlerResult::Done(response::send_create_machine_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError(format!("Inode serialization failed: {}", e)),
        )),
    }
}

/// Handle WriteMachineKeyInode result (final step).
pub fn handle_write_machine_key_inode(
    client_pid: u32,
    record: MachineKeyRecord,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type == storage_result::WRITE_OK {
        syscall::debug(&format!(
            "IdentityService: Stored machine key {:032x} (content + inode)",
            record.machine_id
        ));
        StorageHandlerResult::Done(response::send_create_machine_key_success(
            client_pid,
            &cap_slots,
            record,
        ))
    } else {
        StorageHandlerResult::Done(response::send_create_machine_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError("Inode write failed".into()),
        ))
    }
}

/// Handle ListMachineKeys result.
pub fn handle_list_machine_keys(
    client_pid: u32,
    user_id: u128,
    cap_slots: Vec<u32>,
    result_type: u8,
    data: &[u8],
) -> StorageHandlerResult {
    if result_type != storage_result::LIST_OK {
        return StorageHandlerResult::Done(response::send_list_machine_keys(
            client_pid,
            &cap_slots,
            Vec::new(),
        ));
    }

    match serde_json::from_slice::<Vec<String>>(data) {
        Ok(paths) => {
            let json_paths: Vec<String> = paths
                .into_iter()
                .filter(|p| p.ends_with(".json"))
                .map(|p| format!("content:{}", p))
                .collect();

            if json_paths.is_empty() {
                return StorageHandlerResult::Done(response::send_list_machine_keys(
                    client_pid,
                    &cap_slots,
                    Vec::new(),
                ));
            }

            let mut remaining = json_paths;
            let first = remaining.remove(0);
            StorageHandlerResult::ContinueRead {
                key: first,
                next_op: PendingStorageOp::ReadMachineKey {
                    client_pid,
                    user_id,
                    remaining_paths: remaining,
                    records: Vec::new(),
                    cap_slots,
                },
            }
        }
        Err(_) => StorageHandlerResult::Done(response::send_list_machine_keys(
            client_pid,
            &cap_slots,
            Vec::new(),
        )),
    }
}

/// Handle ReadMachineKey result (iterative read).
pub fn handle_read_machine_key(
    client_pid: u32,
    user_id: u128,
    mut remaining_paths: Vec<String>,
    mut records: Vec<MachineKeyRecord>,
    cap_slots: Vec<u32>,
    result_type: u8,
    data: &[u8],
) -> StorageHandlerResult {
    if result_type == storage_result::READ_OK {
        if let Ok(record) = serde_json::from_slice::<MachineKeyRecord>(data) {
            records.push(record);
        }
    }

    if remaining_paths.is_empty() {
        syscall::debug(&format!(
            "IdentityService: Found {} machine keys",
            records.len()
        ));
        return StorageHandlerResult::Done(response::send_list_machine_keys(
            client_pid,
            &cap_slots,
            records,
        ));
    }

    let next = remaining_paths.remove(0);
    StorageHandlerResult::ContinueRead {
        key: next,
        next_op: PendingStorageOp::ReadMachineKey {
            client_pid,
            user_id,
            remaining_paths,
            records,
            cap_slots,
        },
    }
}

/// Handle DeleteMachineKey content result.
pub fn handle_delete_machine_key(
    client_pid: u32,
    user_id: u128,
    machine_id: u128,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type == storage_result::WRITE_OK {
        syscall::debug("IdentityService: Machine key content deleted, now deleting inode");
        let machine_path = MachineKeyRecord::storage_path(user_id, machine_id);
        StorageHandlerResult::ContinueDelete {
            key: format!("inode:{}", machine_path),
            next_op: PendingStorageOp::DeleteMachineKeyInode { client_pid, cap_slots },
        }
    } else {
        StorageHandlerResult::Done(response::send_revoke_machine_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError("Delete failed".into()),
        ))
    }
}

/// Handle DeleteMachineKeyInode result (final step).
pub fn handle_delete_machine_key_inode(
    client_pid: u32,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type == storage_result::WRITE_OK {
        syscall::debug("IdentityService: Machine key deleted (content + inode)");
        StorageHandlerResult::Done(response::send_revoke_machine_key_success(
            client_pid, &cap_slots,
        ))
    } else {
        StorageHandlerResult::Done(response::send_revoke_machine_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError("Inode delete failed".into()),
        ))
    }
}

/// Handle ReadSingleMachineKey result.
pub fn handle_read_single_machine_key(
    client_pid: u32,
    cap_slots: Vec<u32>,
    result_type: u8,
    data: &[u8],
) -> StorageHandlerResult {
    if result_type == storage_result::READ_OK {
        match serde_json::from_slice::<MachineKeyRecord>(data) {
            Ok(record) => StorageHandlerResult::Done(response::send_get_machine_key_success(
                client_pid,
                &cap_slots,
                Some(record),
            )),
            Err(e) => {
                syscall::debug(&format!(
                    "IdentityService: Failed to parse machine key: {}",
                    e
                ));
                StorageHandlerResult::Done(response::send_get_machine_key_error(
                    client_pid,
                    &cap_slots,
                    KeyError::StorageError(format!("Parse failed: {}", e)),
                ))
            }
        }
    } else {
        StorageHandlerResult::Done(response::send_get_machine_key_success(
            client_pid,
            &cap_slots,
            None,
        ))
    }
}

/// Handle WriteRotatedMachineKeyContent result.
pub fn handle_write_rotated_content(
    client_pid: u32,
    user_id: u128,
    record: MachineKeyRecord,
    json_bytes: Vec<u8>,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type != storage_result::WRITE_OK {
        return StorageHandlerResult::Done(response::send_rotate_machine_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError("Content write failed".into()),
        ));
    }

    let machine_path = MachineKeyRecord::storage_path(user_id, record.machine_id);
    let now = syscall::get_wallclock();
    let inode = Inode::new_file(
        machine_path.clone(),
        parent_path(&machine_path).to_string(),
        machine_path
            .rsplit('/')
            .next()
            .unwrap_or("machine.json")
            .to_string(),
        Some(user_id),
        json_bytes.len() as u64,
        None,
        now,
    );

    match serde_json::to_vec(&inode) {
        Ok(inode_json) => StorageHandlerResult::ContinueWrite {
            key: format!("inode:{}", machine_path),
            value: inode_json,
            next_op: PendingStorageOp::WriteRotatedMachineKeyInode {
                client_pid,
                record,
                cap_slots,
            },
        },
        Err(e) => StorageHandlerResult::Done(response::send_rotate_machine_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError(format!("Inode serialization failed: {}", e)),
        )),
    }
}

/// Handle WriteRotatedMachineKeyInode result (final step).
pub fn handle_write_rotated_inode(
    client_pid: u32,
    record: MachineKeyRecord,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type == storage_result::WRITE_OK {
        syscall::debug(&format!(
            "IdentityService: Rotated keys for machine {:032x} (epoch {}, content + inode)",
            record.machine_id, record.epoch
        ));
        StorageHandlerResult::Done(response::send_rotate_machine_key_success(
            client_pid,
            &cap_slots,
            record,
        ))
    } else {
        StorageHandlerResult::Done(response::send_rotate_machine_key_error(
            client_pid,
            &cap_slots,
            KeyError::StorageError("Inode write failed".into()),
        ))
    }
}
