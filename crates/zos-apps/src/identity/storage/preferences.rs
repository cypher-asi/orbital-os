//! Storage handlers for identity preferences

extern crate alloc;

use alloc::vec::Vec;
use crate::identity::response;
use crate::identity::storage_handlers::StorageHandlerResult;
use zos_identity::ipc::{GetIdentityPreferencesResponse, IdentityPreferences, SetDefaultKeySchemeResponse};
use zos_process::storage_result;

/// Handle read identity preferences result
pub fn handle_read_identity_preferences(
    client_pid: u32,
    user_id: u128,
    cap_slots: Vec<u32>,
    result_type: u8,
    data: &[u8],
) -> StorageHandlerResult {
    let preferences = if result_type == storage_result::READ_OK && !data.is_empty() {
        serde_json::from_slice::<IdentityPreferences>(data).unwrap_or_default()
    } else {
        // File doesn't exist yet, return default preferences
        IdentityPreferences::default()
    };

    let response = GetIdentityPreferencesResponse { preferences };
    let result = response::send_get_identity_preferences_response(client_pid, &cap_slots, response);
    StorageHandlerResult::Done(result)
}

/// Handle write preferences inode result (final step)
pub fn handle_write_preferences_inode(
    client_pid: u32,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> StorageHandlerResult {
    if result_type == storage_result::WRITE_OK {
        let response = SetDefaultKeySchemeResponse { result: Ok(()) };
        let result = response::send_set_default_key_scheme_response(client_pid, &cap_slots, response);
        StorageHandlerResult::Done(result)
    } else {
        let result = response::send_set_default_key_scheme_error(
            client_pid,
            &cap_slots,
            zos_identity::KeyError::StorageError("Inode write failed".into()),
        );
        StorageHandlerResult::Done(result)
    }
}
