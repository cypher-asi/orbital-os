//! Error types for the VFS layer.

use alloc::string::String;
use serde::{Deserialize, Serialize};

/// Errors from VFS operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum VfsError {
    /// Path not found
    NotFound,

    /// Path already exists
    AlreadyExists,

    /// Not a directory
    NotADirectory,

    /// Not a file
    NotAFile,

    /// Directory not empty
    DirectoryNotEmpty,

    /// Permission denied
    PermissionDenied,

    /// Invalid path format
    InvalidPath(String),

    /// Storage backend error
    StorageError(String),

    /// Quota exceeded
    QuotaExceeded,

    /// File too large
    FileTooLarge,

    /// Encryption error
    EncryptionError(String),

    /// Decryption error
    DecryptionError(String),

    /// I/O error
    IoError(String),
}

impl VfsError {
    /// Create a storage error with message.
    pub fn storage(msg: impl Into<String>) -> Self {
        Self::StorageError(msg.into())
    }

    /// Create an I/O error with message.
    pub fn io(msg: impl Into<String>) -> Self {
        Self::IoError(msg.into())
    }

    /// Create an invalid path error with message.
    pub fn invalid_path(msg: impl Into<String>) -> Self {
        Self::InvalidPath(msg.into())
    }
}

/// Errors from storage operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StorageError {
    /// Content not found
    NotFound,

    /// Database error
    DatabaseError(String),

    /// Serialization error
    SerializationError(String),

    /// Chunk missing
    ChunkMissing { path: String, index: u32 },

    /// Hash mismatch
    HashMismatch,
}

impl From<StorageError> for VfsError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::NotFound => VfsError::NotFound,
            StorageError::DatabaseError(msg) => VfsError::StorageError(msg),
            StorageError::SerializationError(msg) => VfsError::StorageError(msg),
            StorageError::ChunkMissing { path, index } => {
                VfsError::StorageError(alloc::format!("Chunk {} missing for {}", index, path))
            }
            StorageError::HashMismatch => {
                VfsError::StorageError(String::from("Content hash mismatch"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_construction() {
        let err = VfsError::storage("test error");
        match err {
            VfsError::StorageError(msg) => assert_eq!(msg, "test error"),
            _ => panic!("Expected StorageError"),
        }
    }

    #[test]
    fn test_storage_error_conversion() {
        let storage_err = StorageError::NotFound;
        let vfs_err: VfsError = storage_err.into();
        assert!(matches!(vfs_err, VfsError::NotFound));
    }
}
