//! Pending operation types for async storage and network tracking
//!
//! The identity service uses async storage and network syscalls. This module
//! defines the state tracking for pending operations awaiting results.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use zos_identity::ipc::CreateMachineKeyRequest;
use zos_identity::ipc::{NeuralKeyGenerated, ZidTokens};
use zos_identity::keystore::{CredentialType, MachineKeyRecord};

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
        client_pid: u32,
        user_id: u128,
        cap_slots: Vec<u32>,
    },
    /// Create identity directory structure
    CreateIdentityDirectory {
        client_pid: u32,
        user_id: u128,
        cap_slots: Vec<u32>,
        directories: Vec<String>,
    },
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

    // =========================================================================
    // Neural Key recovery operations
    // =========================================================================
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

    // =========================================================================
    // Machine Key operations
    // =========================================================================
    /// Read identity key (for create machine key - need stored pubkey for verification)
    ReadIdentityForMachine {
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

    // =========================================================================
    // Credential operations
    // =========================================================================
    /// Read credentials for attach email (to check if email already linked)
    ReadCredentialsForAttach {
        client_pid: u32,
        user_id: u128,
        email: String,
        cap_slots: Vec<u32>,
    },
    /// Get credentials (read credential store)
    GetCredentials {
        client_pid: u32,
        cap_slots: Vec<u32>,
    },
    /// Read credentials for unlink
    ReadCredentialsForUnlink {
        client_pid: u32,
        user_id: u128,
        credential_type: CredentialType,
        cap_slots: Vec<u32>,
    },
    /// Write unlinked credential content (step 1)
    WriteUnlinkedCredentialContent {
        client_pid: u32,
        user_id: u128,
        json_bytes: Vec<u8>,
        cap_slots: Vec<u32>,
    },
    /// Write unlinked credential inode (step 2)
    WriteUnlinkedCredentialInode {
        client_pid: u32,
        cap_slots: Vec<u32>,
    },
    /// Write email credential content after ZID success (step 1)
    WriteEmailCredentialContent {
        client_pid: u32,
        user_id: u128,
        json_bytes: Vec<u8>,
        cap_slots: Vec<u32>,
    },
    /// Write email credential inode after ZID success (step 2)
    WriteEmailCredentialInode {
        client_pid: u32,
        cap_slots: Vec<u32>,
    },

    // =========================================================================
    // ZID session operations
    // =========================================================================
    /// Read machine key for ZID login (to get signing key)
    ReadMachineKeyForZidLogin {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        cap_slots: Vec<u32>,
    },
    /// Write ZID session content (step 1 - then write inode)
    WriteZidSessionContent {
        client_pid: u32,
        user_id: u128,
        tokens: ZidTokens,
        json_bytes: Vec<u8>,
        cap_slots: Vec<u32>,
    },
    /// Write ZID session inode (step 2 - then send response)
    WriteZidSessionInode {
        client_pid: u32,
        tokens: ZidTokens,
        cap_slots: Vec<u32>,
    },
    /// Read machine key for ZID enrollment (to get public key for registration)
    ReadMachineKeyForZidEnroll {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        cap_slots: Vec<u32>,
    },
    /// Write ZID session after enrollment (step 1 - content)
    WriteZidEnrollSessionContent {
        client_pid: u32,
        user_id: u128,
        tokens: ZidTokens,
        json_bytes: Vec<u8>,
        cap_slots: Vec<u32>,
    },
    /// Write ZID session after enrollment (step 2 - inode)
    WriteZidEnrollSessionInode {
        client_pid: u32,
        tokens: ZidTokens,
        cap_slots: Vec<u32>,
    },

    // =========================================================================
    // Identity Preferences operations
    // =========================================================================
    /// Read identity preferences from VFS
    ReadIdentityPreferences {
        client_pid: u32,
        user_id: u128,
        cap_slots: Vec<u32>,
    },
    /// Read preferences before updating (for set default key scheme)
    ReadPreferencesForUpdate {
        client_pid: u32,
        user_id: u128,
        new_key_scheme: zos_identity::KeyScheme,
        cap_slots: Vec<u32>,
    },
    /// Write updated preferences content
    WritePreferencesContent {
        client_pid: u32,
        user_id: u128,
        json_bytes: Vec<u8>,
        cap_slots: Vec<u32>,
    },
    /// Write preferences inode (final step)
    WritePreferencesInode {
        client_pid: u32,
        cap_slots: Vec<u32>,
    },
}

/// Tracks pending network operations awaiting results.
#[derive(Clone)]
pub enum PendingNetworkOp {
    /// Request challenge from ZID server (network request)
    RequestZidChallenge {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        /// Machine key for signing the challenge
        machine_key: Box<MachineKeyRecord>,
        cap_slots: Vec<u32>,
    },
    /// Submit signed challenge to ZID server (network request)
    SubmitZidLogin {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        cap_slots: Vec<u32>,
    },
    /// Submit email credential to ZID server (network request)
    SubmitEmailToZid {
        client_pid: u32,
        user_id: u128,
        email: String,
        cap_slots: Vec<u32>,
    },
    /// Submit enrollment to ZID server (network request)
    SubmitZidEnroll {
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        cap_slots: Vec<u32>,
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
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        cap_slots: Vec<u32>,
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
        client_pid: u32,
        user_id: u128,
        zid_endpoint: String,
        cap_slots: Vec<u32>,
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
