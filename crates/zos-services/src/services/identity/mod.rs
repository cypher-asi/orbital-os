//! Identity Service (PID 3)
//!
//! The IdentityService manages user cryptographic identities. It:
//! - Generates Neural Keys (entropy, key derivation, Shamir splitting)
//! - Stores public keys to VFS (via VFS IPC - Invariant 31 compliant)
//! - Handles key recovery from Shamir shards
//! - Manages machine key records
//!
//! # Safety Invariants (per zos-service.md Rule 0)
//!
//! ## Success Conditions
//! - A request succeeds only when ALL of:
//!   1. Caller is authorized to act on the target user_id
//!   2. All required data is parsed and validated
//!   3. All storage operations complete successfully
//!   4. Response is sent to the original caller
//!
//! ## Acceptable Partial Failure
//! - Orphan VFS content (content written but inode write failed) - GC will clean up
//! - Session write failure after successful ZID authentication (tokens still returned)
//!
//! ## Forbidden States
//! - Inode pointing to missing content
//! - Returning success before storage commit
//! - Processing requests for user_id without authorization check
//! - Silent fallthrough on parse errors (must return InvalidRequest)
//! - Unbounded pending operation growth (enforced via MAX_PENDING_OPS)
//!
//! # Protocol
//!
//! Apps communicate with IdentityService via IPC:
//!
//! - `MSG_GENERATE_NEURAL_KEY (0x7054)`: Generate a new Neural Key
//! - `MSG_RECOVER_NEURAL_KEY (0x7056)`: Recover from shards
//! - `MSG_GET_IDENTITY_KEY (0x7052)`: Get stored public keys
//! - `MSG_CREATE_MACHINE_KEY (0x7060)`: Create machine record
//! - `MSG_LIST_MACHINE_KEYS (0x7062)`: List all machines
//! - `MSG_REVOKE_MACHINE_KEY (0x7066)`: Delete machine record
//! - `MSG_ROTATE_MACHINE_KEY (0x7068)`: Update machine keys
//!
//! # Architecture
//!
//! This service uses modular components:
//! - `vfs_helpers`: Async VFS and network operation starters
//! - `vfs_dispatch`: VFS result handling and dispatch
//! - `network_dispatch`: Network result handling
//! - `handlers`: Message handlers for each IPC message type
//! - `pending`: Async operation state tracking
//! - `response`: IPC response helpers
//! - `network`: Async network result processing
//!
//! # Storage Access
//!
//! This service uses VFS IPC (async pattern) for storage.
//! All storage operations flow through VFS Service (PID 4) per Invariant 31.

extern crate alloc;

// Split modules for dispatch logic
mod auth;
mod keystore_dispatch;
mod keystore_helpers;
mod network_dispatch;
mod vfs_dispatch;
mod vfs_helpers;

// Public modules
pub mod handlers;
pub mod network;
pub mod pending;
pub mod response;
pub mod utils;

// Re-export authorization for use in handlers
pub use auth::{check_user_authorization, log_denial, AuthResult};

/// Maximum number of pending VFS operations (Rule 11: Resource & DoS)
pub const MAX_PENDING_VFS_OPS: usize = 64;

/// Maximum number of pending keystore operations (Rule 11: Resource & DoS)
pub const MAX_PENDING_KEYSTORE_OPS: usize = 64;

/// Maximum number of pending network operations (Rule 11: Resource & DoS)
pub const MAX_PENDING_NET_OPS: usize = 32;

#[cfg(test)]
mod tests;

use alloc::collections::BTreeMap;

use crate::manifests::IDENTITY_SERVICE_MANIFEST;
use pending::{PendingKeystoreOp, PendingNetworkOp, PendingStorageOp};
use zos_apps::syscall;
use zos_apps::{AppContext, AppError, AppManifest, ControlFlow, Message, ZeroApp};
use zos_process::{
    identity_cred, identity_key, identity_machine, identity_prefs, identity_zid, net,
};
use zos_vfs::async_client;
use zos_vfs::client::keystore_async;

/// IdentityService - manages user cryptographic identities
#[derive(Default)]
pub struct IdentityService {
    /// Whether we have registered with init
    pub registered: bool,
    /// Pending VFS operations: vfs_op_id -> operation context
    /// VFS IPC doesn't return request IDs, so we use our own counter.
    pub pending_vfs_ops: BTreeMap<u32, PendingStorageOp>,
    /// Counter for generating VFS operation IDs
    pub next_vfs_op_id: u32,
    /// Pending keystore operations: keystore_op_id -> operation context
    /// Keystore IPC doesn't return request IDs, so we use our own counter.
    pub pending_keystore_ops: BTreeMap<u32, PendingKeystoreOp>,
    /// Counter for generating keystore operation IDs
    pub next_keystore_op_id: u32,
    /// Pending network operations: request_id -> operation context
    pub pending_net_ops: BTreeMap<u32, PendingNetworkOp>,
}

impl ZeroApp for IdentityService {
    fn manifest() -> &'static AppManifest {
        &IDENTITY_SERVICE_MANIFEST
    }

    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError> {
        syscall::debug(&alloc::format!(
            "IdentityService: init called, PID={}, input_slot={:?}",
            ctx.pid,
            ctx.input_endpoint
        ));
        Ok(())
    }

    fn update(&mut self, ctx: &AppContext) -> ControlFlow {
        if !self.registered {
            syscall::debug(&alloc::format!(
                "IdentityService: Registering with init, endpoint_slot={:?}",
                ctx.input_endpoint
            ));
            let name = b"identity_service";
            // Input endpoint is always slot 1 for services
            let endpoint_slot: u64 = ctx.input_endpoint.unwrap_or(1) as u64;
            let mut data = alloc::vec::Vec::with_capacity(1 + name.len() + 8);
            data.push(name.len() as u8);
            data.extend_from_slice(name);
            data.extend_from_slice(&endpoint_slot.to_le_bytes());
            match syscall::send(0, zos_process::init::MSG_REGISTER_SERVICE, &data) {
                Ok(_) => {
                    self.registered = true;
                    syscall::debug("IdentityService: Registration message sent successfully");
                }
                Err(e) => {
                    syscall::debug(&alloc::format!(
                        "IdentityService: Registration FAILED: {:?}",
                        e
                    ));
                }
            }
        }
        ControlFlow::Yield
    }

    fn on_message(&mut self, _ctx: &AppContext, msg: Message) -> Result<(), AppError> {
        syscall::debug(&alloc::format!(
            "IdentityService: Received message tag=0x{:x} from_pid={}",
            msg.tag, msg.from_pid
        ));

        // Check for VFS responses first (for directory operations)
        if async_client::is_vfs_response(msg.tag) {
            return self.handle_vfs_result(&msg);
        }

        // Check for Keystore responses (for key storage operations)
        if keystore_async::is_keystore_response(msg.tag) {
            return self.handle_keystore_result(&msg);
        }

        match msg.tag {
            identity_key::MSG_GENERATE_NEURAL_KEY => {
                handlers::keys::handle_generate_neural_key(self, &msg)
            }
            identity_key::MSG_RECOVER_NEURAL_KEY => {
                handlers::keys::handle_recover_neural_key(self, &msg)
            }
            identity_key::MSG_GET_IDENTITY_KEY => {
                handlers::keys::handle_get_identity_key(self, &msg)
            }
            identity_machine::MSG_CREATE_MACHINE_KEY => {
                handlers::keys::handle_create_machine_key(self, &msg)
            }
            identity_machine::MSG_LIST_MACHINE_KEYS => {
                handlers::keys::handle_list_machine_keys(self, &msg)
            }
            identity_machine::MSG_REVOKE_MACHINE_KEY => {
                handlers::keys::handle_revoke_machine_key(self, &msg)
            }
            identity_machine::MSG_ROTATE_MACHINE_KEY => {
                handlers::keys::handle_rotate_machine_key(self, &msg)
            }
            identity_machine::MSG_GET_MACHINE_KEY => {
                handlers::keys::handle_get_machine_key(self, &msg)
            }
            identity_machine::MSG_CREATE_MACHINE_KEY_AND_ENROLL => {
                handlers::keys::handle_create_machine_key_and_enroll(self, &msg)
            }
            identity_cred::MSG_ATTACH_EMAIL => {
                handlers::credentials::handle_attach_email(self, &msg)
            }
            identity_cred::MSG_GET_CREDENTIALS => {
                handlers::credentials::handle_get_credentials(self, &msg)
            }
            identity_cred::MSG_UNLINK_CREDENTIAL => {
                handlers::credentials::handle_unlink_credential(self, &msg)
            }
            identity_zid::MSG_ZID_LOGIN => handlers::session::handle_zid_login(self, &msg),
            identity_zid::MSG_ZID_ENROLL_MACHINE => {
                handlers::session::handle_zid_enroll_machine(self, &msg)
            }
            identity_zid::MSG_ZID_LOGOUT => handlers::session::handle_zid_logout(self, &msg),
            identity_zid::MSG_ZID_REFRESH => handlers::session::handle_zid_refresh(self, &msg),
            identity_zid::MSG_ZID_LOGIN_EMAIL => {
                handlers::session::handle_zid_login_email(self, &msg)
            }
            identity_prefs::MSG_GET_IDENTITY_PREFERENCES => {
                handlers::preferences::handle_get_preferences(self, &msg)
            }
            identity_prefs::MSG_SET_DEFAULT_KEY_SCHEME => {
                handlers::preferences::handle_set_default_key_scheme(self, &msg)
            }
            identity_prefs::MSG_SET_DEFAULT_MACHINE_KEY => {
                handlers::preferences::handle_set_default_machine_key(self, &msg)
            }
            net::MSG_NET_RESULT => self.handle_net_result(&msg),
            _ => {
                syscall::debug(&alloc::format!(
                    "IdentityService: Unknown message tag 0x{:x}",
                    msg.tag
                ));
                Ok(())
            }
        }
    }

    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("IdentityService: shutdown");
    }
}
