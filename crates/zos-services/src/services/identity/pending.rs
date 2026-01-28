//! Pending operation types for async storage and network tracking
//!
//! The identity service uses async storage, keystore, and network syscalls.
//! This module defines the state tracking for pending operations awaiting results.
//!
//! # Storage Strategy
//!
//! - **VFS operations**: Used for directory structure under `/home/{user_id}/.zos/`
//! - **Keystore operations**: Used for cryptographic key data under `/keys/{user_id}/`

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use zos_identity::ipc::CreateMachineKeyRequest;
use zos_identity::ipc::{NeuralKeyGenerated, ZidTokens};
use zos_identity::keystore::{CredentialType, MachineKeyRecord};

/// Common context for all pending operations.
///
/// This struct captures the fields shared by most pending operations:
/// - `client_pid`: The PID of the client awaiting a response
/// - `cap_slots`: Capability slots for sending the response
///
/// Extracting these common fields reduces duplication in `PendingStorageOp`
/// and simplifies handler signatures from `(client_pid, cap_slots, ...)` to `(ctx, ...)`.
#[derive(Clone, Debug)]
pub struct RequestContext {
    /// PID of the client awaiting a response
    pub client_pid: u32,
    /// Capability slots for sending the response
    pub cap_slots: Vec<u32>,
}

impl RequestContext {
    /// Create a new request context
    pub fn new(client_pid: u32, cap_slots: Vec<u32>) -> Self {
        Self { client_pid, cap_slots }
    }
}

/// Tracks pending storage operations awaiting results.
///
/// Each variant captures the state needed to continue processing
/// once the async storage operation completes.
#[derive(Clone)]
pub enum PendingStorageOp {
    // =========================================================================
    // Neural Key operations
    // =========================================================================
    /// Check if identity directory exists before generating neural key
    CheckIdentityDirectory {
        ctx: RequestContext,
        user_id: u128,
        /// Password for encrypting shards (passed through the async flow)
        password: String,
    },
    /// Create identity directory structure
    CreateIdentityDirectory {
        ctx: RequestContext,
        user_id: u128,
        directories: Vec<String>,
        /// Password for encrypting shards (passed through the async flow)
        password: String,
    },
    /// Check if identity key exists (for generate)
    CheckKeyExists {
        ctx: RequestContext,
        user_id: u128,
        /// Password for encrypting shards (passed through the async flow)
        password: String,
    },
    /// Write identity key store (VFS handles inodes internally)
    WriteKeyStore {
        ctx: RequestContext,
        user_id: u128,
        result: NeuralKeyGenerated,
        json_bytes: Vec<u8>,
    },
    /// Get identity key for retrieval
    GetIdentityKey {
        ctx: RequestContext,
    },

    // =========================================================================
    // Neural Key recovery operations
    // =========================================================================
    /// Read existing identity for recovery verification (SECURITY)
    /// 
    /// Before reconstructing a Neural Key from shards, we must read the stored
    /// identity public key to verify the reconstruction matches. This prevents
    /// attacks where arbitrary shards could be used to reconstruct unauthorized keys.
    ReadIdentityForRecovery {
        ctx: RequestContext,
        user_id: u128,
        /// The parsed shards to use for reconstruction after verification
        zid_shards: Vec<zos_identity::crypto::ZidNeuralShard>,
    },
    /// Write recovered key store (VFS handles inodes internally)
    WriteRecoveredKeyStore {
        ctx: RequestContext,
        user_id: u128,
        result: NeuralKeyGenerated,
        json_bytes: Vec<u8>,
    },

    // =========================================================================
    // Machine Key operations
    // =========================================================================
    /// Read identity key (for create machine key - need stored pubkey for verification)
    ReadIdentityForMachine {
        ctx: RequestContext,
        request: CreateMachineKeyRequest,
    },
    /// Write machine key (VFS handles inodes internally)
    WriteMachineKey {
        ctx: RequestContext,
        user_id: u128,
        record: MachineKeyRecord,
        json_bytes: Vec<u8>,
    },
    /// List machine keys (storage list operation)
    ListMachineKeys {
        ctx: RequestContext,
        user_id: u128,
    },
    /// Read individual machine key record
    ReadMachineKey {
        ctx: RequestContext,
        user_id: u128,
        /// Remaining paths to read
        remaining_paths: Vec<String>,
        /// Collected records so far
        records: Vec<MachineKeyRecord>,
    },
    /// Delete machine key (VFS handles inodes internally)
    DeleteMachineKey {
        ctx: RequestContext,
        user_id: u128,
        machine_id: u128,
    },
    /// Read machine key for rotation
    ReadMachineForRotate {
        ctx: RequestContext,
        user_id: u128,
        machine_id: u128,
    },
    /// Write rotated machine key (VFS handles inodes internally)
    WriteRotatedMachineKey {
        ctx: RequestContext,
        user_id: u128,
        record: MachineKeyRecord,
        json_bytes: Vec<u8>,
    },
    /// Read single machine key by ID
    ReadSingleMachineKey {
        ctx: RequestContext,
    },

    // =========================================================================
    // Credential operations
    // =========================================================================
    /// Read credentials for attach email (to check if email already linked)
    ReadCredentialsForAttach {
        ctx: RequestContext,
        user_id: u128,
        email: String,
    },
    /// Get credentials (read credential store)
    GetCredentials {
        ctx: RequestContext,
    },
    /// Read credentials for unlink
    ReadCredentialsForUnlink {
        ctx: RequestContext,
        user_id: u128,
        credential_type: CredentialType,
    },
    /// Write unlinked credential (VFS handles inodes internally)
    WriteUnlinkedCredential {
        ctx: RequestContext,
        user_id: u128,
        json_bytes: Vec<u8>,
    },
    /// Write email credential after ZID success (VFS handles inodes internally)
    WriteEmailCredential {
        ctx: RequestContext,
        user_id: u128,
        json_bytes: Vec<u8>,
    },

    // =========================================================================
    // ZID session operations
    // =========================================================================
    /// Read machine key for ZID login (to get signing key)
    ReadMachineKeyForZidLogin {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
    },
    /// Write ZID session (VFS handles inodes internally)
    WriteZidSession {
        ctx: RequestContext,
        user_id: u128,
        tokens: ZidTokens,
        json_bytes: Vec<u8>,
    },
    /// Read machine key for ZID enrollment (to get public key for registration)
    ReadMachineKeyForZidEnroll {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
    },
    /// Write ZID session after enrollment (VFS handles inodes internally)
    WriteZidEnrollSession {
        ctx: RequestContext,
        user_id: u128,
        tokens: ZidTokens,
        json_bytes: Vec<u8>,
    },

    // =========================================================================
    // Identity Preferences operations
    // =========================================================================
    /// Read identity preferences from VFS
    ReadIdentityPreferences {
        ctx: RequestContext,
        user_id: u128,
    },
    /// Read preferences before updating (for set default key scheme)
    ReadPreferencesForUpdate {
        ctx: RequestContext,
        user_id: u128,
        new_key_scheme: zos_identity::KeyScheme,
    },
    /// Write updated preferences (VFS handles inodes internally)
    WritePreferences {
        ctx: RequestContext,
        user_id: u128,
        json_bytes: Vec<u8>,
    },
}

/// Tracks pending keystore operations awaiting results.
///
/// These operations communicate with KeystoreService (PID 7) for secure
/// cryptographic key storage, keeping keys isolated from the general filesystem.
#[derive(Clone)]
pub enum PendingKeystoreOp {
    // =========================================================================
    // Identity Key operations (stored in keystore)
    // =========================================================================
    /// Check if identity key exists (for generate)
    CheckKeyExists {
        ctx: RequestContext,
        user_id: u128,
        /// Password for encrypting shards (new field for encrypted shard flow)
        password: String,
    },
    /// Write identity key store to keystore
    WriteKeyStore {
        ctx: RequestContext,
        user_id: u128,
        result: NeuralKeyGenerated,
        json_bytes: Vec<u8>,
        /// Encrypted shard store to write after key store succeeds
        encrypted_shards_json: Vec<u8>,
    },
    /// Write encrypted shards after key store write succeeds
    WriteEncryptedShards {
        ctx: RequestContext,
        user_id: u128,
        result: NeuralKeyGenerated,
    },
    /// Best-effort rollback if encrypted shard write fails
    DeleteIdentityKeyAfterShardFailure {
        ctx: RequestContext,
        user_id: u128,
    },
    /// Get identity key from keystore
    GetIdentityKey {
        ctx: RequestContext,
    },
    /// Read existing identity for recovery verification (SECURITY)
    ReadIdentityForRecovery {
        ctx: RequestContext,
        user_id: u128,
        zid_shards: Vec<zos_identity::crypto::ZidNeuralShard>,
    },
    /// Write recovered key store to keystore
    WriteRecoveredKeyStore {
        ctx: RequestContext,
        user_id: u128,
        result: NeuralKeyGenerated,
        json_bytes: Vec<u8>,
    },

    // =========================================================================
    // Machine Key operations (stored in keystore)
    // =========================================================================
    /// Read identity key for create machine key
    ReadIdentityForMachine {
        ctx: RequestContext,
        request: CreateMachineKeyRequest,
    },
    /// Read encrypted shards for machine key creation
    ReadEncryptedShardsForMachine {
        ctx: RequestContext,
        request: CreateMachineKeyRequest,
        /// Stored identity public key for verification
        stored_identity_pubkey: [u8; 32],
    },
    /// Write machine key to keystore
    WriteMachineKey {
        ctx: RequestContext,
        user_id: u128,
        record: MachineKeyRecord,
        json_bytes: Vec<u8>,
    },
    /// List machine keys from keystore
    ListMachineKeys {
        ctx: RequestContext,
        user_id: u128,
    },
    /// Read individual machine key record from keystore
    ReadMachineKey {
        ctx: RequestContext,
        user_id: u128,
        remaining_paths: Vec<String>,
        records: Vec<MachineKeyRecord>,
    },
    /// Delete machine key from keystore
    DeleteMachineKey {
        ctx: RequestContext,
        user_id: u128,
        machine_id: u128,
    },
    /// Read machine key for rotation
    ReadMachineForRotate {
        ctx: RequestContext,
        user_id: u128,
        machine_id: u128,
    },
    /// Write rotated machine key to keystore
    WriteRotatedMachineKey {
        ctx: RequestContext,
        user_id: u128,
        record: MachineKeyRecord,
        json_bytes: Vec<u8>,
    },
    /// Read single machine key by ID
    ReadSingleMachineKey {
        ctx: RequestContext,
    },

    // =========================================================================
    // ZID session operations (stored in keystore)
    // =========================================================================
    /// Read machine key for ZID login
    ReadMachineKeyForZidLogin {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
    },
    /// Read machine key for ZID enrollment
    ReadMachineKeyForZidEnroll {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
    },
}

/// Tracks pending network operations awaiting results.
#[derive(Clone)]
pub enum PendingNetworkOp {
    /// Request challenge from ZID server (network request)
    RequestZidChallenge {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        /// Machine key for signing the challenge
        machine_key: Box<MachineKeyRecord>,
    },
    /// Submit signed challenge to ZID server (network request)
    SubmitZidLogin {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
    },
    /// Submit email credential to ZID server (network request)
    SubmitEmailToZid {
        ctx: RequestContext,
        user_id: u128,
        email: String,
    },
    /// Submit enrollment to ZID server (network request)
    SubmitZidEnroll {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        /// Derived identity ID for storage
        identity_id: u128,
        /// Derived machine ID for storage
        machine_id: u128,
        /// Identity signing public key (for storage)
        identity_signing_public_key: [u8; 32],
        /// Machine signing public key (for storage)
        machine_signing_public_key: [u8; 32],
        /// Machine encryption public key (for storage)
        machine_encryption_public_key: [u8; 32],
        /// Machine signing seed (for keypair reconstruction)
        machine_signing_sk: [u8; 32],
        /// Machine encryption seed (for keypair reconstruction)
        machine_encryption_sk: [u8; 32],
    },
    /// Request challenge after identity creation (for chained login)
    /// Identity creation returns {identity_id, machine_id, namespace_id} without tokens.
    /// We need to chain into login flow to get tokens.
    RequestZidChallengeAfterEnroll {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        /// Machine ID for storage (our local u128 ID)
        machine_id: u128,
        /// Identity signing public key (for storage)
        identity_signing_public_key: [u8; 32],
        /// Machine signing public key (for storage)
        machine_signing_public_key: [u8; 32],
        /// Machine encryption public key (for storage)
        machine_encryption_public_key: [u8; 32],
        /// Machine signing seed (for keypair reconstruction and signing challenge)
        machine_signing_sk: [u8; 32],
        /// Machine encryption seed (for keypair reconstruction)
        machine_encryption_sk: [u8; 32],
    },
    /// Submit login after identity creation (final step to get tokens)
    SubmitZidLoginAfterEnroll {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        /// Machine ID for storage
        machine_id: u128,
        /// Identity signing public key (for storage)
        identity_signing_public_key: [u8; 32],
        /// Machine signing public key (for storage)
        machine_signing_public_key: [u8; 32],
        /// Machine encryption public key (for storage)
        machine_encryption_public_key: [u8; 32],
        /// Machine signing seed (for storage)
        machine_signing_sk: [u8; 32],
        /// Machine encryption seed (for storage)
        machine_encryption_sk: [u8; 32],
    },
}
