# Database Architecture

> Two-database architecture for kernel and userspace separation.

## Overview

ZOS uses two separate IndexedDB databases to maintain clear separation between kernel/system state and userspace data:

| Database | Purpose | Manager |
|----------|---------|---------|
| `zos-kernel` | Kernel and system state | Kernel |
| `zos-userspace` | Virtual filesystem | VFS Service |

This separation ensures:
1. **Security**: Userspace cannot access kernel state
2. **Isolation**: Filesystem corruption doesn't affect kernel
3. **Portability**: Userspace can be backed up/migrated independently
4. **Clarity**: Clear ownership boundaries

## Database 1: zos-kernel

Managed by the kernel and system services. Contains operational state that should not be directly accessible to user applications.

### Schema

```javascript
// Database: "zos-kernel"
const kernelSchema = {
    name: "zos-kernel",
    version: 1,
    stores: {
        // Process table
        "processes": {
            keyPath: "pid",
            indexes: [
                { name: "status", keyPath: "status" },
                { name: "parent_pid", keyPath: "parent_pid" },
                { name: "user_id", keyPath: "user_id" },
                { name: "session_id", keyPath: "session_id" }
            ]
        },
        
        // Capability table
        "capabilities": {
            keyPath: ["pid", "slot"],
            indexes: [
                { name: "object_type", keyPath: "object_type" },
                { name: "object_id", keyPath: "object_id" }
            ]
        },
        
        // IPC endpoint registry
        "endpoints": {
            keyPath: "endpoint_id",
            indexes: [
                { name: "owner_pid", keyPath: "owner_pid" },
                { name: "service_name", keyPath: "service_name" }
            ]
        },
        
        // Axiom commit log
        "commits": {
            keyPath: "sequence",
            autoIncrement: true,
            indexes: [
                { name: "timestamp", keyPath: "timestamp" },
                { name: "commit_type", keyPath: "commit_type" }
            ]
        },
        
        // Axiom sys log
        "syslog": {
            keyPath: "sequence",
            autoIncrement: true,
            indexes: [
                { name: "timestamp", keyPath: "timestamp" },
                { name: "actor_pid", keyPath: "actor_pid" }
            ]
        },
        
        // System configuration
        "system_config": {
            keyPath: "key"
        },
        
        // Service registry
        "services": {
            keyPath: "service_name",
            indexes: [
                { name: "pid", keyPath: "pid" },
                { name: "service_type", keyPath: "service_type" }
            ]
        }
    }
};
```

### Contents

| Store | Contents | Description |
|-------|----------|-------------|
| `processes` | Process table | PIDs, state, parent, user |
| `capabilities` | Capability table | Per-process capability slots |
| `endpoints` | IPC endpoints | Service registration |
| `commits` | Commit log | State change history |
| `syslog` | System log | Audit trail |
| `system_config` | Config keys | Boot configuration |
| `services` | Service registry | Running services |

### Access Control

Only the kernel and explicitly authorized system services can access `zos-kernel`:

```rust
// Kernel-level access only
fn access_kernel_db() -> Result<Database, DbError> {
    // This is called from kernel context only
    // Userspace processes cannot invoke this
    open_database("zos-kernel")
}
```

## Database 2: zos-userspace

Managed by the VFS service. Contains the entire virtual filesystem including user files.

### Schema

```javascript
// Database: "zos-userspace"
const userspaceSchema = {
    name: "zos-userspace",
    version: 1,
    stores: {
        // Filesystem inodes (metadata)
        "inodes": {
            keyPath: "path",
            indexes: [
                { name: "parent", keyPath: "parent_path" },
                { name: "type", keyPath: "inode_type" },
                { name: "owner", keyPath: "owner_id" },
                { name: "modified", keyPath: "modified_at" },
                { name: "name", keyPath: "name" }
            ]
        },
        
        // File content (separate from metadata)
        "content": {
            keyPath: "path"
            // Schema: { path: string, data: Uint8Array, size: number, hash: string }
        },
        
        // Large file chunks (for files > 1MB)
        "chunks": {
            keyPath: ["path", "chunk_index"]
            // Schema: { path: string, chunk_index: number, data: Uint8Array }
        },
        
        // User quota tracking
        "quotas": {
            keyPath: "user_id",
            indexes: [
                { name: "used_bytes", keyPath: "used_bytes" }
            ]
        }
    }
};
```

### Contents

| Store | Contents | Description |
|-------|----------|-------------|
| `inodes` | File/directory metadata | Paths, permissions, ownership |
| `content` | File content | Binary data for small files |
| `chunks` | Large file chunks | Chunked storage for big files |
| `quotas` | Quota tracking | Per-user storage usage |

### Inode Record

```typescript
interface InodeRecord {
    // Primary key
    path: string;              // e.g., "/home/abc123/Documents/file.txt"
    
    // Hierarchy
    parent_path: string;       // e.g., "/home/abc123/Documents"
    name: string;              // e.g., "file.txt"
    
    // Type
    inode_type: "file" | "directory" | "symlink";
    symlink_target?: string;   // For symlinks only
    
    // Ownership
    owner_id: string | null;   // User UUID or null for system
    
    // Permissions
    permissions: {
        owner_read: boolean;
        owner_write: boolean;
        owner_execute: boolean;
        system_read: boolean;
        system_write: boolean;
        world_read: boolean;
        world_write: boolean;
    };
    
    // Timestamps (nanos since epoch)
    created_at: number;
    modified_at: number;
    accessed_at: number;
    
    // File metadata
    size: number;              // 0 for directories
    encrypted: boolean;        // Is content encrypted?
    content_hash?: string;     // SHA-256 of content (files only)
}
```

### Content Record

```typescript
interface ContentRecord {
    // Primary key
    path: string;              // Matches inode path
    
    // Content
    data: Uint8Array;          // File content (or first chunk if large)
    
    // Metadata
    size: number;              // Total size in bytes
    hash: string;              // SHA-256 hash
    
    // Chunking (if applicable)
    chunked: boolean;          // Is this file chunked?
    chunk_count?: number;      // Number of chunks if chunked
}
```

### Chunk Record

```typescript
interface ChunkRecord {
    // Composite primary key
    path: string;              // File path
    chunk_index: number;       // 0-based chunk index
    
    // Content
    data: Uint8Array;          // Chunk data (max 1MB each)
}
```

## Database Operations

### Opening Databases

```javascript
class DatabaseManager {
    constructor() {
        this.kernelDb = null;
        this.userspaceDb = null;
    }
    
    async initKernel() {
        const request = indexedDB.open('zos-kernel', 1);
        
        request.onupgradeneeded = (event) => {
            const db = event.target.result;
            
            // Create process store
            const processes = db.createObjectStore('processes', { keyPath: 'pid' });
            processes.createIndex('status', 'status');
            processes.createIndex('parent_pid', 'parent_pid');
            processes.createIndex('user_id', 'user_id');
            
            // Create capabilities store
            const caps = db.createObjectStore('capabilities', { keyPath: ['pid', 'slot'] });
            caps.createIndex('object_type', 'object_type');
            caps.createIndex('object_id', 'object_id');
            
            // Create endpoints store
            const endpoints = db.createObjectStore('endpoints', { keyPath: 'endpoint_id' });
            endpoints.createIndex('owner_pid', 'owner_pid');
            endpoints.createIndex('service_name', 'service_name');
            
            // Create commit log store
            const commits = db.createObjectStore('commits', { 
                keyPath: 'sequence', 
                autoIncrement: true 
            });
            commits.createIndex('timestamp', 'timestamp');
            commits.createIndex('commit_type', 'commit_type');
            
            // Create syslog store
            const syslog = db.createObjectStore('syslog', {
                keyPath: 'sequence',
                autoIncrement: true
            });
            syslog.createIndex('timestamp', 'timestamp');
            syslog.createIndex('actor_pid', 'actor_pid');
            
            // Create config store
            db.createObjectStore('system_config', { keyPath: 'key' });
            
            // Create services store
            const services = db.createObjectStore('services', { keyPath: 'service_name' });
            services.createIndex('pid', 'pid');
        };
        
        this.kernelDb = await this.promisify(request);
    }
    
    async initUserspace() {
        const request = indexedDB.open('zos-userspace', 1);
        
        request.onupgradeneeded = (event) => {
            const db = event.target.result;
            
            // Create inodes store
            const inodes = db.createObjectStore('inodes', { keyPath: 'path' });
            inodes.createIndex('parent', 'parent_path');
            inodes.createIndex('type', 'inode_type');
            inodes.createIndex('owner', 'owner_id');
            inodes.createIndex('modified', 'modified_at');
            inodes.createIndex('name', 'name');
            
            // Create content store
            db.createObjectStore('content', { keyPath: 'path' });
            
            // Create chunks store
            db.createObjectStore('chunks', { keyPath: ['path', 'chunk_index'] });
            
            // Create quotas store
            const quotas = db.createObjectStore('quotas', { keyPath: 'user_id' });
            quotas.createIndex('used_bytes', 'used_bytes');
        };
        
        this.userspaceDb = await this.promisify(request);
    }
    
    promisify(request) {
        return new Promise((resolve, reject) => {
            request.onsuccess = () => resolve(request.result);
            request.onerror = () => reject(request.error);
        });
    }
}
```

### Transactions

```javascript
class UserspaceStorage {
    constructor(db) {
        this.db = db;
    }
    
    // Write a file (inode + content in single transaction)
    async writeFile(path, content, permissions, ownerId) {
        const tx = this.db.transaction(['inodes', 'content'], 'readwrite');
        const inodes = tx.objectStore('inodes');
        const contents = tx.objectStore('content');
        
        const now = Date.now() * 1_000_000; // Convert to nanos
        const parentPath = path.substring(0, path.lastIndexOf('/')) || '/';
        const name = path.substring(path.lastIndexOf('/') + 1);
        const hash = await this.hashContent(content);
        
        // Create/update inode
        const inode = {
            path,
            parent_path: parentPath,
            name,
            inode_type: 'file',
            owner_id: ownerId,
            permissions,
            created_at: now,
            modified_at: now,
            accessed_at: now,
            size: content.byteLength,
            encrypted: false,
            content_hash: hash,
        };
        
        // Store inode
        await this.promisify(inodes.put(inode));
        
        // Store content
        const contentRecord = {
            path,
            data: content,
            size: content.byteLength,
            hash,
            chunked: false,
        };
        await this.promisify(contents.put(contentRecord));
        
        // Wait for transaction to complete
        await this.txComplete(tx);
    }
    
    // Read a file
    async readFile(path) {
        const tx = this.db.transaction(['inodes', 'content'], 'readonly');
        const inodes = tx.objectStore('inodes');
        const contents = tx.objectStore('content');
        
        // Get inode
        const inode = await this.promisify(inodes.get(path));
        if (!inode) {
            throw new Error('File not found');
        }
        
        if (inode.inode_type !== 'file') {
            throw new Error('Not a file');
        }
        
        // Get content
        const content = await this.promisify(contents.get(path));
        if (!content) {
            throw new Error('Content not found');
        }
        
        // Handle chunked files
        if (content.chunked) {
            return await this.readChunkedFile(path, content.chunk_count);
        }
        
        return content.data;
    }
    
    // Read directory entries
    async readdir(path) {
        const tx = this.db.transaction('inodes', 'readonly');
        const inodes = tx.objectStore('inodes');
        const index = inodes.index('parent');
        
        const entries = [];
        const range = IDBKeyRange.only(path);
        
        return new Promise((resolve, reject) => {
            const request = index.openCursor(range);
            request.onsuccess = (event) => {
                const cursor = event.target.result;
                if (cursor) {
                    entries.push({
                        name: cursor.value.name,
                        path: cursor.value.path,
                        is_directory: cursor.value.inode_type === 'directory',
                        size: cursor.value.size,
                        modified_at: cursor.value.modified_at,
                    });
                    cursor.continue();
                } else {
                    resolve(entries);
                }
            };
            request.onerror = () => reject(request.error);
        });
    }
}
```

## Separation Rationale

| Concern | Database | Rationale |
|---------|----------|-----------|
| Process state | `zos-kernel` | Kernel-managed, not user-accessible |
| Capabilities | `zos-kernel` | Security-critical, kernel-only |
| IPC endpoints | `zos-kernel` | System infrastructure |
| Commit log | `zos-kernel` | Integrity-critical audit trail |
| User files | `zos-userspace` | User data, permission-controlled |
| Identity keys | `zos-userspace` | Stored as files in user home |
| System config | `zos-kernel` | Boot-time, kernel-managed |
| User preferences | `zos-userspace` | Stored as files in user home |

## Migration and Versioning

### Schema Version Upgrades

```javascript
class DatabaseMigration {
    static async migrateKernel(db, oldVersion, newVersion) {
        // Version 1 -> 2 migration example
        if (oldVersion < 2 && newVersion >= 2) {
            // Add new index to processes
            const tx = db.transaction('processes', 'readwrite');
            const store = tx.objectStore('processes');
            store.createIndex('created_at', 'created_at');
        }
    }
    
    static async migrateUserspace(db, oldVersion, newVersion) {
        // Version 1 -> 2 migration example
        if (oldVersion < 2 && newVersion >= 2) {
            // Add encryption key store
            db.createObjectStore('encryption_keys', { keyPath: 'user_id' });
        }
    }
}
```

## Backup and Restore

The `zos-userspace` database can be exported for backup:

```javascript
class BackupManager {
    // Export userspace to transferable format
    async exportUserspace() {
        const tx = this.db.transaction(['inodes', 'content', 'chunks'], 'readonly');
        
        const inodes = await this.getAllRecords(tx.objectStore('inodes'));
        const content = await this.getAllRecords(tx.objectStore('content'));
        const chunks = await this.getAllRecords(tx.objectStore('chunks'));
        
        return {
            version: 1,
            exported_at: Date.now(),
            inodes,
            content,
            chunks,
        };
    }
    
    // Import userspace from backup
    async importUserspace(backup) {
        // Validate backup format
        if (backup.version !== 1) {
            throw new Error('Unsupported backup version');
        }
        
        const tx = this.db.transaction(['inodes', 'content', 'chunks'], 'readwrite');
        
        // Clear existing data
        await this.clearStore(tx.objectStore('inodes'));
        await this.clearStore(tx.objectStore('content'));
        await this.clearStore(tx.objectStore('chunks'));
        
        // Import data
        for (const inode of backup.inodes) {
            tx.objectStore('inodes').put(inode);
        }
        for (const c of backup.content) {
            tx.objectStore('content').put(c);
        }
        for (const chunk of backup.chunks) {
            tx.objectStore('chunks').put(chunk);
        }
        
        await this.txComplete(tx);
    }
}
```

## Invariants

1. **Separation**: Kernel DB is never accessed from userspace
2. **Consistency**: Inode and content records are updated atomically
3. **Integrity**: Content hashes match actual content
4. **Uniqueness**: Paths are unique within inodes
5. **Hierarchy**: Parent paths exist for all non-root inodes

## Related Specifications

- [02-vfs.md](02-vfs.md) - VFS operations using the database
- [03-storage.md](03-storage.md) - Storage service implementation
- [../02-axiom/02-commitlog.md](../02-axiom/02-commitlog.md) - Commit log in kernel DB
