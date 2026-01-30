//! Network result handlers for identity service
//!
//! This module contains handlers for async network (HTTP) operation results.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use zos_identity::error::{CredentialError, ZidError};
use zos_identity::keystore::MachineKeyRecord;
use zos_network::{HttpResponse, HttpSuccess};

use zos_apps::AppError;
use super::response;
use zos_apps::syscall;

/// Result of handling a network operation.
pub enum NetworkHandlerResult {
    /// Operation complete, send response
    Done(Result<(), AppError>),
    /// Continue ZID login flow after challenge received
    ContinueZidLoginWithChallenge {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        machine_key: Box<MachineKeyRecord>,
        challenge_response: HttpSuccess,
        cap_slots: Vec<u32>,
    },
    /// Continue ZID login flow after tokens received
    ContinueZidLoginWithTokens {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        login_response: HttpSuccess,
        cap_slots: Vec<u32>,
    },
    /// Continue attach email flow after ZID accepted
    ContinueAttachEmail {
        client_pid: u32,
        user_id: u128,
        email: String,
        cap_slots: Vec<u32>,
    },
    /// Continue ZID enrollment after server response (identity created, need to chain login)
    ContinueZidEnroll {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        enroll_response: HttpSuccess,
        cap_slots: Vec<u32>,
    },
    /// Continue enrollment flow after challenge received (chained login)
    ContinueZidEnrollWithChallenge {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        machine_id: u128,
        identity_signing_public_key: [u8; 32],
        machine_signing_public_key: [u8; 32],
        machine_encryption_public_key: [u8; 32],
        machine_signing_sk: [u8; 32],
        machine_encryption_sk: [u8; 32],
        challenge_response: HttpSuccess,
        cap_slots: Vec<u32>,
    },
    /// Continue enrollment flow after tokens received (final step)
    ContinueZidEnrollWithTokens {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        machine_id: u128,
        identity_signing_public_key: [u8; 32],
        machine_signing_public_key: [u8; 32],
        machine_encryption_public_key: [u8; 32],
        machine_signing_sk: [u8; 32],
        machine_encryption_sk: [u8; 32],
        login_response: HttpSuccess,
        cap_slots: Vec<u32>,
    },
    // Combined Machine Key + ZID Enrollment variants
    /// Continue combined flow after identity creation (chain to login)
    ContinueCombinedEnroll {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        server_machine_id: String,
        cap_slots: Vec<u32>,
    },
    /// Continue combined flow after challenge received
    ContinueCombinedChallenge {
        client_pid: u32,
        challenge_response: HttpSuccess,
        cap_slots: Vec<u32>,
    },
    /// Continue combined flow after login - final step
    ContinueCombinedLogin {
        client_pid: u32,
        login_response: HttpSuccess,
        cap_slots: Vec<u32>,
    },
    /// Continue ZID token refresh after receiving new tokens
    ContinueZidRefresh {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        login_type: zos_identity::ipc::LoginType,
        refresh_response: HttpSuccess,
        cap_slots: Vec<u32>,
    },
    /// Continue ZID email login after receiving tokens
    ContinueZidEmailLogin {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        login_response: HttpSuccess,
        cap_slots: Vec<u32>,
    },
}

/// Handle RequestZidChallenge network result.
pub fn handle_zid_challenge_result(
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    machine_key: MachineKeyRecord,
    cap_slots: Vec<u32>,
    http_response: HttpResponse,
) -> NetworkHandlerResult {
    match http_response.result {
        Ok(success) if (200..300).contains(&success.status) => {
            NetworkHandlerResult::ContinueZidLoginWithChallenge {
                client_pid,
                user_id,
                zid_endpoint,
                machine_key: Box::new(machine_key),
                challenge_response: success,
                cap_slots,
            }
        }
        Ok(success) => {
            let error = parse_zid_error_response(&success.body, success.status);
            syscall::debug(&format!(
                "IdentityService: Challenge request failed with status {}: {:?}",
                success.status, error
            ));
            NetworkHandlerResult::Done(response::send_zid_login_error(
                client_pid, &cap_slots, error,
            ))
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Challenge request network error: {:?}",
                e
            ));
            NetworkHandlerResult::Done(response::send_zid_login_error(
                client_pid,
                &cap_slots,
                ZidError::NetworkError(e.message().into()),
            ))
        }
    }
}

/// Handle SubmitZidLogin network result.
pub fn handle_zid_login_result(
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    cap_slots: Vec<u32>,
    http_response: HttpResponse,
) -> NetworkHandlerResult {
    match http_response.result {
        Ok(success) if (200..300).contains(&success.status) => {
            NetworkHandlerResult::ContinueZidLoginWithTokens {
                client_pid,
                user_id,
                zid_endpoint,
                login_response: success,
                cap_slots,
            }
        }
        Ok(success) if success.status == 401 => {
            syscall::debug("IdentityService: Login authentication failed");
            NetworkHandlerResult::Done(response::send_zid_login_error(
                client_pid,
                &cap_slots,
                ZidError::AuthenticationFailed,
            ))
        }
        Ok(success) => {
            let error = parse_zid_error_response(&success.body, success.status);
            syscall::debug(&format!(
                "IdentityService: Login request failed with status {}: {:?}",
                success.status, error
            ));
            NetworkHandlerResult::Done(response::send_zid_login_error(
                client_pid, &cap_slots, error,
            ))
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Login request network error: {:?}",
                e
            ));
            NetworkHandlerResult::Done(response::send_zid_login_error(
                client_pid,
                &cap_slots,
                ZidError::NetworkError(e.message().into()),
            ))
        }
    }
}

/// Handle SubmitEmailToZid network result.
pub fn handle_email_to_zid_result(
    client_pid: u32,
    user_id: u128,
    email: String,
    cap_slots: Vec<u32>,
    http_response: HttpResponse,
) -> NetworkHandlerResult {
    match http_response.result {
        Ok(success) if (200..300).contains(&success.status) => {
            NetworkHandlerResult::ContinueAttachEmail {
                client_pid,
                user_id,
                email,
                cap_slots,
            }
        }
        Ok(success) if success.status == 400 => {
            let error = parse_zid_credential_error(&success.body);
            syscall::debug(&format!("IdentityService: ZID rejected email: {:?}", error));
            NetworkHandlerResult::Done(response::send_attach_email_error(
                client_pid, &cap_slots, error,
            ))
        }
        Ok(success) if success.status == 401 => {
            syscall::debug("IdentityService: ZID auth token invalid/expired");
            NetworkHandlerResult::Done(response::send_attach_email_error(
                client_pid,
                &cap_slots,
                CredentialError::StorageError("ZID session expired, please login again".into()),
            ))
        }
        Ok(success) if success.status == 409 => {
            syscall::debug("IdentityService: Email already registered with ZID");
            NetworkHandlerResult::Done(response::send_attach_email_error(
                client_pid,
                &cap_slots,
                CredentialError::AlreadyLinked,
            ))
        }
        Ok(success) => {
            syscall::debug(&format!(
                "IdentityService: ZID email request failed with status {}",
                success.status
            ));
            NetworkHandlerResult::Done(response::send_attach_email_error(
                client_pid,
                &cap_slots,
                CredentialError::StorageError(format!("ZID error: HTTP {}", success.status)),
            ))
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: ZID email request network error: {:?}",
                e
            ));
            NetworkHandlerResult::Done(response::send_attach_email_error(
                client_pid,
                &cap_slots,
                CredentialError::StorageError(format!("Network error: {}", e.message())),
            ))
        }
    }
}

/// Handle SubmitZidEnroll network result.
pub fn handle_zid_enroll_result(
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    cap_slots: Vec<u32>,
    http_response: HttpResponse,
) -> NetworkHandlerResult {
    match http_response.result {
        Ok(success) if (200..300).contains(&success.status) => {
            NetworkHandlerResult::ContinueZidEnroll {
                client_pid,
                user_id,
                zid_endpoint,
                enroll_response: success,
                cap_slots,
            }
        }
        Ok(success) if success.status == 409 => {
            syscall::debug("IdentityService: Machine already enrolled with ZID");
            NetworkHandlerResult::Done(response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::EnrollmentFailed("Machine already registered. Use Login instead.".into()),
            ))
        }
        Ok(success) => {
            let error = parse_zid_enroll_error(&success.body, success.status);
            syscall::debug(&format!(
                "IdentityService: Enrollment failed with status {}: {:?}",
                success.status, error
            ));
            NetworkHandlerResult::Done(response::send_zid_enroll_error(
                client_pid, &cap_slots, error,
            ))
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Enrollment network error: {:?}",
                e
            ));
            NetworkHandlerResult::Done(response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::NetworkError(e.message().into()),
            ))
        }
    }
}

/// Handle RequestZidChallengeAfterEnroll network result (challenge during chained login).
pub fn handle_zid_challenge_after_enroll_result(
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    machine_id: u128,
    identity_signing_public_key: [u8; 32],
    machine_signing_public_key: [u8; 32],
    machine_encryption_public_key: [u8; 32],
    machine_signing_sk: [u8; 32],
    machine_encryption_sk: [u8; 32],
    cap_slots: Vec<u32>,
    http_response: HttpResponse,
) -> NetworkHandlerResult {
    match http_response.result {
        Ok(success) if (200..300).contains(&success.status) => {
            syscall::debug("IdentityService: Challenge received after enrollment, continuing to login");
            NetworkHandlerResult::ContinueZidEnrollWithChallenge {
                client_pid,
                user_id,
                zid_endpoint,
                machine_id,
                identity_signing_public_key,
                machine_signing_public_key,
                machine_encryption_public_key,
                machine_signing_sk,
                machine_encryption_sk,
                challenge_response: success,
                cap_slots,
            }
        }
        Ok(success) => {
            let error = parse_zid_error_response(&success.body, success.status);
            syscall::debug(&format!(
                "IdentityService: Challenge request after enroll failed with status {}: {:?}",
                success.status, error
            ));
            // Return enrollment error since we're still in the enrollment flow
            NetworkHandlerResult::Done(response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::EnrollmentFailed(format!("Login challenge failed: {:?}", error)),
            ))
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Challenge request after enroll network error: {:?}",
                e
            ));
            NetworkHandlerResult::Done(response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::NetworkError(format!("Challenge request failed: {}", e.message())),
            ))
        }
    }
}

/// Handle SubmitZidLoginAfterEnroll network result (tokens after chained login).
pub fn handle_zid_login_after_enroll_result(
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    machine_id: u128,
    identity_signing_public_key: [u8; 32],
    machine_signing_public_key: [u8; 32],
    machine_encryption_public_key: [u8; 32],
    machine_signing_sk: [u8; 32],
    machine_encryption_sk: [u8; 32],
    cap_slots: Vec<u32>,
    http_response: HttpResponse,
) -> NetworkHandlerResult {
    match http_response.result {
        Ok(success) if (200..300).contains(&success.status) => {
            syscall::debug("IdentityService: Login successful after enrollment, tokens received");
            NetworkHandlerResult::ContinueZidEnrollWithTokens {
                client_pid,
                user_id,
                zid_endpoint,
                machine_id,
                identity_signing_public_key,
                machine_signing_public_key,
                machine_encryption_public_key,
                machine_signing_sk,
                machine_encryption_sk,
                login_response: success,
                cap_slots,
            }
        }
        Ok(success) if success.status == 401 => {
            syscall::debug("IdentityService: Login after enrollment failed - authentication error");
            NetworkHandlerResult::Done(response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::EnrollmentFailed("Login authentication failed after enrollment".into()),
            ))
        }
        Ok(success) => {
            let error = parse_zid_error_response(&success.body, success.status);
            syscall::debug(&format!(
                "IdentityService: Login after enroll failed with status {}: {:?}",
                success.status, error
            ));
            NetworkHandlerResult::Done(response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::EnrollmentFailed(format!("Login failed: {:?}", error)),
            ))
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Login after enroll network error: {:?}",
                e
            ));
            NetworkHandlerResult::Done(response::send_zid_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::NetworkError(format!("Login request failed: {}", e.message())),
            ))
        }
    }
}

// =============================================================================
// Combined Machine Key + ZID Enrollment handlers
// =============================================================================

/// Handle SubmitZidEnrollForCombined network result.
pub fn handle_combined_enroll_result(
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    _machine_id: u128,
    cap_slots: Vec<u32>,
    http_response: HttpResponse,
) -> NetworkHandlerResult {
    match http_response.result {
        Ok(success) if (200..300).contains(&success.status) => {
            // Parse the identity creation response to get the server's machine_id
            #[derive(serde::Deserialize)]
            #[allow(dead_code)]
            struct CreateIdentityResponse {
                identity_id: String,
                machine_id: String,
                namespace_id: String,
            }

            match serde_json::from_slice::<CreateIdentityResponse>(&success.body) {
                Ok(resp) => {
                    syscall::debug(&format!(
                        "IdentityService: Combined flow - identity created, machine_id={}",
                        resp.machine_id
                    ));
                    NetworkHandlerResult::ContinueCombinedEnroll {
                        client_pid,
                        user_id,
                        zid_endpoint,
                        server_machine_id: resp.machine_id,
                        cap_slots,
                    }
                }
                Err(e) => {
                    syscall::debug(&format!(
                        "IdentityService: Failed to parse identity creation response: {}",
                        e
                    ));
                    NetworkHandlerResult::Done(response::send_create_machine_key_and_enroll_error(
                        client_pid,
                        &cap_slots,
                        ZidError::EnrollmentFailed(format!("Invalid identity creation response: {}", e)),
                    ))
                }
            }
        }
        Ok(success) if success.status == 409 => {
            syscall::debug("IdentityService: Combined flow - machine already enrolled");
            NetworkHandlerResult::Done(response::send_create_machine_key_and_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::EnrollmentFailed("Machine already registered. Use Login instead.".into()),
            ))
        }
        Ok(success) => {
            let error = parse_zid_enroll_error(&success.body, success.status);
            syscall::debug(&format!(
                "IdentityService: Combined enrollment failed with status {}: {:?}",
                success.status, error
            ));
            NetworkHandlerResult::Done(response::send_create_machine_key_and_enroll_error(
                client_pid, &cap_slots, error,
            ))
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Combined enrollment network error: {:?}",
                e
            ));
            NetworkHandlerResult::Done(response::send_create_machine_key_and_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::NetworkError(e.message().into()),
            ))
        }
    }
}

/// Handle RequestZidChallengeForCombined network result.
pub fn handle_combined_challenge_result(
    client_pid: u32,
    cap_slots: Vec<u32>,
    http_response: HttpResponse,
) -> NetworkHandlerResult {
    match http_response.result {
        Ok(success) if (200..300).contains(&success.status) => {
            syscall::debug("IdentityService: Combined flow - challenge received");
            NetworkHandlerResult::ContinueCombinedChallenge {
                client_pid,
                challenge_response: success,
                cap_slots,
            }
        }
        Ok(success) => {
            let error = parse_zid_error_response(&success.body, success.status);
            syscall::debug(&format!(
                "IdentityService: Combined challenge failed with status {}: {:?}",
                success.status, error
            ));
            NetworkHandlerResult::Done(response::send_create_machine_key_and_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::EnrollmentFailed(format!("Challenge request failed: {:?}", error)),
            ))
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Combined challenge network error: {:?}",
                e
            ));
            NetworkHandlerResult::Done(response::send_create_machine_key_and_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::NetworkError(e.message().into()),
            ))
        }
    }
}

/// Handle SubmitZidLoginForCombined network result.
pub fn handle_combined_login_result(
    client_pid: u32,
    cap_slots: Vec<u32>,
    http_response: HttpResponse,
) -> NetworkHandlerResult {
    match http_response.result {
        Ok(success) if (200..300).contains(&success.status) => {
            syscall::debug("IdentityService: Combined flow - login successful, tokens received");
            NetworkHandlerResult::ContinueCombinedLogin {
                client_pid,
                login_response: success,
                cap_slots,
            }
        }
        Ok(success) if success.status == 401 => {
            syscall::debug("IdentityService: Combined flow - login authentication failed");
            NetworkHandlerResult::Done(response::send_create_machine_key_and_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::AuthenticationFailed,
            ))
        }
        Ok(success) => {
            let error = parse_zid_error_response(&success.body, success.status);
            syscall::debug(&format!(
                "IdentityService: Combined login failed with status {}: {:?}",
                success.status, error
            ));
            NetworkHandlerResult::Done(response::send_create_machine_key_and_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::EnrollmentFailed(format!("Login failed: {:?}", error)),
            ))
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Combined login network error: {:?}",
                e
            ));
            NetworkHandlerResult::Done(response::send_create_machine_key_and_enroll_error(
                client_pid,
                &cap_slots,
                ZidError::NetworkError(e.message().into()),
            ))
        }
    }
}

// =============================================================================
// ZID Token Refresh handler
// =============================================================================

/// Handle SubmitZidRefresh network result.
pub fn handle_zid_refresh_result(
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    login_type: zos_identity::ipc::LoginType,
    cap_slots: Vec<u32>,
    http_response: HttpResponse,
) -> NetworkHandlerResult {
    match http_response.result {
        Ok(success) if (200..300).contains(&success.status) => {
            syscall::debug("IdentityService: Token refresh successful");
            NetworkHandlerResult::ContinueZidRefresh {
                client_pid,
                user_id,
                zid_endpoint,
                login_type,
                refresh_response: success,
                cap_slots,
            }
        }
        Ok(success) if success.status == 401 => {
            syscall::debug("IdentityService: Refresh token expired or invalid");
            NetworkHandlerResult::Done(response::send_zid_refresh_error(
                client_pid,
                &cap_slots,
                ZidError::InvalidRefreshToken,
            ))
        }
        Ok(success) => {
            let error = parse_zid_error_response(&success.body, success.status);
            syscall::debug(&format!(
                "IdentityService: Token refresh failed with status {}: {:?}",
                success.status, error
            ));
            NetworkHandlerResult::Done(response::send_zid_refresh_error(
                client_pid, &cap_slots, error,
            ))
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Token refresh network error: {:?}",
                e
            ));
            NetworkHandlerResult::Done(response::send_zid_refresh_error(
                client_pid,
                &cap_slots,
                ZidError::NetworkError(e.message().into()),
            ))
        }
    }
}

// =============================================================================
// ZID Email Login handler
// =============================================================================

/// Handle SubmitZidEmailLogin network result.
pub fn handle_zid_email_login_result(
    client_pid: u32,
    user_id: u128,
    zid_endpoint: String,
    cap_slots: Vec<u32>,
    http_response: HttpResponse,
) -> NetworkHandlerResult {
    match http_response.result {
        Ok(success) if (200..300).contains(&success.status) => {
            syscall::debug("IdentityService: Email login successful, tokens received");
            NetworkHandlerResult::ContinueZidEmailLogin {
                client_pid,
                user_id,
                zid_endpoint,
                login_response: success,
                cap_slots,
            }
        }
        Ok(success) if success.status == 401 => {
            syscall::debug("IdentityService: Email login authentication failed");
            NetworkHandlerResult::Done(response::send_zid_email_login_error(
                client_pid,
                &cap_slots,
                ZidError::AuthenticationFailed,
            ))
        }
        Ok(success) if success.status == 403 => {
            // MFA required or account locked
            syscall::debug("IdentityService: Email login forbidden - may require MFA");
            let error = parse_zid_error_response(&success.body, success.status);
            NetworkHandlerResult::Done(response::send_zid_email_login_error(
                client_pid, &cap_slots, error,
            ))
        }
        Ok(success) => {
            let error = parse_zid_error_response(&success.body, success.status);
            syscall::debug(&format!(
                "IdentityService: Email login failed with status {}: {:?}",
                success.status, error
            ));
            NetworkHandlerResult::Done(response::send_zid_email_login_error(
                client_pid, &cap_slots, error,
            ))
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Email login network error: {:?}",
                e
            ));
            NetworkHandlerResult::Done(response::send_zid_email_login_error(
                client_pid,
                &cap_slots,
                ZidError::NetworkError(e.message().into()),
            ))
        }
    }
}

// =============================================================================
// Error parsing helpers
// =============================================================================

/// Parse ZID server error response into a ZidError.
pub fn parse_zid_error_response(body: &[u8], status: u16) -> ZidError {
    #[derive(serde::Deserialize)]
    struct ZidErrorOuter {
        error: serde_json::Value,
    }

    if let Ok(outer) = serde_json::from_slice::<ZidErrorOuter>(body) {
        // Handle nested error object: { "error": { "code": "...", "message": "..." } }
        if let Some(obj) = outer.error.as_object() {
            let code = obj.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let message = obj.get("message").and_then(|v| v.as_str()).unwrap_or("");

            return match code {
                "NOT_FOUND" if message.contains("Machine not found") => {
                    ZidError::MachineNotRegistered(message.into())
                }
                "UNAUTHORIZED" | "AUTHENTICATION_FAILED" => ZidError::AuthenticationFailed,
                "INVALID_CHALLENGE" | "CHALLENGE_EXPIRED" => ZidError::InvalidChallenge,
                _ => ZidError::ServerError(format!("{}: {}", code, message)),
            };
        }

        // Handle string error: { "error": "error_code" }
        if let Some(error_code) = outer.error.as_str() {
            return match error_code {
                "machine_not_found" => {
                    ZidError::MachineNotRegistered("Machine not registered with ZID server".into())
                }
                "authentication_failed" => ZidError::AuthenticationFailed,
                "invalid_challenge" => ZidError::InvalidChallenge,
                _ => ZidError::ServerError(error_code.into()),
            };
        }
    }

    // Fallback: generic error with status
    ZidError::ServerError(format!("HTTP {} error", status))
}

/// Parse ZID credential error response.
///
/// Handles multiple ZID error formats:
/// 1. Simple: `{"error": "email_already_registered", "message": "..."}`
/// 2. Nested: `{"error": {"code": "EMAIL_ALREADY_REGISTERED", "message": "..."}}`
pub fn parse_zid_credential_error(body: &[u8]) -> CredentialError {
    // Log the raw body for debugging
    if let Ok(body_str) = core::str::from_utf8(body) {
        syscall::debug(&format!(
            "IdentityService: Parsing ZID credential error body: {}",
            body_str
        ));
    }

    // Try to parse as a generic JSON value first
    let json_value: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Failed to parse error body as JSON: {}",
                e
            ));
            return CredentialError::StorageError("Invalid error response from ZID".into());
        }
    };

    // Extract the error field
    let error_field = match json_value.get("error") {
        Some(e) => e,
        None => {
            // No "error" field - check for direct message
            if let Some(msg) = json_value.get("message").and_then(|m| m.as_str()) {
                return map_credential_error_message(msg);
            }
            return CredentialError::StorageError("No error field in ZID response".into());
        }
    };

    // Handle nested object: {"error": {"code": "...", "message": "..."}}
    if let Some(obj) = error_field.as_object() {
        let code = obj.get("code").and_then(|v| v.as_str()).unwrap_or("");
        let message = obj
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");

        syscall::debug(&format!(
            "IdentityService: ZID error code={}, message={}",
            code, message
        ));

        return match code.to_uppercase().as_str() {
            "EMAIL_ALREADY_REGISTERED" | "EMAIL_EXISTS" | "CONFLICT" => {
                CredentialError::AlreadyLinked
            }
            "INVALID_EMAIL" | "INVALID_EMAIL_FORMAT" | "BAD_EMAIL" => {
                CredentialError::InvalidFormat
            }
            "PASSWORD_TOO_WEAK" | "WEAK_PASSWORD" => CredentialError::StorageError(
                "Password must be 12+ chars with uppercase, lowercase, number, and symbol".into(),
            ),
            "UNAUTHORIZED" | "AUTH_REQUIRED" => {
                CredentialError::StorageError("ZID session expired, please login again".into())
            }
            _ => {
                // Check message for common patterns
                map_credential_error_message(message)
            }
        };
    }

    // Handle simple string: {"error": "email_already_registered"}
    if let Some(error_code) = error_field.as_str() {
        syscall::debug(&format!(
            "IdentityService: ZID error string code={}",
            error_code
        ));

        return match error_code.to_lowercase().as_str() {
            "email_already_registered" | "email_exists" | "conflict" => {
                CredentialError::AlreadyLinked
            }
            "invalid_email_format" | "invalid_email" | "bad_email" => CredentialError::InvalidFormat,
            "password_too_weak" | "weak_password" => CredentialError::StorageError(
                "Password must be 12+ chars with uppercase, lowercase, number, and symbol".into(),
            ),
            _ => {
                // Check for message field
                if let Some(msg) = json_value.get("message").and_then(|m| m.as_str()) {
                    return map_credential_error_message(msg);
                }
                CredentialError::StorageError(error_code.into())
            }
        };
    }

    CredentialError::StorageError("Unknown ZID error format".into())
}

/// Map error message content to CredentialError
fn map_credential_error_message(message: &str) -> CredentialError {
    let lower = message.to_lowercase();
    if lower.contains("already registered")
        || lower.contains("already exists")
        || lower.contains("already linked")
        || lower.contains("email exists")
    {
        CredentialError::AlreadyLinked
    } else if lower.contains("invalid email") || lower.contains("email format") {
        CredentialError::InvalidFormat
    } else if lower.contains("password") && (lower.contains("weak") || lower.contains("strong")) {
        CredentialError::StorageError(
            "Password must be 12+ chars with uppercase, lowercase, number, and symbol".into(),
        )
    } else {
        CredentialError::StorageError(message.into())
    }
}

/// Parse ZID enrollment error response.
pub fn parse_zid_enroll_error(body: &[u8], status: u16) -> ZidError {
    #[derive(serde::Deserialize)]
    struct ZidErrorDetail {
        code: Option<String>,
        message: Option<String>,
        field: Option<String>,
        details: Option<serde_json::Value>,
    }

    #[derive(serde::Deserialize)]
    struct ZidErrorOuter {
        error: serde_json::Value,
    }

    // Try to parse structured error response
    if let Ok(outer) = serde_json::from_slice::<ZidErrorOuter>(body) {
        // Try to parse as structured error object with field details
        if let Ok(detail) = serde_json::from_value::<ZidErrorDetail>(outer.error.clone()) {
            let code = detail.code.as_deref().unwrap_or("UNKNOWN");
            let message = detail.message.as_deref().unwrap_or("Unknown error");

            // Build detailed error message
            let mut error_msg = String::new();
            error_msg.push_str(code);
            error_msg.push_str(": ");
            error_msg.push_str(message);

            if let Some(field) = detail.field {
                error_msg.push_str(" (field: ");
                error_msg.push_str(&field);
                error_msg.push(')');
            }

            if let Some(details) = detail.details {
                if let Some(details_str) = details.as_str() {
                    error_msg.push_str(" - ");
                    error_msg.push_str(details_str);
                } else if let Some(details_obj) = details.as_object() {
                    error_msg.push_str(" - ");
                    error_msg.push_str(&serde_json::to_string(&details_obj).unwrap_or_default());
                }
            }

            return match code {
                "CONFLICT" | "ALREADY_EXISTS" => ZidError::EnrollmentFailed(
                    "Identity or machine already registered. Use Login instead.".into(),
                ),
                "INVALID_PUBLIC_KEY" | "INVALID_SIGNATURE" | "VALIDATION_ERROR" => {
                    ZidError::EnrollmentFailed(error_msg)
                }
                "INVALID_REQUEST" | "MISSING_FIELD" | "INVALID_FORMAT" => {
                    ZidError::EnrollmentFailed(format!("Request validation failed: {}", error_msg))
                }
                _ => ZidError::EnrollmentFailed(error_msg),
            };
        }

        // Fall back to simple object parsing
        if let Some(obj) = outer.error.as_object() {
            let code = obj.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let message = obj.get("message").and_then(|v| v.as_str()).unwrap_or("");

            return match code {
                "CONFLICT" | "ALREADY_EXISTS" => ZidError::EnrollmentFailed(
                    "Machine already registered. Use Login instead.".into(),
                ),
                "INVALID_PUBLIC_KEY" => {
                    ZidError::EnrollmentFailed(format!("Invalid public key: {}", message))
                }
                _ if !code.is_empty() && !message.is_empty() => {
                    ZidError::EnrollmentFailed(format!("{}: {}", code, message))
                }
                _ => ZidError::EnrollmentFailed("Unknown enrollment error".into()),
            };
        }

        // Fall back to string error
        if let Some(error_str) = outer.error.as_str() {
            return match error_str {
                "conflict" | "already_exists" => ZidError::EnrollmentFailed(
                    "Machine already registered. Use Login instead.".into(),
                ),
                _ => ZidError::EnrollmentFailed(error_str.into()),
            };
        }
    }

    // Try to parse raw body as string for any error details
    if let Ok(body_str) = alloc::str::from_utf8(body) {
        if !body_str.is_empty() && body_str.len() < 500 {
            return ZidError::EnrollmentFailed(format!(
                "HTTP {} error: {}",
                status,
                body_str.trim()
            ));
        }
    }

    ZidError::EnrollmentFailed(format!("HTTP {} error (no details)", status))
}
