//! Identity Service (PID 3)
//!
//! The IdentityService manages user cryptographic identities. It:
//! - Generates Neural Keys (entropy, key derivation, Shamir splitting)
//! - Stores public keys to VFS (via async storage syscalls)
//! - Handles key recovery from Shamir shards
//! - Manages machine key records
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
//! This service uses modular components from `zos_apps::identity`:
//! - `crypto`: Key generation, Shamir splitting, signing
//! - `pending`: Async operation state tracking
//! - `response`: IPC response helpers
//! - `storage_handlers`: Async storage result processing
//! - `network_handlers`: Async network result processing

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

mod handlers;
mod service;

use service::IdentityService;

use zos_apps::manifest::IDENTITY_SERVICE_MANIFEST;
use zos_apps::syscall;
use zos_apps::{app_main, AppContext, AppError, AppManifest, ControlFlow, Message, ZeroApp};
use zos_process::{identity_cred, identity_key, identity_machine, identity_zid, net, MSG_STORAGE_RESULT};

impl ZeroApp for IdentityService {
    fn manifest() -> &'static AppManifest {
        &IDENTITY_SERVICE_MANIFEST
    }

    fn init(&mut self, _ctx: &AppContext) -> Result<(), AppError> {
        syscall::debug("IdentityService: init");
        Ok(())
    }

    fn update(&mut self, ctx: &AppContext) -> ControlFlow {
        if !self.registered {
            syscall::debug("IdentityService: Registering with init");
            let name = b"identity_service";
            // Input endpoint is always slot 1 for services
            let endpoint_slot: u64 = ctx.input_endpoint.unwrap_or(1) as u64;
            let mut data = alloc::vec::Vec::with_capacity(1 + name.len() + 8);
            data.push(name.len() as u8);
            data.extend_from_slice(name);
            data.extend_from_slice(&endpoint_slot.to_le_bytes());
            let _ = syscall::send(0, zos_process::init::MSG_REGISTER_SERVICE, &data);
            self.registered = true;
        }
        ControlFlow::Yield
    }

    fn on_message(&mut self, _ctx: &AppContext, msg: Message) -> Result<(), AppError> {
        match msg.tag {
            identity_key::MSG_GENERATE_NEURAL_KEY => handlers::keys::handle_generate_neural_key(self, &msg),
            identity_key::MSG_RECOVER_NEURAL_KEY => handlers::keys::handle_recover_neural_key(self, &msg),
            identity_key::MSG_GET_IDENTITY_KEY => handlers::keys::handle_get_identity_key(self, &msg),
            identity_machine::MSG_CREATE_MACHINE_KEY => handlers::keys::handle_create_machine_key(self, &msg),
            identity_machine::MSG_LIST_MACHINE_KEYS => handlers::keys::handle_list_machine_keys(self, &msg),
            identity_machine::MSG_REVOKE_MACHINE_KEY => handlers::keys::handle_revoke_machine_key(self, &msg),
            identity_machine::MSG_ROTATE_MACHINE_KEY => handlers::keys::handle_rotate_machine_key(self, &msg),
            identity_machine::MSG_GET_MACHINE_KEY => handlers::keys::handle_get_machine_key(self, &msg),
            identity_cred::MSG_ATTACH_EMAIL => handlers::credentials::handle_attach_email(self, &msg),
            identity_cred::MSG_GET_CREDENTIALS => handlers::credentials::handle_get_credentials(self, &msg),
            identity_cred::MSG_UNLINK_CREDENTIAL => handlers::credentials::handle_unlink_credential(self, &msg),
            identity_zid::MSG_ZID_LOGIN => handlers::session::handle_zid_login(self, &msg),
            identity_zid::MSG_ZID_ENROLL_MACHINE => handlers::session::handle_zid_enroll_machine(self, &msg),
            MSG_STORAGE_RESULT => self.handle_storage_result(&msg),
            net::MSG_NET_RESULT => self.handle_net_result(&msg),
            _ => {
                syscall::debug(&alloc::format!("IdentityService: Unknown message tag 0x{:x}", msg.tag));
                Ok(())
            }
        }
    }

    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("IdentityService: shutdown");
    }
}

app_main!(IdentityService);

// Provide a main function for non-WASM targets (used for cargo check)
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    // This is never called - binaries run as WASM
    panic!("This binary is designed for WASM only");
}
