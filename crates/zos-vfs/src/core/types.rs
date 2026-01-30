//! Core types for the VFS layer.
//!
//! Defines Inode, FilePermissions, and directory entry types.

use alloc::string::String;
use serde::{Deserialize, Serialize};

/// Serde helper for Option<u128> as hex string
///
/// Large u128 values can cause issues with JSON serialization/deserialization
/// when represented as decimal numbers. This helper serializes them as hex strings.
mod option_u128_hex {
    use alloc::format;
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<u128>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(v) => serializer.serialize_str(&format!("{:032x}", v)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<u128>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<&str> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => {
                // Handle both hex strings and legacy decimal numbers
                if s.chars().all(|c| c.is_ascii_hexdigit()) && s.len() <= 32 {
                    u128::from_str_radix(s, 16)
                        .map(Some)
                        .map_err(serde::de::Error::custom)
                } else {
                    // Try parsing as decimal for backwards compatibility
                    s.parse::<u128>()
                        .map(Some)
                        .map_err(serde::de::Error::custom)
                }
            }
            None => Ok(None),
        }
    }
}

/// A unique user identifier (UUID as 128-bit value).
pub type UserId = u128;

/// Virtual filesystem inode.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inode {
    /// Canonical path (primary key)
    pub path: String,

    /// Parent directory path
    pub parent_path: String,

    /// Entry name (filename or directory name)
    pub name: String,

    /// Type of inode
    pub inode_type: InodeType,

    /// Owner user ID (None = system owned)
    /// Serialized as hex string to avoid issues with large u128 values in JSON
    #[serde(with = "option_u128_hex")]
    pub owner_id: Option<UserId>,

    /// Access permissions
    pub permissions: FilePermissions,

    /// Creation timestamp (nanos since epoch)
    pub created_at: u64,

    /// Last modification timestamp
    pub modified_at: u64,

    /// Last access timestamp
    pub accessed_at: u64,

    /// Size in bytes (0 for directories)
    pub size: u64,

    /// Is the content encrypted?
    pub encrypted: bool,

    /// SHA-256 hash of content (files only)
    pub content_hash: Option<[u8; 32]>,
}

impl Inode {
    /// Create a new directory inode.
    pub fn new_directory(
        path: String,
        parent_path: String,
        name: String,
        owner_id: Option<UserId>,
        now: u64,
    ) -> Self {
        Self {
            path,
            parent_path,
            name,
            inode_type: InodeType::Directory,
            owner_id,
            permissions: FilePermissions::user_dir_default(),
            created_at: now,
            modified_at: now,
            accessed_at: now,
            size: 0,
            encrypted: false,
            content_hash: None,
        }
    }

    /// Create a new file inode.
    pub fn new_file(
        path: String,
        parent_path: String,
        name: String,
        owner_id: Option<UserId>,
        size: u64,
        content_hash: Option<[u8; 32]>,
        now: u64,
    ) -> Self {
        Self {
            path,
            parent_path,
            name,
            inode_type: InodeType::File,
            owner_id,
            permissions: FilePermissions::user_default(),
            created_at: now,
            modified_at: now,
            accessed_at: now,
            size,
            encrypted: false,
            content_hash,
        }
    }

    /// Check if this is a directory.
    pub fn is_directory(&self) -> bool {
        matches!(self.inode_type, InodeType::Directory)
    }

    /// Check if this is a file.
    pub fn is_file(&self) -> bool {
        matches!(self.inode_type, InodeType::File)
    }

    /// Check if this is a symlink.
    pub fn is_symlink(&self) -> bool {
        matches!(self.inode_type, InodeType::SymLink { .. })
    }
}

/// Type of filesystem entry.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum InodeType {
    /// Regular file
    File,

    /// Directory
    Directory,

    /// Symbolic link
    SymLink {
        /// Target path
        target: String,
    },
}

/// Unix-like file permissions.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilePermissions {
    /// Owner can read
    pub owner_read: bool,
    /// Owner can write
    pub owner_write: bool,
    /// Owner can execute
    pub owner_execute: bool,
    /// System processes can read
    pub system_read: bool,
    /// System processes can write
    pub system_write: bool,
    /// World (everyone) can read
    pub world_read: bool,
    /// World (everyone) can write
    pub world_write: bool,
}

impl Default for FilePermissions {
    fn default() -> Self {
        Self::user_default()
    }
}

impl FilePermissions {
    /// Default permissions for user files (owner rw, system r)
    pub fn user_default() -> Self {
        Self {
            owner_read: true,
            owner_write: true,
            owner_execute: false,
            system_read: true,
            system_write: false,
            world_read: false,
            world_write: false,
        }
    }

    /// Permissions for user directories (owner rwx, system rx)
    pub fn user_dir_default() -> Self {
        Self {
            owner_read: true,
            owner_write: true,
            owner_execute: true,
            system_read: true,
            system_write: false,
            world_read: false,
            world_write: false,
        }
    }

    /// System-only permissions (system rw)
    pub fn system_only() -> Self {
        Self {
            owner_read: false,
            owner_write: false,
            owner_execute: false,
            system_read: true,
            system_write: true,
            world_read: false,
            world_write: false,
        }
    }

    /// World-readable (owner rw, world r)
    pub fn world_readable() -> Self {
        Self {
            owner_read: true,
            owner_write: true,
            owner_execute: false,
            system_read: true,
            system_write: false,
            world_read: true,
            world_write: false,
        }
    }

    /// World read-write (for /tmp)
    pub fn world_rw() -> Self {
        Self {
            owner_read: true,
            owner_write: true,
            owner_execute: true,
            system_read: true,
            system_write: true,
            world_read: true,
            world_write: true,
        }
    }

    /// Read-only for everyone
    pub fn read_only() -> Self {
        Self {
            owner_read: true,
            owner_write: false,
            owner_execute: false,
            system_read: true,
            system_write: false,
            world_read: true,
            world_write: false,
        }
    }

    /// Identity directory permissions (owner rwx, system rw)
    ///
    /// Used for directories like `~/.zos/identity` where both the owner
    /// and system services (like IdentityService) need write access.
    pub fn identity_dir() -> Self {
        Self {
            owner_read: true,
            owner_write: true,
            owner_execute: true,
            system_read: true,
            system_write: true,
            world_read: false,
            world_write: false,
        }
    }
}

/// Directory entry returned by readdir.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DirEntry {
    /// Entry name
    pub name: String,

    /// Full path
    pub path: String,

    /// Is this a directory?
    pub is_directory: bool,

    /// Is this a symlink?
    pub is_symlink: bool,

    /// File size (0 for directories)
    pub size: u64,

    /// Last modified timestamp
    pub modified_at: u64,
}

impl From<&Inode> for DirEntry {
    fn from(inode: &Inode) -> Self {
        Self {
            name: inode.name.clone(),
            path: inode.path.clone(),
            is_directory: inode.is_directory(),
            is_symlink: inode.is_symlink(),
            size: inode.size,
            modified_at: inode.modified_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inode_types() {
        let dir = Inode::new_directory(
            String::from("/home/user"),
            String::from("/home"),
            String::from("user"),
            Some(1),
            1000,
        );
        assert!(dir.is_directory());
        assert!(!dir.is_file());

        let file = Inode::new_file(
            String::from("/home/user/file.txt"),
            String::from("/home/user"),
            String::from("file.txt"),
            Some(1),
            100,
            None,
            1000,
        );
        assert!(file.is_file());
        assert!(!file.is_directory());
    }

    #[test]
    fn test_permissions() {
        let user = FilePermissions::user_default();
        assert!(user.owner_read);
        assert!(user.owner_write);
        assert!(!user.world_read);

        let system = FilePermissions::system_only();
        assert!(!system.owner_read);
        assert!(system.system_read);
        assert!(system.system_write);

        let world = FilePermissions::world_rw();
        assert!(world.world_read);
        assert!(world.world_write);
    }

    #[test]
    fn test_dir_entry_from_inode() {
        let inode = Inode::new_file(
            String::from("/home/user/doc.txt"),
            String::from("/home/user"),
            String::from("doc.txt"),
            Some(1),
            500,
            None,
            2000,
        );

        let entry = DirEntry::from(&inode);
        assert_eq!(entry.name, "doc.txt");
        assert_eq!(entry.size, 500);
        assert!(!entry.is_directory);
    }
}
