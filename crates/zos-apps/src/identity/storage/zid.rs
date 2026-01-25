//! ZID session storage handlers.

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use zos_identity::error::ZidError;
use zos_process::storage_result;

use crate::identity::response;
use crate::identity::storage_handlers::StorageHandlerResult;
use crate::syscall;

/// Handle WriteZidSessionInode result (final step).
pub fn handle_write_zid_session_inode(
    client_pid: u32,
    tokens: zos_identity::ipc::ZidTokens,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type == storage_result::WRITE_OK {
        syscall::debug("IdentityService: ZID session stored successfully");
        StorageHandlerResult::Done(response::send_zid_login_success(
            client_pid, &cap_slots, tokens,
        ))
    } else {
        StorageHandlerResult::Done(response::send_zid_login_error(
            client_pid,
            &cap_slots,
            ZidError::NetworkError("Session inode write failed".into()),
        ))
    }
}

/// Handle WriteZidEnrollSessionInode result (final step).
pub fn handle_write_zid_enroll_session_inode(
    client_pid: u32,
    tokens: zos_identity::ipc::ZidTokens,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type == storage_result::WRITE_OK {
        syscall::debug("IdentityService: ZID enrollment session stored successfully");
        StorageHandlerResult::Done(response::send_zid_enroll_success(
            client_pid, &cap_slots, tokens,
        ))
    } else {
        StorageHandlerResult::Done(response::send_zid_enroll_error(
            client_pid,
            &cap_slots,
            ZidError::EnrollmentFailed("Session inode write failed".into()),
        ))
    }
}

/// Handle ReadMachineKeyForZidLogin - can be LIST or READ result.
pub fn handle_read_machine_for_zid_login(
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    cap_slots: Vec<u32>,
    result_type: u8,
    data: &[u8],
) -> Result<ZidLoginReadResult, Box<StorageHandlerResult>> {
    if result_type == storage_result::LIST_OK {
        let paths: Vec<String> = if !data.is_empty() {
            serde_json::from_slice(data).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(ZidLoginReadResult::PathList {
            paths,
            client_pid,
            user_id,
            zid_endpoint,
            cap_slots,
        })
    } else if result_type == storage_result::READ_OK && !data.is_empty() {
        Ok(ZidLoginReadResult::MachineKeyData {
            data: data.to_vec(),
            client_pid,
            user_id,
            zid_endpoint,
            cap_slots,
        })
    } else if result_type == storage_result::NOT_FOUND {
        Err(Box::new(StorageHandlerResult::Done(response::send_zid_login_error(
            client_pid,
            &cap_slots,
            ZidError::MachineKeyNotFound,
        ))))
    } else {
        Err(Box::new(StorageHandlerResult::Done(response::send_zid_login_error(
            client_pid,
            &cap_slots,
            ZidError::NetworkError("Storage read failed".into()),
        ))))
    }
}

/// Result of reading machine key for ZID login.
pub enum ZidLoginReadResult {
    PathList {
        paths: Vec<String>,
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        cap_slots: Vec<u32>,
    },
    MachineKeyData {
        data: Vec<u8>,
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        cap_slots: Vec<u32>,
    },
}

/// Handle ReadMachineKeyForZidEnroll - can be LIST or READ result.
pub fn handle_read_machine_for_zid_enroll(
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    cap_slots: Vec<u32>,
    result_type: u8,
    data: &[u8],
) -> Result<ZidEnrollReadResult, Box<StorageHandlerResult>> {
    if result_type == storage_result::LIST_OK {
        let paths: Vec<String> = if !data.is_empty() {
            serde_json::from_slice(data).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(ZidEnrollReadResult::PathList {
            paths,
            client_pid,
            user_id,
            zid_endpoint,
            cap_slots,
        })
    } else if result_type == storage_result::READ_OK && !data.is_empty() {
        Ok(ZidEnrollReadResult::MachineKeyData {
            data: data.to_vec(),
            client_pid,
            user_id,
            zid_endpoint,
            cap_slots,
        })
    } else if result_type == storage_result::NOT_FOUND {
        Err(Box::new(StorageHandlerResult::Done(response::send_zid_enroll_error(
            client_pid,
            &cap_slots,
            ZidError::MachineKeyNotFound,
        ))))
    } else {
        Err(Box::new(StorageHandlerResult::Done(response::send_zid_enroll_error(
            client_pid,
            &cap_slots,
            ZidError::NetworkError("Storage read failed".into()),
        ))))
    }
}

/// Result of reading machine key for ZID enrollment.
pub enum ZidEnrollReadResult {
    PathList {
        paths: Vec<String>,
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        cap_slots: Vec<u32>,
    },
    MachineKeyData {
        data: Vec<u8>,
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        cap_slots: Vec<u32>,
    },
}
