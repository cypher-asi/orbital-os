//! ZID session management and authentication flows
//!
//! Handlers for:
//! - ZID machine login (challenge-response authentication)
//! - ZID machine enrollment (register new identity)
//! - Session persistence and token management

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zos_apps::identity::crypto::{base64_decode, bytes_to_hex, format_uuid, sign_challenge};
use zos_apps::identity::pending::PendingStorageOp;
use zos_apps::identity::response;
use zos_apps::syscall;
use zos_apps::{AppError, Message};
use zos_identity::error::ZidError;
use zos_identity::ipc::{ZidLoginRequest, ZidSession, ZidTokens};
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
        Err(e) => return response::send_zid_login_error(msg.from_pid, &msg.cap_slots, ZidError::NetworkError(format!("Invalid request: {}", e))),
    };

    let machine_dir = format!("/home/{:032x}/.zos/identity/machine", request.user_id);
    service.start_storage_list(&machine_dir, PendingStorageOp::ReadMachineKeyForZidLogin {
        client_pid: msg.from_pid,
        user_id: request.user_id,
        zid_endpoint: request.zid_endpoint,
        cap_slots: msg.cap_slots.clone(),
    })
}

pub fn continue_zid_login_after_list(service: &mut IdentityService, client_pid: u32, user_id: u128, zid_endpoint: String, paths: Vec<String>, cap_slots: Vec<u32>) -> Result<(), AppError> {
    let path = paths.into_iter().find(|p| p.ends_with(".json"));
    match path {
        Some(p) => service.start_storage_read(&format!("content:{}", p), PendingStorageOp::ReadMachineKeyForZidLogin { client_pid, user_id, zid_endpoint, cap_slots }),
        None => response::send_zid_login_error(client_pid, &cap_slots, ZidError::MachineKeyNotFound),
    }
}

pub fn continue_zid_login_after_read(service: &mut IdentityService, client_pid: u32, user_id: u128, zid_endpoint: String, data: &[u8], cap_slots: Vec<u32>) -> Result<(), AppError> {
    let machine_key: MachineKeyRecord = match serde_json::from_slice(data) {
        Ok(r) => r,
        Err(_) => return response::send_zid_login_error(client_pid, &cap_slots, ZidError::MachineKeyNotFound),
    };

    let machine_id_uuid = format_uuid(machine_key.machine_id);
    let challenge_request = HttpRequest::get(format!("{}/v1/auth/challenge?machine_id={}", zid_endpoint, machine_id_uuid)).with_timeout(10_000);
    service.start_network_fetch(&challenge_request, zos_apps::identity::pending::PendingNetworkOp::RequestZidChallenge { client_pid, user_id, zid_endpoint, machine_key: Box::new(machine_key), cap_slots })
}

pub fn continue_zid_login_after_challenge(service: &mut IdentityService, client_pid: u32, user_id: u128, zid_endpoint: String, machine_key: MachineKeyRecord, challenge_response: zos_network::HttpSuccess, cap_slots: Vec<u32>) -> Result<(), AppError> {
    #[derive(serde::Deserialize)]
    struct ChallengeResponse { challenge: String, challenge_id: String }

    let challenge: ChallengeResponse = match serde_json::from_slice(&challenge_response.body) {
        Ok(c) => c,
        Err(_) => return response::send_zid_login_error(client_pid, &cap_slots, ZidError::InvalidChallenge),
    };

    let challenge_bytes = match base64_decode(&challenge.challenge) {
        Ok(b) => b,
        Err(_) => return response::send_zid_login_error(client_pid, &cap_slots, ZidError::InvalidChallenge),
    };

    let signature = sign_challenge(&challenge_bytes, &machine_key.signing_public_key);
    let signature_hex = bytes_to_hex(&signature);
    let machine_id_uuid = format_uuid(machine_key.machine_id);
    let login_body = format!(r#"{{"challenge_id":"{}","machine_id":"{}","signature":"{}"}}"#, challenge.challenge_id, machine_id_uuid, signature_hex);

    let login_request = HttpRequest::post(format!("{}/v1/auth/login/machine", zid_endpoint)).with_json_body(login_body.into_bytes()).with_timeout(10_000);
    service.start_network_fetch(&login_request, zos_apps::identity::pending::PendingNetworkOp::SubmitZidLogin { client_pid, user_id, zid_endpoint, cap_slots })
}

pub fn continue_zid_login_after_login(service: &mut IdentityService, client_pid: u32, user_id: u128, zid_endpoint: String, login_response: zos_network::HttpSuccess, cap_slots: Vec<u32>) -> Result<(), AppError> {
    let tokens: ZidTokens = match serde_json::from_slice(&login_response.body) {
        Ok(t) => t,
        Err(_) => return response::send_zid_login_error(client_pid, &cap_slots, ZidError::AuthenticationFailed),
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

    let session_path = format!("/home/{:032x}/.zos/identity/zid_session.json", user_id);
    match serde_json::to_vec(&session) {
        Ok(json_bytes) => service.start_storage_write(&format!("content:{}", session_path), &json_bytes.clone(), PendingStorageOp::WriteZidSessionContent { client_pid, user_id, tokens, json_bytes, cap_slots }),
        Err(e) => response::send_zid_login_error(client_pid, &cap_slots, ZidError::NetworkError(format!("Serialization failed: {}", e))),
    }
}

pub fn continue_zid_login_after_write_content(service: &mut IdentityService, client_pid: u32, user_id: u128, tokens: ZidTokens, json_bytes: Vec<u8>, cap_slots: Vec<u32>, result_type: u8) -> Result<(), AppError> {
    if result_type != storage_result::WRITE_OK {
        return response::send_zid_login_error(client_pid, &cap_slots, ZidError::NetworkError("Session write failed".into()));
    }

    let session_path = format!("/home/{:032x}/.zos/identity/zid_session.json", user_id);
    let now = syscall::get_wallclock();
    let inode = Inode::new_file(session_path.clone(), parent_path(&session_path).to_string(), "zid_session.json".to_string(), Some(user_id), json_bytes.len() as u64, None, now);

    match serde_json::to_vec(&inode) {
        Ok(inode_json) => service.start_storage_write(&format!("inode:{}", session_path), &inode_json, PendingStorageOp::WriteZidSessionInode { client_pid, tokens, cap_slots }),
        Err(e) => response::send_zid_login_error(client_pid, &cap_slots, ZidError::NetworkError(format!("Inode serialization failed: {}", e))),
    }
}

// =============================================================================
// ZID Enrollment Flow
// =============================================================================

pub fn handle_zid_enroll_machine(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    let request: ZidLoginRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => return response::send_zid_enroll_error(msg.from_pid, &msg.cap_slots, ZidError::NetworkError(format!("Invalid request: {}", e))),
    };

    let machine_dir = format!("/home/{:032x}/.zos/identity/machine", request.user_id);
    service.start_storage_list(&machine_dir, PendingStorageOp::ReadMachineKeyForZidEnroll {
        client_pid: msg.from_pid,
        user_id: request.user_id,
        zid_endpoint: request.zid_endpoint,
        cap_slots: msg.cap_slots.clone(),
    })
}

pub fn continue_zid_enroll_after_list(service: &mut IdentityService, client_pid: u32, user_id: u128, zid_endpoint: String, paths: Vec<String>, cap_slots: Vec<u32>) -> Result<(), AppError> {
    let path = paths.into_iter().find(|p| p.ends_with(".json"));
    match path {
        Some(p) => service.start_storage_read(&format!("content:{}", p), PendingStorageOp::ReadMachineKeyForZidEnroll { client_pid, user_id, zid_endpoint, cap_slots }),
        None => response::send_zid_enroll_error(client_pid, &cap_slots, ZidError::MachineKeyNotFound),
    }
}

pub fn continue_zid_enroll_after_read(service: &mut IdentityService, client_pid: u32, user_id: u128, zid_endpoint: String, data: &[u8], cap_slots: Vec<u32>) -> Result<(), AppError> {
    let machine_key: MachineKeyRecord = match serde_json::from_slice(data) {
        Ok(r) => r,
        Err(_) => return response::send_zid_enroll_error(client_pid, &cap_slots, ZidError::MachineKeyNotFound),
    };

    let machine_id_uuid = format_uuid(machine_key.machine_id);
    let public_key_hex = bytes_to_hex(&machine_key.signing_public_key);
    let enroll_body = format!(r#"{{"machine_id":"{}","public_key":"{}"}}"#, machine_id_uuid, public_key_hex);
    let enroll_request = HttpRequest::post(format!("{}/v1/identity", zid_endpoint)).with_json_body(enroll_body.into_bytes()).with_timeout(10_000);
    service.start_network_fetch(&enroll_request, zos_apps::identity::pending::PendingNetworkOp::SubmitZidEnroll { client_pid, user_id, zid_endpoint, cap_slots })
}

pub fn continue_zid_enroll_after_submit(service: &mut IdentityService, client_pid: u32, user_id: u128, zid_endpoint: String, enroll_response: zos_network::HttpSuccess, cap_slots: Vec<u32>) -> Result<(), AppError> {
    #[derive(serde::Deserialize)]
    struct EnrollResponse { access_token: String, refresh_token: String, session_id: String, expires_in: u64 }

    let enroll: EnrollResponse = match serde_json::from_slice(&enroll_response.body) {
        Ok(e) => e,
        Err(e) => return response::send_zid_enroll_error(client_pid, &cap_slots, ZidError::EnrollmentFailed(format!("Invalid response: {}", e))),
    };

    let tokens = ZidTokens { access_token: enroll.access_token, refresh_token: enroll.refresh_token, session_id: enroll.session_id, expires_in: enroll.expires_in };
    let session = ZidSession {
        zid_endpoint: zid_endpoint.clone(),
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
        session_id: tokens.session_id.clone(),
        expires_at: syscall::get_wallclock() + tokens.expires_in * 1000,
        created_at: syscall::get_wallclock(),
    };

    let json_bytes = match serde_json::to_vec(&session) {
        Ok(b) => b,
        Err(_) => return response::send_zid_enroll_success(client_pid, &cap_slots, tokens),
    };

    let session_path = format!("/home/{:032x}/.zos/identity/zid_session.json", user_id);
    service.start_storage_write(&format!("content:{}", session_path), &json_bytes.clone(), PendingStorageOp::WriteZidEnrollSessionContent { client_pid, user_id, tokens, json_bytes, cap_slots })
}
