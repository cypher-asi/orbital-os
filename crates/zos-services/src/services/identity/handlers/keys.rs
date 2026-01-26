//! Neural key and machine key operations
//!
//! Handlers for:
//! - Neural key generation and recovery
//! - Machine key CRUD operations (create, list, get, revoke, rotate)
//!
//! # Invariant 32 Compliance
//!
//! All `/keys/` paths use Keystore IPC (via KeystoreService PID 7), NOT VFS.
//! Directory operations (e.g., `/home/{user_id}/.zos/identity/`) still use VFS.
//!
//! # Safety Invariants (per zos-service.md Rule 0)
//!
//! ## Success Conditions
//! - Neural key generation: Key generated, split into shards, stored to Keystore, response sent
//! - Machine key creation: Neural Key verified against stored identity, keypair derived, stored
//! - Key rotation: Existing key read, new keys generated, stored atomically
//!
//! ## Acceptable Partial Failure
//! - Orphan content if write fails (cleanup handled)
//!
//! ## Forbidden States
//! - Returning shards before key is persisted
//! - Creating machine key without verifying Neural Key ownership
//! - Silent fallthrough on parse errors (must return InvalidRequest)
//! - Processing requests without authorization check

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use super::super::utils::bytes_to_hex;
use super::super::pending::{PendingKeystoreOp, PendingStorageOp, RequestContext};
use super::super::response;
use super::super::{check_user_authorization, log_denial, AuthResult, IdentityService};
use zos_identity::crypto::{
    combine_shards_verified, derive_identity_signing_keypair, split_neural_key, 
    KeyScheme as ZidKeyScheme, MachineKeyPair, NeuralKey, ZidMachineKeyCapabilities,
    ZidNeuralShard,
};
use uuid::Uuid;
use zos_apps::syscall;
use zos_apps::{AppError, Message};
use zos_identity::ipc::{
    CreateMachineKeyRequest, GenerateNeuralKeyRequest, GetIdentityKeyRequest, GetMachineKeyRequest,
    ListMachineKeysRequest, NeuralKeyGenerated, NeuralShard, PublicIdentifiers, RecoverNeuralKeyRequest,
    RevokeMachineKeyRequest, RotateMachineKeyRequest,
};
use zos_identity::keystore::{KeyScheme, LocalKeyStore, MachineKeyRecord};
use zos_identity::KeyError;

// =============================================================================
// Neural Key Operations
// =============================================================================

/// Continue neural key generation after checking if identity directory exists
pub fn continue_generate_after_directory_check(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    exists: bool,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    if exists {
        // Directory exists, proceed to check if key already exists (via Keystore)
        syscall::debug(&format!(
            "IdentityService: Identity directory exists for user {:032x}",
            user_id
        ));
        let key_path = LocalKeyStore::storage_path(user_id);
        // Invariant 32: /keys/ paths use Keystore IPC, not VFS
        return service.start_keystore_exists(
            &key_path,
            PendingKeystoreOp::CheckKeyExists { ctx, user_id },
        );
    }

    // Directory doesn't exist, create it using VFS mkdir with create_parents
    syscall::debug(&format!(
        "IdentityService: Creating identity directory structure for user {:032x}",
        user_id
    ));

    // Use VFS mkdir_all to create the entire path at once
    let machine_dir = format!("/home/{}/.zos/identity/machine", user_id);
    service.start_vfs_mkdir(
        &machine_dir,
        true, // create_parents = true
        PendingStorageOp::CreateIdentityDirectory {
            ctx,
            user_id,
            directories: vec![], // No remaining dirs - mkdir_all creates them all
        },
    )
}

/// Continue creating directories after VFS mkdir completes.
/// With VFS mkdir_all (create_parents=true), this is called once all directories are created.
pub fn continue_create_directories(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    _directories: Vec<String>,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    // directories is now empty after mkdir_all succeeds
    // Check if key already exists (via Keystore)
    let key_path = LocalKeyStore::storage_path(user_id);
    syscall::debug(&format!(
        "IdentityService: Directories created, checking if key exists at {}",
        key_path
    ));
    let ctx = RequestContext::new(client_pid, cap_slots);
    // Invariant 32: /keys/ paths use Keystore IPC, not VFS
    service.start_keystore_exists(
        &key_path,
        PendingKeystoreOp::CheckKeyExists { ctx, user_id },
    )
}

pub fn handle_generate_neural_key(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    syscall::debug("IdentityService: Handling generate neural key request");

    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: GenerateNeuralKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_neural_key_error(
                msg.from_pid,
                &msg.cap_slots,
                KeyError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("generate_neural_key", msg.from_pid, request.user_id);
        return response::send_neural_key_error(
            msg.from_pid,
            &msg.cap_slots,
            KeyError::Unauthorized,
        );
    }

    let user_id = request.user_id;
    syscall::debug(&format!(
        "IdentityService: Generating Neural Key for user {:032x}",
        user_id
    ));

    // First, ensure the identity directory structure exists (via VFS)
    let identity_dir = format!("/home/{}/.zos/identity", user_id);
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    service.start_vfs_exists(
        &identity_dir,
        PendingStorageOp::CheckIdentityDirectory { ctx, user_id },
    )
}

pub fn continue_generate_after_exists_check(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    exists: bool,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    if exists {
        syscall::debug("IdentityService: Neural Key already exists");
        return response::send_neural_key_error(
            ctx.client_pid,
            &ctx.cap_slots,
            KeyError::IdentityKeyAlreadyExists,
        );
    }

    // Generate a proper Neural Key using getrandom
    syscall::debug("IdentityService: Calling NeuralKey::generate() - uses getrandom for entropy");
    let neural_key = match NeuralKey::generate() {
        Ok(key) => {
            // Rule 10: NEVER log key material. Only verify entropy quality.
            let bytes = key.as_bytes();
            let all_zeros = bytes.iter().all(|&b| b == 0);
            if all_zeros {
                syscall::debug("IdentityService: WARNING - NeuralKey::generate() returned all zeros! Entropy source may be broken");
            } else {
                syscall::debug("IdentityService: NeuralKey::generate() success - entropy validated");
            }
            key
        }
        Err(e) => {
            // Log detailed error for debugging getrandom failures
            syscall::debug(&format!(
                "IdentityService: CRITICAL - NeuralKey::generate() FAILED! Error: {:?}",
                e
            ));
            syscall::debug("IdentityService: This usually means getrandom could not access crypto.getRandomValues");
            syscall::debug("IdentityService: Check browser console for wasm-bindgen import shim errors");
            return response::send_neural_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::CryptoError(format!("Neural Key generation failed: {:?}", e)),
            )
        }
    };

    // Derive identity signing keypair (canonical way)
    // We use a temporary identity_id for initial key derivation
    // This will be replaced when user enrolls with ZID server
    let temp_identity_id = Uuid::from_u128(user_id);
    // The _identity_keypair is intentionally unused here - we only need the public key
    // for storage in LocalKeyStore. The full keypair would only be needed for signing
    // operations, which are performed elsewhere using the Neural Key shards to
    // reconstruct the keypair on-demand (avoiding persistent private key storage).
    let (identity_signing, _identity_keypair) =
        match derive_identity_signing_keypair(&neural_key, &temp_identity_id) {
            Ok(keypair) => keypair,
            Err(e) => {
                return response::send_neural_key_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    KeyError::CryptoError(format!(
                        "Identity key derivation failed during generation: {:?}",
                        e
                    )),
                )
            }
        };

    // Machine signing and encryption keys are NOT stored in LocalKeyStore.
    // They are derived on-demand via CreateMachineKey using Neural Key shards.
    // Placeholder zeros are stored here because LocalKeyStore was originally designed
    // to hold machine keys, but the current architecture derives them separately
    // for each machine via MachineKeyRecord. See CreateMachineKey for actual derivation.
    let machine_signing = [0u8; 32];
    let machine_encryption = [0u8; 32];

    // Split Neural Key into 5 shards (3-of-5 threshold)
    let zid_shards = match split_neural_key(&neural_key) {
        Ok(shards) => shards,
        Err(e) => {
            return response::send_neural_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::CryptoError(format!("Shamir split failed: {:?}", e)),
            )
        }
    };

    // Convert zid-crypto NeuralShard to our IPC NeuralShard format
    let shards: Vec<NeuralShard> = zid_shards
        .iter()
        .enumerate()
        .map(|(i, shard)| NeuralShard {
            index: (i + 1) as u8, // 1-indexed
            hex: shard.to_hex(),
        })
        .collect();

    let public_identifiers = PublicIdentifiers {
        identity_signing_pub_key: format!("0x{}", bytes_to_hex(&identity_signing)),
        machine_signing_pub_key: format!("0x{}", bytes_to_hex(&machine_signing)),
        machine_encryption_pub_key: format!("0x{}", bytes_to_hex(&machine_encryption)),
    };

    let created_at = syscall::get_wallclock();
    let key_store = LocalKeyStore::new(
        user_id,
        identity_signing,
        machine_signing,
        machine_encryption,
        created_at,
    );
    let result = NeuralKeyGenerated {
        public_identifiers,
        shards,
        created_at,
    };

    let key_path = LocalKeyStore::storage_path(user_id);
    match serde_json::to_vec(&key_store) {
        // Invariant 32: /keys/ paths use Keystore IPC, not VFS
        Ok(json_bytes) => service.start_keystore_write(
            &key_path,
            &json_bytes,
            PendingKeystoreOp::WriteKeyStore {
                ctx,
                user_id,
                result,
                json_bytes: json_bytes.clone(),
            },
        ),
        Err(e) => response::send_neural_key_error(
            ctx.client_pid,
            &ctx.cap_slots,
            KeyError::StorageError(format!("Serialization failed: {}", e)),
        ),
    }
}

pub fn handle_recover_neural_key(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    syscall::debug("IdentityService: Handling recover neural key request");

    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: RecoverNeuralKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_recover_key_error(
                msg.from_pid,
                &msg.cap_slots,
                KeyError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("recover_neural_key", msg.from_pid, request.user_id);
        return response::send_recover_key_error(
            msg.from_pid,
            &msg.cap_slots,
            KeyError::Unauthorized,
        );
    }

    if request.shards.len() < 3 {
        return response::send_recover_key_error(
            msg.from_pid,
            &msg.cap_slots,
            KeyError::InsufficientShards,
        );
    }

    // Convert IPC shards to zid-crypto format
    let zid_shards: Result<Vec<ZidNeuralShard>, _> = request
        .shards
        .iter()
        .map(|s| ZidNeuralShard::from_hex(&s.hex))
        .collect();

    let zid_shards = match zid_shards {
        Ok(shards) => shards,
        Err(e) => {
            return response::send_recover_key_error(
                msg.from_pid,
                &msg.cap_slots,
                KeyError::InvalidShard(format!("Invalid shard format: {:?}", e)),
            )
        }
    };

    // SECURITY: Read the existing LocalKeyStore to get the stored identity public key
    // for verification. This prevents attacks where arbitrary shards could be used
    // to reconstruct an unauthorized identity.
    let key_path = LocalKeyStore::storage_path(request.user_id);
    syscall::debug(&format!(
        "IdentityService: RecoverNeuralKey - reading existing identity from: {}",
        key_path
    ));
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    // Invariant 32: /keys/ paths use Keystore IPC, not VFS
    service.start_keystore_read(
        &key_path,
        PendingKeystoreOp::ReadIdentityForRecovery {
            ctx,
            user_id: request.user_id,
            zid_shards,
        },
    )
}

/// Continue neural key recovery after reading the existing identity for verification.
///
/// SECURITY: This function uses `combine_shards_verified()` to ensure the reconstructed
/// Neural Key matches the stored identity public key. This prevents attacks where
/// arbitrary shards could be used to derive unauthorized machine keys.
pub fn continue_recover_after_identity_read(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    zid_shards: Vec<ZidNeuralShard>,
    stored_identity_pubkey: [u8; 32],
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    // SECURITY: Reconstruct Neural Key from shards WITH VERIFICATION against stored identity.
    // This ensures the provided shards actually belong to this user's Neural Key.
    let neural_key = match combine_shards_verified(&zid_shards, user_id, &stored_identity_pubkey) {
        Ok(key) => key,
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Neural Key recovery verification failed: {:?}",
                e
            ));
            return response::send_recover_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                e,
            );
        }
    };

    syscall::debug("IdentityService: Neural Key recovered and verified against stored identity");

    // Derive keys using proper zid-crypto functions
    let temp_identity_id = Uuid::from_u128(user_id);
    // The _identity_keypair is intentionally unused here - we only need the public key
    // for verification and storage. The full keypair would only be needed for signing
    // operations, which are performed elsewhere using the shards.
    let (identity_signing, _identity_keypair) =
        match derive_identity_signing_keypair(&neural_key, &temp_identity_id) {
            Ok(keypair) => keypair,
            Err(e) => {
                return response::send_recover_key_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    KeyError::CryptoError(format!(
                        "Identity key derivation failed during recovery: {:?}",
                        e
                    )),
                )
            }
        };
    // Machine signing/encryption are placeholders - actual machine keys are created
    // separately via CreateMachineKey which derives them from the Neural Key
    let machine_signing = [0u8; 32];
    let machine_encryption = [0u8; 32];

    // Split the recovered neural key into new shards for backup
    let new_zid_shards = match split_neural_key(&neural_key) {
        Ok(shards) => shards,
        Err(e) => {
            return response::send_recover_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::CryptoError(format!("Shamir split failed: {:?}", e)),
            )
        }
    };

    let new_shards: Vec<NeuralShard> = new_zid_shards
        .iter()
        .enumerate()
        .map(|(i, shard)| NeuralShard {
            index: (i + 1) as u8,
            hex: shard.to_hex(),
        })
        .collect();

    let public_identifiers = PublicIdentifiers {
        identity_signing_pub_key: format!("0x{}", bytes_to_hex(&identity_signing)),
        machine_signing_pub_key: format!("0x{}", bytes_to_hex(&machine_signing)),
        machine_encryption_pub_key: format!("0x{}", bytes_to_hex(&machine_encryption)),
    };

    let created_at = syscall::get_wallclock();
    let key_store = LocalKeyStore::new(
        user_id,
        identity_signing,
        machine_signing,
        machine_encryption,
        created_at,
    );
    let result = NeuralKeyGenerated {
        public_identifiers,
        shards: new_shards,
        created_at,
    };

    let key_path = LocalKeyStore::storage_path(user_id);
    match serde_json::to_vec(&key_store) {
        // Invariant 32: /keys/ paths use Keystore IPC, not VFS
        Ok(json_bytes) => service.start_keystore_write(
            &key_path,
            &json_bytes,
            PendingKeystoreOp::WriteRecoveredKeyStore {
                ctx,
                user_id,
                result,
                json_bytes: json_bytes.clone(),
            },
        ),
        Err(e) => response::send_recover_key_error(
            ctx.client_pid,
            &ctx.cap_slots,
            KeyError::StorageError(format!("Serialization failed: {}", e)),
        ),
    }
}

pub fn handle_get_identity_key(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: GetIdentityKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_get_identity_key_error(
                msg.from_pid,
                &msg.cap_slots,
                KeyError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("get_identity_key", msg.from_pid, request.user_id);
        return response::send_get_identity_key_error(
            msg.from_pid,
            &msg.cap_slots,
            KeyError::Unauthorized,
        );
    }

    let key_path = LocalKeyStore::storage_path(request.user_id);
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    // Invariant 32: /keys/ paths use Keystore IPC, not VFS
    service.start_keystore_read(
        &key_path,
        PendingKeystoreOp::GetIdentityKey { ctx },
    )
}

// =============================================================================
// Machine Key Operations
// =============================================================================

pub fn handle_create_machine_key(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: CreateMachineKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_create_machine_key_error(
                msg.from_pid,
                &msg.cap_slots,
                KeyError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("create_machine_key", msg.from_pid, request.user_id);
        return response::send_create_machine_key_error(
            msg.from_pid,
            &msg.cap_slots,
            KeyError::Unauthorized,
        );
    }

    // Read the LocalKeyStore to get the stored identity public key for verification
    let key_path = LocalKeyStore::storage_path(request.user_id);
    syscall::debug(&format!(
        "IdentityService: CreateMachineKey - reading identity from: {}",
        key_path
    ));
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    // Invariant 32: /keys/ paths use Keystore IPC, not VFS
    service.start_keystore_read(
        &key_path,
        PendingKeystoreOp::ReadIdentityForMachine { ctx, request },
    )
}

pub fn continue_create_machine_after_identity_read(
    service: &mut IdentityService,
    client_pid: u32,
    request: CreateMachineKeyRequest,
    stored_identity_pubkey: [u8; 32],
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    // Convert IPC shards to zid-crypto format
    let zid_shards: Result<Vec<ZidNeuralShard>, _> = request
        .shards
        .iter()
        .map(|s| ZidNeuralShard::from_hex(&s.hex))
        .collect();

    let zid_shards = match zid_shards {
        Ok(shards) => shards,
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Invalid shard format: {:?}",
                e
            ));
            return response::send_create_machine_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::InvalidShard(format!("Invalid shard format: {:?}", e)),
            );
        }
    };

    // Reconstruct Neural Key from shards WITH VERIFICATION against stored identity
    // This ensures the provided shards actually belong to this user's Neural Key
    let neural_key = match combine_shards_verified(&zid_shards, request.user_id, &stored_identity_pubkey) {
        Ok(key) => key,
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Neural Key verification failed: {:?}",
                e
            ));
            return response::send_create_machine_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                e,
            );
        }
    };

    syscall::debug("IdentityService: Neural Key reconstructed and verified against stored identity");

    // Generate machine ID using entropy
    syscall::debug("IdentityService: Generating machine ID via NeuralKey::generate()");
    let machine_id_bytes = match NeuralKey::generate() {
        Ok(key) => {
            let bytes = key.as_bytes();
            let all_zeros = bytes[..16].iter().all(|&b| b == 0);
            if all_zeros {
                syscall::debug("IdentityService: WARNING - machine ID entropy returned all zeros!");
            }
            [
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ]
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: CRITICAL - Machine ID generation FAILED! Error: {:?}",
                e
            ));
            return response::send_create_machine_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::CryptoError("Failed to generate machine ID".into()),
            )
        }
    };
    let machine_id = u128::from_le_bytes(machine_id_bytes);

    // Create UUIDs for derivation
    let identity_id = Uuid::from_u128(request.user_id);
    let machine_uuid = Uuid::from_u128(machine_id);

    // Convert capabilities to zid-crypto format
    let zid_capabilities = ZidMachineKeyCapabilities::FULL_DEVICE;

    // Convert key scheme
    let zid_scheme = match request.key_scheme {
        KeyScheme::Classical => ZidKeyScheme::Classical,
        KeyScheme::PqHybrid => ZidKeyScheme::PqHybrid,
    };

    // Derive machine keypair from Neural Key using zid-crypto
    let machine_keypair = match zos_identity::crypto::derive_machine_keypair_with_scheme(
        &neural_key,
        &identity_id,
        &machine_uuid,
        1, // epoch
        zid_capabilities,
        zid_scheme,
    ) {
        Ok(keypair) => keypair,
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Machine keypair derivation failed: {:?}",
                e
            ));
            return response::send_create_machine_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::CryptoError(format!("Machine keypair derivation failed: {:?}", e)),
            );
        }
    };

    syscall::debug(&format!(
        "IdentityService: Derived machine key {:032x} from Neural Key",
        machine_id
    ));

    // Extract public keys
    let signing_key = machine_keypair.signing_public_key();
    let encryption_key = machine_keypair.encryption_public_key();
    let now = syscall::get_wallclock();

    // Get PQ keys if available
    let (pq_signing_public_key, pq_encryption_public_key) = 
        if request.key_scheme == KeyScheme::PqHybrid {
            // For now, PQ keys are not available in WASM
            // This would be populated when full PQ support is added
            syscall::debug(&format!(
                "IdentityService: PQ-Hybrid requested for machine {:032x}, but not yet supported in WASM",
                machine_id
            ));
            (None, None)
        } else {
            (None, None)
        };

    let record = MachineKeyRecord {
        machine_id,
        signing_public_key: signing_key,
        encryption_public_key: encryption_key,
        signing_sk: None, // Seeds not stored - derived from Neural Key
        encryption_sk: None,
        authorized_at: now,
        authorized_by: request.user_id,
        capabilities: request.capabilities,
        machine_name: request.machine_name,
        last_seen_at: now,
        epoch: 1,
        key_scheme: request.key_scheme,
        pq_signing_public_key,
        pq_encryption_public_key,
    };

    let machine_path = MachineKeyRecord::storage_path(request.user_id, machine_id);
    match serde_json::to_vec(&record) {
        // Invariant 32: /keys/ paths use Keystore IPC, not VFS
        Ok(json_bytes) => service.start_keystore_write(
            &machine_path,
            &json_bytes,
            PendingKeystoreOp::WriteMachineKey {
                ctx,
                user_id: request.user_id,
                record,
                json_bytes: json_bytes.clone(),
            },
        ),
        Err(e) => response::send_create_machine_key_error(
            ctx.client_pid,
            &ctx.cap_slots,
            KeyError::StorageError(format!("Serialization failed: {}", e)),
        ),
    }
}

pub fn handle_list_machine_keys(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    // Rule 1: Parse request - return InvalidRequest on parse failure (NOT empty list)
    let request: ListMachineKeysRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_list_machine_keys_error(
                msg.from_pid,
                &msg.cap_slots,
                KeyError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("list_machine_keys", msg.from_pid, request.user_id);
        return response::send_list_machine_keys_error(
            msg.from_pid,
            &msg.cap_slots,
            KeyError::Unauthorized,
        );
    }

    // Invariant 32: /keys/ paths use Keystore IPC, not VFS
    // Use keystore list with prefix to find all machine keys
    let machine_prefix = format!("/keys/{}/identity/machine/", request.user_id);
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    service.start_keystore_list(
        &machine_prefix,
        PendingKeystoreOp::ListMachineKeys { ctx, user_id: request.user_id },
    )
}

pub fn handle_revoke_machine_key(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: RevokeMachineKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_revoke_machine_key_error(
                msg.from_pid,
                &msg.cap_slots,
                KeyError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("revoke_machine_key", msg.from_pid, request.user_id);
        return response::send_revoke_machine_key_error(
            msg.from_pid,
            &msg.cap_slots,
            KeyError::Unauthorized,
        );
    }

    let machine_path = MachineKeyRecord::storage_path(request.user_id, request.machine_id);
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    // Invariant 32: /keys/ paths use Keystore IPC, not VFS
    service.start_keystore_delete(
        &machine_path,
        PendingKeystoreOp::DeleteMachineKey {
            ctx,
            user_id: request.user_id,
            machine_id: request.machine_id,
        },
    )
}

pub fn handle_rotate_machine_key(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: RotateMachineKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_rotate_machine_key_error(
                msg.from_pid,
                &msg.cap_slots,
                KeyError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("rotate_machine_key", msg.from_pid, request.user_id);
        return response::send_rotate_machine_key_error(
            msg.from_pid,
            &msg.cap_slots,
            KeyError::Unauthorized,
        );
    }

    let machine_path = MachineKeyRecord::storage_path(request.user_id, request.machine_id);
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    // Invariant 32: /keys/ paths use Keystore IPC, not VFS
    service.start_keystore_read(
        &machine_path,
        PendingKeystoreOp::ReadMachineForRotate {
            ctx,
            user_id: request.user_id,
            machine_id: request.machine_id,
        },
    )
}

pub fn continue_rotate_after_read(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    machine_id: u128,
    data: &[u8],
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);
    
    let mut record: MachineKeyRecord = match serde_json::from_slice(data) {
        Ok(r) => r,
        Err(e) => {
            return response::send_rotate_machine_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::StorageError(format!("Parse failed: {}", e)),
            )
        }
    };

    // Generate new secure random seeds for key rotation
    syscall::debug("IdentityService: Generating signing seed for key rotation");
    let signing_sk = match NeuralKey::generate() {
        Ok(key) => {
            let bytes = *key.as_bytes();
            let all_zeros = bytes.iter().all(|&b| b == 0);
            if all_zeros {
                syscall::debug("IdentityService: WARNING - signing seed returned all zeros!");
            }
            bytes
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: CRITICAL - Signing seed generation FAILED! Error: {:?}",
                e
            ));
            return response::send_rotate_machine_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::CryptoError("Failed to generate signing seed".into()),
            )
        }
    };

    syscall::debug("IdentityService: Generating encryption seed for key rotation");
    let encryption_sk = match NeuralKey::generate() {
        Ok(key) => {
            let bytes = *key.as_bytes();
            let all_zeros = bytes.iter().all(|&b| b == 0);
            if all_zeros {
                syscall::debug("IdentityService: WARNING - encryption seed returned all zeros!");
            }
            bytes
        }
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: CRITICAL - Encryption seed generation FAILED! Error: {:?}",
                e
            ));
            return response::send_rotate_machine_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::CryptoError("Failed to generate encryption seed".into()),
            )
        }
    };

    // Convert capabilities and key scheme to zid-crypto format
    let zid_capabilities = ZidMachineKeyCapabilities::FULL_DEVICE;
    let zid_scheme = match record.key_scheme {
        KeyScheme::Classical => ZidKeyScheme::Classical,
        KeyScheme::PqHybrid => ZidKeyScheme::PqHybrid,
    };

    // Create new machine keypair using zid-crypto
    let machine_keypair = match MachineKeyPair::from_seeds_with_scheme(
        &signing_sk,
        &encryption_sk,
        None, // No PQ seeds for now (WASM limitation)
        None, // No PQ seeds for now
        zid_capabilities,
        zid_scheme,
    ) {
        Ok(keypair) => keypair,
        Err(e) => {
            return response::send_rotate_machine_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::CryptoError(format!("Machine keypair rotation failed: {:?}", e)),
            )
        }
    };

    // Update record with new keys
    record.signing_public_key = machine_keypair.signing_public_key();
    record.encryption_public_key = machine_keypair.encryption_public_key();
    record.epoch += 1;
    record.last_seen_at = syscall::get_wallclock();

    // Clear PQ keys if in PQ mode (not supported in WASM yet)
    if record.key_scheme == KeyScheme::PqHybrid {
        record.pq_signing_public_key = None;
        record.pq_encryption_public_key = None;

        syscall::debug(&format!(
            "IdentityService: Rotated keys for machine {:032x} (epoch {}), PQ mode not yet supported",
            machine_id, record.epoch
        ));
    }

    let machine_path = MachineKeyRecord::storage_path(user_id, machine_id);
    match serde_json::to_vec(&record) {
        // Invariant 32: /keys/ paths use Keystore IPC, not VFS
        Ok(json_bytes) => service.start_keystore_write(
            &machine_path,
            &json_bytes,
            PendingKeystoreOp::WriteRotatedMachineKey {
                ctx,
                user_id,
                record,
                json_bytes: json_bytes.clone(),
            },
        ),
        Err(e) => response::send_rotate_machine_key_error(
            ctx.client_pid,
            &ctx.cap_slots,
            KeyError::StorageError(format!("Serialization failed: {}", e)),
        ),
    }
}

pub fn handle_get_machine_key(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: GetMachineKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_get_machine_key_error(
                msg.from_pid,
                &msg.cap_slots,
                KeyError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("get_machine_key", msg.from_pid, request.user_id);
        return response::send_get_machine_key_error(
            msg.from_pid,
            &msg.cap_slots,
            KeyError::Unauthorized,
        );
    }

    let machine_path = MachineKeyRecord::storage_path(request.user_id, request.machine_id);
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    // Invariant 32: /keys/ paths use Keystore IPC, not VFS
    service.start_keystore_read(
        &machine_path,
        PendingKeystoreOp::ReadSingleMachineKey { ctx },
    )
}
