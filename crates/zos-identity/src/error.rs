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
    /// User not found
    UserNotFound,
    /// Keys not found
    KeysNotFound,
    /// Invalid passphrase
    InvalidPassphrase,
    /// Key derivation failed
    DerivationFailed,
    /// Encryption/decryption failed
    CryptoError(String),
    /// Storage error
    StorageError(String),
}

/// Errors from credential operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CredentialError {
    /// Credential already linked
    AlreadyLinked,
    /// Invalid credential format
    InvalidFormat,
    /// Verification failed
    VerificationFailed,
    /// Storage error
    StorageError(String),
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
