//! ZID session management and authentication flows
//!
//! Handlers for:
//! - ZID machine login (challenge-response authentication)
//! - ZID machine enrollment (register new identity)
//! - Session persistence and token management
//!
//! # Safety Invariants (per zos-service.md Rule 0)
//!
//! ## Success Conditions
//! - ZID login: Challenge signed, authentication succeeded, session stored, tokens returned
//! - ZID enrollment: Identity created on server, chained login completed, tokens returned
//!
//! ## Acceptable Partial Failure
//! - Session write failure after successful authentication (tokens still returned)
//! - Machine key write failure during enrollment (session still usable)
//!
//! ## Forbidden States
//! - Returning tokens before authentication completes
//! - Storing session with invalid/expired tokens
//! - Silent fallthrough on parse errors (must return InvalidRequest)
//! - Processing requests without authorization check

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::super::utils::{
    base64_decode, bytes_to_hex, canonicalize_identity_creation_message,
    derive_identity_signing_keypair, format_uuid,
    machine_keypair_from_seeds, neural_key_from_bytes, sign_message, sign_with_machine_keypair,
    u128_to_uuid_bytes,
};
use super::super::pending::{PendingKeystoreOp, PendingNetworkOp, PendingStorageOp, RequestContext};
use super::super::response;
use super::super::{check_user_authorization, log_denial, AuthResult, IdentityService};
use zos_identity::crypto::NeuralKey;
use zos_apps::syscall;
use zos_apps::{AppError, Message};
use zos_identity::error::ZidError;
use zos_identity::ipc::{
    CreateIdentityRequest, ZidLoginRequest, ZidMachineKey, ZidSession, ZidTokens,
};
use zos_identity::keystore::MachineKeyRecord;
use zos_network::HttpRequest;

// =============================================================================
// ZID Login Flow
// =============================================================================

pub fn handle_zid_login(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: ZidLoginRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_zid_login_error(
                msg.from_pid,
                &msg.cap_slots,
                ZidError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("zid_login", msg.from_pid, request.user_id);
        return response::send_zid_login_error(
            msg.from_pid,
            &msg.cap_slots,
            ZidError::Unauthorized,
        );
    }

    // Read preferences to get default_machine_id (if set)
    let prefs_path = zos_identity::ipc::IdentityPreferences::storage_path(request.user_id);
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    service.start_vfs_read(
        &prefs_path,
        PendingStorageOp::ReadPreferencesForZidLogin {
            ctx,
            user_id: request.user_id,
            zid_endpoint: request.zid_endpoint,
        },
    )
}

/// Continue ZID login after reading preferences.
/// Uses default_machine_id if set, otherwise lists all and picks first.
pub fn continue_zid_login_after_preferences(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    default_machine_id: Option<u128>,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    if let Some(machine_id) = default_machine_id {
        // Use the default machine key directly
        syscall::debug(&format!(
            "IdentityService: Using default machine key {:032x} for ZID login",
            machine_id
        ));
        let machine_key_path = zos_identity::keystore::MachineKeyRecord::storage_path(user_id, machine_id);
        service.start_keystore_read(
            &machine_key_path,
            PendingKeystoreOp::ReadMachineKeyForZidLogin {
                ctx,
                user_id,
                zid_endpoint,
            },
        )
    } else {
        // No default set - list all and pick first
        syscall::debug("IdentityService: No default machine key set, listing all");
        let machine_prefix = format!("/keys/{}/identity/machine/", user_id);
        service.start_keystore_list(
            &machine_prefix,
            PendingKeystoreOp::ListMachineKeysForZidLogin {
                ctx,
                user_id,
                zid_endpoint,
            },
        )
    }
}

pub fn continue_zid_login_after_list(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    paths: Vec<String>,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);
    let path = paths.into_iter().find(|p| p.ends_with(".json"));
    match path {
        Some(p) => service.start_vfs_read(
            &p,
            PendingStorageOp::ReadMachineKeyForZidLogin { ctx, user_id, zid_endpoint },
        ),
        None => {
            response::send_zid_login_error(ctx.client_pid, &ctx.cap_slots, ZidError::MachineKeyNotFound)
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
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    let machine_key: MachineKeyRecord = match serde_json::from_slice(data) {
        Ok(r) => r,
        Err(_) => {
            return response::send_zid_login_error(
                ctx.client_pid,
                &ctx.cap_slots,
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
        PendingNetworkOp::RequestZidChallenge {
            ctx,
            user_id,
            zid_endpoint,
            machine_key: Box::new(machine_key),
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
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    #[derive(serde::Deserialize)]
    struct ChallengeResponse {
        challenge: String,
    }

    let challenge_resp: ChallengeResponse = match serde_json::from_slice(&challenge_response.body) {
        Ok(c) => c,
        Err(_) => {
            return response::send_zid_login_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::InvalidChallenge,
            )
        }
    };

    // The challenge field is base64-encoded JSON of the full Challenge struct
    let challenge_json = match base64_decode(&challenge_resp.challenge) {
        Ok(b) => b,
        Err(_) => {
            return response::send_zid_login_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::InvalidChallenge,
            )
        }
    };

    // Parse the Challenge struct from JSON
    let challenge: zos_identity::crypto::Challenge = match serde_json::from_slice(&challenge_json) {
        Ok(c) => c,
        Err(e) => {
            syscall::debug(&format!("Failed to parse challenge JSON: {}", e));
            return response::send_zid_login_error(
                ctx.client_pid,
                &ctx.cap_slots,
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
                        ctx.client_pid,
                        &ctx.cap_slots,
                        ZidError::NetworkError(format!("Failed to reconstruct keypair: {:?}", e)),
                    );
                }
            }
        }
        _ => {
            return response::send_zid_login_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::NetworkError("Machine key missing seed material".into()),
            )
        }
    };

    // Sign the CANONICAL challenge message (130 bytes), not the raw JSON
    let canonical_message = zos_identity::crypto::canonicalize_challenge(&challenge);
    let signature = sign_with_machine_keypair(&canonical_message, &machine_keypair);
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
        PendingNetworkOp::SubmitZidLogin {
            ctx,
            user_id,
            zid_endpoint,
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
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    let tokens: ZidTokens = match serde_json::from_slice(&login_response.body) {
        Ok(t) => t,
        Err(_) => {
            return response::send_zid_login_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::AuthenticationFailed,
            )
        }
    };

    let now = syscall::get_wallclock();
    
    // Set login_type for machine key authentication
    let mut tokens = tokens;
    tokens.login_type = zos_identity::ipc::LoginType::MachineKey;
    
    let session = ZidSession {
        zid_endpoint: zid_endpoint.clone(),
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        session_id: tokens.session_id.clone(),
        machine_id: tokens.machine_id.clone(),
        login_type: tokens.login_type,
        expires_at: super::super::utils::parse_rfc3339_to_millis(&tokens.expires_at),
        created_at: now,
    };

    let session_path = ZidSession::storage_path(user_id);
    match serde_json::to_vec(&session) {
        Ok(json_bytes) => service.start_vfs_write(
            &session_path,
            &json_bytes,
            PendingStorageOp::WriteZidSession {
                ctx,
                user_id,
                tokens,
                json_bytes: json_bytes.clone(),
            },
        ),
        Err(e) => response::send_zid_login_error(
            ctx.client_pid,
            &ctx.cap_slots,
            ZidError::NetworkError(format!("Serialization failed: {}", e)),
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
    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: ZidLoginRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_zid_enroll_error(
                msg.from_pid,
                &msg.cap_slots,
                ZidError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("zid_enroll_machine", msg.from_pid, request.user_id);
        return response::send_zid_enroll_error(
            msg.from_pid,
            &msg.cap_slots,
            ZidError::Unauthorized,
        );
    }

    // Get machine keys from keystore (Invariant 32: /keys/ paths use Keystore)
    let machine_prefix = format!("/keys/{}/identity/machine/", request.user_id);
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    service.start_keystore_list(
        &machine_prefix,
        PendingKeystoreOp::ListMachineKeysForZidEnroll {
            ctx,
            user_id: request.user_id,
            zid_endpoint: request.zid_endpoint,
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
    let ctx = RequestContext::new(client_pid, cap_slots);
    let path = paths.into_iter().find(|p| p.ends_with(".json"));
    match path {
        Some(p) => service.start_vfs_read(
            &p,
            PendingStorageOp::ReadMachineKeyForZidEnroll { ctx, user_id, zid_endpoint },
        ),
        None => {
            response::send_zid_enroll_error(ctx.client_pid, &ctx.cap_slots, ZidError::MachineKeyNotFound)
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
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    let machine_key_record: MachineKeyRecord = match serde_json::from_slice(data) {
        Ok(r) => r,
        Err(_) => {
            return response::send_zid_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
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
                ctx.client_pid,
                &ctx.cap_slots,
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
        PendingNetworkOp::SubmitZidEnroll {
            ctx,
            user_id,
            zid_endpoint,
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
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    // ZID identity creation response (no tokens - just identity info)
    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
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
                ctx.client_pid,
                &ctx.cap_slots,
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
        PendingNetworkOp::RequestZidChallengeAfterEnroll {
            ctx,
            user_id,
            zid_endpoint,
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
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    #[derive(serde::Deserialize)]
    struct ChallengeResponseDto {
        challenge: String,
    }

    let challenge_resp: ChallengeResponseDto = match serde_json::from_slice(&challenge_response.body) {
        Ok(c) => c,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse challenge response: {}", e));
            return response::send_zid_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::InvalidChallenge,
            );
        }
    };

    // The challenge field is base64-encoded JSON of the full Challenge struct
    let challenge_json = match base64_decode(&challenge_resp.challenge) {
        Ok(b) => b,
        Err(_) => {
            return response::send_zid_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::InvalidChallenge,
            )
        }
    };

    // Parse the Challenge struct from JSON
    let challenge: zos_identity::crypto::Challenge = match serde_json::from_slice(&challenge_json) {
        Ok(c) => c,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse challenge JSON: {}", e));
            return response::send_zid_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::InvalidChallenge,
            );
        }
    };

    // Reconstruct machine keypair from stored seeds for signing
    let machine_keypair = match machine_keypair_from_seeds(&machine_signing_sk, &machine_encryption_sk) {
        Ok(kp) => kp,
        Err(e) => {
            return response::send_zid_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::EnrollmentFailed(format!("Failed to reconstruct keypair: {:?}", e)),
            );
        }
    };

    // Sign the CANONICAL challenge message (130 bytes), not the raw JSON
    let canonical_message = zos_identity::crypto::canonicalize_challenge(&challenge);
    let signature = sign_with_machine_keypair(&canonical_message, &machine_keypair);
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
        PendingNetworkOp::SubmitZidLoginAfterEnroll {
            ctx,
            user_id,
            zid_endpoint,
            machine_id,
            identity_signing_public_key,
            machine_signing_public_key,
            machine_encryption_public_key,
            machine_signing_sk,
            machine_encryption_sk,
        },
    )
}

// =============================================================================
// Combined Machine Key + ZID Enrollment Flow
// =============================================================================

/// Continue combined enrollment after machine key is stored in keystore.
///
/// This function is called after WriteMachineKeyForEnroll succeeds.
/// It initiates the ZID enrollment using the already-stored machine key.
pub fn continue_combined_enroll_after_machine_write(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    machine_key_record: MachineKeyRecord,
    identity_signing_public_key: [u8; 32],
    identity_signing_sk: [u8; 32],
    machine_signing_sk: [u8; 32],
    machine_encryption_sk: [u8; 32],
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);

    let machine_id = machine_key_record.machine_id;
    let now_ms = syscall::get_wallclock();
    let now_secs = now_ms / 1000;

    // Convert IDs to UUID bytes for zid-crypto
    let identity_uuid_bytes = u128_to_uuid_bytes(user_id);
    let identity_uuid = zos_identity::crypto::uuid_from_bytes(&identity_uuid_bytes);
    let machine_uuid_bytes = u128_to_uuid_bytes(machine_id);

    // Reconstruct identity keypair from the passed-through seed
    // (The seed was derived from Neural Key in continue_create_machine_enroll_after_shards_read)
    let identity_keypair = zos_identity::crypto::Ed25519KeyPair::from_seed(&identity_signing_sk)
        .map_err(|e| AppError::Internal(format!("Identity keypair reconstruction failed: {:?}", e)))?;

    // Reconstruct machine keypair from stored seeds
    let machine_keypair = machine_keypair_from_seeds(&machine_signing_sk, &machine_encryption_sk)
        .map_err(|e| AppError::Internal(format!("Machine key reconstruction failed: {:?}", e)))?;

    // DEBUG: Log the public key being sent to server for enrollment
    syscall::debug(&format!(
        "IdentityService: DEBUG ENROLL - machine_signing_pubkey_to_server: {}",
        bytes_to_hex(&machine_keypair.signing_public_key())
    ));
    syscall::debug(&format!(
        "IdentityService: DEBUG ENROLL - machine_signing_sk_first8: {}",
        bytes_to_hex(&machine_signing_sk[..8])
    ));

    // Build ZID machine key structure
    let zid_machine_key = ZidMachineKey {
        machine_id: format_uuid(machine_id),
        signing_public_key: bytes_to_hex(&machine_keypair.signing_public_key()),
        encryption_public_key: bytes_to_hex(&machine_keypair.encryption_public_key()),
        capabilities: vec!["SIGN".into(), "ENCRYPT".into(), "VAULT_OPERATIONS".into()],
        device_name: machine_key_record.machine_name.clone().unwrap_or_else(|| "Browser".into()),
        device_platform: "web".into(),
    };

    // Create authorization signature
    let message = canonicalize_identity_creation_message(
        &identity_uuid,
        &identity_signing_public_key,
        &zos_identity::crypto::uuid_from_bytes(&machine_uuid_bytes),
        &machine_keypair.signing_public_key(),
        &machine_keypair.encryption_public_key(),
        now_secs,
    );
    let signature = sign_message(&identity_keypair, &message);

    // Build CreateIdentityRequest
    let create_request = CreateIdentityRequest {
        identity_id: format_uuid(user_id),
        identity_signing_public_key: bytes_to_hex(&identity_signing_public_key),
        authorization_signature: bytes_to_hex(&signature),
        machine_key: zid_machine_key,
        namespace_name: "personal".into(),
        created_at: now_secs,
    };

    let enroll_body = match serde_json::to_vec(&create_request) {
        Ok(b) => b,
        Err(e) => {
            return response::send_create_machine_key_and_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::NetworkError(format!("Serialization failed: {}", e)),
            );
        }
    };

    syscall::debug(&format!(
        "IdentityService: Combined flow - enrolling machine {:032x} with ZID",
        machine_id
    ));

    let enroll_request = HttpRequest::post(format!("{}/v1/identity", zid_endpoint))
        .with_json_body(enroll_body)
        .with_timeout(10_000);

    service.start_network_fetch(
        &enroll_request,
        PendingNetworkOp::SubmitZidEnrollForCombined {
            ctx,
            user_id,
            zid_endpoint,
            machine_id,
            identity_signing_public_key,
            machine_signing_public_key: machine_keypair.signing_public_key(),
            machine_encryption_public_key: machine_keypair.encryption_public_key(),
            machine_signing_sk,
            machine_encryption_sk,
            machine_key_record: Box::new(machine_key_record),
        },
    )
}

/// Continue combined flow after identity creation - chain to login.
pub fn continue_combined_enroll_after_identity_create(
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
    machine_key_record: MachineKeyRecord,
    server_machine_id: String,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);

    syscall::debug(&format!(
        "IdentityService: Combined flow - identity created, requesting challenge for machine {}",
        server_machine_id
    ));

    let challenge_request = HttpRequest::get(format!(
        "{}/v1/auth/challenge?machine_id={}",
        zid_endpoint, server_machine_id
    ))
    .with_timeout(10_000);

    service.start_network_fetch(
        &challenge_request,
        PendingNetworkOp::RequestZidChallengeForCombined {
            ctx,
            user_id,
            zid_endpoint,
            machine_id,
            identity_signing_public_key,
            machine_signing_public_key,
            machine_encryption_public_key,
            machine_signing_sk,
            machine_encryption_sk,
            machine_key_record: Box::new(machine_key_record),
        },
    )
}

/// Continue combined flow after challenge - sign and submit login.
pub fn continue_combined_enroll_after_challenge(
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
    machine_key_record: MachineKeyRecord,
    challenge_response: zos_network::HttpSuccess,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);

    #[derive(serde::Deserialize)]
    struct ChallengeResponseDto {
        challenge: String,
    }

    let challenge_resp: ChallengeResponseDto = match serde_json::from_slice(&challenge_response.body) {
        Ok(c) => c,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse challenge: {}", e));
            return response::send_create_machine_key_and_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::InvalidChallenge,
            );
        }
    };

    // The challenge field is base64-encoded JSON of the full Challenge struct
    let challenge_json = match base64_decode(&challenge_resp.challenge) {
        Ok(b) => b,
        Err(_) => {
            return response::send_create_machine_key_and_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::InvalidChallenge,
            );
        }
    };

    // Parse the Challenge struct from JSON
    let challenge: zos_identity::crypto::Challenge = match serde_json::from_slice(&challenge_json) {
        Ok(c) => c,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse challenge JSON: {}", e));
            return response::send_create_machine_key_and_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::InvalidChallenge,
            );
        }
    };

    // Reconstruct machine keypair and sign the CANONICAL challenge message
    let machine_keypair = match machine_keypair_from_seeds(&machine_signing_sk, &machine_encryption_sk) {
        Ok(kp) => kp,
        Err(e) => {
            return response::send_create_machine_key_and_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::NetworkError(format!("Keypair reconstruction failed: {:?}", e)),
            );
        }
    };

    // DEBUG: Log the public keys to verify they match
    let reconstructed_pubkey = machine_keypair.signing_public_key();
    syscall::debug(&format!(
        "IdentityService: DEBUG - signing_pubkey_from_pending_op: {}",
        bytes_to_hex(&machine_signing_public_key)
    ));
    syscall::debug(&format!(
        "IdentityService: DEBUG - signing_pubkey_reconstructed: {}",
        bytes_to_hex(&reconstructed_pubkey)
    ));
    syscall::debug(&format!(
        "IdentityService: DEBUG - keys_match: {}",
        machine_signing_public_key == reconstructed_pubkey
    ));

    // Sign the CANONICAL challenge message (130 bytes), not the raw JSON
    let canonical_message = zos_identity::crypto::canonicalize_challenge(&challenge);
    
    // DEBUG: Log the canonical message for verification
    syscall::debug(&format!(
        "IdentityService: DEBUG - canonical_message_len: {}, first_16_bytes: {}",
        canonical_message.len(),
        bytes_to_hex(&canonical_message[..16])
    ));
    
    let signature = sign_with_machine_keypair(&canonical_message, &machine_keypair);
    let signature_hex = bytes_to_hex(&signature);
    let machine_id_uuid = format_uuid(machine_id);

    let login_body = format!(
        r#"{{"challenge_id":"{}","machine_id":"{}","signature":"{}"}}"#,
        challenge.challenge_id, machine_id_uuid, signature_hex
    );

    syscall::debug(&format!(
        "IdentityService: Combined flow - submitting login for machine {}, signature_len: {}",
        machine_id_uuid, signature_hex.len()
    ));

    let login_request = HttpRequest::post(format!("{}/v1/auth/login/machine", zid_endpoint))
        .with_json_body(login_body.into_bytes())
        .with_timeout(10_000);

    service.start_network_fetch(
        &login_request,
        PendingNetworkOp::SubmitZidLoginForCombined {
            ctx,
            user_id,
            zid_endpoint,
            machine_id,
            identity_signing_public_key,
            machine_signing_public_key,
            machine_encryption_public_key,
            machine_signing_sk,
            machine_encryption_sk,
            machine_key_record: Box::new(machine_key_record),
        },
    )
}

/// Final step of combined flow - return both machine key and tokens.
pub fn continue_combined_enroll_after_login(
    _service: &mut IdentityService,
    client_pid: u32,
    _user_id: u128,
    _zid_endpoint: String,
    machine_key_record: MachineKeyRecord,
    login_response: zos_network::HttpSuccess,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    // Parse tokens from login response
    let tokens: ZidTokens = match serde_json::from_slice(&login_response.body) {
        Ok(t) => t,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse login tokens: {}", e));
            return response::send_create_machine_key_and_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::AuthenticationFailed,
            );
        }
    };

    syscall::debug(&format!(
        "IdentityService: Combined flow complete - machine {:032x} enrolled with ZID",
        machine_key_record.machine_id
    ));

    // Return both machine key and tokens
    response::send_create_machine_key_and_enroll_success(
        client_pid,
        &cap_slots,
        machine_key_record,
        tokens,
    )
}

// =============================================================================
// ZID Logout Flow
// =============================================================================

pub fn handle_zid_logout(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: zos_identity::ipc::ZidLogoutRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse logout request: {}", e));
            return response::send_zid_logout_error(
                msg.from_pid,
                &msg.cap_slots,
                ZidError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("zid_logout", msg.from_pid, request.user_id);
        return response::send_zid_logout_error(
            msg.from_pid,
            &msg.cap_slots,
            ZidError::Unauthorized,
        );
    }

    // Delete session file from VFS
    let session_path = ZidSession::storage_path(request.user_id);
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    service.start_vfs_delete(
        &session_path,
        PendingStorageOp::DeleteZidSession { ctx },
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
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    // NOW we expect tokens from the login response
    let tokens: ZidTokens = match serde_json::from_slice(&login_response.body) {
        Ok(t) => t,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse login response: {}", e));
            if let Ok(body_str) = alloc::str::from_utf8(&login_response.body) {
                syscall::debug(&format!("IdentityService: Login response body: {}", body_str));
            }
            return response::send_zid_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
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
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::EnrollmentFailed(format!("Failed to serialize machine key: {}", e)),
            );
        }
    };

    // Write machine key via VFS (fire and forget - don't block on this)
    // VFS handles both content and inode atomically
    if let Err(e) = zos_vfs::async_client::send_write_request(&machine_key_path, &machine_key_json) {
        syscall::debug(&format!("Warning: Failed to store machine key: {:?}", e));
        // Don't fail enrollment if machine key storage fails - we can still use the session
    } else {
        syscall::debug("Machine key write request sent via VFS");
    }

    // Set login_type for machine key enrollment
    let mut tokens = tokens;
    tokens.login_type = zos_identity::ipc::LoginType::MachineKey;

    // Store ZID session
    let session = ZidSession {
        zid_endpoint: zid_endpoint.clone(),
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        session_id: tokens.session_id.clone(),
        machine_id: tokens.machine_id.clone(),
        login_type: tokens.login_type,
        expires_at: super::super::utils::parse_rfc3339_to_millis(&tokens.expires_at),
        created_at: now,
    };

    let session_json = match serde_json::to_vec(&session) {
        Ok(b) => b,
        Err(_) => return response::send_zid_enroll_success(ctx.client_pid, &ctx.cap_slots, tokens),
    };

    // Store the session via VFS
    let session_path = ZidSession::storage_path(user_id);
    service.start_vfs_write(
        &session_path,
        &session_json,
        PendingStorageOp::WriteZidEnrollSession {
            ctx,
            user_id,
            tokens,
            json_bytes: session_json.clone(),
        },
    )
}

// =============================================================================
// ZID Email Login Flow
// =============================================================================

/// Handle ZID email login request.
///
/// This authenticates with ZID server using email and password:
/// 1. Parse and validate request
/// 2. POST to {zid_endpoint}/v1/auth/login/email with email/password
/// 3. Store session on success
/// 4. Return tokens
pub fn handle_zid_login_email(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    syscall::debug("IdentityService: Handling ZID email login request");

    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: zos_identity::ipc::ZidEmailLoginRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse email login request: {}", e));
            return response::send_zid_email_login_error(
                msg.from_pid,
                &msg.cap_slots,
                ZidError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("zid_login_email", msg.from_pid, request.user_id);
        return response::send_zid_email_login_error(
            msg.from_pid,
            &msg.cap_slots,
            ZidError::Unauthorized,
        );
    }

    // Validate email format
    if !request.email.contains('@') || request.email.len() < 5 {
        return response::send_zid_email_login_error(
            msg.from_pid,
            &msg.cap_slots,
            ZidError::InvalidRequest("Invalid email format".into()),
        );
    }

    // Validate password length
    if request.password.len() < 8 {
        return response::send_zid_email_login_error(
            msg.from_pid,
            &msg.cap_slots,
            ZidError::InvalidRequest("Password must be at least 8 characters".into()),
        );
    }

    // Build request body for ZID email login
    let mut body = format!(
        r#"{{"email":"{}","password":"{}""#,
        request.email, request.password
    );

    // Add optional machine_id if provided
    if let Some(ref machine_id) = request.machine_id {
        body.push_str(&format!(r#","machine_id":"{}""#, machine_id));
    }

    // Add optional mfa_code if provided
    if let Some(ref mfa_code) = request.mfa_code {
        body.push_str(&format!(r#","mfa_code":"{}""#, mfa_code));
    }

    body.push('}');

    syscall::debug(&format!(
        "IdentityService: Submitting email login to {}/v1/auth/login/email",
        request.zid_endpoint
    ));

    let http_request = HttpRequest::post(format!("{}/v1/auth/login/email", request.zid_endpoint))
        .with_json_body(body.into_bytes())
        .with_timeout(15_000);

    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    service.start_network_fetch(
        &http_request,
        PendingNetworkOp::SubmitZidEmailLogin {
            ctx,
            user_id: request.user_id,
            zid_endpoint: request.zid_endpoint,
        },
    )
}

/// Continue ZID email login after receiving tokens from server.
pub fn continue_zid_email_login_after_network(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    login_response: zos_network::HttpSuccess,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);

    // Parse tokens from response
    let tokens: ZidTokens = match serde_json::from_slice(&login_response.body) {
        Ok(t) => t,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse email login response: {}", e));
            return response::send_zid_email_login_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::AuthenticationFailed,
            );
        }
    };

    syscall::debug(&format!(
        "IdentityService: Email login successful, session_id={}",
        tokens.session_id
    ));

    // Set login_type for email authentication
    let mut tokens = tokens;
    tokens.login_type = zos_identity::ipc::LoginType::Email;

    // Build session to store
    let now = syscall::get_wallclock();
    let session = ZidSession {
        zid_endpoint: zid_endpoint.clone(),
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        session_id: tokens.session_id.clone(),
        machine_id: tokens.machine_id.clone(),
        login_type: tokens.login_type,
        expires_at: super::super::utils::parse_rfc3339_to_millis(&tokens.expires_at),
        created_at: now,
    };

    let session_path = ZidSession::storage_path(user_id);
    match serde_json::to_vec(&session) {
        Ok(json_bytes) => service.start_vfs_write(
            &session_path,
            &json_bytes,
            PendingStorageOp::WriteZidEmailLoginSession {
                ctx,
                user_id,
                tokens,
                json_bytes: json_bytes.clone(),
            },
        ),
        Err(e) => {
            // Even if serialization fails, return tokens since auth succeeded
            syscall::debug(&format!(
                "IdentityService: Session serialization failed but returning tokens: {}",
                e
            ));
            response::send_zid_email_login_success(ctx.client_pid, &ctx.cap_slots, tokens)
        }
    }
}

// =============================================================================
// ZID Token Refresh Flow
// =============================================================================

/// Handle ZID token refresh request.
///
/// Flow:
/// 1. Parse request (user_id, zid_endpoint)
/// 2. Authorization check
/// 3. Read stored ZID session from VFS (to get refresh_token)
/// 4. POST to {zid_endpoint}/v1/auth/refresh
/// 5. Update stored session with new tokens
/// 6. Return ZidRefreshResponse with new tokens
pub fn handle_zid_refresh(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: zos_identity::ipc::ZidRefreshRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse refresh request: {}", e));
            return response::send_zid_refresh_error(
                msg.from_pid,
                &msg.cap_slots,
                ZidError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("zid_refresh", msg.from_pid, request.user_id);
        return response::send_zid_refresh_error(
            msg.from_pid,
            &msg.cap_slots,
            ZidError::Unauthorized,
        );
    }

    // Read stored ZID session (need session_id and machine_id even if refresh_token is provided)
    // The refresh_token_override will be used instead of VFS token to prevent race conditions
    let session_path = ZidSession::storage_path(request.user_id);
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    service.start_vfs_read(
        &session_path,
        PendingStorageOp::ReadZidSessionForRefresh {
            ctx,
            user_id: request.user_id,
            zid_endpoint: request.zid_endpoint,
            refresh_token_override: request.refresh_token,
        },
    )
}

/// Continue ZID refresh after reading stored session.
///
/// If `refresh_token_override` is provided (from frontend), use it instead of the
/// VFS session's refresh_token. This prevents race conditions where the frontend
/// has a newer token that hasn't been persisted to VFS yet.
pub fn continue_zid_refresh_after_session_read(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    data: &[u8],
    refresh_token_override: Option<String>,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);

    // Parse stored session to get session_id, machine_id, and fallback refresh_token
    let session: ZidSession = match serde_json::from_slice(data) {
        Ok(s) => s,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse stored session: {}", e));
            return response::send_zid_refresh_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::InvalidRequest("No valid session found".into()),
            );
        }
    };

    // Use override token if provided (from frontend), otherwise use VFS session token
    let refresh_token = refresh_token_override.unwrap_or_else(|| session.refresh_token.clone());

    if refresh_token.is_empty() {
        return response::send_zid_refresh_error(
            ctx.client_pid,
            &ctx.cap_slots,
            ZidError::InvalidRequest("No refresh token available".into()),
        );
    }

    // Build refresh request body (ZID requires refresh_token, session_id, machine_id)
    let refresh_body = format!(
        r#"{{"refresh_token":"{}","session_id":"{}","machine_id":"{}"}}"#,
        refresh_token, session.session_id, session.machine_id
    );

    syscall::debug(&format!(
        "IdentityService: Submitting token refresh for session {} machine {}",
        session.session_id, session.machine_id
    ));

    let refresh_request = zos_network::HttpRequest::post(format!(
        "{}/v1/auth/refresh",
        zid_endpoint
    ))
    .with_json_body(refresh_body.into_bytes())
    .with_timeout(10_000);

    // Pass session_id and machine_id through - refresh response may not include them
    let stored_session_id = session.session_id.clone();
    let stored_machine_id = session.machine_id.clone();

    service.start_network_fetch(
        &refresh_request,
        PendingNetworkOp::SubmitZidRefresh {
            ctx,
            user_id,
            zid_endpoint,
            session_id: stored_session_id,
            machine_id: stored_machine_id,
            login_type: session.login_type,
        },
    )
}

/// Partial tokens from ZID refresh response.
/// The refresh endpoint may not return session_id and machine_id since they don't change.
/// Note: login_type is intentionally not parsed here - we preserve it from the original session.
#[derive(Clone, Debug, serde::Deserialize)]
struct RefreshTokensPartial {
    pub access_token: String,
    pub refresh_token: String,
    /// Session ID - may be omitted in refresh response
    #[serde(default)]
    pub session_id: Option<String>,
    /// Machine ID - may be omitted in refresh response
    #[serde(default)]
    pub machine_id: Option<String>,
    pub expires_at: String,
    #[serde(default)]
    pub warning: Option<String>,
}

/// Continue ZID refresh after receiving new tokens from server.
pub fn continue_zid_refresh_after_network(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    stored_session_id: String,
    stored_machine_id: String,
    login_type: zos_identity::ipc::LoginType,
    refresh_response: zos_network::HttpSuccess,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);

    // Parse new tokens from response (session_id and machine_id may be omitted)
    let partial_tokens: RefreshTokensPartial = match serde_json::from_slice(&refresh_response.body) {
        Ok(t) => t,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse refresh response: {}", e));
            return response::send_zid_refresh_error(
                ctx.client_pid,
                &ctx.cap_slots,
                ZidError::NetworkError(format!("Invalid refresh response: {}", e)),
            );
        }
    };

    // Use session_id/machine_id from response if provided, otherwise use stored values
    let session_id = partial_tokens.session_id.unwrap_or(stored_session_id);
    let machine_id = partial_tokens.machine_id.unwrap_or(stored_machine_id);

    // Build full tokens struct for response
    let tokens = ZidTokens {
        access_token: partial_tokens.access_token.clone(),
        refresh_token: partial_tokens.refresh_token.clone(),
        session_id: session_id.clone(),
        machine_id: machine_id.clone(),
        expires_at: partial_tokens.expires_at.clone(),
        login_type, // Preserve from original session
        warning: partial_tokens.warning,
    };

    syscall::debug(&format!(
        "IdentityService: Token refresh successful, new expiry: {}",
        tokens.expires_at
    ));

    // Build updated session
    let now = syscall::get_wallclock();
    let session = ZidSession {
        zid_endpoint: zid_endpoint.clone(),
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        session_id,
        machine_id,
        login_type: tokens.login_type,
        expires_at: super::super::utils::parse_rfc3339_to_millis(&tokens.expires_at),
        created_at: now,
    };

    let session_json = match serde_json::to_vec(&session) {
        Ok(b) => b,
        Err(_) => {
            // Still return success with tokens even if serialization fails
            return response::send_zid_refresh_success(ctx.client_pid, &ctx.cap_slots, tokens);
        }
    };

    // Store updated session via VFS
    let session_path = ZidSession::storage_path(user_id);
    service.start_vfs_write(
        &session_path,
        &session_json,
        PendingStorageOp::WriteRefreshedZidSession {
            ctx,
            user_id,
            tokens,
            json_bytes: session_json.clone(),
        },
    )
}
