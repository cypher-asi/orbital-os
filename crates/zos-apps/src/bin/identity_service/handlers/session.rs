//! ZID session management and authentication flows
//!
//! Handlers for:
//! - ZID machine login (challenge-response authentication)
//! - ZID machine enrollment (register new identity)
//! - Session persistence and token management

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zos_apps::identity::crypto::{
    base64_decode, bytes_to_hex, canonicalize_identity_creation_message,
    derive_identity_signing_keypair, format_uuid,
    machine_keypair_from_seeds, neural_key_from_bytes, sign_message, sign_with_machine_keypair,
    u128_to_uuid_bytes,
};
use zos_identity::crypto::NeuralKey;
use zos_apps::identity::pending::PendingStorageOp;
use zos_apps::identity::response;
use zos_apps::syscall;
use zos_apps::{AppError, Message};
use zos_identity::error::ZidError;
use zos_identity::ipc::{
    CreateIdentityRequest, ZidLoginRequest, ZidMachineKey, ZidSession, ZidTokens,
};
use zos_identity::keystore::MachineKeyRecord;
use zos_network::HttpRequest;
use zos_process::storage_result;
use zos_vfs::{parent_path, Inode};

use crate::service::IdentityService;

// =============================================================================
// ZID Login Flow
// =============================================================================

pub fn handle_zid_login(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    let request: ZidLoginRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            return response::send_zid_login_error(
                msg.from_pid,
                &msg.cap_slots,
                ZidError::NetworkError(format!("Invalid request: {}", e)),
            )
        }
    };

    // Get machine directory to find any existing machine key
    let machine_dir = format!("/home/{}/.zos/identity/machine", request.user_id);
    service.start_storage_list(
        &machine_dir,
        PendingStorageOp::ReadMachineKeyForZidLogin {
            client_pid: msg.from_pid,
            user_id: request.user_id,
            zid_endpoint: request.zid_endpoint,
            cap_slots: msg.cap_slots.clone(),
        },
    )
}

pub fn continue_zid_login_after_list(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    paths: Vec<String>,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let path = paths.into_iter().find(|p| p.ends_with(".json"));
    match path {
        Some(p) => service.start_storage_read(
            &format!("content:{}", p),
            PendingStorageOp::ReadMachineKeyForZidLogin {
                client_pid,
                user_id,
                zid_endpoint,
                cap_slots,
            },
        ),
        None => {
            response::send_zid_login_error(client_pid, &cap_slots, ZidError::MachineKeyNotFound)
        }
    }
}

pub fn continue_zid_login_after_read(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    data: &[u8],
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let machine_key: MachineKeyRecord = match serde_json::from_slice(data) {
        Ok(r) => r,
        Err(_) => {
            return response::send_zid_login_error(
                client_pid,
                &cap_slots,
                ZidError::MachineKeyNotFound,
            )
        }
    };

    let machine_id_uuid = format_uuid(machine_key.machine_id);
    let challenge_request = HttpRequest::get(format!(
        "{}/v1/auth/challenge?machine_id={}",
        zid_endpoint, machine_id_uuid
    ))
    .with_timeout(10_000);
    service.start_network_fetch(
        &challenge_request,
        zos_apps::identity::pending::PendingNetworkOp::RequestZidChallenge {
            client_pid,
            user_id,
            zid_endpoint,
            machine_key: Box::new(machine_key),
            cap_slots,
        },
    )
}

pub fn continue_zid_login_after_challenge(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    machine_key: MachineKeyRecord,
    challenge_response: zos_network::HttpSuccess,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    #[derive(serde::Deserialize)]
    struct ChallengeResponse {
        challenge: String,
        challenge_id: String,
    }

    let challenge: ChallengeResponse = match serde_json::from_slice(&challenge_response.body) {
        Ok(c) => c,
        Err(_) => {
            return response::send_zid_login_error(
                client_pid,
                &cap_slots,
                ZidError::InvalidChallenge,
            )
        }
    };

    let challenge_bytes = match base64_decode(&challenge.challenge) {
        Ok(b) => b,
        Err(_) => {
            return response::send_zid_login_error(
                client_pid,
                &cap_slots,
                ZidError::InvalidChallenge,
            )
        }
    };

    // Reconstruct the machine keypair from stored seeds for signing
    let machine_keypair = match (&machine_key.signing_sk, &machine_key.encryption_sk) {
        (Some(signing_sk), Some(encryption_sk)) => {
            match machine_keypair_from_seeds(signing_sk, encryption_sk) {
                Ok(kp) => kp,
                Err(e) => {
                    return response::send_zid_login_error(
                        client_pid,
                        &cap_slots,
                        ZidError::NetworkError(format!("Failed to reconstruct keypair: {:?}", e)),
                    );
                }
            }
        }
        _ => {
            return response::send_zid_login_error(
                client_pid,
                &cap_slots,
                ZidError::NetworkError("Machine key missing seed material".into()),
            )
        }
    };

    // Sign the challenge with the machine key
    let signature = sign_with_machine_keypair(&challenge_bytes, &machine_keypair);
    let signature_hex = bytes_to_hex(&signature);
    let machine_id_uuid = format_uuid(machine_key.machine_id);
    let login_body = format!(
        r#"{{"challenge_id":"{}","machine_id":"{}","signature":"{}"}}"#,
        challenge.challenge_id, machine_id_uuid, signature_hex
    );

    let login_request = HttpRequest::post(format!("{}/v1/auth/login/machine", zid_endpoint))
        .with_json_body(login_body.into_bytes())
        .with_timeout(10_000);
    service.start_network_fetch(
        &login_request,
        zos_apps::identity::pending::PendingNetworkOp::SubmitZidLogin {
            client_pid,
            user_id,
            zid_endpoint,
            cap_slots,
        },
    )
}

pub fn continue_zid_login_after_login(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    login_response: zos_network::HttpSuccess,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let tokens: ZidTokens = match serde_json::from_slice(&login_response.body) {
        Ok(t) => t,
        Err(_) => {
            return response::send_zid_login_error(
                client_pid,
                &cap_slots,
                ZidError::AuthenticationFailed,
            )
        }
    };

    let now = syscall::get_wallclock();
    let session = ZidSession {
        zid_endpoint: zid_endpoint.clone(),
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        session_id: tokens.session_id.clone(),
        expires_at: now + (tokens.expires_in * 1000),
        created_at: now,
    };

    let session_path = ZidSession::storage_path(user_id);
    match serde_json::to_vec(&session) {
        Ok(json_bytes) => service.start_storage_write(
            &format!("content:{}", session_path),
            &json_bytes.clone(),
            PendingStorageOp::WriteZidSessionContent {
                client_pid,
                user_id,
                tokens,
                json_bytes,
                cap_slots,
            },
        ),
        Err(e) => response::send_zid_login_error(
            client_pid,
            &cap_slots,
            ZidError::NetworkError(format!("Serialization failed: {}", e)),
        ),
    }
}

pub fn continue_zid_login_after_write_content(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    tokens: ZidTokens,
    json_bytes: Vec<u8>,
    cap_slots: Vec<u32>,
    result_type: u8,
) -> Result<(), AppError> {
    if result_type != storage_result::WRITE_OK {
        return response::send_zid_login_error(
            client_pid,
            &cap_slots,
            ZidError::NetworkError("Session write failed".into()),
        );
    }

    let session_path = ZidSession::storage_path(user_id);
    let now = syscall::get_wallclock();
    let inode = Inode::new_file(
        session_path.clone(),
        parent_path(&session_path).to_string(),
        "zid_session.json".to_string(),
        Some(user_id),
        json_bytes.len() as u64,
        None,
        now,
    );

    match serde_json::to_vec(&inode) {
        Ok(inode_json) => service.start_storage_write(
            &format!("inode:{}", session_path),
            &inode_json,
            PendingStorageOp::WriteZidSessionInode {
                client_pid,
                tokens,
                cap_slots,
            },
        ),
        Err(e) => response::send_zid_login_error(
            client_pid,
            &cap_slots,
            ZidError::NetworkError(format!("Inode serialization failed: {}", e)),
        ),
    }
}

// =============================================================================
// ZID Enrollment Flow
// =============================================================================

pub fn handle_zid_enroll_machine(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    let request: ZidLoginRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            return response::send_zid_enroll_error(
                msg.from_pid,
                &msg.cap_slots,
                ZidError::NetworkError(format!("Invalid request: {}", e)),
            )
        }
    };

    // Get machine directory to find any existing machine key
    let machine_dir = format!("/home/{}/.zos/identity/machine", request.user_id);
    service.start_storage_list(
        &machine_dir,
        PendingStorageOp::ReadMachineKeyForZidEnroll {
            client_pid: msg.from_pid,
            user_id: request.user_id,
            zid_endpoint: request.zid_endpoint,
            cap_slots: msg.cap_slots.clone(),
        },
    )
}

pub fn continue_zid_enroll_after_list(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    paths: Vec<String>,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let path = paths.into_iter().find(|p| p.ends_with(".json"));
    match path {
        Some(p) => service.start_storage_read(
            &format!("content:{}", p),
            PendingStorageOp::ReadMachineKeyForZidEnroll {
                client_pid,
                user_id,
                zid_endpoint,
                cap_slots,
            },
        ),
        None => {
            response::send_zid_enroll_error(client_pid, &cap_slots, ZidError::MachineKeyNotFound)
        }
    }
}

pub fn continue_zid_enroll_after_read(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    data: &[u8],
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let machine_key_record: MachineKeyRecord = match serde_json::from_slice(data) {
        Ok(r) => r,
        Err(_) => {
            return response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::MachineKeyNotFound,
            )
        }
    };

    // 1. Use existing machine key as neural key seed (MVP approach)
    // In production, this would be a proper Neural Key stored securely
    let neural_key_seed = &machine_key_record.signing_public_key;

    // 2. Generate new identity ID and machine ID
    let identity_id = syscall::get_wallclock() as u128 ^ (user_id << 64);
    let machine_id = syscall::get_wallclock() as u128 ^ (user_id << 32);
    let now_ms = syscall::get_wallclock();
    let now_secs = now_ms / 1000; // Convert milliseconds to seconds for ZID

    // 3. Convert IDs to UUID bytes for zid-crypto
    // We construct Uuid from raw bytes since we can't import uuid crate here
    let identity_uuid_bytes = u128_to_uuid_bytes(identity_id);
    let identity_uuid = zos_identity::crypto::uuid_from_bytes(&identity_uuid_bytes);
    let machine_uuid_bytes = u128_to_uuid_bytes(machine_id);
    let machine_uuid = zos_identity::crypto::uuid_from_bytes(&machine_uuid_bytes);
    let neural_key = neural_key_from_bytes(neural_key_seed);

    // 4. Derive identity signing keypair from neural key
    let (identity_signing_public_key, identity_keypair) =
        derive_identity_signing_keypair(&neural_key, &identity_uuid)
            .map_err(|e| AppError::Internal(format!("Key derivation failed: {:?}", e)))?;

    // 5. Generate independent random seeds for the machine key
    //    Machine keys operate independently and don't need the neural key for daily operations
    let machine_signing_sk = *NeuralKey::generate()
        .map_err(|e| AppError::Internal(format!("Failed to generate signing sk: {:?}", e)))?
        .as_bytes();
    let machine_encryption_sk = *NeuralKey::generate()
        .map_err(|e| AppError::Internal(format!("Failed to generate encryption sk: {:?}", e)))?
        .as_bytes();
    
    // 6. Create machine keypair from the random seeds
    let machine_keypair = machine_keypair_from_seeds(
        &machine_signing_sk,
        &machine_encryption_sk,
    )
    .map_err(|e| AppError::Internal(format!("Machine key creation failed: {:?}", e)))?;

    // 7. Build simplified MachineKey struct for enrollment
    let machine_key = ZidMachineKey {
        machine_id: format_uuid(machine_id),
        signing_public_key: bytes_to_hex(&machine_keypair.signing_public_key()),
        encryption_public_key: bytes_to_hex(&machine_keypair.encryption_public_key()),
        capabilities: vec!["SIGN".into(), "ENCRYPT".into(), "VAULT_OPERATIONS".into()],
        device_name: "Browser".into(),
        device_platform: "web".into(),
    };

    // 8. Create authorization signature per ZID spec (canonical 137-byte format)
    // Message: "create" + identityId.bytes + identity_signing_public_key + 
    //          machineKey.signingPublicKey + machineKey.encryptionPublicKey + createdAt.bytes
    let message = canonicalize_identity_creation_message(
        &identity_uuid,
        &identity_signing_public_key,
        &machine_uuid,
        &machine_keypair.signing_public_key(),
        &machine_keypair.encryption_public_key(),
        now_secs,
    );
    let signature = sign_message(&identity_keypair, &message);

    // 9. Build CreateIdentityRequest
    syscall::debug(&format!("IdentityService: Building enrollment request for identity_id={:032x}", identity_id));
    let request = CreateIdentityRequest {
        identity_id: format_uuid(identity_id),
        identity_signing_public_key: bytes_to_hex(&identity_signing_public_key),
        authorization_signature: bytes_to_hex(&signature),
        machine_key,
        namespace_name: "personal".into(),
        created_at: now_secs, // Unix timestamp in seconds
    };
    syscall::debug(&format!("IdentityService: Request struct created, identity_id field = {}", request.identity_id));

    // 8. Serialize to JSON
    syscall::debug("IdentityService: Serializing enrollment request to JSON");
    let enroll_body = match serde_json::to_vec(&request) {
        Ok(b) => {
            syscall::debug(&format!("IdentityService: Serialization successful, {} bytes", b.len()));
            b
        },
        Err(e) => {
            syscall::debug(&format!("IdentityService: Serialization FAILED: {}", e));
            return response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::NetworkError(format!("Serialization failed: {}", e)),
            )
        }
    };

    // Debug: Log the JSON payload being sent
    if let Ok(json_str) = alloc::str::from_utf8(&enroll_body) {
        syscall::debug(&format!("IdentityService: Sending enrollment JSON: {}", json_str));
    } else {
        syscall::debug("IdentityService: Warning - could not convert JSON bytes to UTF-8 string");
    }

    // 10. Send HTTP request with seeds for secure storage
    let enroll_request = HttpRequest::post(format!("{}/v1/identity", zid_endpoint))
        .with_json_body(enroll_body)
        .with_timeout(10_000);

    service.start_network_fetch(
        &enroll_request,
        zos_apps::identity::pending::PendingNetworkOp::SubmitZidEnroll {
            client_pid,
            user_id,
            zid_endpoint,
            cap_slots,
            identity_id,
            machine_id,
            identity_signing_public_key,
            machine_signing_public_key: machine_keypair.signing_public_key(),
            machine_encryption_public_key: machine_keypair.encryption_public_key(),
            machine_signing_sk,
            machine_encryption_sk,
        },
    )
}

/// Handle identity creation response and chain to login flow.
///
/// Per ZID API spec:
/// - POST /v1/identity returns: {identity_id, machine_id, namespace_id} (NO tokens)
/// - We need to chain into login: GET /v1/auth/challenge + POST /v1/auth/login/machine
pub fn continue_zid_enroll_after_submit(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    enroll_response: zos_network::HttpSuccess,
    cap_slots: Vec<u32>,
    _identity_id: u128,
    machine_id: u128,
    identity_signing_public_key: [u8; 32],
    machine_signing_public_key: [u8; 32],
    machine_encryption_public_key: [u8; 32],
    machine_signing_sk: [u8; 32],
    machine_encryption_sk: [u8; 32],
) -> Result<(), AppError> {
    // ZID identity creation response (no tokens - just identity info)
    #[derive(serde::Deserialize)]
    struct CreateIdentityResponse {
        identity_id: String,
        machine_id: String,
        namespace_id: String,
    }

    // Log the response body for debugging
    if let Ok(body_str) = alloc::str::from_utf8(&enroll_response.body) {
        syscall::debug(&format!("IdentityService: Identity creation response: {}", body_str));
    }

    let create_response: CreateIdentityResponse = match serde_json::from_slice(&enroll_response.body) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse identity creation response: {}", e));
            return response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::EnrollmentFailed(format!("Invalid identity creation response: {}", e)),
            );
        }
    };

    syscall::debug(&format!(
        "IdentityService: Identity created successfully. identity_id={}, machine_id={}. Chaining to login...",
        create_response.identity_id, create_response.machine_id
    ));

    // Chain into login flow - request challenge using the machine_id from the server response
    // This proves the machine was registered and we can authenticate with it
    let challenge_request = HttpRequest::get(format!(
        "{}/v1/auth/challenge?machine_id={}",
        zid_endpoint, create_response.machine_id
    ))
    .with_timeout(10_000);

    service.start_network_fetch(
        &challenge_request,
        zos_apps::identity::pending::PendingNetworkOp::RequestZidChallengeAfterEnroll {
            client_pid,
            user_id,
            zid_endpoint,
            cap_slots,
            machine_id,
            identity_signing_public_key,
            machine_signing_public_key,
            machine_encryption_public_key,
            machine_signing_sk,
            machine_encryption_sk,
        },
    )
}

/// Handle challenge response during chained login after enrollment.
/// Signs the challenge and submits login request.
pub fn continue_zid_enroll_after_challenge(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    machine_id: u128,
    identity_signing_public_key: [u8; 32],
    machine_signing_public_key: [u8; 32],
    machine_encryption_public_key: [u8; 32],
    machine_signing_sk: [u8; 32],
    machine_encryption_sk: [u8; 32],
    challenge_response: zos_network::HttpSuccess,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    #[derive(serde::Deserialize)]
    struct ChallengeResponse {
        challenge: String,
        challenge_id: String,
    }

    let challenge: ChallengeResponse = match serde_json::from_slice(&challenge_response.body) {
        Ok(c) => c,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse challenge response: {}", e));
            return response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::InvalidChallenge,
            );
        }
    };

    let challenge_bytes = match base64_decode(&challenge.challenge) {
        Ok(b) => b,
        Err(_) => {
            return response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::InvalidChallenge,
            )
        }
    };

    // Reconstruct machine keypair from stored seeds for signing
    let machine_keypair = match machine_keypair_from_seeds(&machine_signing_sk, &machine_encryption_sk) {
        Ok(kp) => kp,
        Err(e) => {
            return response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::EnrollmentFailed(format!("Failed to reconstruct keypair: {:?}", e)),
            );
        }
    };

    // Sign the challenge with the machine key
    let signature = sign_with_machine_keypair(&challenge_bytes, &machine_keypair);
    let signature_hex = bytes_to_hex(&signature);
    let machine_id_uuid = format_uuid(machine_id);

    let login_body = format!(
        r#"{{"challenge_id":"{}","machine_id":"{}","signature":"{}"}}"#,
        challenge.challenge_id, machine_id_uuid, signature_hex
    );

    syscall::debug(&format!(
        "IdentityService: Submitting login after enrollment, machine_id={}",
        machine_id_uuid
    ));

    let login_request = HttpRequest::post(format!("{}/v1/auth/login/machine", zid_endpoint))
        .with_json_body(login_body.into_bytes())
        .with_timeout(10_000);

    service.start_network_fetch(
        &login_request,
        zos_apps::identity::pending::PendingNetworkOp::SubmitZidLoginAfterEnroll {
            client_pid,
            user_id,
            zid_endpoint,
            cap_slots,
            machine_id,
            identity_signing_public_key,
            machine_signing_public_key,
            machine_encryption_public_key,
            machine_signing_sk,
            machine_encryption_sk,
        },
    )
}

/// Handle login response (tokens) after chained login during enrollment.
/// This is the final step - stores machine key and session, then returns tokens.
pub fn continue_zid_enroll_after_login(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    machine_id: u128,
    identity_signing_public_key: [u8; 32],
    machine_signing_public_key: [u8; 32],
    machine_encryption_public_key: [u8; 32],
    machine_signing_sk: [u8; 32],
    machine_encryption_sk: [u8; 32],
    login_response: zos_network::HttpSuccess,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    // NOW we expect tokens from the login response
    let tokens: ZidTokens = match serde_json::from_slice(&login_response.body) {
        Ok(t) => t,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse login response: {}", e));
            if let Ok(body_str) = alloc::str::from_utf8(&login_response.body) {
                syscall::debug(&format!("IdentityService: Login response body: {}", body_str));
            }
            return response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::EnrollmentFailed(format!("Failed to parse login tokens: {}", e)),
            );
        }
    };

    syscall::debug("IdentityService: Login successful after enrollment, storing machine key and session");

    let now = syscall::get_wallclock();

    // Store identity keys (public keys only for now)
    let _key_store = zos_identity::keystore::LocalKeyStore::new(
        user_id,
        identity_signing_public_key,
        machine_signing_public_key,
        machine_encryption_public_key,
        now,
    );

    // Store machine key record with seeds for future use
    let machine_key_record = zos_identity::keystore::MachineKeyRecord {
        machine_id,
        signing_public_key: machine_signing_public_key,
        encryption_public_key: machine_encryption_public_key,
        signing_sk: Some(machine_signing_sk),
        encryption_sk: Some(machine_encryption_sk),
        authorized_at: now,
        authorized_by: user_id,
        capabilities: zos_identity::keystore::MachineKeyCapabilities::full(),
        machine_name: Some("Zero OS Device".into()),
        last_seen_at: now,
        epoch: 1,
        key_scheme: zos_identity::keystore::KeyScheme::Classical,
        pq_signing_public_key: None,
        pq_encryption_public_key: None,
    };

    // Store the machine key to VFS
    let machine_key_path = zos_identity::keystore::MachineKeyRecord::storage_path(user_id, machine_id);
    let machine_key_json = match serde_json::to_vec(&machine_key_record) {
        Ok(json) => json,
        Err(e) => {
            syscall::debug(&format!("Failed to serialize machine key: {}", e));
            return response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::EnrollmentFailed(format!("Failed to serialize machine key: {}", e)),
            );
        }
    };

    // Write machine key content (fire and forget - don't block on this)
    match syscall::storage_write_async(&format!("content:{}", machine_key_path), &machine_key_json) {
        Ok(_) => syscall::debug("Machine key stored successfully"),
        Err(e) => {
            syscall::debug(&format!("Warning: Failed to store machine key: {}", e));
            // Don't fail enrollment if machine key storage fails - we can still use the session
        }
    }

    // Store ZID session
    let session = ZidSession {
        zid_endpoint: zid_endpoint.clone(),
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        session_id: tokens.session_id.clone(),
        expires_at: now + tokens.expires_in * 1000,
        created_at: now,
    };

    let session_json = match serde_json::to_vec(&session) {
        Ok(b) => b,
        Err(_) => return response::send_zid_enroll_success(client_pid, &cap_slots, tokens),
    };

    // Store the session
    let session_path = ZidSession::storage_path(user_id);
    service.start_storage_write(
        &format!("content:{}", session_path),
        &session_json.clone(),
        PendingStorageOp::WriteZidEnrollSessionContent {
            client_pid,
            user_id,
            tokens,
            json_bytes: session_json,
            cap_slots,
        },
    )
}
