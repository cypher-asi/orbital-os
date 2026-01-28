//! Error types for the Identity layer.

use alloc::string::String;
use serde::{Deserialize, Serialize};

/// Errors from user operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum UserError {
    /// User not found
    NotFound,
    /// User already exists
    AlreadyExists,
    /// Permission denied
    PermissionDenied,
    /// Storage error
    StorageError(String),
    /// Invalid display name
    InvalidDisplayName,
}

/// Errors from session operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SessionError {
    /// Session not found
    NotFound,
    /// Session has expired
    Expired,
    /// User not found
    UserNotFound,
    /// Remote authentication failed
    RemoteAuthFailed(String),
    /// Token refresh failed
    RefreshFailed(String),
    /// Storage error
    StorageError(String),
}

/// Errors from key operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum KeyError {
    /// Caller is not authorized to perform this operation
    Unauthorized,
    /// Invalid request format (JSON parse error, missing fields)
    InvalidRequest(String),
    /// User not found
    UserNotFound,
    /// Keys not found
    KeysNotFound,
    /// Identity key required but not registered
    IdentityKeyRequired,
    /// Identity key already exists (use rotate instead)
    IdentityKeyAlreadyExists,
    /// Machine key not found
    MachineKeyNotFound,
    /// Cannot revoke the primary/current machine key
    CannotRevokePrimaryMachine,
    /// Invalid passphrase
    InvalidPassphrase,
    /// Key derivation failed
    DerivationFailed,
    /// Encryption/decryption failed
    CryptoError(String),
    /// Storage error
    StorageError(String),
    /// Insufficient shards for recovery (need at least 3 of 5)
    InsufficientShards,
    /// Invalid shard data
    InvalidShard(String),
    /// Neural Key verification failed - reconstructed key doesn't match stored identity
    NeuralKeyMismatch,
    /// Password doesn't meet requirements (too short, etc.)
    InvalidPassword(String),
    /// Decryption failed - wrong password or corrupted data
    DecryptionFailed,
    /// No encrypted shards found for this user
    EncryptedShardsNotFound,
}

/// Errors from credential operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CredentialError {
    /// Caller is not authorized to perform this operation
    Unauthorized,
    /// Invalid request format (JSON parse error, missing fields)
    InvalidRequest(String),
    /// Credential already linked
    AlreadyLinked,
    /// Invalid credential format
    InvalidFormat,
    /// Verification failed
    VerificationFailed,
    /// Verification code expired
    CodeExpired,
    /// No pending verification for this email
    NoPendingVerification,
    /// Credential not found
    NotFound,
    /// Storage error
    StorageError(String),
}

/// Errors from ZID API operations.
///
/// These errors occur during machine key login and credential
/// attachment flows with the ZERO-ID remote server.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ZidError {
    /// Caller is not authorized to perform this operation
    Unauthorized,
    /// Invalid request format (JSON parse error, missing fields)
    InvalidRequest(String),
    /// Network error during API call
    NetworkError(String),
    /// Authentication failed (invalid signature, unknown machine)
    AuthenticationFailed,
    /// Challenge expired or invalid
    InvalidChallenge,
    /// Machine key not found locally
    MachineKeyNotFound,
    /// Machine not registered with ZID server (needs enrollment)
    MachineNotRegistered(String),
    /// Machine enrollment failed
    EnrollmentFailed(String),
    /// ZID server error (5xx response)
    ServerError(String),
    /// Email already registered to another account
    EmailAlreadyRegistered,
    /// Invalid email format
    InvalidEmailFormat,
    /// Password too weak (must meet complexity requirements)
    PasswordTooWeak,
    /// Session expired
    SessionExpired,
    /// Invalid or expired refresh token
    InvalidRefreshToken,
}

/// General identity layer error.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum IdentityError {
    /// User error
    User(UserError),
    /// Session error
    Session(SessionError),
    /// Key error
    Key(KeyError),
    /// Credential error
    Credential(CredentialError),
}

impl From<UserError> for IdentityError {
    fn from(e: UserError) -> Self {
        IdentityError::User(e)
    }
}

impl From<SessionError> for IdentityError {
    fn from(e: SessionError) -> Self {
        IdentityError::Session(e)
    }
}

impl From<KeyError> for IdentityError {
    fn from(e: KeyError) -> Self {
        IdentityError::Key(e)
    }
}

impl From<CredentialError> for IdentityError {
    fn from(e: CredentialError) -> Self {
        IdentityError::Credential(e)
    }
}
