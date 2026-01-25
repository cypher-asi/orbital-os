//! Service trait for the VFS layer.
//!
//! Defines the interface for filesystem operations.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::VfsError;
use crate::storage::{StorageQuota, StorageUsage};
use crate::types::{DirEntry, FilePermissions, Inode, UserId};

/// Virtual filesystem service interface.
pub trait VfsService {
    // ========== Directory Operations ==========

    /// Create a directory.
    fn mkdir(&self, path: &str) -> Result<(), VfsError>;

    /// Create a directory and all parent directories.
    fn mkdir_p(&self, path: &str) -> Result<(), VfsError>;

    /// Remove an empty directory.
    fn rmdir(&self, path: &str) -> Result<(), VfsError>;

    /// Remove a directory and all contents recursively.
    fn rmdir_recursive(&self, path: &str) -> Result<(), VfsError>;

    /// List directory contents.
    fn readdir(&self, path: &str) -> Result<Vec<DirEntry>, VfsError>;

    // ========== File Operations ==========

    /// Write a file (create or overwrite).
    fn write_file(&self, path: &str, content: &[u8]) -> Result<(), VfsError>;

    /// Write an encrypted file.
    fn write_file_encrypted(
        &self,
        path: &str,
        content: &[u8],
        key: &[u8; 32],
    ) -> Result<(), VfsError>;

    /// Read a file.
    fn read_file(&self, path: &str) -> Result<Vec<u8>, VfsError>;

    /// Read an encrypted file.
    fn read_file_encrypted(&self, path: &str, key: &[u8; 32]) -> Result<Vec<u8>, VfsError>;

    /// Delete a file.
    fn unlink(&self, path: &str) -> Result<(), VfsError>;

    /// Rename/move a file or directory.
    fn rename(&self, from: &str, to: &str) -> Result<(), VfsError>;

    /// Copy a file.
    fn copy(&self, from: &str, to: &str) -> Result<(), VfsError>;

    // ========== Metadata Operations ==========

    /// Get file/directory metadata.
    fn stat(&self, path: &str) -> Result<Inode, VfsError>;

    /// Check if a path exists.
    fn exists(&self, path: &str) -> Result<bool, VfsError>;

    /// Change permissions.
    fn chmod(&self, path: &str, perms: FilePermissions) -> Result<(), VfsError>;

    /// Change ownership.
    fn chown(&self, path: &str, owner_id: Option<UserId>) -> Result<(), VfsError>;

    // ========== Symlink Operations ==========

    /// Create a symbolic link.
    fn symlink(&self, target: &str, link_path: &str) -> Result<(), VfsError>;

    /// Read a symbolic link target.
    fn readlink(&self, path: &str) -> Result<String, VfsError>;

    // ========== Path Utilities ==========

    /// Get user home directory path.
    fn get_home_dir(&self, user_id: UserId) -> String {
        alloc::format!("/home/{}", user_id)
    }

    /// Get user's .zos directory path.
    fn get_zos_dir(&self, user_id: UserId) -> String {
        alloc::format!("/home/{}/.zos", user_id)
    }

    /// Resolve a path (follow symlinks, normalize).
    fn resolve_path(&self, path: &str) -> Result<String, VfsError>;

    // ========== Quota Operations ==========

    /// Get storage usage for a path subtree.
    fn get_usage(&self, path: &str) -> Result<StorageUsage, VfsError>;

    /// Get quota for a user.
    fn get_quota(&self, user_id: UserId) -> Result<StorageQuota, VfsError>;

    /// Set quota for a user.
    fn set_quota(&self, user_id: UserId, max_bytes: u64) -> Result<(), VfsError>;
}

/// Process classification for permission checking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessClass {
    /// System processes (init, terminal, etc.)
    System,
    /// Runtime services (storage, network, identity, etc.)
    Runtime,
    /// User applications
    Application,
}

/// Permission check context.
pub struct PermissionContext {
    /// Calling user ID (if authenticated)
    pub user_id: Option<UserId>,
    /// Process classification
    pub process_class: ProcessClass,
}

impl PermissionContext {
    /// Create a system context.
    pub fn system() -> Self {
        Self {
            user_id: None,
            process_class: ProcessClass::System,
        }
    }

    /// Create a user context.
    pub fn user(user_id: UserId) -> Self {
        Self {
            user_id: Some(user_id),
            process_class: ProcessClass::Application,
        }
    }
}

/// Check if a context has read permission on an inode.
pub fn check_read(inode: &Inode, ctx: &PermissionContext) -> bool {
    // System processes check system_read
    if ctx.process_class == ProcessClass::System || ctx.process_class == ProcessClass::Runtime {
        return inode.permissions.system_read;
    }

    // Owner check
    if let Some(user_id) = ctx.user_id {
        if inode.owner_id == Some(user_id) {
            return inode.permissions.owner_read;
        }
    }

    // World check
    inode.permissions.world_read
}

/// Check if a context has write permission on an inode.
pub fn check_write(inode: &Inode, ctx: &PermissionContext) -> bool {
    // System processes check system_write
    if ctx.process_class == ProcessClass::System || ctx.process_class == ProcessClass::Runtime {
        return inode.permissions.system_write;
    }

    // Owner check
    if let Some(user_id) = ctx.user_id {
        if inode.owner_id == Some(user_id) {
            return inode.permissions.owner_write;
        }
    }

    // World check
    inode.permissions.world_write
}

/// Check if a context has execute (traverse) permission on a directory.
pub fn check_execute(inode: &Inode, ctx: &PermissionContext) -> bool {
    if !inode.is_directory() {
        return false;
    }

    // System processes always have traverse
    if ctx.process_class == ProcessClass::System || ctx.process_class == ProcessClass::Runtime {
        return true;
    }

    // Owner check
    if let Some(user_id) = ctx.user_id {
        if inode.owner_id == Some(user_id) {
            return inode.permissions.owner_execute;
        }
    }

    // For directories, world_read implies traverse
    inode.permissions.world_read
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FilePermissions;

    #[test]
    fn test_permission_check_read() {
        let inode = Inode::new_file(
            alloc::string::String::from("/test"),
            alloc::string::String::from("/"),
            alloc::string::String::from("test"),
            Some(1),
            100,
            None,
            1000,
        );

        // Owner can read
        let owner_ctx = PermissionContext::user(1);
        assert!(check_read(&inode, &owner_ctx));

        // Non-owner cannot read (no world read)
        let other_ctx = PermissionContext::user(2);
        assert!(!check_read(&inode, &other_ctx));

        // System can read
        let system_ctx = PermissionContext::system();
        assert!(check_read(&inode, &system_ctx));
    }

    #[test]
    fn test_permission_check_write() {
        let mut inode = Inode::new_file(
            alloc::string::String::from("/tmp/test"),
            alloc::string::String::from("/tmp"),
            alloc::string::String::from("test"),
            Some(1),
            100,
            None,
            1000,
        );
        inode.permissions = FilePermissions::world_rw();

        // Owner can write
        let owner_ctx = PermissionContext::user(1);
        assert!(check_write(&inode, &owner_ctx));

        // World can write (world_rw permissions)
        let other_ctx = PermissionContext::user(2);
        assert!(check_write(&inode, &other_ctx));
    }
}
