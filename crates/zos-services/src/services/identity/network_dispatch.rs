//! Network Result Dispatch
//!
//! Handles MSG_NET_RESULT messages and routes to appropriate handlers
//! based on the pending network operation type.

use super::handlers::{credentials, session};
use super::network::{self as network_handlers, NetworkHandlerResult};
use super::pending::PendingNetworkOp;
use super::IdentityService;
use zos_apps::{AppError, Message};
use zos_network::{HttpResponse, NetworkError};

impl IdentityService {
    // =========================================================================
    // Network result handler
    // =========================================================================

    pub fn handle_net_result(&mut self, msg: &Message) -> Result<(), AppError> {
        if msg.data.len() < 9 {
            return Ok(());
        }

        let request_id = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);
        let result_type = msg.data[4];
        let data_len =
            u32::from_le_bytes([msg.data[5], msg.data[6], msg.data[7], msg.data[8]]) as usize;
        let data = if data_len > 0 && msg.data.len() >= 9 + data_len {
            &msg.data[9..9 + data_len]
        } else {
            &[]
        };

        let pending_op = match self.pending_net_ops.remove(&request_id) {
            Some(op) => op,
            None => return Ok(()),
        };

        let http_response: HttpResponse = if result_type == 0 && !data.is_empty() {
            serde_json::from_slice(data)
                .unwrap_or_else(|_| HttpResponse::err(NetworkError::Other("Parse error".into())))
        } else {
            HttpResponse::err(NetworkError::Other("Network error".into()))
        };

        self.dispatch_network_result(pending_op, http_response)
    }

    fn dispatch_network_result(
        &mut self,
        op: PendingNetworkOp,
        http_response: HttpResponse,
    ) -> Result<(), AppError> {
        match op {
            PendingNetworkOp::RequestZidChallenge {
                ctx,
                user_id,
                zid_endpoint,
                machine_key,
            } => {
                match network_handlers::handle_zid_challenge_result(
                    ctx.client_pid,
                    user_id,
                    zid_endpoint,
                    *machine_key,
                    ctx.cap_slots,
                    http_response,
                ) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueZidLoginWithChallenge {
                        client_pid,
                        user_id,
                        zid_endpoint,
                        machine_key,
                        challenge_response,
                        cap_slots,
                    } => session::continue_zid_login_after_challenge(
                        self,
                        client_pid,
                        user_id,
                        zid_endpoint,
                        *machine_key,
                        challenge_response,
                        cap_slots,
                    ),
                    _ => Ok(()),
                }
            }
            PendingNetworkOp::SubmitZidLogin {
                ctx,
                user_id,
                zid_endpoint,
            } => {
                match network_handlers::handle_zid_login_result(
                    ctx.client_pid,
                    user_id,
                    zid_endpoint,
                    ctx.cap_slots,
                    http_response,
                ) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueZidLoginWithTokens {
                        client_pid,
                        user_id,
                        zid_endpoint,
                        login_response,
                        cap_slots,
                    } => session::continue_zid_login_after_login(
                        self,
                        client_pid,
                        user_id,
                        zid_endpoint,
                        login_response,
                        cap_slots,
                    ),
                    _ => Ok(()),
                }
            }
            PendingNetworkOp::SubmitEmailToZid {
                ctx,
                user_id,
                email,
            } => {
                match network_handlers::handle_email_to_zid_result(
                    ctx.client_pid,
                    user_id,
                    email,
                    ctx.cap_slots,
                    http_response,
                ) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueAttachEmail {
                        client_pid,
                        user_id,
                        email,
                        cap_slots,
                    } => credentials::continue_attach_email_after_zid(
                        self, client_pid, user_id, email, cap_slots,
                    ),
                    _ => Ok(()),
                }
            }
            PendingNetworkOp::SubmitZidEnroll {
                ctx,
                user_id,
                zid_endpoint,
                identity_id,
                machine_id,
                identity_signing_public_key,
                machine_signing_public_key,
                machine_encryption_public_key,
                machine_signing_sk,
                machine_encryption_sk,
            } => {
                match network_handlers::handle_zid_enroll_result(
                    ctx.client_pid,
                    user_id,
                    zid_endpoint.clone(),
                    ctx.cap_slots.clone(),
                    http_response,
                ) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueZidEnroll {
                        client_pid: _,
                        user_id: _,
                        zid_endpoint: _,
                        enroll_response,
                        cap_slots: _,
                    } => session::continue_zid_enroll_after_submit(
                        self,
                        ctx.client_pid,
                        user_id,
                        zid_endpoint,
                        enroll_response,
                        ctx.cap_slots,
                        identity_id,
                        machine_id,
                        identity_signing_public_key,
                        machine_signing_public_key,
                        machine_encryption_public_key,
                        machine_signing_sk,
                        machine_encryption_sk,
                    ),
                    _ => Ok(()),
                }
            }
            // Chained login flow after identity creation
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
            } => {
                match network_handlers::handle_zid_challenge_after_enroll_result(
                    ctx.client_pid,
                    user_id,
                    zid_endpoint.clone(),
                    machine_id,
                    identity_signing_public_key,
                    machine_signing_public_key,
                    machine_encryption_public_key,
                    machine_signing_sk,
                    machine_encryption_sk,
                    ctx.cap_slots.clone(),
                    http_response,
                ) {
                    NetworkHandlerResult::Done(r) => r,
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
                        challenge_response,
                        cap_slots,
                    } => session::continue_zid_enroll_after_challenge(
                        self,
                        client_pid,
                        user_id,
                        zid_endpoint,
                        machine_id,
                        identity_signing_public_key,
                        machine_signing_public_key,
                        machine_encryption_public_key,
                        machine_signing_sk,
                        machine_encryption_sk,
                        challenge_response,
                        cap_slots,
                    ),
                    _ => Ok(()),
                }
            }
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
            } => {
                match network_handlers::handle_zid_login_after_enroll_result(
                    ctx.client_pid,
                    user_id,
                    zid_endpoint.clone(),
                    machine_id,
                    identity_signing_public_key,
                    machine_signing_public_key,
                    machine_encryption_public_key,
                    machine_signing_sk,
                    machine_encryption_sk,
                    ctx.cap_slots.clone(),
                    http_response,
                ) {
                    NetworkHandlerResult::Done(r) => r,
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
                        login_response,
                        cap_slots,
                    } => session::continue_zid_enroll_after_login(
                        self,
                        client_pid,
                        user_id,
                        zid_endpoint,
                        machine_id,
                        identity_signing_public_key,
                        machine_signing_public_key,
                        machine_encryption_public_key,
                        machine_signing_sk,
                        machine_encryption_sk,
                        login_response,
                        cap_slots,
                    ),
                    _ => Ok(()),
                }
            }
            // Combined Machine Key + ZID Enrollment flow
            PendingNetworkOp::SubmitZidEnrollForCombined {
                ctx,
                user_id,
                zid_endpoint,
                machine_id,
                identity_signing_public_key,
                machine_signing_public_key,
                machine_encryption_public_key,
                machine_signing_sk,
                machine_encryption_sk,
                machine_key_record,
            } => {
                match network_handlers::handle_combined_enroll_result(
                    ctx.client_pid,
                    user_id,
                    zid_endpoint.clone(),
                    machine_id,
                    ctx.cap_slots.clone(),
                    http_response,
                ) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueCombinedEnroll {
                        client_pid,
                        user_id,
                        zid_endpoint,
                        server_machine_id,
                        cap_slots,
                    } => session::continue_combined_enroll_after_identity_create(
                        self,
                        client_pid,
                        user_id,
                        zid_endpoint,
                        machine_id,
                        identity_signing_public_key,
                        machine_signing_public_key,
                        machine_encryption_public_key,
                        machine_signing_sk,
                        machine_encryption_sk,
                        *machine_key_record,
                        server_machine_id,
                        cap_slots,
                    ),
                    _ => Ok(()),
                }
            }
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
                machine_key_record,
            } => {
                match network_handlers::handle_combined_challenge_result(
                    ctx.client_pid,
                    ctx.cap_slots.clone(),
                    http_response,
                ) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueCombinedChallenge {
                        client_pid: _,
                        challenge_response,
                        cap_slots,
                    } => session::continue_combined_enroll_after_challenge(
                        self,
                        ctx.client_pid,
                        user_id,
                        zid_endpoint,
                        machine_id,
                        identity_signing_public_key,
                        machine_signing_public_key,
                        machine_encryption_public_key,
                        machine_signing_sk,
                        machine_encryption_sk,
                        *machine_key_record,
                        challenge_response,
                        cap_slots,
                    ),
                    _ => Ok(()),
                }
            }
            PendingNetworkOp::SubmitZidLoginForCombined {
                ctx,
                user_id,
                zid_endpoint,
                machine_id: _,
                identity_signing_public_key: _,
                machine_signing_public_key: _,
                machine_encryption_public_key: _,
                machine_signing_sk: _,
                machine_encryption_sk: _,
                machine_key_record,
            } => {
                match network_handlers::handle_combined_login_result(
                    ctx.client_pid,
                    ctx.cap_slots.clone(),
                    http_response,
                ) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueCombinedLogin {
                        client_pid: _,
                        login_response,
                        cap_slots,
                    } => session::continue_combined_enroll_after_login(
                        self,
                        ctx.client_pid,
                        user_id,
                        zid_endpoint,
                        *machine_key_record,
                        login_response,
                        cap_slots,
                    ),
                    _ => Ok(()),
                }
            }
            // ZID Token Refresh
            PendingNetworkOp::SubmitZidRefresh {
                ctx,
                user_id,
                zid_endpoint,
                session_id,
                machine_id,
                login_type,
            } => {
                match network_handlers::handle_zid_refresh_result(
                    ctx.client_pid,
                    user_id,
                    zid_endpoint.clone(),
                    session_id,
                    machine_id,
                    login_type,
                    ctx.cap_slots.clone(),
                    http_response,
                ) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueZidRefresh {
                        client_pid,
                        user_id,
                        zid_endpoint,
                        session_id,
                        machine_id,
                        login_type,
                        refresh_response,
                        cap_slots,
                    } => session::continue_zid_refresh_after_network(
                        self,
                        client_pid,
                        user_id,
                        zid_endpoint,
                        session_id,
                        machine_id,
                        login_type,
                        refresh_response,
                        cap_slots,
                    ),
                    _ => Ok(()),
                }
            }
            // ZID Email Login
            PendingNetworkOp::SubmitZidEmailLogin {
                ctx,
                user_id,
                zid_endpoint,
            } => {
                match network_handlers::handle_zid_email_login_result(
                    ctx.client_pid,
                    user_id,
                    zid_endpoint.clone(),
                    ctx.cap_slots.clone(),
                    http_response,
                ) {
                    NetworkHandlerResult::Done(r) => r,
                    NetworkHandlerResult::ContinueZidEmailLogin {
                        client_pid,
                        user_id,
                        zid_endpoint,
                        login_response,
                        cap_slots,
                    } => session::continue_zid_email_login_after_network(
                        self,
                        client_pid,
                        user_id,
                        zid_endpoint,
                        login_response,
                        cap_slots,
                    ),
                    _ => Ok(()),
                }
            }
        }
    }
}
