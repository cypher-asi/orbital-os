# Storage Service

> File operations, encryption, and quota management.

## Overview

The Storage Service provides the implementation layer for VFS operations, including:

1. **File I/O**: Reading and writing file content
2. **Encryption**: At-rest encryption for sensitive files
3. **Quota management**: Per-user storage limits
4. **Large file handling**: Chunked storage for big files

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Storage Service                                      │
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                    Content Manager                                     │   │
│  │                                                                       │   │
│  │  • Small files: stored directly in content store                      │   │
│  │  • Large files: chunked into 1MB pieces                               │   │
│  │  • Hash verification on read                                          │   │
│  └────────────────────────────────────┬──────────────────────────────────┘   │
│                                       │                                      │
│  ┌──────────────────────────┐        │        ┌──────────────────────────┐  │
│  │   Encryption Layer       │        │        │   Quota Tracker          │  │
│  │                          │        │        │                          │  │
│  │  • AES-256-GCM           │◀───────┴───────▶│  • Per-user tracking     │  │
│  │  • Per-file keys         │                 │  • Enforcement           │  │
│  │  • Key derivation        │                 │  • Reporting             │  │
│  └──────────────────────────┘                 └──────────────────────────┘  │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       ▼
                    ┌──────────────────────────────────┐
                    │     IndexedDB (zos-userspace)     │
                    │                                  │
                    │  content store │ chunks store    │
                    └──────────────────────────────────┘
```

## Data Structures

### StorageUsage

```rust
use uuid::Uuid;
use serde::{Serialize, Deserialize};

/// Storage usage statistics.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
```

### StorageQuota

```rust
/// Per-user storage quota.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StorageQuota {
    /// User ID
    pub user_id: Uuid,
    
    /// Maximum allowed bytes
    pub max_bytes: u64,
    
    /// Currently used bytes
    pub used_bytes: u64,
    
    /// Soft limit (warning threshold)
    pub soft_limit_bytes: u64,
    
    /// Whether the user is over quota
    pub over_quota: bool,
}

impl StorageQuota {
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
}

/// Default quota (100 MB).
pub const DEFAULT_QUOTA_BYTES: u64 = 100 * 1024 * 1024;

/// Soft limit percentage (80%).
pub const SOFT_LIMIT_PERCENT: u64 = 80;
```

### ContentRecord

```rust
/// Stored file content.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentRecord {
    /// File path (primary key)
    pub path: String,
    
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
```

### ChunkRecord

```rust
/// File chunk for large files.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkRecord {
    /// File path
    pub path: String,
    
    /// Chunk index (0-based)
    pub chunk_index: u32,
    
    /// Chunk data
    pub data: Vec<u8>,
}

/// Maximum chunk size (1 MB).
pub const CHUNK_SIZE: usize = 1024 * 1024;

/// Threshold for chunking files.
pub const CHUNK_THRESHOLD: usize = CHUNK_SIZE;
```

## Storage Service Trait

```rust
/// Low-level storage operations.
pub trait StorageService {
    // ========== Content Operations ==========
    
    /// Store file content.
    fn put_content(&self, path: &str, data: &[u8]) -> Result<ContentRecord, StorageError>;
    
    /// Store encrypted file content.
    fn put_content_encrypted(&self, path: &str, data: &[u8], key: &[u8; 32]) -> Result<ContentRecord, StorageError>;
    
    /// Get file content.
    fn get_content(&self, path: &str) -> Result<Vec<u8>, StorageError>;
    
    /// Get encrypted file content.
    fn get_content_decrypted(&self, path: &str, key: &[u8; 32]) -> Result<Vec<u8>, StorageError>;
    
    /// Delete file content.
    fn delete_content(&self, path: &str) -> Result<(), StorageError>;
    
    /// Get content record without data.
    fn get_content_metadata(&self, path: &str) -> Result<ContentRecord, StorageError>;
    
    // ========== Atomic Operations ==========
    
    /// Store inode and content atomically.
    fn put_inode_and_content(&self, inode: &Inode, data: &[u8]) -> Result<(), StorageError>;
    
    /// Delete inode and content atomically.
    fn delete_inode_and_content(&self, path: &str) -> Result<(), StorageError>;
    
    // ========== Quota Operations ==========
    
    /// Get quota for user.
    fn get_quota(&self, user_id: Uuid) -> Result<StorageQuota, StorageError>;
    
    /// Set quota for user.
    fn set_quota(&self, user_id: Uuid, max_bytes: u64) -> Result<(), StorageError>;
    
    /// Update usage (delta can be negative for deletions).
    fn update_usage(&self, user_id: Uuid, delta: i64) -> Result<StorageQuota, StorageError>;
    
    /// Calculate actual usage (full scan).
    fn recalculate_usage(&self, user_id: Uuid) -> Result<StorageUsage, StorageError>;
}

/// Storage operation errors.
#[derive(Clone, Debug)]
pub enum StorageError {
    /// Content not found
    NotFound,
    /// Quota exceeded
    QuotaExceeded,
    /// Hash mismatch on read
    IntegrityError,
    /// Encryption/decryption failed
    CryptoError(String),
    /// Database error
    DatabaseError(String),
}
```

## Encryption

### Encryption Scheme

```rust
/// Encrypt content with AES-256-GCM.
fn encrypt_content(data: &[u8], key: &[u8; 32]) -> Result<EncryptedContent, StorageError> {
    // Generate random nonce
    let mut nonce = [0u8; 12];
    random_bytes(&mut nonce);
    
    // Encrypt with AES-256-GCM
    let ciphertext = aes_256_gcm_encrypt(key, &nonce, data)
        .map_err(|e| StorageError::CryptoError(e.to_string()))?;
    
    Ok(EncryptedContent {
        ciphertext,
        nonce,
    })
}

/// Decrypt content.
fn decrypt_content(encrypted: &EncryptedContent, key: &[u8; 32]) -> Result<Vec<u8>, StorageError> {
    aes_256_gcm_decrypt(key, &encrypted.nonce, &encrypted.ciphertext)
        .map_err(|e| StorageError::CryptoError(e.to_string()))
}

/// Encrypted content container.
struct EncryptedContent {
    ciphertext: Vec<u8>,
    nonce: [u8; 12],
}
```

### Key Derivation for Files

```rust
/// Derive a file-specific key from user's master key.
fn derive_file_key(master_key: &[u8; 32], path: &str) -> [u8; 32] {
    // Use HKDF to derive file-specific key
    let info = format!("zos-file:{}", path);
    hkdf_sha256(
        master_key,
        b"zos-file-encryption",  // salt
        info.as_bytes(),
    )
}
```

## Large File Handling

### Chunked Write

```rust
impl StorageServiceImpl {
    fn put_content_chunked(&self, path: &str, data: &[u8]) -> Result<ContentRecord, StorageError> {
        let hash = sha256(data);
        let chunk_count = (data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;
        
        // Begin transaction
        let tx = self.db.transaction(&["content", "chunks"], "readwrite")?;
        
        // Store chunks
        for (i, chunk_data) in data.chunks(CHUNK_SIZE).enumerate() {
            let chunk = ChunkRecord {
                path: path.to_string(),
                chunk_index: i as u32,
                data: chunk_data.to_vec(),
            };
            tx.put("chunks", &chunk)?;
        }
        
        // Store content record (without inline data)
        let record = ContentRecord {
            path: path.to_string(),
            data: vec![],  // No inline data for chunked files
            size: data.len() as u64,
            hash,
            chunked: true,
            chunk_count: Some(chunk_count as u32),
            encrypted: false,
            nonce: None,
        };
        tx.put("content", &record)?;
        
        // Commit transaction
        tx.commit()?;
        
        Ok(record)
    }
}
```

### Chunked Read

```rust
impl StorageServiceImpl {
    fn get_content_chunked(&self, path: &str, chunk_count: u32) -> Result<Vec<u8>, StorageError> {
        let tx = self.db.transaction(&["chunks"], "readonly")?;
        let store = tx.object_store("chunks")?;
        
        let mut data = Vec::new();
        
        for i in 0..chunk_count {
            let key = (path, i);
            let chunk: ChunkRecord = store.get(&key)?
                .ok_or(StorageError::IntegrityError)?;
            data.extend_from_slice(&chunk.data);
        }
        
        Ok(data)
    }
}
```

## Quota Management

### Quota Tracking

```rust
impl StorageServiceImpl {
    fn check_quota(&self, user_id: Uuid, additional: u64) -> Result<(), StorageError> {
        let quota = self.get_quota(user_id)?;
        
        if quota.would_exceed(additional) {
            return Err(StorageError::QuotaExceeded);
        }
        
        Ok(())
    }
    
    fn update_usage(&self, user_id: Uuid, delta: i64) -> Result<StorageQuota, StorageError> {
        let tx = self.db.transaction(&["quotas"], "readwrite")?;
        let store = tx.object_store("quotas")?;
        
        let mut quota: StorageQuota = store.get(&user_id)?
            .unwrap_or_else(|| StorageQuota {
                user_id,
                max_bytes: DEFAULT_QUOTA_BYTES,
                used_bytes: 0,
                soft_limit_bytes: DEFAULT_QUOTA_BYTES * SOFT_LIMIT_PERCENT / 100,
                over_quota: false,
            });
        
        // Update usage
        if delta >= 0 {
            quota.used_bytes = quota.used_bytes.saturating_add(delta as u64);
        } else {
            quota.used_bytes = quota.used_bytes.saturating_sub((-delta) as u64);
        }
        
        quota.over_quota = quota.used_bytes > quota.max_bytes;
        
        store.put(&quota)?;
        tx.commit()?;
        
        Ok(quota)
    }
    
    fn recalculate_usage(&self, user_id: Uuid) -> Result<StorageUsage, StorageError> {
        let home_path = format!("/home/{}", user_id);
        let mut usage = StorageUsage {
            used_bytes: 0,
            file_count: 0,
            directory_count: 0,
            encrypted_bytes: 0,
        };
        
        // Walk the user's home directory
        self.walk_directory(&home_path, &mut |inode| {
            match inode.inode_type {
                InodeType::File => {
                    usage.file_count += 1;
                    usage.used_bytes += inode.size;
                    if inode.encrypted {
                        usage.encrypted_bytes += inode.size;
                    }
                }
                InodeType::Directory => {
                    usage.directory_count += 1;
                }
                _ => {}
            }
        })?;
        
        // Update quota record with actual usage
        let mut quota = self.get_quota(user_id)?;
        quota.used_bytes = usage.used_bytes;
        quota.over_quota = quota.used_bytes > quota.max_bytes;
        self.set_quota_record(&quota)?;
        
        Ok(usage)
    }
}
```

## JavaScript Backend

### IndexedDB Operations

```javascript
class StorageBackend {
    constructor(db) {
        this.db = db;
    }
    
    // Write content (handles chunking automatically)
    async putContent(path, data, options = {}) {
        const { encrypted = false, encryptionKey = null } = options;
        
        // Calculate hash
        const hash = await this.sha256(data);
        
        // Encrypt if requested
        let contentData = data;
        let nonce = null;
        
        if (encrypted && encryptionKey) {
            const result = await this.encrypt(data, encryptionKey);
            contentData = result.ciphertext;
            nonce = result.nonce;
        }
        
        // Check if chunking needed
        if (contentData.byteLength > CHUNK_THRESHOLD) {
            return this.putContentChunked(path, contentData, hash, encrypted, nonce);
        }
        
        // Store directly
        const tx = this.db.transaction(['content'], 'readwrite');
        const store = tx.objectStore('content');
        
        const record = {
            path,
            data: contentData,
            size: data.byteLength,  // Original size
            hash: Array.from(hash),
            chunked: false,
            chunk_count: null,
            encrypted,
            nonce: nonce ? Array.from(nonce) : null,
        };
        
        await this.promisify(store.put(record));
        return record;
    }
    
    // Read content
    async getContent(path, options = {}) {
        const { decryptionKey = null } = options;
        
        const tx = this.db.transaction(['content', 'chunks'], 'readonly');
        const contentStore = tx.objectStore('content');
        
        const record = await this.promisify(contentStore.get(path));
        if (!record) {
            throw new Error('Content not found');
        }
        
        // Get data (handling chunks if needed)
        let data;
        if (record.chunked) {
            data = await this.getChunkedContent(tx, path, record.chunk_count);
        } else {
            data = new Uint8Array(record.data);
        }
        
        // Decrypt if needed
        if (record.encrypted && decryptionKey) {
            data = await this.decrypt(data, decryptionKey, new Uint8Array(record.nonce));
        }
        
        // Verify hash
        const actualHash = await this.sha256(data);
        if (!this.hashEquals(actualHash, new Uint8Array(record.hash))) {
            throw new Error('Integrity check failed');
        }
        
        return data;
    }
    
    // Encryption helpers
    async encrypt(data, key) {
        const nonce = crypto.getRandomValues(new Uint8Array(12));
        const cryptoKey = await crypto.subtle.importKey(
            'raw', key, 'AES-GCM', false, ['encrypt']
        );
        const ciphertext = await crypto.subtle.encrypt(
            { name: 'AES-GCM', iv: nonce },
            cryptoKey,
            data
        );
        return {
            ciphertext: new Uint8Array(ciphertext),
            nonce,
        };
    }
    
    async decrypt(ciphertext, key, nonce) {
        const cryptoKey = await crypto.subtle.importKey(
            'raw', key, 'AES-GCM', false, ['decrypt']
        );
        const plaintext = await crypto.subtle.decrypt(
            { name: 'AES-GCM', iv: nonce },
            cryptoKey,
            ciphertext
        );
        return new Uint8Array(plaintext);
    }
    
    async sha256(data) {
        const hashBuffer = await crypto.subtle.digest('SHA-256', data);
        return new Uint8Array(hashBuffer);
    }
    
    hashEquals(a, b) {
        if (a.length !== b.length) return false;
        for (let i = 0; i < a.length; i++) {
            if (a[i] !== b[i]) return false;
        }
        return true;
    }
}
```

## IPC Protocol

### Get Usage

```rust
/// Get usage request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsGetUsageRequest {
    /// Path to calculate usage for
    pub path: String,
}

/// Get usage response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsGetUsageResponse {
    pub result: Result<StorageUsage, VfsError>,
}
```

### Get Quota

```rust
/// Get quota request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsGetQuotaRequest {
    /// User ID (or None for caller's quota)
    pub user_id: Option<Uuid>,
}

/// Get quota response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfsGetQuotaResponse {
    pub result: Result<StorageQuota, VfsError>,
}
```

## Invariants

1. **Hash integrity**: Content hash matches stored content
2. **Chunk completeness**: All chunks present for chunked files
3. **Quota consistency**: Used bytes matches actual content size
4. **Encryption completeness**: Encrypted files have nonce stored
5. **Atomic updates**: Inode and content updated together

## Security Considerations

1. **Key security**: Encryption keys never stored unencrypted
2. **Nonce uniqueness**: Each encryption uses random nonce
3. **Hash verification**: Content verified on every read
4. **Quota enforcement**: Cannot exceed allocated storage
5. **Chunk isolation**: Chunks keyed by path and index

## WASM Notes

- AES-256-GCM uses SubtleCrypto `encrypt`/`decrypt`
- SHA-256 uses SubtleCrypto `digest`
- Random bytes use `crypto.getRandomValues`
- IndexedDB transactions ensure atomicity
- Large files chunked to avoid memory pressure

## Related Specifications

- [01-database.md](01-database.md) - Database schema
- [02-vfs.md](02-vfs.md) - VFS operations that use storage
- [../05-identity/03-zero-id.md](../05-identity/03-zero-id.md) - Key storage for encryption
