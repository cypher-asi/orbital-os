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

use crate::error::AppError;
use crate::identity::response;
use crate::syscall;

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
pub fn parse_zid_credential_error(body: &[u8]) -> CredentialError {
    #[derive(serde::Deserialize)]
    struct ZidErrorResponse {
        error: Option<String>,
        message: Option<String>,
    }

    if let Ok(err_response) = serde_json::from_slice::<ZidErrorResponse>(body) {
        if let Some(error_code) = err_response.error {
            return match error_code.as_str() {
                "email_already_registered" => CredentialError::AlreadyLinked,
                "invalid_email_format" => CredentialError::InvalidFormat,
                "password_too_weak" => CredentialError::StorageError(
                    "Password must be 12+ chars with uppercase, lowercase, number, and symbol"
                        .into(),
                ),
                _ => CredentialError::StorageError(err_response.message.unwrap_or(error_code)),
            };
        }
    }

    CredentialError::StorageError("Unknown ZID error".into())
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
