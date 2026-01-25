//! Response helper functions for identity service
//!
//! This module provides typed response builders for all identity service IPC responses.

extern crate alloc;

use crate::error::AppError;
use crate::syscall;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use zos_identity::error::{CredentialError, ZidError};
use zos_identity::ipc::{
    AttachEmailResponse, CreateMachineKeyResponse, GenerateNeuralKeyResponse,
    GetCredentialsResponse, GetIdentityKeyResponse, GetMachineKeyResponse, ListMachineKeysResponse,
    RecoverNeuralKeyResponse, RevokeMachineKeyResponse, RotateMachineKeyResponse,
    UnlinkCredentialResponse, ZidEnrollMachineResponse, ZidLoginResponse, ZidTokens,
};
use zos_identity::keystore::{LinkedCredential, LocalKeyStore, MachineKeyRecord};
use zos_identity::KeyError;
use zos_process::{identity_cred, identity_key, identity_machine, identity_zid};

/// Send a generic serialized response to a specific PID via debug channel routing.
pub fn send_response_to_pid<T: serde::Serialize>(
    to_pid: u32,
    cap_slots: &[u32],
    tag: u32,
    response: &T,
) -> Result<(), AppError> {
    match serde_json::to_vec(response) {
        Ok(data) => {
            // Try to send via transferred reply capability first
            if let Some(&reply_slot) = cap_slots.first() {
                syscall::debug(&format!(
                    "IdentityService: Sending response via reply cap slot {} (tag 0x{:x})",
                    reply_slot, tag
                ));
                match syscall::send(reply_slot, tag, &data) {
                    Ok(()) => {
                        syscall::debug("IdentityService: Response sent via reply cap");
                        return Ok(());
                    }
                    Err(e) => {
                        syscall::debug(&format!(
                            "IdentityService: Reply cap send failed ({}), falling back to debug channel",
                            e
                        ));
                    }
                }
            }

            // Fallback: send via debug channel for supervisor to route
            let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
            syscall::debug(&format!("SERVICE:RESPONSE:{}:{:08x}:{}", to_pid, tag, hex));
            Ok(())
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Failed to serialize response: {}",
                e
            ));
            Err(AppError::IpcError(format!("Serialization failed: {}", e)))
        }
    }
}

// =============================================================================
// Neural Key responses
// =============================================================================

/// Send neural key generation success response.
pub fn send_neural_key_success(
    client_pid: u32,
    cap_slots: &[u32],
    result: zos_identity::ipc::NeuralKeyGenerated,
) -> Result<(), AppError> {
    let response = GenerateNeuralKeyResponse { result: Ok(result) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_key::MSG_GENERATE_NEURAL_KEY_RESPONSE,
        &response,
    )
}

/// Send neural key generation error response.
pub fn send_neural_key_error(
    client_pid: u32,
    cap_slots: &[u32],
    error: KeyError,
) -> Result<(), AppError> {
    syscall::debug(&format!(
        "IdentityService: Sending neural key error to PID {}: {:?}",
        client_pid, error
    ));
    let response = GenerateNeuralKeyResponse { result: Err(error) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_key::MSG_GENERATE_NEURAL_KEY_RESPONSE,
        &response,
    )
}

/// Send neural key recovery success response.
pub fn send_recover_key_success(
    client_pid: u32,
    cap_slots: &[u32],
    result: zos_identity::ipc::NeuralKeyGenerated,
) -> Result<(), AppError> {
    let response = RecoverNeuralKeyResponse { result: Ok(result) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_key::MSG_RECOVER_NEURAL_KEY_RESPONSE,
        &response,
    )
}

/// Send neural key recovery error response.
pub fn send_recover_key_error(
    client_pid: u32,
    cap_slots: &[u32],
    error: KeyError,
) -> Result<(), AppError> {
    let response = RecoverNeuralKeyResponse { result: Err(error) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_key::MSG_RECOVER_NEURAL_KEY_RESPONSE,
        &response,
    )
}

/// Send get identity key success response.
pub fn send_get_identity_key_success(
    client_pid: u32,
    cap_slots: &[u32],
    key_store: Option<LocalKeyStore>,
) -> Result<(), AppError> {
    let response = GetIdentityKeyResponse {
        result: Ok(key_store),
    };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_key::MSG_GET_IDENTITY_KEY_RESPONSE,
        &response,
    )
}

/// Send get identity key error response.
pub fn send_get_identity_key_error(
    client_pid: u32,
    cap_slots: &[u32],
    error: KeyError,
) -> Result<(), AppError> {
    let response = GetIdentityKeyResponse { result: Err(error) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_key::MSG_GET_IDENTITY_KEY_RESPONSE,
        &response,
    )
}

// =============================================================================
// Machine Key responses
// =============================================================================

/// Send create machine key success response.
pub fn send_create_machine_key_success(
    client_pid: u32,
    cap_slots: &[u32],
    record: MachineKeyRecord,
) -> Result<(), AppError> {
    let response = CreateMachineKeyResponse { result: Ok(record) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_machine::MSG_CREATE_MACHINE_KEY_RESPONSE,
        &response,
    )
}

/// Send create machine key error response.
pub fn send_create_machine_key_error(
    client_pid: u32,
    cap_slots: &[u32],
    error: KeyError,
) -> Result<(), AppError> {
    let response = CreateMachineKeyResponse { result: Err(error) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_machine::MSG_CREATE_MACHINE_KEY_RESPONSE,
        &response,
    )
}

/// Send list machine keys response.
pub fn send_list_machine_keys(
    client_pid: u32,
    cap_slots: &[u32],
    machines: Vec<MachineKeyRecord>,
) -> Result<(), AppError> {
    let response = ListMachineKeysResponse { machines };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_machine::MSG_LIST_MACHINE_KEYS_RESPONSE,
        &response,
    )
}

/// Send get machine key success response.
pub fn send_get_machine_key_success(
    client_pid: u32,
    cap_slots: &[u32],
    record: Option<MachineKeyRecord>,
) -> Result<(), AppError> {
    let response = GetMachineKeyResponse { result: Ok(record) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_machine::MSG_GET_MACHINE_KEY_RESPONSE,
        &response,
    )
}

/// Send get machine key error response.
pub fn send_get_machine_key_error(
    client_pid: u32,
    cap_slots: &[u32],
    error: KeyError,
) -> Result<(), AppError> {
    let response = GetMachineKeyResponse { result: Err(error) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_machine::MSG_GET_MACHINE_KEY_RESPONSE,
        &response,
    )
}

/// Send revoke machine key success response.
pub fn send_revoke_machine_key_success(client_pid: u32, cap_slots: &[u32]) -> Result<(), AppError> {
    let response = RevokeMachineKeyResponse { result: Ok(()) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_machine::MSG_REVOKE_MACHINE_KEY_RESPONSE,
        &response,
    )
}

/// Send revoke machine key error response.
pub fn send_revoke_machine_key_error(
    client_pid: u32,
    cap_slots: &[u32],
    error: KeyError,
) -> Result<(), AppError> {
    let response = RevokeMachineKeyResponse { result: Err(error) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_machine::MSG_REVOKE_MACHINE_KEY_RESPONSE,
        &response,
    )
}

/// Send rotate machine key success response.
pub fn send_rotate_machine_key_success(
    client_pid: u32,
    cap_slots: &[u32],
    record: MachineKeyRecord,
) -> Result<(), AppError> {
    let response = RotateMachineKeyResponse { result: Ok(record) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_machine::MSG_ROTATE_MACHINE_KEY_RESPONSE,
        &response,
    )
}

/// Send rotate machine key error response.
pub fn send_rotate_machine_key_error(
    client_pid: u32,
    cap_slots: &[u32],
    error: KeyError,
) -> Result<(), AppError> {
    let response = RotateMachineKeyResponse { result: Err(error) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_machine::MSG_ROTATE_MACHINE_KEY_RESPONSE,
        &response,
    )
}

// =============================================================================
// Credential responses
// =============================================================================

/// Send attach email success response.
pub fn send_attach_email_success(client_pid: u32, cap_slots: &[u32]) -> Result<(), AppError> {
    let response = AttachEmailResponse { result: Ok(()) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_cred::MSG_ATTACH_EMAIL_RESPONSE,
        &response,
    )
}

/// Send attach email error response.
pub fn send_attach_email_error(
    client_pid: u32,
    cap_slots: &[u32],
    error: CredentialError,
) -> Result<(), AppError> {
    let response = AttachEmailResponse { result: Err(error) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_cred::MSG_ATTACH_EMAIL_RESPONSE,
        &response,
    )
}

/// Send get credentials response.
pub fn send_get_credentials(
    client_pid: u32,
    cap_slots: &[u32],
    credentials: Vec<LinkedCredential>,
) -> Result<(), AppError> {
    let response = GetCredentialsResponse { credentials };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_cred::MSG_GET_CREDENTIALS_RESPONSE,
        &response,
    )
}

/// Send unlink credential success response.
pub fn send_unlink_credential_success(client_pid: u32, cap_slots: &[u32]) -> Result<(), AppError> {
    let response = UnlinkCredentialResponse { result: Ok(()) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_cred::MSG_UNLINK_CREDENTIAL_RESPONSE,
        &response,
    )
}

/// Send unlink credential error response.
pub fn send_unlink_credential_error(
    client_pid: u32,
    cap_slots: &[u32],
    error: CredentialError,
) -> Result<(), AppError> {
    let response = UnlinkCredentialResponse { result: Err(error) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_cred::MSG_UNLINK_CREDENTIAL_RESPONSE,
        &response,
    )
}

// =============================================================================
// ZID responses
// =============================================================================

/// Send ZID login success response.
pub fn send_zid_login_success(
    client_pid: u32,
    cap_slots: &[u32],
    tokens: ZidTokens,
) -> Result<(), AppError> {
    let response = ZidLoginResponse { result: Ok(tokens) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_zid::MSG_ZID_LOGIN_RESPONSE,
        &response,
    )
}

/// Send ZID login error response.
pub fn send_zid_login_error(
    client_pid: u32,
    cap_slots: &[u32],
    error: ZidError,
) -> Result<(), AppError> {
    let response = ZidLoginResponse { result: Err(error) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_zid::MSG_ZID_LOGIN_RESPONSE,
        &response,
    )
}

/// Send ZID enrollment success response.
pub fn send_zid_enroll_success(
    client_pid: u32,
    cap_slots: &[u32],
    tokens: ZidTokens,
) -> Result<(), AppError> {
    let response = ZidEnrollMachineResponse { result: Ok(tokens) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_zid::MSG_ZID_ENROLL_MACHINE_RESPONSE,
        &response,
    )
}

/// Send ZID enrollment error response.
pub fn send_zid_enroll_error(
    client_pid: u32,
    cap_slots: &[u32],
    error: ZidError,
) -> Result<(), AppError> {
    let response = ZidEnrollMachineResponse { result: Err(error) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        identity_zid::MSG_ZID_ENROLL_MACHINE_RESPONSE,
        &response,
    )
}

// =============================================================================
// Identity Preferences responses
// =============================================================================

/// Send get identity preferences response.
pub fn send_get_identity_preferences_response(
    client_pid: u32,
    cap_slots: &[u32],
    response: zos_identity::ipc::GetIdentityPreferencesResponse,
) -> Result<(), AppError> {
    send_response_to_pid(
        client_pid,
        cap_slots,
        zos_process::identity_prefs::MSG_GET_IDENTITY_PREFERENCES_RESPONSE,
        &response,
    )
}

/// Send set default key scheme success response.
pub fn send_set_default_key_scheme_response(
    client_pid: u32,
    cap_slots: &[u32],
    response: zos_identity::ipc::SetDefaultKeySchemeResponse,
) -> Result<(), AppError> {
    send_response_to_pid(
        client_pid,
        cap_slots,
        zos_process::identity_prefs::MSG_SET_DEFAULT_KEY_SCHEME_RESPONSE,
        &response,
    )
}

/// Send set default key scheme error response.
pub fn send_set_default_key_scheme_error(
    client_pid: u32,
    cap_slots: &[u32],
    error: KeyError,
) -> Result<(), AppError> {
    let response = zos_identity::ipc::SetDefaultKeySchemeResponse { result: Err(error) };
    send_response_to_pid(
        client_pid,
        cap_slots,
        zos_process::identity_prefs::MSG_SET_DEFAULT_KEY_SCHEME_RESPONSE,
        &response,
    )
}
