# Virtual Filesystem

> Hierarchical filesystem abstraction with inodes, paths, and permissions.

## Overview

The Virtual Filesystem (VFS) provides a Unix-like hierarchical filesystem abstraction over IndexedDB. It supports:

1. **Hierarchical paths**: `/home/user/Documents/file.txt`
2. **Inodes**: Metadata records for files and directories
3. **Permissions**: Owner/system/world access control
4. **Symbolic links**: Path indirection
5. **Directory operations**: mkdir, rmdir, readdir

## Data Structures

### Inode

```rust
use uuid::Uuid;
use serde::{Serialize, Deserialize};

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
    pub owner_id: Option<Uuid>,
    
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
```

### InodeType

```rust
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
        target: String 
    },
}
```

### FilePermissions

```rust
/// Unix-like file permissions.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
}
```

### DirEntry

```rust
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
```

## VFS Service Trait

```rust
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
    fn write_file_encrypted(&self, path: &str, content: &[u8], key: &[u8; 32]) -> Result<(), VfsError>;
    
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
    fn chown(&self, path: &str, owner_id: Option<Uuid>) -> Result<(), VfsError>;
    
    // ========== Symlink Operations ==========
    
    /// Create a symbolic link.
    fn symlink(&self, target: &str, link_path: &str) -> Result<(), VfsError>;
    
    /// Read a symbolic link target.
    fn readlink(&self, path: &str) -> Result<String, VfsError>;
    
    // ========== Path Utilities ==========
    
    /// Get user home directory path.
    fn get_home_dir(&self, user_id: Uuid) -> String;
    
    /// Get user's .zos directory path.
    fn get_zos_dir(&self, user_id: Uuid) -> String;
    
    /// Resolve a path (follow symlinks, normalize).
    fn resolve_path(&self, path: &str) -> Result<String, VfsError>;
    
    // ========== Quota Operations ==========
    
    /// Get storage usage for a path subtree.
    fn get_usage(&self, path: &str) -> Result<StorageUsage, VfsError>;
    
    /// Get quota for a user.
    fn get_quota(&self, user_id: Uuid) -> Result<StorageQuota, VfsError>;
}
```

### VfsError

```rust
/// Errors from VFS operations.
#[derive(Clone, Debug)]
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
    
    /// Invalid path
    InvalidPath(String),
    
    /// Quota exceeded
    QuotaExceeded,
    
    /// Symlink loop detected
    SymlinkLoop,
    
    /// Storage backend error
    StorageError(String),
    
    /// Encryption error
    EncryptionError(String),
}
```

## Path Resolution

### Canonicalization

```rust
impl VfsServiceImpl {
    /// Canonicalize a path (resolve . and .., normalize separators).
    fn canonicalize(&self, path: &str) -> Result<String, VfsError> {
        if path.is_empty() {
            return Err(VfsError::InvalidPath("Empty path".into()));
        }
        
        // Must be absolute
        if !path.starts_with('/') {
            return Err(VfsError::InvalidPath("Path must be absolute".into()));
        }
        
        let mut components = Vec::new();
        
        for component in path.split('/') {
            match component {
                "" | "." => continue,
                ".." => {
                    if components.is_empty() {
                        return Err(VfsError::InvalidPath("Path escapes root".into()));
                    }
                    components.pop();
                }
                c => components.push(c),
            }
        }
        
        if components.is_empty() {
            Ok("/".to_string())
        } else {
            Ok(format!("/{}", components.join("/")))
        }
    }
    
    /// Resolve symlinks (with loop detection).
    fn resolve_symlinks(&self, path: &str, depth: u32) -> Result<String, VfsError> {
        const MAX_DEPTH: u32 = 40;
        
        if depth > MAX_DEPTH {
            return Err(VfsError::SymlinkLoop);
        }
        
        let canonical = self.canonicalize(path)?;
        
        // Check if path exists and is a symlink
        match self.stat(&canonical) {
            Ok(inode) => {
                if let InodeType::SymLink { target } = &inode.inode_type {
                    // Resolve relative to symlink's parent
                    let parent = self.parent_path(&canonical);
                    let resolved = if target.starts_with('/') {
                        target.clone()
                    } else {
                        format!("{}/{}", parent, target)
                    };
                    self.resolve_symlinks(&resolved, depth + 1)
                } else {
                    Ok(canonical)
                }
            }
            Err(VfsError::NotFound) => Ok(canonical),
            Err(e) => Err(e),
        }
    }
    
    fn parent_path(&self, path: &str) -> String {
        match path.rfind('/') {
            Some(0) => "/".to_string(),
            Some(pos) => path[..pos].to_string(),
            None => "/".to_string(),
        }
    }
}
```

## Permission Checking

```rust
impl VfsServiceImpl {
    /// Check if caller has read permission.
    fn check_read(&self, inode: &Inode, caller_user: Option<Uuid>, caller_class: ProcessClass) -> bool {
        // System processes always have read access
        if caller_class == ProcessClass::System || caller_class == ProcessClass::Runtime {
            return inode.permissions.system_read;
        }
        
        // Owner check
        if let Some(user_id) = caller_user {
            if inode.owner_id == Some(user_id) {
                return inode.permissions.owner_read;
            }
        }
        
        // World check
        inode.permissions.world_read
    }
    
    /// Check if caller has write permission.
    fn check_write(&self, inode: &Inode, caller_user: Option<Uuid>, caller_class: ProcessClass) -> bool {
        // System processes check system_write
        if caller_class == ProcessClass::System || caller_class == ProcessClass::Runtime {
            return inode.permissions.system_write;
        }
        
        // Owner check
        if let Some(user_id) = caller_user {
            if inode.owner_id == Some(user_id) {
                return inode.permissions.owner_write;
            }
        }
        
        // World check
        inode.permissions.world_write
    }
    
    /// Check directory execute (traverse) permission.
    fn check_execute(&self, inode: &Inode, caller_user: Option<Uuid>, caller_class: ProcessClass) -> bool {
        if inode.inode_type != InodeType::Directory {
            return false;
        }
        
        // System processes always have traverse
        if caller_class == ProcessClass::System || caller_class == ProcessClass::Runtime {
            return true;
        }
        
        // Owner check
        if let Some(user_id) = caller_user {
            if inode.owner_id == Some(user_id) {
                return inode.permissions.owner_execute;
            }
        }
        
        // For directories, world_read implies traverse
        inode.permissions.world_read
    }
}
```

## IPC Protocol

### Write File

```rust
/// Write file request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsWriteRequest {
    /// Destination path
    pub path: String,
    /// File content
    pub content: Vec<u8>,
    /// Whether to encrypt
    pub encrypted: bool,
    /// Encryption key (if encrypted)
    pub encryption_key: Option<[u8; 32]>,
}

/// Write file response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsWriteResponse {
    pub result: Result<(), VfsError>,
}
```

### Read File

```rust
/// Read file request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsReadRequest {
    /// File path
    pub path: String,
    /// Byte offset to start reading
    pub offset: u64,
    /// Number of bytes to read (None = entire file)
    pub length: Option<u64>,
    /// Decryption key (if encrypted)
    pub decryption_key: Option<[u8; 32]>,
}

/// Read file response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsReadResponse {
    pub result: Result<Vec<u8>, VfsError>,
}
```

### Stat

```rust
/// Stat request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsStatRequest {
    /// Path to stat
    pub path: String,
    /// Follow symlinks?
    pub follow_symlinks: bool,
}

/// Stat response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsStatResponse {
    pub result: Result<Inode, VfsError>,
}
```

### Mkdir

```rust
/// Mkdir request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsMkdirRequest {
    /// Path to create
    pub path: String,
    /// Create parent directories?
    pub recursive: bool,
    /// Permissions for new directory
    pub permissions: Option<FilePermissions>,
}

/// Mkdir response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsMkdirResponse {
    pub result: Result<(), VfsError>,
}
```

### Readdir

```rust
/// Readdir request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsReaddirRequest {
    /// Directory path
    pub path: String,
}

/// Readdir response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsReaddirResponse {
    pub result: Result<Vec<DirEntry>, VfsError>,
}
```

## Implementation

### Directory Creation

```rust
impl VfsService for VfsServiceImpl {
    fn mkdir(&self, path: &str) -> Result<(), VfsError> {
        let canonical = self.canonicalize(path)?;
        
        // Check if already exists
        if self.exists(&canonical)? {
            return Err(VfsError::AlreadyExists);
        }
        
        // Check parent exists and is a directory
        let parent = self.parent_path(&canonical);
        let parent_inode = self.stat(&parent)?;
        
        if parent_inode.inode_type != InodeType::Directory {
            return Err(VfsError::NotADirectory);
        }
        
        // Check write permission on parent
        if !self.check_write(&parent_inode, self.caller_user(), self.caller_class()) {
            return Err(VfsError::PermissionDenied);
        }
        
        // Create inode
        let now = current_timestamp();
        let name = canonical.rsplit('/').next().unwrap_or("");
        
        let inode = Inode {
            path: canonical.clone(),
            parent_path: parent,
            name: name.to_string(),
            inode_type: InodeType::Directory,
            owner_id: self.caller_user(),
            permissions: FilePermissions::user_dir_default(),
            created_at: now,
            modified_at: now,
            accessed_at: now,
            size: 0,
            encrypted: false,
            content_hash: None,
        };
        
        self.storage.put_inode(&inode)?;
        
        Ok(())
    }
    
    fn mkdir_p(&self, path: &str) -> Result<(), VfsError> {
        let canonical = self.canonicalize(path)?;
        
        // Build path components
        let mut current = String::new();
        for component in canonical.split('/').filter(|c| !c.is_empty()) {
            current = format!("{}/{}", current, component);
            
            if !self.exists(&current)? {
                self.mkdir(&current)?;
            }
        }
        
        Ok(())
    }
}
```

### File Writing

```rust
impl VfsService for VfsServiceImpl {
    fn write_file(&self, path: &str, content: &[u8]) -> Result<(), VfsError> {
        let canonical = self.canonicalize(path)?;
        
        // Check parent directory
        let parent = self.parent_path(&canonical);
        let parent_inode = self.stat(&parent)?;
        
        if parent_inode.inode_type != InodeType::Directory {
            return Err(VfsError::NotADirectory);
        }
        
        // Check write permission
        if self.exists(&canonical)? {
            // Updating existing file
            let existing = self.stat(&canonical)?;
            if !self.check_write(&existing, self.caller_user(), self.caller_class()) {
                return Err(VfsError::PermissionDenied);
            }
        } else {
            // Creating new file - check parent write permission
            if !self.check_write(&parent_inode, self.caller_user(), self.caller_class()) {
                return Err(VfsError::PermissionDenied);
            }
        }
        
        // Check quota
        if let Some(user_id) = self.caller_user() {
            self.check_quota(user_id, content.len() as u64)?;
        }
        
        // Create/update inode
        let now = current_timestamp();
        let name = canonical.rsplit('/').next().unwrap_or("");
        let hash = sha256(content);
        
        let inode = Inode {
            path: canonical.clone(),
            parent_path: parent,
            name: name.to_string(),
            inode_type: InodeType::File,
            owner_id: self.caller_user(),
            permissions: FilePermissions::user_default(),
            created_at: now,
            modified_at: now,
            accessed_at: now,
            size: content.len() as u64,
            encrypted: false,
            content_hash: Some(hash),
        };
        
        // Store atomically
        self.storage.put_inode_and_content(&inode, content)?;
        
        // Update quota
        if let Some(user_id) = self.caller_user() {
            self.update_quota(user_id, content.len() as i64)?;
        }
        
        Ok(())
    }
}
```

## Invariants

1. **Path uniqueness**: Each path maps to exactly one inode
2. **Parent existence**: Non-root inodes have existing parent directories
3. **Consistency**: Inode and content are updated atomically
4. **Permission inheritance**: New files inherit owner from creator
5. **Symlink safety**: Symlink resolution has depth limit

## WASM Notes

- Paths use forward slashes regardless of host OS
- Timestamps use `performance.now()` converted to nanoseconds
- SHA-256 uses SubtleCrypto `digest`
- Large files (>1MB) are chunked into 1MB pieces

## Related Specifications

- [01-database.md](01-database.md) - Underlying storage
- [03-storage.md](03-storage.md) - Storage service operations
- [../05-identity/01-users.md](../05-identity/01-users.md) - User home directories
