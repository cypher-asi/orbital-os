//! Credential storage handlers.

extern crate alloc;

use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;
use zos_identity::error::CredentialError;
use zos_identity::keystore::CredentialStore;
use zos_process::storage_result;
use zos_vfs::{parent_path, Inode};

use crate::identity::pending::PendingStorageOp;
use crate::identity::response;
use crate::identity::storage_handlers::StorageHandlerResult;
use crate::syscall;

/// Handle GetCredentials result.
pub fn handle_get_credentials(
    client_pid: u32,
    cap_slots: Vec<u32>,
    result_type: u8,
    data: &[u8],
) -> StorageHandlerResult {
    if result_type == storage_result::READ_OK && !data.is_empty() {
        match serde_json::from_slice::<CredentialStore>(data) {
            Ok(store) => StorageHandlerResult::Done(response::send_get_credentials(
                client_pid,
                &cap_slots,
                store.credentials,
            )),
            Err(e) => {
                syscall::debug(&format!(
                    "IdentityService: Failed to parse credentials: {}",
                    e
                ));
                StorageHandlerResult::Done(response::send_get_credentials(
                    client_pid,
                    &cap_slots,
                    Vec::new(),
                ))
            }
        }
    } else {
        StorageHandlerResult::Done(response::send_get_credentials(
            client_pid,
            &cap_slots,
            Vec::new(),
        ))
    }
}

/// Handle WriteUnlinkedCredentialContent result.
pub fn handle_write_unlinked_content(
    client_pid: u32,
    user_id: u128,
    json_bytes: Vec<u8>,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type != storage_result::WRITE_OK {
        return StorageHandlerResult::Done(response::send_unlink_credential_error(
            client_pid,
            &cap_slots,
            CredentialError::StorageError("Content write failed".into()),
        ));
    }

    let cred_path = CredentialStore::storage_path(user_id);
    let now = syscall::get_wallclock();
    let inode = Inode::new_file(
        cred_path.clone(),
        parent_path(&cred_path).to_string(),
        cred_path
            .rsplit('/')
            .next()
            .unwrap_or("credentials.json")
            .to_string(),
        Some(user_id),
        json_bytes.len() as u64,
        None,
        now,
    );

    match serde_json::to_vec(&inode) {
        Ok(inode_json) => StorageHandlerResult::ContinueWrite {
            key: format!("inode:{}", cred_path),
            value: inode_json,
            next_op: PendingStorageOp::WriteUnlinkedCredentialInode { client_pid, cap_slots },
        },
        Err(e) => StorageHandlerResult::Done(response::send_unlink_credential_error(
            client_pid,
            &cap_slots,
            CredentialError::StorageError(format!("Inode serialization failed: {}", e)),
        )),
    }
}

/// Handle WriteUnlinkedCredentialInode result (final step).
pub fn handle_write_unlinked_inode(
    client_pid: u32,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type == storage_result::WRITE_OK {
        syscall::debug("IdentityService: Credential unlinked (content + inode)");
        StorageHandlerResult::Done(response::send_unlink_credential_success(
            client_pid, &cap_slots,
        ))
    } else {
        StorageHandlerResult::Done(response::send_unlink_credential_error(
            client_pid,
            &cap_slots,
            CredentialError::StorageError("Inode write failed".into()),
        ))
    }
}

/// Handle WriteEmailCredentialContent result.
pub fn handle_write_email_cred_content(
    client_pid: u32,
    user_id: u128,
    json_bytes: Vec<u8>,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type != storage_result::WRITE_OK {
        return StorageHandlerResult::Done(response::send_attach_email_error(
            client_pid,
            &cap_slots,
            CredentialError::StorageError("Content write failed".into()),
        ));
    }

    let cred_path = CredentialStore::storage_path(user_id);
    let now = syscall::get_wallclock();
    let inode = Inode::new_file(
        cred_path.clone(),
        parent_path(&cred_path).to_string(),
        cred_path
            .rsplit('/')
            .next()
            .unwrap_or("credentials.json")
            .to_string(),
        Some(user_id),
        json_bytes.len() as u64,
        None,
        now,
    );

    match serde_json::to_vec(&inode) {
        Ok(inode_json) => StorageHandlerResult::ContinueWrite {
            key: format!("inode:{}", cred_path),
            value: inode_json,
            next_op: PendingStorageOp::WriteEmailCredentialInode { client_pid, cap_slots },
        },
        Err(e) => StorageHandlerResult::Done(response::send_attach_email_error(
            client_pid,
            &cap_slots,
            CredentialError::StorageError(format!("Inode serialization failed: {}", e)),
        )),
    }
}

/// Handle WriteEmailCredentialInode result (final step).
pub fn handle_write_email_cred_inode(
    client_pid: u32,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type == storage_result::WRITE_OK {
        syscall::debug("IdentityService: Email credential stored via ZID (content + inode)");
        StorageHandlerResult::Done(response::send_attach_email_success(client_pid, &cap_slots))
    } else {
        StorageHandlerResult::Done(response::send_attach_email_error(
            client_pid,
            &cap_slots,
            CredentialError::StorageError("Inode write failed".into()),
        ))
    }
}
