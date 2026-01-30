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
use alloc::vec::Vec;

use super::super::utils::bytes_to_hex;
use super::super::pending::{PendingKeystoreOp, PendingStorageOp, RequestContext};
use super::super::response;
use super::super::{check_user_authorization, log_denial, AuthResult, IdentityService};
use sha2::{Sha256, Digest};
use zos_identity::types::UserId;
use zos_identity::crypto::{
    combine_shards_verified, create_kdf_params, decrypt_shard, derive_identity_signing_keypair,
    derive_machine_encryption_seed, derive_machine_seed, derive_machine_signing_seed,
    encrypt_shard, select_shards_to_encrypt, split_neural_key, validate_password,
    KeyScheme as ZidKeyScheme, MachineKeyPair, NeuralKey, ZidMachineKeyCapabilities,
    ZidNeuralShard,
};
use uuid::Uuid;
use zos_apps::syscall;
use zos_apps::{AppError, Message};
use zos_identity::ipc::{
    CreateMachineKeyAndEnrollRequest, CreateMachineKeyRequest, GenerateNeuralKeyRequest,
    GetIdentityKeyRequest, GetMachineKeyRequest, ListMachineKeysRequest, NeuralKeyGenerated,
    NeuralShard, PublicIdentifiers, RecoverNeuralKeyRequest, RevokeMachineKeyRequest,
    RotateMachineKeyRequest,
};
use zos_identity::keystore::{EncryptedShardStore, KeyScheme, LocalKeyStore, MachineKeyRecord};
use zos_identity::KeyError;

// =============================================================================
// User ID Derivation
// =============================================================================

/// Derive a user ID from the identity signing public key.
///
/// Takes the first 128 bits (16 bytes) of SHA-256 hash of the public key.
/// This creates a deterministic, unique user ID from the cryptographic identity.
fn derive_user_id_from_pubkey(identity_signing_public_key: &[u8; 32]) -> UserId {
    let mut hasher = Sha256::new();
    hasher.update(identity_signing_public_key);
    let hash = hasher.finalize();
    
    // Take first 16 bytes (128 bits) as user ID
    let mut id_bytes = [0u8; 16];
    id_bytes.copy_from_slice(&hash[..16]);
    u128::from_be_bytes(id_bytes)
}

// =============================================================================
// Neural Key Operations
// =============================================================================

/// Continue neural key generation after checking if identity directory exists
pub fn continue_generate_after_directory_check(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    exists: bool,
    password: String,
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
            PendingKeystoreOp::CheckKeyExists { ctx, user_id, password },
        );
    }

    // Directory doesn't exist, create it with create_parents=true
    // This creates the entire directory structure in a single VFS operation
    syscall::debug(&format!(
        "IdentityService: Creating identity directory structure for user {}",
        user_id
    ));

    // Create the deepest directory path - VFS will create all parents
    let identity_dir = format!("/home/{}/.zos/identity", user_id);

    service.start_vfs_mkdir(
        &identity_dir,
        true, // create_parents = true - creates all parent directories
        PendingStorageOp::CreateIdentityDirectoryComplete {
            ctx,
            user_id,
            password,
        },
    )
}

/// Continue creating directories after VFS mkdir completes.
/// Creates directories one at a time since VFS does not support create_parents.
pub fn continue_create_directories(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    directories: Vec<String>,
    password: String,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);

    if directories.is_empty() {
        // All directories created, proceed to check if key already exists (via Keystore)
        let key_path = LocalKeyStore::storage_path(user_id);
        syscall::debug(&format!(
            "IdentityService: Directories created, checking if key exists at {}",
            key_path
        ));
        // Invariant 32: /keys/ paths use Keystore IPC, not VFS
        return service.start_keystore_exists(
            &key_path,
            PendingKeystoreOp::CheckKeyExists { ctx, user_id, password },
        );
    }

    // Create the next directory in the list
    let next_dir = directories[0].clone();
    let remaining_dirs: Vec<String> = directories[1..].to_vec();

    syscall::debug(&format!(
        "IdentityService: Creating directory {} ({} remaining)",
        next_dir,
        remaining_dirs.len()
    ));

    service.start_vfs_mkdir(
        &next_dir,
        false, // create_parents = false (not supported by VFS)
        PendingStorageOp::CreateIdentityDirectory {
            ctx,
            user_id,
            directories: remaining_dirs,
            password,
        },
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

    // Validate password before proceeding
    if let Err(e) = validate_password(&request.password) {
        syscall::debug(&format!("IdentityService: Password validation failed: {:?}", e));
        return response::send_neural_key_error(msg.from_pid, &msg.cap_slots, e);
    }

    let user_id = request.user_id;
    let password = request.password;
    syscall::debug(&format!(
        "IdentityService: Generating Neural Key for user {:032x}",
        user_id
    ));

    // First, ensure the identity directory structure exists (via VFS)
    let identity_dir = format!("/home/{}/.zos/identity", user_id);
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    service.start_vfs_exists(
        &identity_dir,
        PendingStorageOp::CheckIdentityDirectory { ctx, user_id, password },
    )
}

pub fn continue_generate_after_exists_check(
    service: &mut IdentityService,
    client_pid: u32,
    user_id: u128,
    exists: bool,
    password: String,
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
    let temp_identity_id = Uuid::from_u128(user_id);
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

    // Machine signing and encryption keys are placeholders - derived via CreateMachineKey
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

    // Convert zid-crypto NeuralShard to our IPC NeuralShard format (all 5 shards)
    let all_shards: Vec<NeuralShard> = zid_shards
        .iter()
        .enumerate()
        .map(|(i, shard)| NeuralShard {
            index: (i + 1) as u8, // 1-indexed
            hex: shard.to_hex(),
        })
        .collect();

    // Select which 2 shards to encrypt and which 3 to return as external
    let (encrypted_indices, external_indices) = match select_shards_to_encrypt() {
        Ok(indices) => indices,
        Err(e) => {
            return response::send_neural_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                e,
            )
        }
    };

    syscall::debug(&format!(
        "IdentityService: Encrypting shards {:?}, external shards {:?}",
        encrypted_indices, external_indices
    ));

    // Create KDF parameters with random salt
    let kdf = match create_kdf_params() {
        Ok(k) => k,
        Err(e) => {
            return response::send_neural_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                e,
            )
        }
    };

    // Encrypt the selected shards
    let mut encrypted_shards = Vec::new();
    for &idx in &encrypted_indices {
        let shard = &all_shards[(idx - 1) as usize];
        match encrypt_shard(&shard.hex, idx, &password, &kdf) {
            Ok(encrypted) => encrypted_shards.push(encrypted),
            Err(e) => {
                return response::send_neural_key_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    e,
                )
            }
        }
    }

    // Filter to only external shards (the 3 that user will backup)
    let external_shards: Vec<NeuralShard> = external_indices
        .iter()
        .map(|&idx| all_shards[(idx - 1) as usize].clone())
        .collect();

    let created_at = syscall::get_wallclock();

    // Derive the canonical user ID from the identity signing public key
    // This creates a deterministic identity based on the cryptographic key.
    // The storage path uses derived_user_id so frontend can find keys after updating user ID.
    // But the key_store.user_id keeps the ORIGINAL user_id because it was used for key derivation
    // and verification needs to use the same user_id for re-derivation.
    let derived_user_id = derive_user_id_from_pubkey(&identity_signing);
    syscall::debug(&format!(
        "IdentityService: Derived user_id {:032x} from identity signing key (original: {:032x})",
        derived_user_id, user_id
    ));

    // Create encrypted shard store - use ORIGINAL user_id because that's what was used
    // for identity signing key derivation and will be needed for verification
    let encrypted_shard_store = EncryptedShardStore {
        user_id,
        encrypted_shards,
        external_shard_indices: external_indices.clone(),
        kdf,
        created_at,
    };

    let public_identifiers = PublicIdentifiers {
        identity_signing_pub_key: format!("0x{}", bytes_to_hex(&identity_signing)),
        machine_signing_pub_key: format!("0x{}", bytes_to_hex(&machine_signing)),
        machine_encryption_pub_key: format!("0x{}", bytes_to_hex(&machine_encryption)),
    };

    // Create key store - use ORIGINAL user_id because it was used for identity key derivation
    // and verification requires the same user_id to re-derive and verify the pubkey
    let key_store = LocalKeyStore::new(
        user_id,
        identity_signing,
        machine_signing,
        machine_encryption,
        created_at,
    );

    // Result contains only the 3 external shards
    let result = NeuralKeyGenerated {
        user_id: derived_user_id,
        public_identifiers,
        shards: external_shards,
        created_at,
    };

    // Serialize both key store and encrypted shards
    let key_json = match serde_json::to_vec(&key_store) {
        Ok(json) => json,
        Err(e) => {
            return response::send_neural_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::StorageError(format!("Key store serialization failed: {}", e)),
            )
        }
    };

    let encrypted_shards_json = match serde_json::to_vec(&encrypted_shard_store) {
        Ok(json) => json,
        Err(e) => {
            return response::send_neural_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::StorageError(format!("Encrypted shards serialization failed: {}", e)),
            )
        }
    };

    // Store under the DERIVED user_id so subsequent operations can find it
    let key_path = LocalKeyStore::storage_path(derived_user_id);
    // Invariant 32: /keys/ paths use Keystore IPC, not VFS
    service.start_keystore_write(
        &key_path,
        &key_json,
        PendingKeystoreOp::WriteKeyStore {
            ctx,
            user_id: derived_user_id,
            result,
            json_bytes: key_json.clone(),
            encrypted_shards_json,
        },
    )
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

    // Derive the real user ID from the identity signing public key
    let derived_user_id = derive_user_id_from_pubkey(&identity_signing);
    syscall::debug(&format!(
        "IdentityService: Recovered key - derived user_id {:032x}",
        derived_user_id
    ));

    let result = NeuralKeyGenerated {
        user_id: derived_user_id,
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

/// Legacy function - now just a stub that should not be called directly.
/// Machine key creation now goes through continue_create_machine_after_shards_read.
pub fn continue_create_machine_after_identity_read(
    _service: &mut IdentityService,
    client_pid: u32,
    _request: CreateMachineKeyRequest,
    _stored_identity_pubkey: [u8; 32],
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    // This should not be called directly anymore - see keystore_dispatch.rs
    // which now chains to ReadEncryptedShardsForMachine
    response::send_create_machine_key_error(
        client_pid,
        &cap_slots,
        KeyError::StorageError("Internal error: legacy path invoked".into()),
    )
}

/// Continue machine key creation after reading encrypted shards from keystore.
///
/// This function:
/// 1. Decrypts the 2 stored shards using the password
/// 2. Combines with the 1 external shard (total 3)
/// 3. Reconstructs the Neural Key
/// 4. Verifies against stored identity public key
/// 5. Derives machine keypair
pub fn continue_create_machine_after_shards_read(
    service: &mut IdentityService,
    client_pid: u32,
    request: CreateMachineKeyRequest,
    stored_identity_pubkey: [u8; 32],
    encrypted_store: EncryptedShardStore,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);

    // Decrypt the 2 stored shards
    let mut decrypted_shard_hexes = Vec::new();
    for encrypted_shard in &encrypted_store.encrypted_shards {
        match decrypt_shard(encrypted_shard, &request.password, &encrypted_store.kdf) {
            Ok(hex) => decrypted_shard_hexes.push((encrypted_shard.index, hex)),
            Err(e) => {
                syscall::debug(&format!(
                    "IdentityService: Failed to decrypt shard {}: {:?}",
                    encrypted_shard.index, e
                ));
                return response::send_create_machine_key_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    e,
                );
            }
        }
    }

    syscall::debug(&format!(
        "IdentityService: Successfully decrypted {} stored shards",
        decrypted_shard_hexes.len()
    ));

    // Validate external shard index is expected and unique
    if !encrypted_store
        .external_shard_indices
        .contains(&request.external_shard.index)
    {
        return response::send_create_machine_key_error(
            ctx.client_pid,
            &ctx.cap_slots,
            KeyError::InvalidShard("External shard index not recognized".into()),
        );
    }

    let mut shard_indices = Vec::new();
    shard_indices.push(request.external_shard.index);
    shard_indices.extend(encrypted_store.encrypted_shards.iter().map(|s| s.index));

    shard_indices.sort_unstable();
    shard_indices.dedup();
    if shard_indices.len() != 3 {
        return response::send_create_machine_key_error(
            ctx.client_pid,
            &ctx.cap_slots,
            KeyError::InvalidShard("Shard indices must be unique (3 total)".into()),
        );
    }

    // Convert all shards (1 external + 2 decrypted) to zid-crypto format
    let mut all_shards = Vec::new();

    // Add external shard
    match ZidNeuralShard::from_hex(&request.external_shard.hex) {
        Ok(shard) => all_shards.push(shard),
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Invalid external shard format: {:?}",
                e
            ));
            return response::send_create_machine_key_error(
                ctx.client_pid,
                &ctx.cap_slots,
                KeyError::InvalidShard(format!("Invalid external shard format: {:?}", e)),
            );
        }
    }

    // Add decrypted shards
    for (_idx, hex) in decrypted_shard_hexes {
        match ZidNeuralShard::from_hex(&hex) {
            Ok(shard) => all_shards.push(shard),
            Err(e) => {
                syscall::debug(&format!(
                    "IdentityService: Invalid decrypted shard format: {:?}",
                    e
                ));
                return response::send_create_machine_key_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    KeyError::InvalidShard(format!("Invalid decrypted shard format: {:?}", e)),
                );
            }
        }
    }

    syscall::debug(&format!(
        "IdentityService: Total shards for reconstruction: {}",
        all_shards.len()
    ));

    // Reconstruct Neural Key from shards WITH VERIFICATION against stored identity
    let neural_key = match combine_shards_verified(&all_shards, request.user_id, &stored_identity_pubkey) {
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

// =============================================================================
// Combined Machine Key + ZID Enrollment Operations
// =============================================================================

/// Handle combined machine key creation and ZID enrollment.
///
/// This endpoint solves the signature mismatch problem by:
/// 1. Reconstructing the Neural Key from shards + password
/// 2. Deriving the machine keypair canonically
/// 3. Storing the machine key with SK seeds
/// 4. Enrolling with ZID using the SAME derived keypair
///
/// This ensures the keypair used for local storage matches the one registered with ZID.
pub fn handle_create_machine_key_and_enroll(
    service: &mut IdentityService,
    msg: &Message,
) -> Result<(), AppError> {
    syscall::debug("IdentityService: Handling create machine key AND enroll request");

    // Rule 1: Parse request - return InvalidRequest on parse failure
    let request: CreateMachineKeyAndEnrollRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_create_machine_key_and_enroll_error(
                msg.from_pid,
                &msg.cap_slots,
                zos_identity::error::ZidError::InvalidRequest(format!("JSON parse error: {}", e)),
            );
        }
    };

    // Rule 4: Authorization check (FAIL-CLOSED)
    if check_user_authorization(msg.from_pid, request.user_id) == AuthResult::Denied {
        log_denial("create_machine_key_and_enroll", msg.from_pid, request.user_id);
        return response::send_create_machine_key_and_enroll_error(
            msg.from_pid,
            &msg.cap_slots,
            zos_identity::error::ZidError::Unauthorized,
        );
    }

    // Read the LocalKeyStore to get the stored identity public key for verification
    let key_path = LocalKeyStore::storage_path(request.user_id);
    syscall::debug(&format!(
        "IdentityService: CreateMachineKeyAndEnroll - reading identity from: {}",
        key_path
    ));
    let ctx = RequestContext::new(msg.from_pid, msg.cap_slots.clone());
    // Invariant 32: /keys/ paths use Keystore IPC, not VFS
    service.start_keystore_read(
        &key_path,
        PendingKeystoreOp::ReadIdentityForMachineEnroll { ctx, request },
    )
}

/// Continue combined machine key + enroll after reading encrypted shards from keystore.
///
/// This function:
/// 1. Decrypts the 2 stored shards using the password
/// 2. Combines with the 1 external shard (total 3)
/// 3. Reconstructs the Neural Key
/// 4. Verifies against stored identity public key
/// 5. Derives machine keypair (with SK seeds for enrollment signing)
/// 6. Stores machine key, then chains to ZID enrollment
///
/// # Arguments
/// * `derivation_user_id` - The user_id that was used to derive the identity signing keypair.
///   This may differ from `request.user_id` if the user_id was derived from the pubkey.
///   Verification must use this value to re-derive and compare the pubkey.
pub fn continue_create_machine_enroll_after_shards_read(
    service: &mut IdentityService,
    client_pid: u32,
    request: CreateMachineKeyAndEnrollRequest,
    stored_identity_pubkey: [u8; 32],
    derivation_user_id: u128,
    encrypted_store: EncryptedShardStore,
    cap_slots: Vec<u32>,
) -> Result<(), AppError> {
    let ctx = RequestContext::new(client_pid, cap_slots);

    // Decrypt the 2 stored shards
    let mut decrypted_shard_hexes = Vec::new();
    for encrypted_shard in &encrypted_store.encrypted_shards {
        match decrypt_shard(encrypted_shard, &request.password, &encrypted_store.kdf) {
            Ok(hex) => decrypted_shard_hexes.push((encrypted_shard.index, hex)),
            Err(e) => {
                syscall::debug(&format!(
                    "IdentityService: Failed to decrypt shard {}: {:?}",
                    encrypted_shard.index, e
                ));
                return response::send_create_machine_key_and_enroll_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    zos_identity::error::ZidError::AuthenticationFailed,
                );
            }
        }
    }

    syscall::debug(&format!(
        "IdentityService: Successfully decrypted {} stored shards for combined flow",
        decrypted_shard_hexes.len()
    ));

    // Validate external shard index
    if !encrypted_store
        .external_shard_indices
        .contains(&request.external_shard.index)
    {
        return response::send_create_machine_key_and_enroll_error(
            ctx.client_pid,
            &ctx.cap_slots,
            zos_identity::error::ZidError::InvalidRequest("External shard index not recognized".into()),
        );
    }

    // Collect and validate shard indices
    let mut shard_indices = Vec::new();
    shard_indices.push(request.external_shard.index);
    shard_indices.extend(encrypted_store.encrypted_shards.iter().map(|s| s.index));
    shard_indices.sort_unstable();
    shard_indices.dedup();
    if shard_indices.len() != 3 {
        return response::send_create_machine_key_and_enroll_error(
            ctx.client_pid,
            &ctx.cap_slots,
            zos_identity::error::ZidError::InvalidRequest("Shard indices must be unique (3 total)".into()),
        );
    }

    // Convert all shards to zid-crypto format
    let mut all_shards = Vec::new();

    // Add external shard
    match ZidNeuralShard::from_hex(&request.external_shard.hex) {
        Ok(shard) => all_shards.push(shard),
        Err(e) => {
            return response::send_create_machine_key_and_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                zos_identity::error::ZidError::InvalidRequest(format!("Invalid external shard: {:?}", e)),
            );
        }
    }

    // Add decrypted shards
    for (_idx, hex) in decrypted_shard_hexes {
        match ZidNeuralShard::from_hex(&hex) {
            Ok(shard) => all_shards.push(shard),
            Err(e) => {
                return response::send_create_machine_key_and_enroll_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    zos_identity::error::ZidError::InvalidRequest(format!("Invalid decrypted shard: {:?}", e)),
                );
            }
        }
    }

    // Reconstruct Neural Key from shards WITH VERIFICATION against stored identity
    // IMPORTANT: Use derivation_user_id (from key_store.user_id), not request.user_id,
    // because the identity pubkey was derived using derivation_user_id
    let neural_key = match combine_shards_verified(&all_shards, derivation_user_id, &stored_identity_pubkey) {
        Ok(key) => key,
        Err(e) => {
            syscall::debug(&format!(
                "IdentityService: Neural Key verification failed in combined flow: {:?} (derivation_user_id={:032x})",
                e, derivation_user_id
            ));
            return response::send_create_machine_key_and_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                zos_identity::error::ZidError::AuthenticationFailed,
            );
        }
    };

    syscall::debug("IdentityService: Neural Key reconstructed for combined machine key + enroll");

    // Generate machine ID
    let machine_id_bytes = match NeuralKey::generate() {
        Ok(key) => {
            let bytes = key.as_bytes();
            [
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
            ]
        }
        Err(e) => {
            return response::send_create_machine_key_and_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                zos_identity::error::ZidError::NetworkError(format!("Machine ID generation failed: {:?}", e)),
            );
        }
    };
    let machine_id = u128::from_le_bytes(machine_id_bytes);

    // Create UUIDs for derivation
    let identity_id = Uuid::from_u128(request.user_id);
    let machine_uuid = Uuid::from_u128(machine_id);

    // Derive identity signing keypair (needed for ZID enrollment signature)
    let (identity_signing_public_key, identity_keypair) =
        match derive_identity_signing_keypair(&neural_key, &identity_id) {
            Ok(keypair) => keypair,
            Err(e) => {
                return response::send_create_machine_key_and_enroll_error(
                    ctx.client_pid,
                    &ctx.cap_slots,
                    zos_identity::error::ZidError::NetworkError(format!("Identity key derivation failed: {:?}", e)),
                );
            }
        };
    
    // Extract identity signing seed for ZID enrollment authorization signature
    let identity_signing_sk = identity_keypair.seed_bytes();

    // Convert capabilities and key scheme
    let zid_capabilities = ZidMachineKeyCapabilities::FULL_DEVICE;
    let zid_scheme = match request.key_scheme {
        KeyScheme::Classical => ZidKeyScheme::Classical,
        KeyScheme::PqHybrid => ZidKeyScheme::PqHybrid,
    };

    // Derive the seeds first so we can store them
    // Step 1: Derive machine seed from Neural Key
    let machine_seed = match derive_machine_seed(&neural_key, &identity_id, &machine_uuid, 1) {
        Ok(seed) => seed,
        Err(e) => {
            return response::send_create_machine_key_and_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                zos_identity::error::ZidError::NetworkError(format!("Machine seed derivation failed: {:?}", e)),
            );
        }
    };

    // Step 2: Derive signing seed from machine seed
    let machine_signing_sk = match derive_machine_signing_seed(&machine_seed, &machine_uuid) {
        Ok(seed) => *seed,
        Err(e) => {
            return response::send_create_machine_key_and_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                zos_identity::error::ZidError::NetworkError(format!("Signing seed derivation failed: {:?}", e)),
            );
        }
    };

    // Step 3: Derive encryption seed from machine seed
    let machine_encryption_sk = match derive_machine_encryption_seed(&machine_seed, &machine_uuid) {
        Ok(seed) => *seed,
        Err(e) => {
            return response::send_create_machine_key_and_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                zos_identity::error::ZidError::NetworkError(format!("Encryption seed derivation failed: {:?}", e)),
            );
        }
    };

    // Step 4: Create machine keypair from the derived seeds
    let machine_keypair = match MachineKeyPair::from_seeds_with_scheme(
        &machine_signing_sk,
        &machine_encryption_sk,
        None, // No PQ signing seed in WASM
        None, // No PQ encryption seed in WASM
        zid_capabilities,
        zid_scheme,
    ) {
        Ok(keypair) => keypair,
        Err(e) => {
            return response::send_create_machine_key_and_enroll_error(
                ctx.client_pid,
                &ctx.cap_slots,
                zos_identity::error::ZidError::NetworkError(format!("Machine keypair creation failed: {:?}", e)),
            );
        }
    };

    syscall::debug(&format!(
        "IdentityService: Derived machine key {:032x} for combined flow",
        machine_id
    ));

    // Extract public keys
    let signing_key = machine_keypair.signing_public_key();
    let encryption_key = machine_keypair.encryption_public_key();
    let now = syscall::get_wallclock();

    // Create machine key record WITH SK seeds (needed for ZID enrollment signing)
    let record = MachineKeyRecord {
        machine_id,
        signing_public_key: signing_key,
        encryption_public_key: encryption_key,
        signing_sk: Some(machine_signing_sk),
        encryption_sk: Some(machine_encryption_sk),
        authorized_at: now,
        authorized_by: request.user_id,
        capabilities: request.capabilities,
        machine_name: request.machine_name,
        last_seen_at: now,
        epoch: 1,
        key_scheme: request.key_scheme,
        pq_signing_public_key: None,
        pq_encryption_public_key: None,
    };

    // Store machine key first, then chain to ZID enrollment
    let machine_path = MachineKeyRecord::storage_path(request.user_id, machine_id);
    match serde_json::to_vec(&record) {
        Ok(json_bytes) => service.start_keystore_write(
            &machine_path,
            &json_bytes,
            PendingKeystoreOp::WriteMachineKeyForEnroll {
                ctx,
                user_id: request.user_id,
                record,
                json_bytes: json_bytes.clone(),
                zid_endpoint: request.zid_endpoint,
                identity_signing_public_key,
                identity_signing_sk,
                machine_signing_sk,
                machine_encryption_sk,
            },
        ),
        Err(e) => response::send_create_machine_key_and_enroll_error(
            ctx.client_pid,
            &ctx.cap_slots,
            zos_identity::error::ZidError::NetworkError(format!("Serialization failed: {}", e)),
        ),
    }
}
