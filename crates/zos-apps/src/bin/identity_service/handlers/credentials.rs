//! Credential management operations
//!
//! Handlers for:
//! - Attaching email credentials (with ZID verification)
//! - Unlinking credentials
//! - Retrieving credential lists

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zos_apps::identity::pending::PendingStorageOp;
use zos_apps::identity::response;
use zos_apps::syscall;
use zos_apps::{AppError, Message};
use zos_identity::error::CredentialError;
use zos_identity::ipc::{AttachEmailRequest, GetCredentialsRequest, UnlinkCredentialRequest};
use zos_identity::keystore::{CredentialStore, CredentialType, LinkedCredential};
use zos_network::HttpRequest;

use crate::service::IdentityService;

// =============================================================================
// Email Credential Operations
// =============================================================================

pub fn handle_attach_email(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    let request: AttachEmailRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(_) => {
            return response::send_attach_email_error(
                msg.from_pid,
                &msg.cap_slots,
                CredentialError::InvalidFormat,
            )
        }
    };

    if !request.email.contains('@') || request.email.len() < 5 {
        return response::send_attach_email_error(
            msg.from_pid,
            &msg.cap_slots,
            CredentialError::InvalidFormat,
        );
    }

    if request.password.len() < 12 {
        return response::send_attach_email_error(
            msg.from_pid,
            &msg.cap_slots,
            CredentialError::StorageError("Password must be at least 12 characters".into()),
        );
    }

    let body = format!(
        r#"{{"email":"{}","password":"{}"}}"#,
        request.email, request.password
    );
    let http_request = HttpRequest::post(format!("{}/v1/credentials/email", request.zid_endpoint))
        .with_header("Authorization", format!("Bearer {}", request.access_token))
        .with_json_body(body.into_bytes())
        .with_timeout(15_000);

    service.start_network_fetch(
        &http_request,
        zos_apps::identity::pending::PendingNetworkOp::SubmitEmailToZid {
            client_pid: msg.from_pid,
            user_id: request.user_id,
            email: request.email,
            cap_slots: msg.cap_slots.clone(),
        },
    )
}

pub fn continue_attach_email_after_zid(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    email: String,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let cred_path = CredentialStore::storage_path(user_id);
    service.start_storage_read(
        &format!("content:{}", cred_path),
        PendingStorageOp::ReadCredentialsForAttach {
            client_pid,
            user_id,
            email,
            cap_slots,
        },
    )
}

pub fn continue_attach_email_after_read(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    email: String,
    existing_store: Option<CredentialStore>,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let now = syscall::get_wallclock();
    let mut store = existing_store.unwrap_or_else(|| CredentialStore::new(user_id));
    store.credentials.retain(|c| {
        !(c.credential_type == CredentialType::Email && c.value == email && !c.verified)
    });
    store.credentials.push(LinkedCredential {
        credential_type: CredentialType::Email,
        value: email,
        verified: true,
        linked_at: now,
        verified_at: Some(now),
        is_primary: store.find_by_type(CredentialType::Email).is_empty(),
    });

    let cred_path = CredentialStore::storage_path(user_id);
    match serde_json::to_vec(&store) {
        Ok(json_bytes) => service.start_storage_write(
            &format!("content:{}", cred_path),
            &json_bytes.clone(),
            PendingStorageOp::WriteEmailCredentialContent {
                client_pid,
                user_id,
                json_bytes,
                cap_slots,
            },
        ),
        Err(e) => response::send_attach_email_error(
            client_pid,
            &cap_slots,
            CredentialError::StorageError(format!("Serialization failed: {}", e)),
        ),
    }
}

// =============================================================================
// Credential Retrieval
// =============================================================================

pub fn handle_get_credentials(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    let request: GetCredentialsRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(_) => return response::send_get_credentials(msg.from_pid, &msg.cap_slots, vec![]),
    };

    let cred_path = CredentialStore::storage_path(request.user_id);
    service.start_storage_read(
        &format!("content:{}", cred_path),
        PendingStorageOp::GetCredentials {
            client_pid: msg.from_pid,
            cap_slots: msg.cap_slots.clone(),
        },
    )
}

// =============================================================================
// Credential Unlinking
// =============================================================================

pub fn handle_unlink_credential(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    let request: UnlinkCredentialRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(_) => {
            return response::send_unlink_credential_error(
                msg.from_pid,
                &msg.cap_slots,
                CredentialError::InvalidFormat,
            )
        }
    };

    let cred_path = CredentialStore::storage_path(request.user_id);
    service.start_storage_read(
        &format!("content:{}", cred_path),
        PendingStorageOp::ReadCredentialsForUnlink {
            client_pid: msg.from_pid,
            user_id: request.user_id,
            credential_type: request.credential_type,
            cap_slots: msg.cap_slots.clone(),
        },
    )
}

pub fn continue_unlink_credential_after_read(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    credential_type: CredentialType,
    data: &[u8],
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let mut store: CredentialStore = match serde_json::from_slice(data) {
        Ok(s) => s,
        Err(e) => {
            return response::send_unlink_credential_error(
                client_pid,
                &cap_slots,
                CredentialError::StorageError(format!("Parse failed: {}", e)),
            )
        }
    };

    let original_len = store.credentials.len();
    store
        .credentials
        .retain(|c| c.credential_type != credential_type);

    if store.credentials.len() == original_len {
        return response::send_unlink_credential_error(
            client_pid,
            &cap_slots,
            CredentialError::NotFound,
        );
    }

    let cred_path = CredentialStore::storage_path(user_id);
    match serde_json::to_vec(&store) {
        Ok(json_bytes) => service.start_storage_write(
            &format!("content:{}", cred_path),
            &json_bytes.clone(),
            PendingStorageOp::WriteUnlinkedCredentialContent {
                client_pid,
                user_id,
                json_bytes,
                cap_slots,
            },
        ),
        Err(e) => response::send_unlink_credential_error(
            client_pid,
            &cap_slots,
            CredentialError::StorageError(format!("Serialization failed: {}", e)),
        ),
    }
}
