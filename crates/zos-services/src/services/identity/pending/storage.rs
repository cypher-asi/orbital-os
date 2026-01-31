//! VFS storage pending operations

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use zos_identity::ipc::{NeuralKeyGenerated, ZidTokens};
use zos_identity::keystore::{CredentialType, MachineKeyRecord};

use super::RequestContext;

/// The type of VFS response expected by a pending operation.
///
/// This is used to match VFS responses to pending operations correctly
/// when multiple operations of different types are in flight concurrently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedVfsResponse {
    Read,
    Write,
    Exists,
    Mkdir,
    Readdir,
    Unlink,
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
    /// Create identity directory structure (legacy - one directory at a time)
    CreateIdentityDirectory {
        ctx: RequestContext,
        user_id: u128,
        directories: Vec<String>,
        /// Password for encrypting shards (passed through the async flow)
        password: String,
    },
    /// Create identity directory structure complete (uses create_parents=true)
    CreateIdentityDirectoryComplete {
        ctx: RequestContext,
        user_id: u128,
        /// Password for encrypting shards (passed through the async flow)
        password: String,
    },
    /// Create VFS directory for derived user_id after neural key generation
    /// This is needed because the directory was originally created with the
    /// temporary user_id, but we need it at the derived user_id path for
    /// preferences and other VFS operations.
    CreateDerivedUserDirectory {
        ctx: RequestContext,
        /// The derived user_id (deterministic identity based on crypto key)
        derived_user_id: u128,
        /// The neural key generation result to return after directory creation
        result: NeuralKeyGenerated,
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
        request: zos_identity::ipc::CreateMachineKeyRequest,
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
    /// Create credentials directory for existing users (on-demand)
    CreateCredentialsDirectory {
        ctx: RequestContext,
        user_id: u128,
        json_bytes: Vec<u8>,
    },
    /// Retry writing email credential after directory creation
    WriteEmailCredentialRetry {
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
    /// Delete ZID session from VFS (logout)
    DeleteZidSession {
        ctx: RequestContext,
    },
    /// Write ZID session after email login (VFS handles inodes internally)
    WriteZidEmailLoginSession {
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
    /// Read preferences before updating default machine key
    ReadPreferencesForDefaultMachine {
        ctx: RequestContext,
        user_id: u128,
        new_default_machine_id: u128,
    },
    /// Write updated preferences with new default machine key
    WritePreferencesForDefaultMachine {
        ctx: RequestContext,
        user_id: u128,
        json_bytes: Vec<u8>,
    },
    /// Create identity directory for preferences write (on-demand when directory doesn't exist)
    CreateIdentityDirForPreferences {
        ctx: RequestContext,
        user_id: u128,
        json_bytes: Vec<u8>,
    },
    /// Retry writing preferences after directory creation
    WritePreferencesForDefaultMachineRetry {
        ctx: RequestContext,
        user_id: u128,
        json_bytes: Vec<u8>,
    },
    /// Read preferences before ZID login to get default_machine_id
    ReadPreferencesForZidLogin {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
    },

    // =========================================================================
    // ZID Token Refresh operations
    // =========================================================================
    /// Read ZID session for token refresh (to get refresh_token, session_id, machine_id)
    /// If `refresh_token_override` is provided (from frontend), use it instead of VFS token
    /// to prevent race conditions when VFS hasn't been updated yet.
    ReadZidSessionForRefresh {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        /// Optional refresh token from frontend - if Some, use instead of VFS session token
        refresh_token_override: Option<String>,
    },
    /// Write refreshed ZID session (with new tokens)
    WriteRefreshedZidSession {
        ctx: RequestContext,
        user_id: u128,
        tokens: ZidTokens,
        json_bytes: Vec<u8>,
    },
}

impl PendingStorageOp {
    /// Returns the type of VFS response this operation expects.
    ///
    /// This enables matching VFS responses to the correct pending operation
    /// when multiple operations of different types are in flight concurrently.
    pub fn expected_response(&self) -> ExpectedVfsResponse {
        match self {
            // EXISTS response operations
            PendingStorageOp::CheckIdentityDirectory { .. } |
            PendingStorageOp::CheckKeyExists { .. } => ExpectedVfsResponse::Exists,

            // MKDIR response operations
            PendingStorageOp::CreateIdentityDirectory { .. } |
            PendingStorageOp::CreateIdentityDirectoryComplete { .. } |
            PendingStorageOp::CreateDerivedUserDirectory { .. } |
            PendingStorageOp::CreateCredentialsDirectory { .. } |
            PendingStorageOp::CreateIdentityDirForPreferences { .. } => ExpectedVfsResponse::Mkdir,

            // READ response operations
            PendingStorageOp::GetIdentityKey { .. } |
            PendingStorageOp::ReadIdentityForRecovery { .. } |
            PendingStorageOp::ReadIdentityForMachine { .. } |
            PendingStorageOp::ReadMachineKey { .. } |
            PendingStorageOp::ReadMachineForRotate { .. } |
            PendingStorageOp::ReadSingleMachineKey { .. } |
            PendingStorageOp::ReadCredentialsForAttach { .. } |
            PendingStorageOp::GetCredentials { .. } |
            PendingStorageOp::ReadCredentialsForUnlink { .. } |
            PendingStorageOp::ReadMachineKeyForZidLogin { .. } |
            PendingStorageOp::ReadMachineKeyForZidEnroll { .. } |
            PendingStorageOp::ReadIdentityPreferences { .. } |
            PendingStorageOp::ReadPreferencesForUpdate { .. } |
            PendingStorageOp::ReadPreferencesForDefaultMachine { .. } |
            PendingStorageOp::ReadPreferencesForZidLogin { .. } |
            PendingStorageOp::ReadZidSessionForRefresh { .. } => ExpectedVfsResponse::Read,

            // WRITE response operations
            PendingStorageOp::WriteKeyStore { .. } |
            PendingStorageOp::WriteRecoveredKeyStore { .. } |
            PendingStorageOp::WriteMachineKey { .. } |
            PendingStorageOp::WriteRotatedMachineKey { .. } |
            PendingStorageOp::WriteUnlinkedCredential { .. } |
            PendingStorageOp::WriteEmailCredential { .. } |
            PendingStorageOp::WriteEmailCredentialRetry { .. } |
            PendingStorageOp::WriteZidSession { .. } |
            PendingStorageOp::WriteZidEnrollSession { .. } |
            PendingStorageOp::WriteZidEmailLoginSession { .. } |
            PendingStorageOp::WritePreferences { .. } |
            PendingStorageOp::WritePreferencesForDefaultMachine { .. } |
            PendingStorageOp::WritePreferencesForDefaultMachineRetry { .. } |
            PendingStorageOp::WriteRefreshedZidSession { .. } => ExpectedVfsResponse::Write,

            // READDIR response operations
            PendingStorageOp::ListMachineKeys { .. } => ExpectedVfsResponse::Readdir,

            // UNLINK response operations
            PendingStorageOp::DeleteMachineKey { .. } |
            PendingStorageOp::DeleteZidSession { .. } => ExpectedVfsResponse::Unlink,
        }
    }
}
