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
//! # Storage Access
//!
//! This service uses async storage syscalls (routed through supervisor to IndexedDB)
//! instead of blocking VfsClient to avoid IPC deadlock. The pattern follows vfs_service.rs:
//!
//! ```text
//! Client Process (e.g. React)
//!        │
//!        │ IPC (MSG_GENERATE_NEURAL_KEY)
//!        ▼
//! ┌─────────────────┐
//! │ IdentityService │  ◄── This service
//! └────────┬────────┘
//!          │
//!          │ SYS_STORAGE_WRITE syscall (returns request_id immediately)
//!          ▼
//! ┌─────────────────┐
//! │  Kernel/Axiom   │
//! └────────┬────────┘
//!          │
//!          │ HAL async storage
//!          ▼
//! ┌─────────────────┐
//! │   Supervisor    │  ◄── notify_storage_write_complete()
//! └────────┬────────┘
//!          │
//!          │ IPC (MSG_STORAGE_RESULT)
//!          ▼
//! ┌─────────────────┐
//! │ IdentityService │  ◄── Matches request_id, sends response to client
//! └─────────────────┘
//! ```

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use zos_apps::manifest::IDENTITY_SERVICE_MANIFEST;
use zos_apps::syscall;
use zos_apps::{app_main, AppContext, AppError, AppManifest, ControlFlow, Message, ZeroApp};
use zos_identity::ipc::{
    CreateMachineKeyRequest, CreateMachineKeyResponse, GenerateNeuralKeyRequest,
    GenerateNeuralKeyResponse, GetIdentityKeyRequest, GetIdentityKeyResponse,
    GetMachineKeyRequest, GetMachineKeyResponse, ListMachineKeysRequest, ListMachineKeysResponse,
    NeuralKeyGenerated, NeuralShard, PublicIdentifiers, RecoverNeuralKeyRequest,
    RecoverNeuralKeyResponse, RevokeMachineKeyRequest, RevokeMachineKeyResponse,
    RotateMachineKeyRequest, RotateMachineKeyResponse,
};
use zos_identity::keystore::{LocalKeyStore, MachineKeyRecord};
use zos_identity::KeyError;
use zos_process::{identity_key, identity_machine, storage_result, MSG_STORAGE_RESULT};
use zos_vfs::{parent_path, Inode};

// =============================================================================
// Crypto Helpers (simplified for WASM - production would use proper crates)
// =============================================================================

/// Generate random bytes using the kernel's getrandom
fn generate_random_bytes(len: usize) -> Vec<u8> {
    // Use wallclock and PID for entropy source in WASM
    // In production, this would use the getrandom syscall
    let mut bytes = vec![0u8; len];
    let time = syscall::get_wallclock();
    let pid = syscall::get_pid();
    
    // Simple PRNG seeded with time and PID
    let mut state = time ^ ((pid as u64) << 32);
    for byte in bytes.iter_mut() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *byte = (state >> 56) as u8;
    }
    bytes
}

/// Convert bytes to hex string
fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Convert hex string to bytes
fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, &'static str> {
    if hex.len() % 2 != 0 {
        return Err("Invalid hex length");
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| "Invalid hex"))
        .collect()
}

/// Simple Shamir secret sharing (3-of-5) - mock implementation
/// Production would use a proper Shamir library
fn shamir_split(secret: &[u8], threshold: usize, shares: usize) -> Vec<NeuralShard> {
    let _ = threshold; // Would be used in real implementation
    let mut shards = Vec::with_capacity(shares);
    
    for i in 1..=shares {
        // Generate a shard by XORing secret with deterministic "random" data
        let mut shard_bytes = Vec::with_capacity(secret.len() + 1);
        shard_bytes.push(i as u8); // Shard index
        
        // Generate deterministic padding based on index
        let mut state = (i as u64).wrapping_mul(0x9E3779B97F4A7C15);
        for &byte in secret.iter() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(i as u64);
            shard_bytes.push(byte ^ (state >> 56) as u8);
        }
        
        shards.push(NeuralShard {
            index: i as u8,
            hex: bytes_to_hex(&shard_bytes),
        });
    }
    
    shards
}

/// Reconstruct secret from shards (mock implementation)
fn shamir_reconstruct(shards: &[NeuralShard]) -> Result<Vec<u8>, KeyError> {
    if shards.len() < 3 {
        return Err(KeyError::InsufficientShards);
    }
    
    // Use first shard to reconstruct (simplified - real Shamir uses polynomial interpolation)
    let shard = &shards[0];
    let shard_bytes = hex_to_bytes(&shard.hex)
        .map_err(|e| KeyError::InvalidShard(String::from(e)))?;
    
    if shard_bytes.is_empty() {
        return Err(KeyError::InvalidShard(String::from("Empty shard")));
    }
    
    let index = shard_bytes[0] as u64;
    let mut secret = Vec::with_capacity(shard_bytes.len() - 1);
    
    // Reverse the XOR operation
    let mut state = index.wrapping_mul(0x9E3779B97F4A7C15);
    for &byte in &shard_bytes[1..] {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(index);
        secret.push(byte ^ (state >> 56) as u8);
    }
    
    Ok(secret)
}

/// Derive a public key from entropy with a salt (mock Ed25519/X25519)
fn derive_public_key(entropy: &[u8], salt: &str) -> [u8; 32] {
    let mut combined = Vec::with_capacity(entropy.len() + salt.len());
    combined.extend_from_slice(entropy);
    combined.extend_from_slice(salt.as_bytes());
    
    // XOR fold to 32 bytes (mock derivation)
    let mut public_key = [0u8; 32];
    for (i, &byte) in combined.iter().enumerate() {
        public_key[i % 32] ^= byte;
    }
    
    // Add more mixing
    for i in 0..32 {
        let state = (public_key[i] as u64)
            .wrapping_mul(0x9E3779B97F4A7C15)
            .wrapping_add(i as u64);
        public_key[i] = (state >> 56) as u8;
    }
    
    public_key
}

// =============================================================================
// Pending Storage Operations
// =============================================================================

/// Tracks pending storage operations awaiting results
#[derive(Clone)]
enum PendingOp {
    /// Check if identity key exists (for generate)
    CheckKeyExists {
        client_pid: u32,
        user_id: u128,
        cap_slots: Vec<u32>,
    },
    /// Write identity key store content (step 1 - then write inode)
    WriteKeyStoreContent {
        client_pid: u32,
        user_id: u128,
        result: NeuralKeyGenerated,
        json_bytes: Vec<u8>,
        cap_slots: Vec<u32>,
    },
    /// Write identity key store inode (step 2 - then send response)
    WriteKeyStoreInode {
        client_pid: u32,
        result: NeuralKeyGenerated,
        cap_slots: Vec<u32>,
    },
    /// Get identity key for retrieval
    GetIdentityKey {
        client_pid: u32,
        cap_slots: Vec<u32>,
    },
    /// Write recovered key store content (step 1 - then write inode)
    WriteRecoveredKeyStoreContent {
        client_pid: u32,
        user_id: u128,
        result: NeuralKeyGenerated,
        json_bytes: Vec<u8>,
        cap_slots: Vec<u32>,
    },
    /// Write recovered key store inode (step 2 - then send response)
    WriteRecoveredKeyStoreInode {
        client_pid: u32,
        result: NeuralKeyGenerated,
        cap_slots: Vec<u32>,
    },
    /// Check identity key exists (for create machine key)
    CheckIdentityForMachine {
        client_pid: u32,
        request: CreateMachineKeyRequest,
        cap_slots: Vec<u32>,
    },
    /// Write machine key content (step 1 - then write inode)
    WriteMachineKeyContent {
        client_pid: u32,
        user_id: u128,
        record: MachineKeyRecord,
        json_bytes: Vec<u8>,
        cap_slots: Vec<u32>,
    },
    /// Write machine key inode (step 2 - then send response)
    WriteMachineKeyInode {
        client_pid: u32,
        record: MachineKeyRecord,
        cap_slots: Vec<u32>,
    },
    /// List machine keys (storage list operation)
    ListMachineKeys {
        client_pid: u32,
        user_id: u128,
        cap_slots: Vec<u32>,
    },
    /// Read individual machine key record
    ReadMachineKey {
        client_pid: u32,
        user_id: u128,
        /// Remaining paths to read
        remaining_paths: Vec<String>,
        /// Collected records so far
        records: Vec<MachineKeyRecord>,
        cap_slots: Vec<u32>,
    },
    /// Delete machine key content (step 1 - then delete inode)
    DeleteMachineKey {
        client_pid: u32,
        user_id: u128,
        machine_id: u128,
        cap_slots: Vec<u32>,
    },
    /// Delete machine key inode (step 2 - then send response)
    DeleteMachineKeyInode {
        client_pid: u32,
        cap_slots: Vec<u32>,
    },
    /// Read machine key for rotation
    ReadMachineForRotate {
        client_pid: u32,
        user_id: u128,
        machine_id: u128,
        cap_slots: Vec<u32>,
    },
    /// Write rotated machine key content (step 1 - then write inode)
    WriteRotatedMachineKeyContent {
        client_pid: u32,
        user_id: u128,
        record: MachineKeyRecord,
        json_bytes: Vec<u8>,
        cap_slots: Vec<u32>,
    },
    /// Write rotated machine key inode (step 2 - then send response)
    WriteRotatedMachineKeyInode {
        client_pid: u32,
        record: MachineKeyRecord,
        cap_slots: Vec<u32>,
    },
    /// Read single machine key by ID
    ReadSingleMachineKey {
        client_pid: u32,
        cap_slots: Vec<u32>,
    },
}

// =============================================================================
// IdentityService Application
// =============================================================================

/// IdentityService - manages user cryptographic identities
pub struct IdentityService {
    /// Whether we have registered with init
    registered: bool,
    /// Pending storage operations: request_id -> operation context
    pending_ops: BTreeMap<u32, PendingOp>,
}

impl Default for IdentityService {
    fn default() -> Self {
        Self {
            registered: false,
            pending_ops: BTreeMap::new(),
        }
    }
}

impl IdentityService {
    // =========================================================================
    // Storage syscall helpers (async, non-blocking)
    // =========================================================================

    /// Start async storage read and track the pending operation
    fn start_storage_read(&mut self, key: &str, pending_op: PendingOp) -> Result<(), AppError> {
        match syscall::storage_read_async(key) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "IdentityService: storage_read_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("IdentityService: storage_read_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage read failed: {}", e)))
            }
        }
    }

    /// Start async storage write and track the pending operation
    fn start_storage_write(
        &mut self,
        key: &str,
        value: &[u8],
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        match syscall::storage_write_async(key, value) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "IdentityService: storage_write_async({}, {} bytes) -> request_id={}",
                    key,
                    value.len(),
                    request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("IdentityService: storage_write_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage write failed: {}", e)))
            }
        }
    }

    /// Start async storage delete and track the pending operation
    fn start_storage_delete(&mut self, key: &str, pending_op: PendingOp) -> Result<(), AppError> {
        match syscall::storage_delete_async(key) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "IdentityService: storage_delete_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("IdentityService: storage_delete_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage delete failed: {}", e)))
            }
        }
    }

    /// Start async storage exists check and track the pending operation
    fn start_storage_exists(&mut self, key: &str, pending_op: PendingOp) -> Result<(), AppError> {
        match syscall::storage_exists_async(key) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "IdentityService: storage_exists_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("IdentityService: storage_exists_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage exists failed: {}", e)))
            }
        }
    }

    /// Start async storage list and track the pending operation
    fn start_storage_list(&mut self, prefix: &str, pending_op: PendingOp) -> Result<(), AppError> {
        match syscall::storage_list_async(prefix) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "IdentityService: storage_list_async({}) -> request_id={}",
                    prefix, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("IdentityService: storage_list_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage list failed: {}", e)))
            }
        }
    }

    // =========================================================================
    // Request handlers (start async operations)
    // =========================================================================

    /// Handle MSG_GENERATE_NEURAL_KEY
    fn handle_generate_neural_key(
        &mut self,
        _ctx: &AppContext,
        msg: &Message,
    ) -> Result<(), AppError> {
        syscall::debug("IdentityService: Handling generate neural key request");

        // Parse request
        let request: GenerateNeuralKeyRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
                return self.send_neural_key_error_direct(msg, KeyError::DerivationFailed);
            }
        };

        let user_id = request.user_id;
        syscall::debug(&format!(
            "IdentityService: Generating Neural Key for user {:032x}",
            user_id
        ));

        // Start async exists check for key
        let key_path = LocalKeyStore::storage_path(user_id);
        self.start_storage_exists(
            &format!("inode:{}", key_path),
            PendingOp::CheckKeyExists {
                client_pid: msg.from_pid,
                user_id,
                cap_slots: msg.cap_slots.clone(),
            },
        )
    }

    /// Continue generate_neural_key after exists check completes
    fn continue_generate_after_exists_check(
        &mut self,
        client_pid: u32,
        user_id: u128,
        exists: bool,
        cap_slots: Vec<u32>,
    ) -> Result<(), AppError> {
        if exists {
            syscall::debug("IdentityService: Neural Key already exists");
            return self.send_neural_key_error_to_pid(
                client_pid,
                &cap_slots,
                KeyError::IdentityKeyAlreadyExists,
            );
        }

        // Generate 32 bytes of entropy
        let entropy = generate_random_bytes(32);
        syscall::debug(&format!(
            "IdentityService: Generated {} bytes of entropy",
            entropy.len()
        ));

        // Derive keypairs
        let identity_signing = derive_public_key(&entropy, "identity-signing");
        let machine_signing = derive_public_key(&entropy, "machine-signing");
        let machine_encryption = derive_public_key(&entropy, "machine-encryption");

        // Split entropy into 5 Shamir shards with threshold 3
        let shards = shamir_split(&entropy, 3, 5);
        syscall::debug(&format!(
            "IdentityService: Split entropy into {} shards",
            shards.len()
        ));

        // Create public identifiers
        let public_identifiers = PublicIdentifiers {
            identity_signing_pub_key: format!("0x{}", bytes_to_hex(&identity_signing)),
            machine_signing_pub_key: format!("0x{}", bytes_to_hex(&machine_signing)),
            machine_encryption_pub_key: format!("0x{}", bytes_to_hex(&machine_encryption)),
        };

        // Get creation timestamp
        let created_at = syscall::get_wallclock();

        // Create LocalKeyStore
        let key_store = LocalKeyStore::new(
            user_id,
            identity_signing,
            machine_signing,
            machine_encryption,
            created_at,
        );

        // Prepare result (shards will be returned to client)
        let result = NeuralKeyGenerated {
            public_identifiers,
            shards,
            created_at,
        };

        // Serialize and write to storage
        let key_path = LocalKeyStore::storage_path(user_id);
        match serde_json::to_vec(&key_store) {
            Ok(json_bytes) => {
                // Write both content and inode for VFS compatibility
                // Step 1: Write content (inode write follows in handle_storage_result)
                self.start_storage_write(
                    &format!("content:{}", key_path),
                    &json_bytes.clone(),
                    PendingOp::WriteKeyStoreContent {
                        client_pid,
                        user_id,
                        result,
                        json_bytes,
                        cap_slots,
                    },
                )
            }
            Err(e) => {
                syscall::debug(&format!("IdentityService: Failed to serialize keys: {}", e));
                self.send_neural_key_error_to_pid(
                    client_pid,
                    &cap_slots,
                    KeyError::StorageError(format!("Serialization failed: {}", e)),
                )
            }
        }
    }

    /// Handle MSG_RECOVER_NEURAL_KEY
    fn handle_recover_neural_key(
        &mut self,
        _ctx: &AppContext,
        msg: &Message,
    ) -> Result<(), AppError> {
        syscall::debug("IdentityService: Handling recover neural key request");

        // Parse request
        let request: RecoverNeuralKeyRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
                return self.send_recover_key_error_direct(msg, KeyError::DerivationFailed);
            }
        };

        let user_id = request.user_id;
        let shards = request.shards;

        syscall::debug(&format!(
            "IdentityService: Recovering Neural Key for user {:032x} with {} shards",
            user_id,
            shards.len()
        ));

        if shards.len() < 3 {
            return self.send_recover_key_error_direct(msg, KeyError::InsufficientShards);
        }

        // Reconstruct entropy from shards
        let entropy = match shamir_reconstruct(&shards) {
            Ok(e) => e,
            Err(e) => {
                syscall::debug(&format!("IdentityService: Shard reconstruction failed: {:?}", e));
                return self.send_recover_key_error_direct(msg, e);
            }
        };

        // Re-derive keypairs
        let identity_signing = derive_public_key(&entropy, "identity-signing");
        let machine_signing = derive_public_key(&entropy, "machine-signing");
        let machine_encryption = derive_public_key(&entropy, "machine-encryption");

        // Create public identifiers
        let public_identifiers = PublicIdentifiers {
            identity_signing_pub_key: format!("0x{}", bytes_to_hex(&identity_signing)),
            machine_signing_pub_key: format!("0x{}", bytes_to_hex(&machine_signing)),
            machine_encryption_pub_key: format!("0x{}", bytes_to_hex(&machine_encryption)),
        };

        // Get creation timestamp
        let created_at = syscall::get_wallclock();

        // Create LocalKeyStore
        let key_store = LocalKeyStore::new(
            user_id,
            identity_signing,
            machine_signing,
            machine_encryption,
            created_at,
        );

        // Re-split entropy for new shards
        let new_shards = shamir_split(&entropy, 3, 5);

        let result = NeuralKeyGenerated {
            public_identifiers,
            shards: new_shards,
            created_at,
        };

        // Serialize and write to storage
        let key_path = LocalKeyStore::storage_path(user_id);
        match serde_json::to_vec(&key_store) {
            Ok(json_bytes) => {
                // Write both content and inode for VFS compatibility
                // Step 1: Write content (inode write follows in handle_storage_result)
                self.start_storage_write(
                    &format!("content:{}", key_path),
                    &json_bytes.clone(),
                    PendingOp::WriteRecoveredKeyStoreContent {
                        client_pid: msg.from_pid,
                        user_id,
                        result,
                        json_bytes,
                        cap_slots: msg.cap_slots.clone(),
                    },
                )
            }
            Err(e) => self.send_recover_key_error_direct(
                msg,
                KeyError::StorageError(format!("Serialization failed: {}", e)),
            ),
        }
    }

    /// Handle MSG_GET_IDENTITY_KEY
    fn handle_get_identity_key(
        &mut self,
        _ctx: &AppContext,
        msg: &Message,
    ) -> Result<(), AppError> {
        syscall::debug("IdentityService: Handling get identity key request");

        // Parse request
        let request: GetIdentityKeyRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
                let response = GetIdentityKeyResponse {
                    result: Err(KeyError::DerivationFailed),
                };
                return self.send_response_to_pid(
                    msg.from_pid,
                    &msg.cap_slots,
                    identity_key::MSG_GET_IDENTITY_KEY_RESPONSE,
                    &response,
                );
            }
        };

        let user_id = request.user_id;
        let key_path = LocalKeyStore::storage_path(user_id);

        syscall::debug(&format!(
            "IdentityService: Getting identity key for user {:032x}",
            user_id
        ));

        // Start async read
        self.start_storage_read(
            &format!("content:{}", key_path),
            PendingOp::GetIdentityKey {
                client_pid: msg.from_pid,
                cap_slots: msg.cap_slots.clone(),
            },
        )
    }

    /// Handle MSG_CREATE_MACHINE_KEY
    fn handle_create_machine_key(
        &mut self,
        _ctx: &AppContext,
        msg: &Message,
    ) -> Result<(), AppError> {
        syscall::debug("IdentityService: Handling create machine key request");

        // Parse request
        let request: CreateMachineKeyRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
                return self.send_create_machine_error_direct(msg, KeyError::DerivationFailed);
            }
        };

        let user_id = request.user_id;
        syscall::debug(&format!(
            "IdentityService: Creating machine key for user {:032x}",
            user_id
        ));

        // Start async exists check for identity key
        let key_path = LocalKeyStore::storage_path(user_id);
        self.start_storage_exists(
            &format!("inode:{}", key_path),
            PendingOp::CheckIdentityForMachine {
                client_pid: msg.from_pid,
                request,
                cap_slots: msg.cap_slots.clone(),
            },
        )
    }

    /// Continue create_machine_key after identity key exists check
    fn continue_create_machine_after_identity_check(
        &mut self,
        client_pid: u32,
        request: CreateMachineKeyRequest,
        exists: bool,
        cap_slots: Vec<u32>,
    ) -> Result<(), AppError> {
        if !exists {
            syscall::debug("IdentityService: Identity key must exist before creating machine keys");
            return self.send_create_machine_error_to_pid(
                client_pid,
                &cap_slots,
                KeyError::IdentityKeyRequired,
            );
        }

        let user_id = request.user_id;

        // Generate unique entropy for this machine (same pattern as Neural Key)
        let machine_entropy = generate_random_bytes(32);
        let machine_id_bytes = generate_random_bytes(16);
        let machine_id =
            u128::from_le_bytes(machine_id_bytes[..16].try_into().unwrap_or([0u8; 16]));

        // Derive keys from entropy (matching Neural Key pattern)
        let signing_key = derive_public_key(&machine_entropy, "machine-signing");
        let encryption_key = derive_public_key(&machine_entropy, "machine-encryption");

        let now = syscall::get_wallclock();

        // Create the machine key record with derived keys
        let record = MachineKeyRecord {
            machine_id,
            signing_public_key: signing_key,
            encryption_public_key: encryption_key,
            authorized_at: now,
            authorized_by: user_id,
            capabilities: request.capabilities,
            machine_name: request.machine_name,
            last_seen_at: now,
            epoch: 1, // First epoch
        };

        syscall::debug(&format!(
            "IdentityService: Created machine key {:032x} with derived keys",
            machine_id
        ));

        // Write machine key to storage
        // Write both content and inode for VFS compatibility
        let machine_path = MachineKeyRecord::storage_path(user_id, machine_id);
        match serde_json::to_vec(&record) {
            Ok(json_bytes) => self.start_storage_write(
                &format!("content:{}", machine_path),
                &json_bytes.clone(),
                PendingOp::WriteMachineKeyContent {
                    client_pid,
                    user_id,
                    record,
                    json_bytes,
                    cap_slots,
                },
            ),
            Err(e) => {
                syscall::debug(&format!(
                    "IdentityService: Failed to serialize machine key: {}",
                    e
                ));
                self.send_create_machine_error_to_pid(
                    client_pid,
                    &cap_slots,
                    KeyError::StorageError(format!("Serialization failed: {}", e)),
                )
            }
        }
    }

    /// Handle MSG_LIST_MACHINE_KEYS
    fn handle_list_machine_keys(
        &mut self,
        _ctx: &AppContext,
        msg: &Message,
    ) -> Result<(), AppError> {
        syscall::debug("IdentityService: Handling list machine keys request");

        // Parse request
        let request: ListMachineKeysRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
                let response = ListMachineKeysResponse { machines: vec![] };
                return self.send_response_to_pid(
                    msg.from_pid,
                    &msg.cap_slots,
                    identity_machine::MSG_LIST_MACHINE_KEYS_RESPONSE,
                    &response,
                );
            }
        };

        let user_id = request.user_id;
        syscall::debug(&format!(
            "IdentityService: Listing machine keys for user {:032x}",
            user_id
        ));

        // Start async list of machine directory
        let machine_dir = format!("/home/{:032x}/.zos/identity/machine", user_id);
        self.start_storage_list(
            &format!("inode:{}", machine_dir),
            PendingOp::ListMachineKeys {
                client_pid: msg.from_pid,
                user_id,
                cap_slots: msg.cap_slots.clone(),
            },
        )
    }

    /// Handle MSG_REVOKE_MACHINE_KEY
    fn handle_revoke_machine_key(
        &mut self,
        _ctx: &AppContext,
        msg: &Message,
    ) -> Result<(), AppError> {
        syscall::debug("IdentityService: Handling revoke machine key request");

        // Parse request
        let request: RevokeMachineKeyRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
                return self.send_revoke_machine_error_direct(msg, KeyError::DerivationFailed);
            }
        };

        let user_id = request.user_id;
        let machine_id = request.machine_id;

        syscall::debug(&format!(
            "IdentityService: Revoking machine {:032x} for user {:032x}",
            machine_id, user_id
        ));

        // Skip exists check - just proceed to delete directly
        // IndexedDB delete is idempotent (deleting non-existent content succeeds)
        // This handles cases where machine keys exist in UI (Zustand store) but were
        // never properly persisted to storage.
        let machine_path = MachineKeyRecord::storage_path(user_id, machine_id);
        self.start_storage_delete(
            &format!("content:{}", machine_path),
            PendingOp::DeleteMachineKey {
                client_pid: msg.from_pid,
                user_id,
                machine_id,
                cap_slots: msg.cap_slots.clone(),
            },
        )
    }

    /// Handle MSG_ROTATE_MACHINE_KEY
    fn handle_rotate_machine_key(
        &mut self,
        _ctx: &AppContext,
        msg: &Message,
    ) -> Result<(), AppError> {
        syscall::debug("IdentityService: Handling rotate machine key request");

        // Parse request
        let request: RotateMachineKeyRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
                return self.send_rotate_machine_error_direct(msg, KeyError::DerivationFailed);
            }
        };

        let user_id = request.user_id;
        let machine_id = request.machine_id;

        syscall::debug(&format!(
            "IdentityService: Rotating keys for machine {:032x} user {:032x}",
            machine_id, user_id
        ));

        // Start async read of machine key
        let machine_path = MachineKeyRecord::storage_path(user_id, machine_id);
        self.start_storage_read(
            &format!("content:{}", machine_path),
            PendingOp::ReadMachineForRotate {
                client_pid: msg.from_pid,
                user_id,
                machine_id,
                cap_slots: msg.cap_slots.clone(),
            },
        )
    }

    /// Handle MSG_GET_MACHINE_KEY
    fn handle_get_machine_key(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        syscall::debug("IdentityService: Handling get machine key request");

        // Parse request
        let request: GetMachineKeyRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
                let response = GetMachineKeyResponse {
                    result: Err(KeyError::DerivationFailed),
                };
                return self.send_response_to_pid(
                    msg.from_pid,
                    &msg.cap_slots,
                    identity_machine::MSG_GET_MACHINE_KEY_RESPONSE,
                    &response,
                );
            }
        };

        let user_id = request.user_id;
        let machine_id = request.machine_id;

        syscall::debug(&format!(
            "IdentityService: Getting machine key {:032x} for user {:032x}",
            machine_id, user_id
        ));

        // Start async read of machine key
        let machine_path = MachineKeyRecord::storage_path(user_id, machine_id);
        self.start_storage_read(
            &format!("content:{}", machine_path),
            PendingOp::ReadSingleMachineKey {
                client_pid: msg.from_pid,
                cap_slots: msg.cap_slots.clone(),
            },
        )
    }

    /// Continue rotate after reading machine key
    fn continue_rotate_after_read(
        &mut self,
        client_pid: u32,
        user_id: u128,
        machine_id: u128,
        data: &[u8],
        cap_slots: Vec<u32>,
    ) -> Result<(), AppError> {
        // Parse existing record
        let mut record: MachineKeyRecord = match serde_json::from_slice(data) {
            Ok(r) => r,
            Err(e) => {
                syscall::debug(&format!(
                    "IdentityService: Failed to parse machine key: {}",
                    e
                ));
                return self.send_rotate_machine_error_to_pid(
                    client_pid,
                    &cap_slots,
                    KeyError::StorageError(format!("Parse failed: {}", e)),
                );
            }
        };

        let now = syscall::get_wallclock();

        // Generate NEW entropy for rotated keys (same pattern as create)
        let new_entropy = generate_random_bytes(32);
        let new_signing = derive_public_key(&new_entropy, "machine-signing");
        let new_encryption = derive_public_key(&new_entropy, "machine-encryption");

        // Update the record with new derived keys and increment epoch
        record.signing_public_key = new_signing;
        record.encryption_public_key = new_encryption;
        record.epoch += 1;
        record.last_seen_at = now;

        syscall::debug(&format!(
            "IdentityService: Rotating machine key {:032x} to epoch {}",
            machine_id, record.epoch
        ));

        // Write updated record
        // Write both content and inode for VFS compatibility
        let machine_path = MachineKeyRecord::storage_path(user_id, machine_id);
        match serde_json::to_vec(&record) {
            Ok(json_bytes) => self.start_storage_write(
                &format!("content:{}", machine_path),
                &json_bytes.clone(),
                PendingOp::WriteRotatedMachineKeyContent {
                    client_pid,
                    user_id,
                    record,
                    json_bytes,
                    cap_slots,
                },
            ),
            Err(e) => {
                syscall::debug(&format!(
                    "IdentityService: Failed to serialize rotated key: {}",
                    e
                ));
                self.send_rotate_machine_error_to_pid(
                    client_pid,
                    &cap_slots,
                    KeyError::StorageError(format!("Serialization failed: {}", e)),
                )
            }
        }
    }

    // =========================================================================
    // Storage result handler
    // =========================================================================

    /// Handle MSG_STORAGE_RESULT - async storage operation completed
    fn handle_storage_result(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        // Parse storage result
        // Format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
        if msg.data.len() < 9 {
            syscall::debug("IdentityService: storage result too short");
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

        syscall::debug(&format!(
            "IdentityService: storage result request_id={}, type={}, data_len={}",
            request_id, result_type, data_len
        ));

        // Look up pending operation
        let pending_op = match self.pending_ops.remove(&request_id) {
            Some(op) => op,
            None => {
                syscall::debug(&format!(
                    "IdentityService: unknown request_id {}",
                    request_id
                ));
                return Ok(());
            }
        };

        // Dispatch based on operation type
        match pending_op {
            PendingOp::CheckKeyExists {
                client_pid,
                user_id,
                cap_slots,
            } => {
                let exists = if result_type == storage_result::EXISTS_OK {
                    !data.is_empty() && data[0] == 1
                } else {
                    false
                };
                self.continue_generate_after_exists_check(client_pid, user_id, exists, cap_slots)
            }

            PendingOp::WriteKeyStoreContent {
                client_pid,
                user_id,
                result,
                json_bytes,
                cap_slots,
            } => {
                if result_type == storage_result::WRITE_OK {
                    // Step 2: Now write the inode
                    let key_path = LocalKeyStore::storage_path(user_id);
                    let now = syscall::get_wallclock();
                    let inode = Inode::new_file(
                        key_path.clone(),
                        parent_path(&key_path).to_string(),
                        key_path.rsplit('/').next().unwrap_or("keys.json").to_string(),
                        Some(user_id),
                        json_bytes.len() as u64,
                        None,
                        now,
                    );
                    match serde_json::to_vec(&inode) {
                        Ok(inode_json) => {
                            self.start_storage_write(
                                &format!("inode:{}", key_path),
                                &inode_json,
                                PendingOp::WriteKeyStoreInode {
                                    client_pid,
                                    result,
                                    cap_slots,
                                },
                            )
                        }
                        Err(e) => {
                            syscall::debug(&format!("IdentityService: Failed to serialize inode: {}", e));
                            self.send_neural_key_error_to_pid(
                                client_pid,
                                &cap_slots,
                                KeyError::StorageError(format!("Inode serialization failed: {}", e)),
                            )
                        }
                    }
                } else {
                    self.send_neural_key_error_to_pid(
                        client_pid,
                        &cap_slots,
                        KeyError::StorageError("Content write failed".into()),
                    )
                }
            }

            PendingOp::WriteKeyStoreInode {
                client_pid,
                result,
                cap_slots,
            } => {
                if result_type == storage_result::WRITE_OK {
                    syscall::debug("IdentityService: Neural key stored (content + inode)");
                    let response = GenerateNeuralKeyResponse { result: Ok(result) };
                    self.send_response_to_pid(
                        client_pid,
                        &cap_slots,
                        identity_key::MSG_GENERATE_NEURAL_KEY_RESPONSE,
                        &response,
                    )
                } else {
                    self.send_neural_key_error_to_pid(
                        client_pid,
                        &cap_slots,
                        KeyError::StorageError("Inode write failed".into()),
                    )
                }
            }

            PendingOp::GetIdentityKey {
                client_pid,
                cap_slots,
            } => {
                let response = if result_type == storage_result::READ_OK {
                    match serde_json::from_slice::<LocalKeyStore>(data) {
                        Ok(key_store) => GetIdentityKeyResponse {
                            result: Ok(Some(key_store)),
                        },
                        Err(e) => {
                            syscall::debug(&format!(
                                "IdentityService: Failed to parse stored keys: {}",
                                e
                            ));
                            GetIdentityKeyResponse {
                                result: Err(KeyError::StorageError(format!("Parse failed: {}", e))),
                            }
                        }
                    }
                } else {
                    // Key not found
                    GetIdentityKeyResponse { result: Ok(None) }
                };
                self.send_response_to_pid(
                    client_pid,
                    &cap_slots,
                    identity_key::MSG_GET_IDENTITY_KEY_RESPONSE,
                    &response,
                )
            }

            PendingOp::WriteRecoveredKeyStoreContent {
                client_pid,
                user_id,
                result,
                json_bytes,
                cap_slots,
            } => {
                if result_type == storage_result::WRITE_OK {
                    // Step 2: Now write the inode
                    let key_path = LocalKeyStore::storage_path(user_id);
                    let now = syscall::get_wallclock();
                    let inode = Inode::new_file(
                        key_path.clone(),
                        parent_path(&key_path).to_string(),
                        key_path.rsplit('/').next().unwrap_or("keys.json").to_string(),
                        Some(user_id),
                        json_bytes.len() as u64,
                        None,
                        now,
                    );
                    match serde_json::to_vec(&inode) {
                        Ok(inode_json) => {
                            self.start_storage_write(
                                &format!("inode:{}", key_path),
                                &inode_json,
                                PendingOp::WriteRecoveredKeyStoreInode {
                                    client_pid,
                                    result,
                                    cap_slots,
                                },
                            )
                        }
                        Err(e) => {
                            syscall::debug(&format!("IdentityService: Failed to serialize inode: {}", e));
                            self.send_recover_key_error_to_pid(
                                client_pid,
                                &cap_slots,
                                KeyError::StorageError(format!("Inode serialization failed: {}", e)),
                            )
                        }
                    }
                } else {
                    self.send_recover_key_error_to_pid(
                        client_pid,
                        &cap_slots,
                        KeyError::StorageError("Content write failed".into()),
                    )
                }
            }

            PendingOp::WriteRecoveredKeyStoreInode {
                client_pid,
                result,
                cap_slots,
            } => {
                if result_type == storage_result::WRITE_OK {
                    syscall::debug("IdentityService: Recovered key stored (content + inode)");
                    let response = RecoverNeuralKeyResponse { result: Ok(result) };
                    self.send_response_to_pid(
                        client_pid,
                        &cap_slots,
                        identity_key::MSG_RECOVER_NEURAL_KEY_RESPONSE,
                        &response,
                    )
                } else {
                    self.send_recover_key_error_to_pid(
                        client_pid,
                        &cap_slots,
                        KeyError::StorageError("Inode write failed".into()),
                    )
                }
            }

            PendingOp::CheckIdentityForMachine {
                client_pid,
                request,
                cap_slots,
            } => {
                let exists = if result_type == storage_result::EXISTS_OK {
                    !data.is_empty() && data[0] == 1
                } else {
                    false
                };
                self.continue_create_machine_after_identity_check(
                    client_pid, request, exists, cap_slots,
                )
            }

            PendingOp::WriteMachineKeyContent {
                client_pid,
                user_id,
                record,
                json_bytes,
                cap_slots,
            } => {
                if result_type == storage_result::WRITE_OK {
                    // Step 2: Now write the inode
                    let machine_path = MachineKeyRecord::storage_path(user_id, record.machine_id);
                    let now = syscall::get_wallclock();
                    let inode = Inode::new_file(
                        machine_path.clone(),
                        parent_path(&machine_path).to_string(),
                        machine_path.rsplit('/').next().unwrap_or("machine.json").to_string(),
                        Some(user_id),
                        json_bytes.len() as u64,
                        None,
                        now,
                    );
                    match serde_json::to_vec(&inode) {
                        Ok(inode_json) => {
                            self.start_storage_write(
                                &format!("inode:{}", machine_path),
                                &inode_json,
                                PendingOp::WriteMachineKeyInode {
                                    client_pid,
                                    record,
                                    cap_slots,
                                },
                            )
                        }
                        Err(e) => {
                            syscall::debug(&format!("IdentityService: Failed to serialize inode: {}", e));
                            self.send_create_machine_error_to_pid(
                                client_pid,
                                &cap_slots,
                                KeyError::StorageError(format!("Inode serialization failed: {}", e)),
                            )
                        }
                    }
                } else {
                    self.send_create_machine_error_to_pid(
                        client_pid,
                        &cap_slots,
                        KeyError::StorageError("Content write failed".into()),
                    )
                }
            }

            PendingOp::WriteMachineKeyInode {
                client_pid,
                record,
                cap_slots,
            } => {
                if result_type == storage_result::WRITE_OK {
                    syscall::debug(&format!(
                        "IdentityService: Stored machine key {:032x} (content + inode)",
                        record.machine_id
                    ));
                    let response = CreateMachineKeyResponse {
                        result: Ok(record),
                    };
                    self.send_response_to_pid(
                        client_pid,
                        &cap_slots,
                        identity_machine::MSG_CREATE_MACHINE_KEY_RESPONSE,
                        &response,
                    )
                } else {
                    self.send_create_machine_error_to_pid(
                        client_pid,
                        &cap_slots,
                        KeyError::StorageError("Inode write failed".into()),
                    )
                }
            }

            PendingOp::ListMachineKeys {
                client_pid,
                user_id,
                cap_slots,
            } => {
                // Storage list returns JSON array of paths
                if result_type == storage_result::LIST_OK {
                    match serde_json::from_slice::<Vec<String>>(data) {
                        Ok(paths) => {
                            // Filter to .json files and start reading them
                            let json_paths: Vec<String> = paths
                                .into_iter()
                                .filter(|p| p.ends_with(".json"))
                                .map(|p| format!("content:{}", p))
                                .collect();

                            if json_paths.is_empty() {
                                // No machine keys
                                let response = ListMachineKeysResponse { machines: vec![] };
                                return self.send_response_to_pid(
                                    client_pid,
                                    &cap_slots,
                                    identity_machine::MSG_LIST_MACHINE_KEYS_RESPONSE,
                                    &response,
                                );
                            }

                            // Start reading first machine key file
                            let mut remaining = json_paths;
                            let first = remaining.remove(0);
                            self.start_storage_read(
                                &first,
                                PendingOp::ReadMachineKey {
                                    client_pid,
                                    user_id,
                                    remaining_paths: remaining,
                                    records: vec![],
                                    cap_slots,
                                },
                            )
                        }
                        Err(_) => {
                            let response = ListMachineKeysResponse { machines: vec![] };
                            self.send_response_to_pid(
                                client_pid,
                                &cap_slots,
                                identity_machine::MSG_LIST_MACHINE_KEYS_RESPONSE,
                                &response,
                            )
                        }
                    }
                } else {
                    // Directory doesn't exist or error - return empty list
                    let response = ListMachineKeysResponse { machines: vec![] };
                    self.send_response_to_pid(
                        client_pid,
                        &cap_slots,
                        identity_machine::MSG_LIST_MACHINE_KEYS_RESPONSE,
                        &response,
                    )
                }
            }

            PendingOp::ReadMachineKey {
                client_pid,
                user_id,
                mut remaining_paths,
                mut records,
                cap_slots,
            } => {
                // Parse the machine key record
                if result_type == storage_result::READ_OK {
                    if let Ok(record) = serde_json::from_slice::<MachineKeyRecord>(data) {
                        records.push(record);
                    }
                }

                // Read next file or send response
                if remaining_paths.is_empty() {
                    syscall::debug(&format!(
                        "IdentityService: Found {} machine keys",
                        records.len()
                    ));
                    let response = ListMachineKeysResponse { machines: records };
                    self.send_response_to_pid(
                        client_pid,
                        &cap_slots,
                        identity_machine::MSG_LIST_MACHINE_KEYS_RESPONSE,
                        &response,
                    )
                } else {
                    let next = remaining_paths.remove(0);
                    self.start_storage_read(
                        &next,
                        PendingOp::ReadMachineKey {
                            client_pid,
                            user_id,
                            remaining_paths,
                            records,
                            cap_slots,
                        },
                    )
                }
            }

            PendingOp::DeleteMachineKey {
                client_pid,
                user_id,
                machine_id,
                cap_slots,
            } => {
                if result_type == storage_result::WRITE_OK {
                    // Step 2: Now delete the inode
                    syscall::debug("IdentityService: Machine key content deleted, now deleting inode");
                    let machine_path = MachineKeyRecord::storage_path(user_id, machine_id);
                    self.start_storage_delete(
                        &format!("inode:{}", machine_path),
                        PendingOp::DeleteMachineKeyInode {
                            client_pid,
                            cap_slots,
                        },
                    )
                } else {
                    self.send_revoke_machine_error_to_pid(
                        client_pid,
                        &cap_slots,
                        KeyError::StorageError("Delete failed".into()),
                    )
                }
            }

            PendingOp::DeleteMachineKeyInode {
                client_pid,
                cap_slots,
            } => {
                if result_type == storage_result::WRITE_OK {
                    syscall::debug("IdentityService: Machine key deleted (content + inode)");
                    let response = RevokeMachineKeyResponse { result: Ok(()) };
                    self.send_response_to_pid(
                        client_pid,
                        &cap_slots,
                        identity_machine::MSG_REVOKE_MACHINE_KEY_RESPONSE,
                        &response,
                    )
                } else {
                    self.send_revoke_machine_error_to_pid(
                        client_pid,
                        &cap_slots,
                        KeyError::StorageError("Inode delete failed".into()),
                    )
                }
            }

            PendingOp::ReadMachineForRotate {
                client_pid,
                user_id,
                machine_id,
                cap_slots,
            } => {
                if result_type == storage_result::READ_OK {
                    self.continue_rotate_after_read(client_pid, user_id, machine_id, data, cap_slots)
                } else {
                    self.send_rotate_machine_error_to_pid(
                        client_pid,
                        &cap_slots,
                        KeyError::MachineKeyNotFound,
                    )
                }
            }

            PendingOp::WriteRotatedMachineKeyContent {
                client_pid,
                user_id,
                record,
                json_bytes,
                cap_slots,
            } => {
                if result_type == storage_result::WRITE_OK {
                    // Step 2: Now write the inode
                    let machine_path = MachineKeyRecord::storage_path(user_id, record.machine_id);
                    let now = syscall::get_wallclock();
                    let inode = Inode::new_file(
                        machine_path.clone(),
                        parent_path(&machine_path).to_string(),
                        machine_path.rsplit('/').next().unwrap_or("machine.json").to_string(),
                        Some(user_id),
                        json_bytes.len() as u64,
                        None,
                        now,
                    );
                    match serde_json::to_vec(&inode) {
                        Ok(inode_json) => {
                            self.start_storage_write(
                                &format!("inode:{}", machine_path),
                                &inode_json,
                                PendingOp::WriteRotatedMachineKeyInode {
                                    client_pid,
                                    record,
                                    cap_slots,
                                },
                            )
                        }
                        Err(e) => {
                            syscall::debug(&format!("IdentityService: Failed to serialize inode: {}", e));
                            self.send_rotate_machine_error_to_pid(
                                client_pid,
                                &cap_slots,
                                KeyError::StorageError(format!("Inode serialization failed: {}", e)),
                            )
                        }
                    }
                } else {
                    self.send_rotate_machine_error_to_pid(
                        client_pid,
                        &cap_slots,
                        KeyError::StorageError("Content write failed".into()),
                    )
                }
            }

            PendingOp::WriteRotatedMachineKeyInode {
                client_pid,
                record,
                cap_slots,
            } => {
                if result_type == storage_result::WRITE_OK {
                    syscall::debug(&format!(
                        "IdentityService: Rotated keys for machine {:032x} (epoch {}, content + inode)",
                        record.machine_id, record.epoch
                    ));
                    let response = RotateMachineKeyResponse {
                        result: Ok(record),
                    };
                    self.send_response_to_pid(
                        client_pid,
                        &cap_slots,
                        identity_machine::MSG_ROTATE_MACHINE_KEY_RESPONSE,
                        &response,
                    )
                } else {
                    self.send_rotate_machine_error_to_pid(
                        client_pid,
                        &cap_slots,
                        KeyError::StorageError("Inode write failed".into()),
                    )
                }
            }

            PendingOp::ReadSingleMachineKey {
                client_pid,
                cap_slots,
            } => {
                let response = if result_type == storage_result::READ_OK {
                    match serde_json::from_slice::<MachineKeyRecord>(data) {
                        Ok(record) => GetMachineKeyResponse {
                            result: Ok(Some(record)),
                        },
                        Err(e) => {
                            syscall::debug(&format!(
                                "IdentityService: Failed to parse machine key: {}",
                                e
                            ));
                            GetMachineKeyResponse {
                                result: Err(KeyError::StorageError(format!("Parse failed: {}", e))),
                            }
                        }
                    }
                } else {
                    // Key not found
                    GetMachineKeyResponse { result: Ok(None) }
                };
                self.send_response_to_pid(
                    client_pid,
                    &cap_slots,
                    identity_machine::MSG_GET_MACHINE_KEY_RESPONSE,
                    &response,
                )
            }
        }
    }

    // =========================================================================
    // Response helpers
    // =========================================================================

    /// Send response to a specific PID via debug channel routing
    fn send_response_to_pid<T: serde::Serialize>(
        &self,
        to_pid: u32,
        cap_slots: &[u32],
        tag: u32,
        response: &T,
    ) -> Result<(), AppError> {
        match serde_json::to_vec(response) {
            Ok(data) => {
                // Try to send via transferred reply capability first
                if let Some(&reply_slot) = cap_slots.first() {
                    syscall::debug(&format!(
                        "IdentityService: Sending response via reply cap slot {} (tag 0x{:x})",
                        reply_slot, tag
                    ));
                    match syscall::send(reply_slot, tag, &data) {
                        Ok(()) => {
                            syscall::debug("IdentityService: Response sent via reply cap");
                            return Ok(());
                        }
                        Err(e) => {
                            syscall::debug(&format!(
                                "IdentityService: Reply cap send failed ({}), falling back to debug channel",
                                e
                            ));
                        }
                    }
                }

                // Fallback: send via debug channel for supervisor to route
                let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
                syscall::debug(&format!("SERVICE:RESPONSE:{}:{:08x}:{}", to_pid, tag, hex));
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!(
                    "IdentityService: Failed to serialize response: {}",
                    e
                ));
                Err(AppError::IpcError(format!("Serialization failed: {}", e)))
            }
        }
    }

    /// Send error response for generate neural key (from message)
    fn send_neural_key_error_direct(
        &self,
        msg: &Message,
        error: KeyError,
    ) -> Result<(), AppError> {
        let response = GenerateNeuralKeyResponse { result: Err(error) };
        self.send_response_to_pid(
            msg.from_pid,
            &msg.cap_slots,
            identity_key::MSG_GENERATE_NEURAL_KEY_RESPONSE,
            &response,
        )
    }

    /// Send error response for generate neural key (to specific PID)
    fn send_neural_key_error_to_pid(
        &self,
        client_pid: u32,
        cap_slots: &[u32],
        error: KeyError,
    ) -> Result<(), AppError> {
        let response = GenerateNeuralKeyResponse { result: Err(error) };
        self.send_response_to_pid(
            client_pid,
            cap_slots,
            identity_key::MSG_GENERATE_NEURAL_KEY_RESPONSE,
            &response,
        )
    }

    /// Send error response for recover neural key (from message)
    fn send_recover_key_error_direct(
        &self,
        msg: &Message,
        error: KeyError,
    ) -> Result<(), AppError> {
        let response = RecoverNeuralKeyResponse { result: Err(error) };
        self.send_response_to_pid(
            msg.from_pid,
            &msg.cap_slots,
            identity_key::MSG_RECOVER_NEURAL_KEY_RESPONSE,
            &response,
        )
    }

    /// Send error response for recover neural key (to specific PID)
    fn send_recover_key_error_to_pid(
        &self,
        client_pid: u32,
        cap_slots: &[u32],
        error: KeyError,
    ) -> Result<(), AppError> {
        let response = RecoverNeuralKeyResponse { result: Err(error) };
        self.send_response_to_pid(
            client_pid,
            cap_slots,
            identity_key::MSG_RECOVER_NEURAL_KEY_RESPONSE,
            &response,
        )
    }

    /// Send error response for create machine key (from message)
    fn send_create_machine_error_direct(
        &self,
        msg: &Message,
        error: KeyError,
    ) -> Result<(), AppError> {
        let response = CreateMachineKeyResponse { result: Err(error) };
        self.send_response_to_pid(
            msg.from_pid,
            &msg.cap_slots,
            identity_machine::MSG_CREATE_MACHINE_KEY_RESPONSE,
            &response,
        )
    }

    /// Send error response for create machine key (to specific PID)
    fn send_create_machine_error_to_pid(
        &self,
        client_pid: u32,
        cap_slots: &[u32],
        error: KeyError,
    ) -> Result<(), AppError> {
        let response = CreateMachineKeyResponse { result: Err(error) };
        self.send_response_to_pid(
            client_pid,
            cap_slots,
            identity_machine::MSG_CREATE_MACHINE_KEY_RESPONSE,
            &response,
        )
    }

    /// Send error response for revoke machine key (from message)
    fn send_revoke_machine_error_direct(
        &self,
        msg: &Message,
        error: KeyError,
    ) -> Result<(), AppError> {
        let response = RevokeMachineKeyResponse { result: Err(error) };
        self.send_response_to_pid(
            msg.from_pid,
            &msg.cap_slots,
            identity_machine::MSG_REVOKE_MACHINE_KEY_RESPONSE,
            &response,
        )
    }

    /// Send error response for revoke machine key (to specific PID)
    fn send_revoke_machine_error_to_pid(
        &self,
        client_pid: u32,
        cap_slots: &[u32],
        error: KeyError,
    ) -> Result<(), AppError> {
        let response = RevokeMachineKeyResponse { result: Err(error) };
        self.send_response_to_pid(
            client_pid,
            cap_slots,
            identity_machine::MSG_REVOKE_MACHINE_KEY_RESPONSE,
            &response,
        )
    }

    /// Send error response for rotate machine key (from message)
    fn send_rotate_machine_error_direct(
        &self,
        msg: &Message,
        error: KeyError,
    ) -> Result<(), AppError> {
        let response = RotateMachineKeyResponse { result: Err(error) };
        self.send_response_to_pid(
            msg.from_pid,
            &msg.cap_slots,
            identity_machine::MSG_ROTATE_MACHINE_KEY_RESPONSE,
            &response,
        )
    }

    /// Send error response for rotate machine key (to specific PID)
    fn send_rotate_machine_error_to_pid(
        &self,
        client_pid: u32,
        cap_slots: &[u32],
        error: KeyError,
    ) -> Result<(), AppError> {
        let response = RotateMachineKeyResponse { result: Err(error) };
        self.send_response_to_pid(
            client_pid,
            cap_slots,
            identity_machine::MSG_ROTATE_MACHINE_KEY_RESPONSE,
            &response,
        )
    }
}

impl ZeroApp for IdentityService {
    fn manifest() -> &'static AppManifest {
        &IDENTITY_SERVICE_MANIFEST
    }

    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError> {
        syscall::debug(&format!("IdentityService starting (PID {})", ctx.pid));

        // Register with init as "identity" service
        let service_name = "identity";
        let name_bytes = service_name.as_bytes();
        let mut data = Vec::with_capacity(1 + name_bytes.len() + 8);
        data.push(name_bytes.len() as u8);
        data.extend_from_slice(name_bytes);
        // Endpoint ID (placeholder - would be actual endpoint)
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());

        // Send to init's endpoint (slot 2 is typically init)
        let _ = syscall::send(syscall::INIT_ENDPOINT_SLOT, syscall::MSG_REGISTER_SERVICE, &data);
        self.registered = true;

        syscall::debug("IdentityService: Registered with init");

        Ok(())
    }

    fn update(&mut self, _ctx: &AppContext) -> ControlFlow {
        ControlFlow::Yield
    }

    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError> {
        syscall::debug(&format!(
            "IdentityService: Received message tag 0x{:x} from PID {}",
            msg.tag, msg.from_pid
        ));

        match msg.tag {
            MSG_STORAGE_RESULT => self.handle_storage_result(ctx, &msg),
            identity_key::MSG_GENERATE_NEURAL_KEY => self.handle_generate_neural_key(ctx, &msg),
            identity_key::MSG_RECOVER_NEURAL_KEY => self.handle_recover_neural_key(ctx, &msg),
            identity_key::MSG_GET_IDENTITY_KEY => self.handle_get_identity_key(ctx, &msg),
            identity_machine::MSG_CREATE_MACHINE_KEY => self.handle_create_machine_key(ctx, &msg),
            identity_machine::MSG_LIST_MACHINE_KEYS => self.handle_list_machine_keys(ctx, &msg),
            identity_machine::MSG_GET_MACHINE_KEY => self.handle_get_machine_key(ctx, &msg),
            identity_machine::MSG_REVOKE_MACHINE_KEY => self.handle_revoke_machine_key(ctx, &msg),
            identity_machine::MSG_ROTATE_MACHINE_KEY => self.handle_rotate_machine_key(ctx, &msg),
            _ => {
                syscall::debug(&format!(
                    "IdentityService: Unknown message tag 0x{:x} from PID {}",
                    msg.tag, msg.from_pid
                ));
                Ok(())
            }
        }
    }

    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("IdentityService: shutting down");
    }
}

// Entry point
app_main!(IdentityService);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("IdentityService is meant to run as WASM in Zero OS");
}
