//! IPC protocol definitions for the VFS layer.
//!
//! Defines message types for inter-process communication.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::error::VfsError;
use crate::storage::{StorageQuota, StorageUsage};
use crate::types::{DirEntry, FilePermissions, Inode, UserId};

/// VFS service IPC message types.
pub mod vfs_msg {
    // Directory Operations
    /// Create directory request
    pub const MSG_VFS_MKDIR: u32 = 0x8000;
    /// Create directory response
    pub const MSG_VFS_MKDIR_RESPONSE: u32 = 0x8001;
    /// Remove directory request
    pub const MSG_VFS_RMDIR: u32 = 0x8002;
    /// Remove directory response
    pub const MSG_VFS_RMDIR_RESPONSE: u32 = 0x8003;
    /// Read directory request
    pub const MSG_VFS_READDIR: u32 = 0x8004;
    /// Read directory response
    pub const MSG_VFS_READDIR_RESPONSE: u32 = 0x8005;

    // File Operations
    /// Write file request
    pub const MSG_VFS_WRITE: u32 = 0x8010;
    /// Write file response
    pub const MSG_VFS_WRITE_RESPONSE: u32 = 0x8011;
    /// Read file request
    pub const MSG_VFS_READ: u32 = 0x8012;
    /// Read file response
    pub const MSG_VFS_READ_RESPONSE: u32 = 0x8013;
    /// Delete file request
    pub const MSG_VFS_UNLINK: u32 = 0x8014;
    /// Delete file response
    pub const MSG_VFS_UNLINK_RESPONSE: u32 = 0x8015;
    /// Rename file request
    pub const MSG_VFS_RENAME: u32 = 0x8016;
    /// Rename file response
    pub const MSG_VFS_RENAME_RESPONSE: u32 = 0x8017;
    /// Copy file request
    pub const MSG_VFS_COPY: u32 = 0x8018;
    /// Copy file response
    pub const MSG_VFS_COPY_RESPONSE: u32 = 0x8019;

    // Metadata Operations
    /// Stat request
    pub const MSG_VFS_STAT: u32 = 0x8020;
    /// Stat response
    pub const MSG_VFS_STAT_RESPONSE: u32 = 0x8021;
    /// Exists request
    pub const MSG_VFS_EXISTS: u32 = 0x8022;
    /// Exists response
    pub const MSG_VFS_EXISTS_RESPONSE: u32 = 0x8023;
    /// Change permissions request
    pub const MSG_VFS_CHMOD: u32 = 0x8024;
    /// Change permissions response
    pub const MSG_VFS_CHMOD_RESPONSE: u32 = 0x8025;
    /// Change owner request
    pub const MSG_VFS_CHOWN: u32 = 0x8026;
    /// Change owner response
    pub const MSG_VFS_CHOWN_RESPONSE: u32 = 0x8027;

    // Quota Operations
    /// Get usage request
    pub const MSG_VFS_GET_USAGE: u32 = 0x8030;
    /// Get usage response
    pub const MSG_VFS_GET_USAGE_RESPONSE: u32 = 0x8031;
    /// Get quota request
    pub const MSG_VFS_GET_QUOTA: u32 = 0x8032;
    /// Get quota response
    pub const MSG_VFS_GET_QUOTA_RESPONSE: u32 = 0x8033;
}

// ============================================================================
// Directory Request/Response Types
// ============================================================================

/// Create directory request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MkdirRequest {
    /// Path to create
    pub path: String,
    /// Create parent directories if needed
    pub create_parents: bool,
}

/// Create directory response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MkdirResponse {
    /// Result of operation
    pub result: Result<(), VfsError>,
}

/// Remove directory request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RmdirRequest {
    /// Path to remove
    pub path: String,
    /// Remove recursively
    pub recursive: bool,
}

/// Remove directory response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RmdirResponse {
    /// Result of operation
    pub result: Result<(), VfsError>,
}

/// Read directory request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReaddirRequest {
    /// Directory path to read
    pub path: String,
}

/// Read directory response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReaddirResponse {
    /// Result containing directory entries or error
    pub result: Result<Vec<DirEntry>, VfsError>,
}

// ============================================================================
// File Request/Response Types
// ============================================================================

/// Write file request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WriteFileRequest {
    /// File path
    pub path: String,
    /// File content
    pub content: Vec<u8>,
    /// Encrypt the file
    pub encrypt: bool,
}

/// Write file response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WriteFileResponse {
    /// Result of operation
    pub result: Result<(), VfsError>,
}

/// Read file request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadFileRequest {
    /// File path
    pub path: String,
    /// Offset to start reading from (None = start)
    pub offset: Option<u64>,
    /// Number of bytes to read (None = all)
    pub length: Option<u64>,
}

/// Read file response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadFileResponse {
    /// Result containing file content or error
    pub result: Result<Vec<u8>, VfsError>,
}

/// Delete file request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnlinkRequest {
    /// File path to delete
    pub path: String,
}

/// Delete file response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnlinkResponse {
    /// Result of operation
    pub result: Result<(), VfsError>,
}

/// Rename request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenameRequest {
    /// Source path
    pub from: String,
    /// Destination path
    pub to: String,
}

/// Rename response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenameResponse {
    /// Result of operation
    pub result: Result<(), VfsError>,
}

/// Copy file request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CopyRequest {
    /// Source path
    pub from: String,
    /// Destination path
    pub to: String,
}

/// Copy response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CopyResponse {
    /// Result of operation
    pub result: Result<(), VfsError>,
}

// ============================================================================
// Metadata Request/Response Types
// ============================================================================

/// Stat request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatRequest {
    /// Path to stat
    pub path: String,
}

/// Stat response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatResponse {
    /// Result containing inode or error
    pub result: Result<Inode, VfsError>,
}

/// Exists request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExistsRequest {
    /// Path to check
    pub path: String,
}

/// Exists response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExistsResponse {
    /// Whether the path exists
    pub exists: bool,
}

/// Change permissions request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChmodRequest {
    /// Path to modify
    pub path: String,
    /// New permissions
    pub permissions: FilePermissions,
}

/// Change permissions response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChmodResponse {
    /// Result of operation
    pub result: Result<(), VfsError>,
}

/// Change owner request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChownRequest {
    /// Path to modify
    pub path: String,
    /// New owner (None = system)
    pub owner_id: Option<UserId>,
}

/// Change owner response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChownResponse {
    /// Result of operation
    pub result: Result<(), VfsError>,
}

// ============================================================================
// Quota Request/Response Types
// ============================================================================

/// Get usage request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetUsageRequest {
    /// Path to get usage for
    pub path: String,
}

/// Get usage response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetUsageResponse {
    /// Result containing usage stats or error
    pub result: Result<StorageUsage, VfsError>,
}

/// Get quota request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetQuotaRequest {
    /// User ID to get quota for
    pub user_id: UserId,
}

/// Get quota response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetQuotaResponse {
    /// Result containing quota or error
    pub result: Result<StorageQuota, VfsError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_constants() {
        // Ensure VFS messages are in the 0x8000 range
        assert!(vfs_msg::MSG_VFS_MKDIR >= 0x8000);
        assert!(vfs_msg::MSG_VFS_GET_QUOTA_RESPONSE < 0x9000);

        // Ensure no overlap with identity messages (0x7000 range)
        assert!(vfs_msg::MSG_VFS_MKDIR > 0x7FFF);
    }

    #[test]
    fn test_request_creation() {
        let req = MkdirRequest {
            path: String::from("/home/user/Documents"),
            create_parents: true,
        };
        assert_eq!(req.path, "/home/user/Documents");
        assert!(req.create_parents);
    }
}
