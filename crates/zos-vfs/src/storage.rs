//! Storage types for the VFS layer.
//!
//! Defines content records, quota management, and storage usage tracking.

use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::types::UserId;

/// Storage usage statistics.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StorageUsage {
    /// Total bytes used
    pub used_bytes: u64,

    /// Number of files
    pub file_count: u64,

    /// Number of directories
    pub directory_count: u64,

    /// Encrypted content bytes
    pub encrypted_bytes: u64,
}

impl StorageUsage {
    /// Create new empty usage stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a file to the usage stats.
    pub fn add_file(&mut self, size: u64, encrypted: bool) {
        self.file_count += 1;
        self.used_bytes += size;
        if encrypted {
            self.encrypted_bytes += size;
        }
    }

    /// Add a directory to the usage stats.
    pub fn add_directory(&mut self) {
        self.directory_count += 1;
    }

    /// Remove a file from the usage stats.
    pub fn remove_file(&mut self, size: u64, encrypted: bool) {
        self.file_count = self.file_count.saturating_sub(1);
        self.used_bytes = self.used_bytes.saturating_sub(size);
        if encrypted {
            self.encrypted_bytes = self.encrypted_bytes.saturating_sub(size);
        }
    }

    /// Remove a directory from the usage stats.
    pub fn remove_directory(&mut self) {
        self.directory_count = self.directory_count.saturating_sub(1);
    }
}

/// Per-user storage quota.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StorageQuota {
    /// User ID
    pub user_id: UserId,

    /// Maximum allowed bytes
    pub max_bytes: u64,

    /// Currently used bytes
    pub used_bytes: u64,

    /// Soft limit (warning threshold)
    pub soft_limit_bytes: u64,

    /// Whether the user is over quota
    pub over_quota: bool,
}

/// Default quota (100 MB).
pub const DEFAULT_QUOTA_BYTES: u64 = 100 * 1024 * 1024;

/// Soft limit percentage (80%).
pub const SOFT_LIMIT_PERCENT: u64 = 80;

impl StorageQuota {
    /// Create a new quota with default limits.
    pub fn new(user_id: UserId) -> Self {
        Self {
            user_id,
            max_bytes: DEFAULT_QUOTA_BYTES,
            used_bytes: 0,
            soft_limit_bytes: DEFAULT_QUOTA_BYTES * SOFT_LIMIT_PERCENT / 100,
            over_quota: false,
        }
    }

    /// Create a quota with custom limits.
    pub fn with_limit(user_id: UserId, max_bytes: u64) -> Self {
        Self {
            user_id,
            max_bytes,
            used_bytes: 0,
            soft_limit_bytes: max_bytes * SOFT_LIMIT_PERCENT / 100,
            over_quota: false,
        }
    }

    /// Check if operation would exceed quota.
    pub fn would_exceed(&self, additional_bytes: u64) -> bool {
        self.used_bytes + additional_bytes > self.max_bytes
    }

    /// Check if at soft limit (warning).
    pub fn at_soft_limit(&self) -> bool {
        self.used_bytes >= self.soft_limit_bytes
    }

    /// Remaining bytes available.
    pub fn remaining(&self) -> u64 {
        if self.used_bytes >= self.max_bytes {
            0
        } else {
            self.max_bytes - self.used_bytes
        }
    }

    /// Update usage (delta can be positive or negative).
    pub fn update_usage(&mut self, delta: i64) {
        if delta >= 0 {
            self.used_bytes = self.used_bytes.saturating_add(delta as u64);
        } else {
            self.used_bytes = self.used_bytes.saturating_sub((-delta) as u64);
        }
        self.over_quota = self.used_bytes > self.max_bytes;
    }

    /// Usage percentage (0-100+).
    pub fn usage_percent(&self) -> u64 {
        if self.max_bytes == 0 {
            return 0;
        }
        (self.used_bytes * 100) / self.max_bytes
    }
}

/// Stored file content record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentRecord {
    /// File path (primary key)
    pub path: alloc::string::String,

    /// Content data (or first chunk if large)
    pub data: Vec<u8>,

    /// Total size in bytes
    pub size: u64,

    /// SHA-256 hash
    pub hash: [u8; 32],

    /// Is this file chunked?
    pub chunked: bool,

    /// Number of chunks (if chunked)
    pub chunk_count: Option<u32>,

    /// Is content encrypted?
    pub encrypted: bool,

    /// Encryption nonce (if encrypted)
    pub nonce: Option<[u8; 12]>,
}

impl ContentRecord {
    /// Create a new content record for small files.
    pub fn new(path: alloc::string::String, data: Vec<u8>, hash: [u8; 32]) -> Self {
        let size = data.len() as u64;
        Self {
            path,
            data,
            size,
            hash,
            chunked: false,
            chunk_count: None,
            encrypted: false,
            nonce: None,
        }
    }

    /// Create a chunked content record for large files.
    pub fn new_chunked(
        path: alloc::string::String,
        size: u64,
        hash: [u8; 32],
        chunk_count: u32,
    ) -> Self {
        Self {
            path,
            data: Vec::new(),
            size,
            hash,
            chunked: true,
            chunk_count: Some(chunk_count),
            encrypted: false,
            nonce: None,
        }
    }
}

/// File chunk for large files.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkRecord {
    /// File path
    pub path: alloc::string::String,

    /// Chunk index (0-based)
    pub chunk_index: u32,

    /// Chunk data
    pub data: Vec<u8>,
}

/// Maximum chunk size (1 MB).
pub const CHUNK_SIZE: usize = 1024 * 1024;

/// Threshold for chunking files.
pub const CHUNK_THRESHOLD: usize = CHUNK_SIZE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_usage() {
        let mut usage = StorageUsage::new();

        usage.add_file(1000, false);
        usage.add_file(500, true);
        usage.add_directory();

        assert_eq!(usage.file_count, 2);
        assert_eq!(usage.used_bytes, 1500);
        assert_eq!(usage.encrypted_bytes, 500);
        assert_eq!(usage.directory_count, 1);

        usage.remove_file(500, true);
        assert_eq!(usage.file_count, 1);
        assert_eq!(usage.used_bytes, 1000);
        assert_eq!(usage.encrypted_bytes, 0);
    }

    #[test]
    fn test_storage_quota() {
        let mut quota = StorageQuota::new(1);

        assert_eq!(quota.max_bytes, DEFAULT_QUOTA_BYTES);
        assert!(!quota.would_exceed(1000));
        assert!(!quota.at_soft_limit());

        quota.update_usage(80 * 1024 * 1024); // 80 MB
        assert!(quota.at_soft_limit());
        assert!(!quota.over_quota);

        quota.update_usage(30 * 1024 * 1024); // Now 110 MB total
        assert!(quota.over_quota);
        assert!(quota.would_exceed(1));

        quota.update_usage(-50 * 1024 * 1024); // Back to 60 MB
        assert!(!quota.over_quota);
    }

    #[test]
    fn test_content_record() {
        let data = alloc::vec![1u8, 2, 3, 4, 5];
        let hash = [0u8; 32];
        let record = ContentRecord::new(alloc::string::String::from("/test"), data, hash);

        assert_eq!(record.size, 5);
        assert!(!record.chunked);
        assert!(!record.encrypted);
    }

    #[test]
    fn test_chunked_content() {
        let hash = [0u8; 32];
        let record = ContentRecord::new_chunked(
            alloc::string::String::from("/large"),
            5 * 1024 * 1024,
            hash,
            5,
        );

        assert!(record.chunked);
        assert_eq!(record.chunk_count, Some(5));
        assert!(record.data.is_empty());
    }
}
