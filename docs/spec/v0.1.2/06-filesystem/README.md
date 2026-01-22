# Filesystem Layer

> Virtual filesystem with hierarchical paths, user home directories, and persistent storage.

## Overview

The Filesystem layer provides a canonical hierarchical filesystem for Zero OS. Unlike traditional filesystems, ZOS filesystem is:

1. **Virtual**: Abstraction over IndexedDB (WASM) or native storage
2. **User-centric**: Each user has an isolated home directory
3. **Permission-aware**: File access controlled by ownership and permissions
4. **Encryption-ready**: Support for encrypted file storage

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            Filesystem Layer                                   │
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                         VFS Service                                    │   │
│  │                                                                       │   │
│  │  • Path resolution      • Permission checking                         │   │
│  │  • Inode management     • Directory operations                        │   │
│  │  • File read/write      • Metadata operations                         │   │
│  └────────────────────────────────┬──────────────────────────────────────┘   │
│                                   │                                          │
│                                   ▼                                          │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                       Storage Backend                                  │   │
│  │                                                                       │   │
│  │  ┌─────────────────────────┐  ┌─────────────────────────┐            │   │
│  │  │   zos-userspace DB      │  │   Content Store         │            │   │
│  │  │                         │  │                         │            │   │
│  │  │  • Inodes (metadata)    │  │  • File content blobs   │            │   │
│  │  │  • Directory entries    │  │  • Encrypted content    │            │   │
│  │  │  • Path index          │  │  • Content hashes       │            │   │
│  │  └─────────────────────────┘  └─────────────────────────┘            │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Files

| File | Description |
|------|-------------|
| [01-database.md](01-database.md) | Two-database architecture (zos-kernel, zos-userspace) |
| [02-vfs.md](02-vfs.md) | Virtual filesystem, inodes, path resolution |
| [03-storage.md](03-storage.md) | File operations, encryption, quota management |

## Canonical Hierarchy

```
/                                    # Root (system-owned)
├── system/                          # System-wide configuration
│   ├── config/                      # Global settings
│   │   └── machine.json            # Machine ID, boot config
│   └── services/                    # Service configurations
│       └── {service_name}.json
├── users/                           # User registry
│   └── registry.json               # List of user IDs and metadata
├── tmp/                            # Temporary files (cleared on boot)
└── home/                           # User home directories
    └── {user_id}/                  # Per-user root (UUID)
        ├── .zos/                   # ZOS system data (hidden)
        │   ├── identity/           # Identity & cryptographic material
        │   ├── sessions/           # Active sessions
        │   ├── credentials/        # Linked credentials
        │   ├── tokens/             # Token families
        │   └── config/             # User ZOS settings
        ├── Documents/
        ├── Downloads/
        ├── Desktop/
        ├── Pictures/
        ├── Music/
        └── Apps/                   # Per-app data directories
            └── {app_id}/
```

## Path Conventions

| Path Pattern | Owner | Description |
|--------------|-------|-------------|
| `/system/**` | system | System configuration, read-only for users |
| `/users/registry.json` | system | User registry, read-only for users |
| `/tmp/**` | system | Temporary storage, world-writable |
| `/home/{user_id}/**` | user | User's home directory |
| `/home/{user_id}/.zos/**` | user | Hidden ZOS system data |
| `/home/{user_id}/Apps/{app_id}/**` | app | App-specific data within user context |

## IPC Protocol

### VFS Service Messages

```rust
pub mod vfs_msg {
    // Directory Operations
    pub const MSG_VFS_MKDIR: u32 = 0x8000;
    pub const MSG_VFS_MKDIR_RESPONSE: u32 = 0x8001;
    pub const MSG_VFS_RMDIR: u32 = 0x8002;
    pub const MSG_VFS_RMDIR_RESPONSE: u32 = 0x8003;
    pub const MSG_VFS_READDIR: u32 = 0x8004;
    pub const MSG_VFS_READDIR_RESPONSE: u32 = 0x8005;
    
    // File Operations
    pub const MSG_VFS_WRITE: u32 = 0x8010;
    pub const MSG_VFS_WRITE_RESPONSE: u32 = 0x8011;
    pub const MSG_VFS_READ: u32 = 0x8012;
    pub const MSG_VFS_READ_RESPONSE: u32 = 0x8013;
    pub const MSG_VFS_UNLINK: u32 = 0x8014;
    pub const MSG_VFS_UNLINK_RESPONSE: u32 = 0x8015;
    pub const MSG_VFS_RENAME: u32 = 0x8016;
    pub const MSG_VFS_RENAME_RESPONSE: u32 = 0x8017;
    pub const MSG_VFS_COPY: u32 = 0x8018;
    pub const MSG_VFS_COPY_RESPONSE: u32 = 0x8019;
    
    // Metadata Operations
    pub const MSG_VFS_STAT: u32 = 0x8020;
    pub const MSG_VFS_STAT_RESPONSE: u32 = 0x8021;
    pub const MSG_VFS_EXISTS: u32 = 0x8022;
    pub const MSG_VFS_EXISTS_RESPONSE: u32 = 0x8023;
    pub const MSG_VFS_CHMOD: u32 = 0x8024;
    pub const MSG_VFS_CHMOD_RESPONSE: u32 = 0x8025;
    pub const MSG_VFS_CHOWN: u32 = 0x8026;
    pub const MSG_VFS_CHOWN_RESPONSE: u32 = 0x8027;
    
    // Quota Operations
    pub const MSG_VFS_GET_USAGE: u32 = 0x8030;
    pub const MSG_VFS_GET_USAGE_RESPONSE: u32 = 0x8031;
    pub const MSG_VFS_GET_QUOTA: u32 = 0x8032;
    pub const MSG_VFS_GET_QUOTA_RESPONSE: u32 = 0x8033;
}
```

## Service Discovery

Applications discover the VFS service via init:

```rust
// Look up VFS service
let vfs_ep = lookup_service("vfs")?;

// Read a file
let request = VfsReadRequest {
    path: "/home/user-id/Documents/file.txt".to_string(),
    offset: 0,
    length: None,  // Read entire file
};
send(vfs_ep, MSG_VFS_READ, &encode(&request));
let response = receive_reply();
let content = decode::<VfsReadResponse>(&response.data)?;
```

## Boot Initialization

On system boot, the VFS service initializes the root filesystem:

```rust
impl VfsService {
    async fn init_filesystem(&self) -> Result<(), VfsError> {
        // Check if already initialized
        if self.exists("/system")? {
            // Clean temporary directory
            self.clean_tmp().await?;
            return Ok(());
        }
        
        // Create root directories
        self.mkdir_with_perms("/system", FilePermissions::system_only())?;
        self.mkdir_with_perms("/system/config", FilePermissions::system_only())?;
        self.mkdir_with_perms("/system/services", FilePermissions::system_only())?;
        self.mkdir_with_perms("/users", FilePermissions::system_only())?;
        self.mkdir_with_perms("/tmp", FilePermissions::world_rw())?;
        self.mkdir_with_perms("/home", FilePermissions::system_only())?;
        
        // Initialize machine config
        let machine_config = MachineConfig {
            machine_id: Uuid::new_v4(),
            created_at: current_timestamp(),
            boot_count: 1,
        };
        self.write_file(
            "/system/config/machine.json",
            &serde_json::to_vec(&machine_config)?,
        )?;
        
        // Initialize empty user registry
        let registry = UserRegistry { users: vec![] };
        self.write_file(
            "/users/registry.json",
            &serde_json::to_vec(&registry)?,
        )?;
        
        Ok(())
    }
    
    async fn clean_tmp(&self) -> Result<(), VfsError> {
        let entries = self.readdir("/tmp")?;
        for entry in entries {
            let path = format!("/tmp/{}", entry.name);
            if entry.is_directory {
                self.rmdir_recursive(&path)?;
            } else {
                self.unlink(&path)?;
            }
        }
        Ok(())
    }
}
```

## Permission Model

Files and directories have Unix-like permissions:

```rust
pub struct FilePermissions {
    pub owner_read: bool,
    pub owner_write: bool,
    pub owner_execute: bool,
    pub system_read: bool,
    pub system_write: bool,
    pub world_read: bool,
    pub world_write: bool,
}
```

Access is determined by:
1. **Owner**: User who owns the file (via `owner_id`)
2. **System**: Processes with system classification
3. **World**: All other processes

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| **Hierarchical paths** | Familiar Unix-like semantics |
| **UUID-based home dirs** | Avoids name collisions, enables rename |
| **Separate content store** | Efficient for large files, deduplication |
| **Two databases** | Clear kernel/userspace separation |
| **Per-user quota** | Fair resource allocation |
| **Encryption support** | Privacy for sensitive files |

## WASM Implementation

- VFS metadata stored in `zos-userspace` IndexedDB database
- File content stored in IndexedDB `content` object store
- Large files chunked for efficient storage
- Encryption uses SubtleCrypto AES-GCM

## Related Specifications

- [../USERSPACE.md](../USERSPACE.md) - Userspace layer overview
- [../05-identity/01-users.md](../05-identity/01-users.md) - User home directory creation
- [../03-kernel/03-capabilities.md](../03-kernel/03-capabilities.md) - Capability-based access control
