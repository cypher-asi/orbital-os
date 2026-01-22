//! In-memory VFS implementation for testing.
//!
//! Provides a HashMap-based VFS that doesn't persist data.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::error::VfsError;
use crate::path::{filename, join_path, normalize_path, parent_path};
use crate::service::VfsService;
use crate::storage::{StorageQuota, StorageUsage};
use crate::types::{DirEntry, FilePermissions, Inode, InodeType, UserId};

/// In-memory VFS for testing.
pub struct MemoryVfs {
    /// Inode storage (path -> inode)
    inodes: RefCell<BTreeMap<String, Inode>>,
    /// Content storage (path -> content)
    content: RefCell<BTreeMap<String, Vec<u8>>>,
    /// User quotas
    quotas: RefCell<BTreeMap<UserId, StorageQuota>>,
    /// Current timestamp generator
    now: RefCell<u64>,
}

impl Default for MemoryVfs {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryVfs {
    /// Create a new empty in-memory VFS.
    pub fn new() -> Self {
        let vfs = Self {
            inodes: RefCell::new(BTreeMap::new()),
            content: RefCell::new(BTreeMap::new()),
            quotas: RefCell::new(BTreeMap::new()),
            now: RefCell::new(1000),
        };

        // Create root directory
        let root = Inode::new_directory(
            String::from("/"),
            String::from(""),
            String::from(""),
            None,
            1000,
        );
        vfs.inodes.borrow_mut().insert(String::from("/"), root);

        vfs
    }

    /// Get current timestamp and advance it.
    fn get_now(&self) -> u64 {
        let mut now = self.now.borrow_mut();
        let current = *now;
        *now += 1;
        current
    }

    /// Set the current timestamp (for testing).
    pub fn set_now(&self, timestamp: u64) {
        *self.now.borrow_mut() = timestamp;
    }
}

impl VfsService for MemoryVfs {
    fn mkdir(&self, path: &str) -> Result<(), VfsError> {
        let path = normalize_path(path)?;

        // Check if already exists
        if self.inodes.borrow().contains_key(&path) {
            return Err(VfsError::AlreadyExists);
        }

        // Check parent exists and is a directory
        let parent = parent_path(&path);
        {
            let inodes = self.inodes.borrow();
            match inodes.get(&parent) {
                Some(p) if p.is_directory() => {}
                Some(_) => return Err(VfsError::NotADirectory),
                None => return Err(VfsError::NotFound),
            }
        }

        // Create the directory
        let name = filename(&path);
        let now = self.get_now();
        let inode = Inode::new_directory(path.clone(), parent, String::from(name), None, now);
        self.inodes.borrow_mut().insert(path, inode);

        Ok(())
    }

    fn mkdir_p(&self, path: &str) -> Result<(), VfsError> {
        let path = normalize_path(path)?;

        // Split into components and create each
        let mut current = String::new();
        for component in path.split('/') {
            if component.is_empty() {
                continue;
            }
            current = join_path(&current, component);
            if current.is_empty() {
                current = String::from("/");
            }

            // Create if doesn't exist
            if !self.inodes.borrow().contains_key(&current) {
                let parent = parent_path(&current);
                let now = self.get_now();
                let inode = Inode::new_directory(
                    current.clone(),
                    parent,
                    String::from(component),
                    None,
                    now,
                );
                self.inodes.borrow_mut().insert(current.clone(), inode);
            }
        }

        Ok(())
    }

    fn rmdir(&self, path: &str) -> Result<(), VfsError> {
        let path = normalize_path(path)?;

        if path == "/" {
            return Err(VfsError::PermissionDenied);
        }

        // Check exists and is directory
        {
            let inodes = self.inodes.borrow();
            match inodes.get(&path) {
                Some(i) if i.is_directory() => {}
                Some(_) => return Err(VfsError::NotADirectory),
                None => return Err(VfsError::NotFound),
            }
        }

        // Check empty
        let prefix = if path == "/" {
            String::from("/")
        } else {
            alloc::format!("{}/", path)
        };

        let has_children = self
            .inodes
            .borrow()
            .keys()
            .any(|k| k != &path && k.starts_with(&prefix));

        if has_children {
            return Err(VfsError::DirectoryNotEmpty);
        }

        self.inodes.borrow_mut().remove(&path);
        Ok(())
    }

    fn rmdir_recursive(&self, path: &str) -> Result<(), VfsError> {
        let path = normalize_path(path)?;

        if path == "/" {
            return Err(VfsError::PermissionDenied);
        }

        // Check exists
        if !self.inodes.borrow().contains_key(&path) {
            return Err(VfsError::NotFound);
        }

        // Collect all paths to remove
        let prefix = alloc::format!("{}/", path);
        let to_remove: Vec<String> = self
            .inodes
            .borrow()
            .keys()
            .filter(|k| *k == &path || k.starts_with(&prefix))
            .cloned()
            .collect();

        // Remove all
        let mut inodes = self.inodes.borrow_mut();
        let mut content = self.content.borrow_mut();
        for p in to_remove {
            inodes.remove(&p);
            content.remove(&p);
        }

        Ok(())
    }

    fn readdir(&self, path: &str) -> Result<Vec<DirEntry>, VfsError> {
        let path = normalize_path(path)?;

        // Check exists and is directory
        {
            let inodes = self.inodes.borrow();
            match inodes.get(&path) {
                Some(i) if i.is_directory() => {}
                Some(_) => return Err(VfsError::NotADirectory),
                None => return Err(VfsError::NotFound),
            }
        }

        // Find direct children
        let prefix = if path == "/" {
            String::from("/")
        } else {
            alloc::format!("{}/", path)
        };

        let entries: Vec<DirEntry> = self
            .inodes
            .borrow()
            .iter()
            .filter(|(k, _)| {
                if *k == &path {
                    return false;
                }
                if !k.starts_with(&prefix) {
                    return false;
                }
                // Must be direct child (no more slashes after prefix)
                let rest = &k[prefix.len()..];
                !rest.contains('/')
            })
            .map(|(_, inode)| DirEntry::from(inode))
            .collect();

        Ok(entries)
    }

    fn write_file(&self, path: &str, content: &[u8]) -> Result<(), VfsError> {
        let path = normalize_path(path)?;

        // Check parent exists and is directory
        let parent = parent_path(&path);
        {
            let inodes = self.inodes.borrow();
            match inodes.get(&parent) {
                Some(p) if p.is_directory() => {}
                Some(_) => return Err(VfsError::NotADirectory),
                None => return Err(VfsError::NotFound),
            }
        }

        let name = filename(&path);
        let now = self.get_now();
        let size = content.len() as u64;

        // Create or update inode
        let inode = Inode::new_file(
            path.clone(),
            parent,
            String::from(name),
            None,
            size,
            None,
            now,
        );

        self.inodes.borrow_mut().insert(path.clone(), inode);
        self.content.borrow_mut().insert(path, content.to_vec());

        Ok(())
    }

    fn write_file_encrypted(
        &self,
        path: &str,
        content: &[u8],
        _key: &[u8; 32],
    ) -> Result<(), VfsError> {
        // For testing, just store as-is (real impl would encrypt)
        let path = normalize_path(path)?;

        let parent = parent_path(&path);
        {
            let inodes = self.inodes.borrow();
            match inodes.get(&parent) {
                Some(p) if p.is_directory() => {}
                Some(_) => return Err(VfsError::NotADirectory),
                None => return Err(VfsError::NotFound),
            }
        }

        let name = filename(&path);
        let now = self.get_now();
        let size = content.len() as u64;

        let mut inode = Inode::new_file(
            path.clone(),
            parent,
            String::from(name),
            None,
            size,
            None,
            now,
        );
        inode.encrypted = true;

        self.inodes.borrow_mut().insert(path.clone(), inode);
        self.content.borrow_mut().insert(path, content.to_vec());

        Ok(())
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        let path = normalize_path(path)?;

        // Check exists and is file
        {
            let inodes = self.inodes.borrow();
            match inodes.get(&path) {
                Some(i) if i.is_file() => {}
                Some(_) => return Err(VfsError::NotAFile),
                None => return Err(VfsError::NotFound),
            }
        }

        self.content
            .borrow()
            .get(&path)
            .cloned()
            .ok_or(VfsError::NotFound)
    }

    fn read_file_encrypted(&self, path: &str, _key: &[u8; 32]) -> Result<Vec<u8>, VfsError> {
        // For testing, just read as-is (real impl would decrypt)
        self.read_file(path)
    }

    fn unlink(&self, path: &str) -> Result<(), VfsError> {
        let path = normalize_path(path)?;

        // Check exists and is file
        {
            let inodes = self.inodes.borrow();
            match inodes.get(&path) {
                Some(i) if i.is_file() || i.is_symlink() => {}
                Some(_) => return Err(VfsError::NotAFile),
                None => return Err(VfsError::NotFound),
            }
        }

        self.inodes.borrow_mut().remove(&path);
        self.content.borrow_mut().remove(&path);

        Ok(())
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), VfsError> {
        let from = normalize_path(from)?;
        let to = normalize_path(to)?;

        // Get source inode
        let inode = {
            let inodes = self.inodes.borrow();
            inodes.get(&from).cloned().ok_or(VfsError::NotFound)?
        };

        // Check destination parent exists
        let to_parent = parent_path(&to);
        {
            let inodes = self.inodes.borrow();
            match inodes.get(&to_parent) {
                Some(p) if p.is_directory() => {}
                Some(_) => return Err(VfsError::NotADirectory),
                None => return Err(VfsError::NotFound),
            }
        }

        // Move content if file
        let content = self.content.borrow_mut().remove(&from);

        // Update inode with new path
        let mut new_inode = inode;
        new_inode.path = to.clone();
        new_inode.parent_path = to_parent;
        new_inode.name = String::from(filename(&to));
        new_inode.modified_at = self.get_now();

        // Remove old, insert new
        self.inodes.borrow_mut().remove(&from);
        self.inodes.borrow_mut().insert(to.clone(), new_inode);

        if let Some(c) = content {
            self.content.borrow_mut().insert(to, c);
        }

        Ok(())
    }

    fn copy(&self, from: &str, to: &str) -> Result<(), VfsError> {
        let from = normalize_path(from)?;
        let to = normalize_path(to)?;

        // Check source is file
        {
            let inodes = self.inodes.borrow();
            match inodes.get(&from) {
                Some(i) if i.is_file() => {}
                Some(_) => return Err(VfsError::NotAFile),
                None => return Err(VfsError::NotFound),
            }
        }

        // Read content and write to new location
        let content = self.read_file(&from)?;
        self.write_file(&to, &content)
    }

    fn stat(&self, path: &str) -> Result<Inode, VfsError> {
        let path = normalize_path(path)?;

        self.inodes
            .borrow()
            .get(&path)
            .cloned()
            .ok_or(VfsError::NotFound)
    }

    fn exists(&self, path: &str) -> Result<bool, VfsError> {
        let path = normalize_path(path)?;
        Ok(self.inodes.borrow().contains_key(&path))
    }

    fn chmod(&self, path: &str, perms: FilePermissions) -> Result<(), VfsError> {
        let path = normalize_path(path)?;

        let mut inodes = self.inodes.borrow_mut();
        let inode = inodes.get_mut(&path).ok_or(VfsError::NotFound)?;
        inode.permissions = perms;
        inode.modified_at = self.get_now();

        Ok(())
    }

    fn chown(&self, path: &str, owner_id: Option<UserId>) -> Result<(), VfsError> {
        let path = normalize_path(path)?;

        let mut inodes = self.inodes.borrow_mut();
        let inode = inodes.get_mut(&path).ok_or(VfsError::NotFound)?;
        inode.owner_id = owner_id;
        inode.modified_at = self.get_now();

        Ok(())
    }

    fn symlink(&self, target: &str, link_path: &str) -> Result<(), VfsError> {
        let link_path = normalize_path(link_path)?;

        // Check parent exists
        let parent = parent_path(&link_path);
        {
            let inodes = self.inodes.borrow();
            match inodes.get(&parent) {
                Some(p) if p.is_directory() => {}
                Some(_) => return Err(VfsError::NotADirectory),
                None => return Err(VfsError::NotFound),
            }
        }

        let name = filename(&link_path);
        let now = self.get_now();

        let inode = Inode {
            path: link_path.clone(),
            parent_path: parent,
            name: String::from(name),
            inode_type: InodeType::SymLink {
                target: String::from(target),
            },
            owner_id: None,
            permissions: FilePermissions::user_default(),
            created_at: now,
            modified_at: now,
            accessed_at: now,
            size: target.len() as u64,
            encrypted: false,
            content_hash: None,
        };

        self.inodes.borrow_mut().insert(link_path, inode);

        Ok(())
    }

    fn readlink(&self, path: &str) -> Result<String, VfsError> {
        let path = normalize_path(path)?;

        let inodes = self.inodes.borrow();
        let inode = inodes.get(&path).ok_or(VfsError::NotFound)?;

        match &inode.inode_type {
            InodeType::SymLink { target } => Ok(target.clone()),
            _ => Err(VfsError::NotAFile),
        }
    }

    fn resolve_path(&self, path: &str) -> Result<String, VfsError> {
        let path = normalize_path(path)?;

        // Simple implementation - doesn't follow symlinks
        // A real implementation would resolve symlinks recursively
        if self.inodes.borrow().contains_key(&path) {
            Ok(path)
        } else {
            Err(VfsError::NotFound)
        }
    }

    fn get_usage(&self, path: &str) -> Result<StorageUsage, VfsError> {
        let path = normalize_path(path)?;

        if !self.inodes.borrow().contains_key(&path) {
            return Err(VfsError::NotFound);
        }

        let prefix = if path == "/" {
            String::new()
        } else {
            path.clone()
        };

        let mut usage = StorageUsage::new();

        for (p, inode) in self.inodes.borrow().iter() {
            if !p.starts_with(&prefix) && p != &path {
                continue;
            }

            match &inode.inode_type {
                InodeType::File => {
                    usage.add_file(inode.size, inode.encrypted);
                }
                InodeType::Directory => {
                    usage.add_directory();
                }
                InodeType::SymLink { .. } => {
                    // Symlinks don't count toward storage
                }
            }
        }

        Ok(usage)
    }

    fn get_quota(&self, user_id: UserId) -> Result<StorageQuota, VfsError> {
        let quotas = self.quotas.borrow();
        Ok(quotas
            .get(&user_id)
            .cloned()
            .unwrap_or_else(|| StorageQuota::new(user_id)))
    }

    fn set_quota(&self, user_id: UserId, max_bytes: u64) -> Result<(), VfsError> {
        let mut quotas = self.quotas.borrow_mut();
        let quota = quotas
            .entry(user_id)
            .or_insert_with(|| StorageQuota::new(user_id));
        quota.max_bytes = max_bytes;
        quota.soft_limit_bytes = max_bytes * 80 / 100;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::DEFAULT_QUOTA_BYTES;

    #[test]
    fn test_mkdir() {
        let vfs = MemoryVfs::new();

        vfs.mkdir("/home").unwrap();
        assert!(vfs.exists("/home").unwrap());

        // Should fail - already exists
        assert!(vfs.mkdir("/home").is_err());
    }

    #[test]
    fn test_mkdir_p() {
        let vfs = MemoryVfs::new();

        vfs.mkdir_p("/home/user/Documents").unwrap();
        assert!(vfs.exists("/home").unwrap());
        assert!(vfs.exists("/home/user").unwrap());
        assert!(vfs.exists("/home/user/Documents").unwrap());
    }

    #[test]
    fn test_write_read_file() {
        let vfs = MemoryVfs::new();

        vfs.mkdir("/home").unwrap();
        vfs.write_file("/home/test.txt", b"Hello, World!").unwrap();

        let content = vfs.read_file("/home/test.txt").unwrap();
        assert_eq!(content, b"Hello, World!");
    }

    #[test]
    fn test_readdir() {
        let vfs = MemoryVfs::new();

        vfs.mkdir_p("/home/user").unwrap();
        vfs.write_file("/home/user/file1.txt", b"1").unwrap();
        vfs.write_file("/home/user/file2.txt", b"2").unwrap();
        vfs.mkdir("/home/user/Documents").unwrap();

        let entries = vfs.readdir("/home/user").unwrap();
        assert_eq!(entries.len(), 3);

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"file1.txt"));
        assert!(names.contains(&"file2.txt"));
        assert!(names.contains(&"Documents"));
    }

    #[test]
    fn test_rmdir() {
        let vfs = MemoryVfs::new();

        vfs.mkdir_p("/home/user").unwrap();

        // Can't remove non-empty directory
        assert!(vfs.rmdir("/home").is_err());

        // Can remove empty directory
        vfs.rmdir("/home/user").unwrap();
        assert!(!vfs.exists("/home/user").unwrap());
    }

    #[test]
    fn test_rmdir_recursive() {
        let vfs = MemoryVfs::new();

        vfs.mkdir_p("/home/user/Documents").unwrap();
        vfs.write_file("/home/user/Documents/file.txt", b"data").unwrap();

        vfs.rmdir_recursive("/home/user").unwrap();

        assert!(!vfs.exists("/home/user").unwrap());
        assert!(vfs.exists("/home").unwrap());
    }

    #[test]
    fn test_rename() {
        let vfs = MemoryVfs::new();

        vfs.mkdir("/home").unwrap();
        vfs.write_file("/home/old.txt", b"content").unwrap();

        vfs.rename("/home/old.txt", "/home/new.txt").unwrap();

        assert!(!vfs.exists("/home/old.txt").unwrap());
        assert!(vfs.exists("/home/new.txt").unwrap());
        assert_eq!(vfs.read_file("/home/new.txt").unwrap(), b"content");
    }

    #[test]
    fn test_copy() {
        let vfs = MemoryVfs::new();

        vfs.mkdir("/home").unwrap();
        vfs.write_file("/home/original.txt", b"content").unwrap();

        vfs.copy("/home/original.txt", "/home/copy.txt").unwrap();

        assert!(vfs.exists("/home/original.txt").unwrap());
        assert!(vfs.exists("/home/copy.txt").unwrap());
        assert_eq!(vfs.read_file("/home/copy.txt").unwrap(), b"content");
    }

    #[test]
    fn test_stat() {
        let vfs = MemoryVfs::new();

        vfs.mkdir("/home").unwrap();
        vfs.write_file("/home/test.txt", b"Hello").unwrap();

        let inode = vfs.stat("/home/test.txt").unwrap();
        assert!(inode.is_file());
        assert_eq!(inode.size, 5);
        assert_eq!(inode.name, "test.txt");

        let dir_inode = vfs.stat("/home").unwrap();
        assert!(dir_inode.is_directory());
    }

    #[test]
    fn test_chmod_chown() {
        let vfs = MemoryVfs::new();

        vfs.mkdir("/home").unwrap();

        // Change permissions
        vfs.chmod("/home", FilePermissions::world_rw()).unwrap();
        let inode = vfs.stat("/home").unwrap();
        assert!(inode.permissions.world_write);

        // Change owner
        vfs.chown("/home", Some(12345)).unwrap();
        let inode = vfs.stat("/home").unwrap();
        assert_eq!(inode.owner_id, Some(12345));
    }

    #[test]
    fn test_symlink() {
        let vfs = MemoryVfs::new();

        vfs.mkdir("/home").unwrap();
        vfs.write_file("/home/target.txt", b"content").unwrap();
        vfs.symlink("/home/target.txt", "/home/link.txt").unwrap();

        let target = vfs.readlink("/home/link.txt").unwrap();
        assert_eq!(target, "/home/target.txt");

        let inode = vfs.stat("/home/link.txt").unwrap();
        assert!(inode.is_symlink());
    }

    #[test]
    fn test_get_usage() {
        let vfs = MemoryVfs::new();

        vfs.mkdir_p("/home/user").unwrap();
        vfs.write_file("/home/user/file1.txt", b"12345").unwrap();
        vfs.write_file("/home/user/file2.txt", b"67890").unwrap();

        let usage = vfs.get_usage("/home/user").unwrap();
        assert_eq!(usage.file_count, 2);
        assert_eq!(usage.used_bytes, 10);
        assert_eq!(usage.directory_count, 1); // /home/user itself
    }

    #[test]
    fn test_quota() {
        let vfs = MemoryVfs::new();

        let quota = vfs.get_quota(123).unwrap();
        assert_eq!(quota.max_bytes, DEFAULT_QUOTA_BYTES);

        vfs.set_quota(123, 1000).unwrap();
        let quota = vfs.get_quota(123).unwrap();
        assert_eq!(quota.max_bytes, 1000);
    }
}
