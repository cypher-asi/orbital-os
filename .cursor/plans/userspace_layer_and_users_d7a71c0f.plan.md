---
name: Userspace Layer - Specification and Implementation
overview: Update specifications in docs/spec/v0.1.2/ and implement the Userspace layer (layers 05-08). Each layer has a specification phase followed by an implementation phase. Layers are implemented bottom-up starting with Identity (05), then Filesystem (06), Applications (07), and Desktop (08).
todos:
  - id: phase0-userspace-readme
    content: "[SPEC] Update docs/spec/v0.1.2/USERSPACE.md overview document explaining the Userspace concept (layers 05-08)"
    status: completed
  - id: phase0-cleanup-old-runtime
    content: "[SPEC] Update docs/spec/v0.1.2/README.md with userspace layer links and references"
    status: completed
  - id: phase1a-identity-folder
    content: "[SPEC] Update docs/spec/v0.1.2/05-identity/README.md"
    status: completed
  - id: phase1a-identity-users-spec
    content: "[SPEC] Update docs/spec/v0.1.2/05-identity/01-users.md with User struct, UserStatus, UserPreferences"
    status: completed
  - id: phase1a-identity-sessions-spec
    content: "[SPEC] Update docs/spec/v0.1.2/05-identity/02-sessions.md with LocalSession, RemoteAuthState, session management"
    status: completed
  - id: phase1a-identity-zero-id-spec
    content: "[SPEC] Update docs/spec/v0.1.2/05-identity/03-zero-id.md with LocalKeyStore, credentials, file-based storage"
    status: completed
  - id: phase1a-identity-permissions-spec
    content: "[SPEC] Update docs/spec/v0.1.2/05-identity/04-permissions.md with capability policy enforcement"
    status: completed
  - id: phase1b-identity-crate
    content: "[IMPL] Create zos-identity crate with Cargo.toml, lib.rs, and module structure"
    status: completed
  - id: phase1b-identity-types
    content: "[IMPL] Implement User, UserStatus, UserPreferences types in zos-identity/types.rs"
    status: completed
  - id: phase1b-identity-sessions
    content: "[IMPL] Implement LocalSession, SessionMetadata, RemoteAuthState in zos-identity/session.rs"
    status: completed
  - id: phase1b-identity-keystore
    content: "[IMPL] Implement LocalKeyStore, key storage/retrieval in zos-identity/keystore.rs"
    status: completed
  - id: phase1b-identity-service
    content: "[IMPL] Implement UserService trait and in-memory implementation"
    status: completed
  - id: phase1b-identity-ipc
    content: "[IMPL] Implement identity IPC protocol messages (MSG_CREATE_USER, MSG_LOGIN, etc.)"
    status: completed
  - id: phase1b-identity-tests
    content: "[IMPL] Add unit tests for identity types, session management, and service"
    status: completed
  - id: phase2a-filesystem-folder
    content: "[SPEC] Update docs/spec/v0.1.2/06-filesystem/README.md"
    status: completed
  - id: phase2a-fs-database-spec
    content: "[SPEC] Update docs/spec/v0.1.2/06-filesystem/01-database.md with two-database architecture"
    status: completed
  - id: phase2a-fs-vfs-spec
    content: "[SPEC] Update docs/spec/v0.1.2/06-filesystem/02-vfs.md with canonical hierarchy, Inode, VfsService"
    status: completed
  - id: phase2a-fs-storage-spec
    content: "[SPEC] Update docs/spec/v0.1.2/06-filesystem/03-storage.md with file operations, encryption, quotas"
    status: completed
  - id: phase2b-filesystem-crate
    content: "[IMPL] Create zos-vfs crate with Cargo.toml, lib.rs, and module structure"
    status: completed
  - id: phase2b-vfs-types
    content: "[IMPL] Implement Inode, InodeType, FilePermissions, DirEntry in zos-vfs/types.rs"
    status: completed
  - id: phase2b-vfs-service
    content: "[IMPL] Implement VfsService trait with directory and file operations"
    status: completed
  - id: phase2b-vfs-indexeddb
    content: "[IMPL] Implement IndexedDB backend for VFS (zos-userspace database)"
    status: pending
  - id: phase2b-vfs-bootstrap
    content: "[IMPL] Implement filesystem bootstrap (init_filesystem, clean_tmp)"
    status: completed
  - id: phase2b-vfs-ipc
    content: "[IMPL] Implement VFS IPC protocol messages (MSG_VFS_MKDIR, MSG_VFS_READ, etc.)"
    status: completed
  - id: phase2b-vfs-tests
    content: "[IMPL] Add unit tests for VFS types, operations, and IndexedDB backend"
    status: completed
  - id: phase2b-vfs-memory
    content: "[IMPL] Implement in-memory VFS backend for testing"
    status: completed
  - id: phase3a-apps-review
    content: "[SPEC] Review and update docs/spec/v0.1.2/07-applications/ specs for userspace integration"
    status: completed
  - id: phase3b-apps-identity-integration
    content: "[IMPL] Integrate identity layer with app manifest permissions"
    status: completed
  - id: phase3b-apps-vfs-integration
    content: "[IMPL] Integrate VFS with app data directories (/home/{user}/Apps/{app_id}/)"
    status: completed
  - id: phase3b-apps-tests
    content: "[IMPL] Add integration tests for app identity and storage"
    status: pending
  - id: phase4a-desktop-review
    content: "[SPEC] Review and update docs/spec/v0.1.2/08-desktop/ specs for userspace integration"
    status: completed
  - id: phase4b-desktop-identity-ui
    content: "[IMPL] Implement user login/logout UI in desktop compositor"
    status: completed
  - id: phase4b-desktop-session-mgmt
    content: "[IMPL] Integrate session management with desktop (user switching, session display)"
    status: completed
  - id: phase4b-desktop-tests
    content: "[IMPL] Add integration tests for desktop identity features"
    status: pending
  - id: phase5-move-process-to-init
    content: "[SPEC] Update docs/spec/v0.1.2/04-init/03-process-manager.md with process manager spec content"
    status: pending
  - id: phase5-end-to-end-tests
    content: "[IMPL] Add end-to-end tests for user creation → login → app launch → file access"
    status: pending
  - id: phase5-update-docs
    content: "[DOCS] Update docs/spec/v0.1.2/README.md with Userspace layer diagram and links"
    status: pending
---

# Userspace Layer Specification & Implementation

## Summary

This plan updates the specifications in **docs/spec/v0.1.2/** and implements the **Userspace layer** (layers 05-08). Each layer follows a two-phase approach: specification (A) followed by implementation (B). Layers are built bottom-up.

**Implementation Phases:**

| Phase | Layer | Spec (A) | Implementation (B) |

|-------|-------|----------|-------------------|

| 0 | Foundation | USERSPACE.md, cleanup | - |

| 1 | Identity (05) | Users, sessions, Zero-ID, permissions | zos-identity crate |

| 2 | Filesystem (06) | Database, VFS, storage | zos-vfs crate |

| 3 | Applications (07) | Review existing | Identity/VFS integration |

| 4 | Desktop (08) | Review existing | Login UI, session management |

| 5 | Integration | Process manager move | End-to-end tests, docs |

**Why this order?** Identity must exist before filesystem (files are owned by users). Filesystem must exist before applications (apps store data in user directories). Applications must exist before desktop (desktop launches apps).

**Userspace Architecture:**

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            USERSPACE (Layers 05-08)                          │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │  Layer 08: Desktop/Compositor                    [08-desktop/]          │ │
│  │            Window management, input routing, visual shell               │ │
│  ├────────────────────────────────────────────────────────────────────────┤ │
│  │  Layer 07: Applications                          [07-applications/]     │ │
│  │            Sandboxed user applications, app model                       │ │
│  ├────────────────────────────────────────────────────────────────────────┤ │
│  │  Layer 06: Filesystem                            [06-filesystem/]       │ │
│  │            VFS, storage services, user home directories                 │ │
│  ├────────────────────────────────────────────────────────────────────────┤ │
│  │  Layer 05: Identity                              [05-identity/]         │ │
│  │            Users, sessions, Zero-ID, permissions                        │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  Layer 04: Init Process             [04-init/]                               │
│            Bootstrap, service supervision, process manager                   │
├─────────────────────────────────────────────────────────────────────────────┤
│  Layer 03: Microkernel              [03-kernel/]                             │
│            Capabilities, threads, VMM, IPC, interrupts                       │
├─────────────────────────────────────────────────────────────────────────────┤
│  Layer 02: Axiom (Verification)     [02-axiom/]                              │
│            SysLog (audit), CommitLog (replay), sender verification           │
├─────────────────────────────────────────────────────────────────────────────┤
│  Layers 00-01: Boot & HAL           [00-boot/, 01-hal/]                      │
│                Platform abstraction                                          │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Key Architectural Decisions:**

1. **Userspace as a concept**: Layers 05-08 form a cohesive "Userspace" that sits above kernel/init
2. **Two IndexedDB databases**: `zos-kernel` for kernel/system state, `zos-userspace` for filesystem
3. **Canonical filesystem**: Hierarchical VFS with user home directories in `/home/{user_id}/`
4. **File-based identity storage**: All identity material stored as files within the VFS
5. **Process Manager in Init**: Process lifecycle is a supervision responsibility, not a separate layer

---

## Part 1: Target Folder Structure

### Userspace Layer in docs/spec/v0.1.2/

```
docs/spec/v0.1.2/
├── USERSPACE.md                 # Userspace concept overview
├── ...
├── 04-init/
│   ├── README.md
│   ├── 01-bootstrap.md
│   ├── 02-supervision.md
│   └── 03-process-manager.md    # Process lifecycle
├── 05-identity/                 # Identity layer
│   ├── README.md
│   ├── 01-users.md              # User primitive, home directories
│   ├── 02-sessions.md           # Local/remote sessions
│   ├── 03-zero-id.md            # Zero-ID integration, key storage
│   └── 04-permissions.md        # Capability policy
├── 06-filesystem/               # Filesystem layer
│   ├── README.md
│   ├── 01-database.md           # Two-database architecture
│   ├── 02-vfs.md                # Virtual filesystem, inodes
│   └── 03-storage.md            # File operations, encryption
├── 07-applications/             # Applications layer
│   └── ...
└── 08-desktop/                  # Desktop layer
    └── ...
```

---

## Part 2: Two-Database Architecture

ZOS uses two separate IndexedDB databases to maintain clear separation:

### Database 1: `zos-kernel` (Kernel & System)

Managed by the kernel and system services. Contains:

- Process table and state
- Capability tables
- IPC endpoint registry
- Axiom commit log
- System configuration
- Service registry
```javascript
// Database: "zos-kernel"
const kernelSchema = {
    name: "zos-kernel",
    version: 1,
    stores: {
        "processes": {
            keyPath: "pid",
            indexes: [
                { name: "status", keyPath: "status" },
                { name: "parent_pid", keyPath: "parent_pid" }
            ]
        },
        "capabilities": {
            keyPath: ["pid", "slot"],
            indexes: [
                { name: "object_type", keyPath: "object_type" }
            ]
        },
        "endpoints": {
            keyPath: "endpoint_id",
            indexes: [
                { name: "owner_pid", keyPath: "owner_pid" }
            ]
        },
        "commits": {
            keyPath: "sequence",
            autoIncrement: true
        },
        "system_config": {
            keyPath: "key"
        },
        "services": {
            keyPath: "service_name",
            indexes: [
                { name: "pid", keyPath: "pid" }
            ]
        }
    }
};
```


### Database 2: `zos-userspace` (Userspace Filesystem)

Managed by the VFS service. Contains the entire virtual filesystem:

```javascript
// Database: "zos-userspace"
const userspaceSchema = {
    name: "zos-userspace",
    version: 1,
    stores: {
        "inodes": {
            keyPath: "path",
            indexes: [
                { name: "parent", keyPath: "parent_path" },
                { name: "type", keyPath: "inode_type" },
                { name: "owner", keyPath: "owner_id" },
                { name: "modified", keyPath: "modified_at" }
            ]
        },
        "content": {
            keyPath: "path"
            // Stores: { path, data: Uint8Array, size, hash }
        }
    }
};
```

### Separation Rationale

| Concern | Database | Rationale |

|---------|----------|-----------|

| Process state | `zos-kernel` | Kernel-managed, not user-accessible |

| Capabilities | `zos-kernel` | Security-critical, kernel-only |

| User files | `zos-userspace` | User data, permission-controlled |

| Identity keys | `zos-userspace` | Stored as files in user home |

| System config | `zos-kernel` | Boot-time, kernel-managed |

| User preferences | `zos-userspace` | Stored as files in user home |

---

## Part 3: Canonical Filesystem Hierarchy

The userspace VFS provides a hierarchical filesystem abstraction:

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
        │   │   ├── user.json       # User record
        │   │   ├── public_keys.json
        │   │   ├── private_keys.enc
        │   │   └── machine/
        │   │       └── {machine_id}.json
        │   ├── sessions/           # Active sessions
        │   │   └── {session_id}.json
        │   ├── credentials/        # Linked credentials
        │   │   └── credentials.json
        │   ├── tokens/             # Token families
        │   │   └── {family_id}.json
        │   └── config/             # User ZOS settings
        │       └── preferences.json
        ├── Documents/
        ├── Downloads/
        ├── Desktop/
        ├── Pictures/
        ├── Music/
        └── Apps/                   # Per-app data directories
            └── {app_id}/
                ├── config/
                ├── data/
                └── cache/
```

### Path Conventions

| Path Pattern | Owner | Description |

|--------------|-------|-------------|

| `/system/**` | system | System configuration, read-only for users |

| `/users/registry.json` | system | User registry, read-only for users |

| `/tmp/**` | system | Temporary storage, world-writable |

| `/home/{user_id}/**` | user | User's home directory |

| `/home/{user_id}/.zos/**` | user | Hidden ZOS system data |

| `/home/{user_id}/Apps/{app_id}/**` | app | App-specific data within user context |

---

## Part 4: 05-identity Specifications

### 01-users.md - User Primitive

```rust
use uuid::Uuid;
use serde::{Serialize, Deserialize};

/// A ZOS user backed by a zero-id Identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    /// Local user ID (matches zero-id identity_id)
    pub id: Uuid,
    
    /// Display name for UI
    pub display_name: String,
    
    /// User status in the system
    pub status: UserStatus,
    
    /// Default namespace for this user's resources
    pub default_namespace_id: Uuid,
    
    /// When the user was created locally
    pub created_at: u64,
    
    /// Last activity timestamp
    pub last_active_at: u64,
}

impl User {
    pub fn home_dir(&self) -> String {
        format!("/home/{}", self.id)
    }
    
    pub fn zos_dir(&self) -> String {
        format!("/home/{}/.zos", self.id)
    }
    
    pub fn identity_dir(&self) -> String {
        format!("/home/{}/.zos/identity", self.id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserStatus {
    Active,    // Has at least one local session
    Offline,   // Exists but no active sessions
    Suspended, // Account is suspended
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserPreferences {
    pub theme: Option<String>,
    pub locale: Option<String>,
    pub wallpaper: Option<String>,
    pub custom: BTreeMap<String, String>,
}
```

### 02-sessions.md - Session Management

```rust
/// A local ZOS session - works fully offline.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub machine_id: Uuid,
    pub created_at: u64,
    pub expires_at: u64,
    pub process_ids: Vec<u32>,
    pub remote_auth: Option<RemoteAuthState>,
    pub mfa_verified: bool,
    pub capabilities: Vec<String>,
    pub metadata: SessionMetadata,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub location_hint: Option<String>,
    pub last_activity_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteAuthState {
    pub server_endpoint: String,
    pub access_token: String,
    pub token_expires_at: u64,
    pub refresh_token: Option<String>,
    pub scopes: Vec<String>,
    pub token_family_id: Uuid,
}
```

### 03-zero-id.md - Zero-ID Integration

All identity material stored as files within the VFS:

| Data | Path | Encrypted |

|------|------|-----------|

| User record | `/home/{id}/.zos/identity/user.json` | No |

| Public keys | `/home/{id}/.zos/identity/public_keys.json` | No |

| Private keys | `/home/{id}/.zos/identity/private_keys.enc` | Yes |

| Machine keys | `/home/{id}/.zos/identity/machine/{machine_id}.json` | Partial |

| Sessions | `/home/{id}/.zos/sessions/{session_id}.json` | No |

| Credentials | `/home/{id}/.zos/credentials/credentials.json` | Partial |

```rust
/// Local storage for user cryptographic material (public keys).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalKeyStore {
    pub user_id: Uuid,
    pub identity_signing_public_key: [u8; 32],
    pub machine_signing_public_key: [u8; 32],
    pub machine_encryption_public_key: [u8; 32],
    pub key_scheme: KeyScheme,
    pub capabilities: MachineKeyCapabilities,
    pub epoch: u64,
    // PQ keys (only if PqHybrid scheme)
    pub pq_signing_public_key: Option<Vec<u8>>,
    pub pq_encryption_public_key: Option<Vec<u8>>,
}
```

### 04-permissions.md - Capability Policy

Moved from 05-runtime/02-permissions.md. Defines how capabilities are granted and revoked based on user identity and app manifests.

---

## Part 5: 06-filesystem Specifications

### 01-database.md - Two-Database Architecture

Documents the separation between `zos-kernel` and `zos-userspace` IndexedDB databases.

### 02-vfs.md - Virtual Filesystem

```rust
/// Virtual filesystem inode.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inode {
    pub path: String,
    pub parent_path: String,
    pub name: String,
    pub inode_type: InodeType,
    pub owner_id: Option<Uuid>,
    pub permissions: FilePermissions,
    pub created_at: u64,
    pub modified_at: u64,
    pub accessed_at: u64,
    pub size: u64,
    pub encrypted: bool,
    pub content_hash: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum InodeType {
    File,
    Directory,
    SymLink { target: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

### VfsService Trait

```rust
pub trait VfsService {
    // Directory Operations
    fn mkdir(&self, path: &str) -> Result<(), VfsError>;
    fn mkdir_p(&self, path: &str) -> Result<(), VfsError>;
    fn rmdir(&self, path: &str) -> Result<(), VfsError>;
    fn readdir(&self, path: &str) -> Result<Vec<DirEntry>, VfsError>;
    
    // File Operations
    fn write_file(&self, path: &str, content: &[u8]) -> Result<(), VfsError>;
    fn write_file_encrypted(&self, path: &str, content: &[u8], key: &[u8; 32]) -> Result<(), VfsError>;
    fn read_file(&self, path: &str) -> Result<Vec<u8>, VfsError>;
    fn read_file_encrypted(&self, path: &str, key: &[u8; 32]) -> Result<Vec<u8>, VfsError>;
    fn unlink(&self, path: &str) -> Result<(), VfsError>;
    fn rename(&self, from: &str, to: &str) -> Result<(), VfsError>;
    fn copy(&self, from: &str, to: &str) -> Result<(), VfsError>;
    
    // Metadata Operations
    fn stat(&self, path: &str) -> Result<Inode, VfsError>;
    fn exists(&self, path: &str) -> Result<bool, VfsError>;
    fn chmod(&self, path: &str, perms: FilePermissions) -> Result<(), VfsError>;
    fn chown(&self, path: &str, owner_id: Option<Uuid>) -> Result<(), VfsError>;
    
    // Path Utilities
    fn get_home_dir(&self, user_id: Uuid) -> String;
    fn get_zos_dir(&self, user_id: Uuid) -> String;
    
    // Quota Management
    fn get_usage(&self, path: &str) -> Result<StorageUsage, VfsError>;
    fn get_quota(&self, user_id: Uuid) -> Result<StorageQuota, VfsError>;
}
```

### 03-storage.md - Storage Service

Documents file operations, encryption at rest, and quota management. Includes the JavaScript bridge for IndexedDB access.

---

## Part 6: IPC Protocols

### User Service Messages (05-identity)

```rust
pub mod user_msg {
    // User Management
    pub const MSG_CREATE_USER: u32 = 0x7000;
    pub const MSG_CREATE_USER_RESPONSE: u32 = 0x7001;
    pub const MSG_GET_USER: u32 = 0x7002;
    pub const MSG_GET_USER_RESPONSE: u32 = 0x7003;
    pub const MSG_LIST_USERS: u32 = 0x7004;
    pub const MSG_LIST_USERS_RESPONSE: u32 = 0x7005;
    pub const MSG_DELETE_USER: u32 = 0x7006;
    pub const MSG_DELETE_USER_RESPONSE: u32 = 0x7007;
    
    // Local Login (Offline)
    pub const MSG_LOGIN_CHALLENGE: u32 = 0x7010;
    pub const MSG_LOGIN_CHALLENGE_RESPONSE: u32 = 0x7011;
    pub const MSG_LOGIN_VERIFY: u32 = 0x7012;
    pub const MSG_LOGIN_VERIFY_RESPONSE: u32 = 0x7013;
    pub const MSG_LOGOUT: u32 = 0x7014;
    pub const MSG_LOGOUT_RESPONSE: u32 = 0x7015;
    
    // Remote Authentication
    pub const MSG_REMOTE_AUTH: u32 = 0x7020;
    pub const MSG_REMOTE_AUTH_RESPONSE: u32 = 0x7021;
    
    // Process Queries
    pub const MSG_WHOAMI: u32 = 0x7030;
    pub const MSG_WHOAMI_RESPONSE: u32 = 0x7031;
    
    // Credential Management
    pub const MSG_ATTACH_EMAIL: u32 = 0x7040;
    pub const MSG_ATTACH_EMAIL_RESPONSE: u32 = 0x7041;
    pub const MSG_GET_CREDENTIALS: u32 = 0x7042;
    pub const MSG_GET_CREDENTIALS_RESPONSE: u32 = 0x7043;
}
```

### VFS Service Messages (06-filesystem)

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
    
    // Metadata Operations
    pub const MSG_VFS_STAT: u32 = 0x8020;
    pub const MSG_VFS_STAT_RESPONSE: u32 = 0x8021;
    pub const MSG_VFS_EXISTS: u32 = 0x8022;
    pub const MSG_VFS_EXISTS_RESPONSE: u32 = 0x8023;
    
    // Quota Operations
    pub const MSG_VFS_GET_USAGE: u32 = 0x8030;
    pub const MSG_VFS_GET_USAGE_RESPONSE: u32 = 0x8031;
}
```

---

## Part 7: User Home Directory Bootstrap

When creating a user, the system bootstraps their home directory:

```rust
impl UserService {
    fn create_user(&self, req: CreateUserRequest) -> Result<User, UserError> {
        let user_id = Uuid::new_v4();
        let home = format!("/home/{}", user_id);
        
        // Create home directory structure
        self.vfs.mkdir_p(&home)?;
        self.vfs.chown(&home, Some(user_id))?;
        
        // Hidden ZOS directory
        self.vfs.mkdir(&format!("{}/.zos", home))?;
        self.vfs.mkdir(&format!("{}/.zos/identity", home))?;
        self.vfs.mkdir(&format!("{}/.zos/sessions", home))?;
        self.vfs.mkdir(&format!("{}/.zos/credentials", home))?;
        self.vfs.mkdir(&format!("{}/.zos/tokens", home))?;
        self.vfs.mkdir(&format!("{}/.zos/config", home))?;
        
        // Standard directories
        self.vfs.mkdir(&format!("{}/Documents", home))?;
        self.vfs.mkdir(&format!("{}/Downloads", home))?;
        self.vfs.mkdir(&format!("{}/Desktop", home))?;
        self.vfs.mkdir(&format!("{}/Pictures", home))?;
        self.vfs.mkdir(&format!("{}/Music", home))?;
        self.vfs.mkdir(&format!("{}/Apps", home))?;
        
        // Store user record and keys...
        // Update user registry...
        
        Ok(user)
    }
}
```

---

## Part 8: Boot Sequence

### Filesystem Initialization

On boot, the VFS service initializes the root filesystem structure:

```rust
impl VfsService {
    async fn init_filesystem(&self) -> Result<(), VfsError> {
        if self.exists("/system")? {
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
        self.write_file("/system/config/machine.json", &serde_json::to_vec(&machine_config)?)?;
        
        // Initialize empty user registry
        let registry = UserRegistry { users: vec![] };
        self.write_file("/users/registry.json", &serde_json::to_vec(&registry)?)?;
        
        Ok(())
    }
    
    async fn clean_tmp(&self) -> Result<(), VfsError> {
        let entries = self.readdir_all("/tmp")?;
        for entry in entries {
            let path = format!("/tmp/{}", entry.name);
            if entry.entry_type == InodeType::Directory {
                self.rmdir_recursive(&path)?;
            } else {
                self.unlink(&path)?;
            }
        }
        Ok(())
    }
}
```

---

## Key Design Decisions

| Decision | Rationale |

|----------|-----------|

| **Userspace concept** | Layers 05-08 form a cohesive user-facing layer above kernel/init |

| **Two IndexedDB databases** | Clear separation between kernel (`zos-kernel`) and userspace (`zos-userspace`) |

| **Canonical filesystem** | Hierarchical VFS with Unix-like paths and permissions |

| **File-based identity** | All identity material stored as files in `/home/{id}/.zos/` |

| **Process Manager in Init** | Process lifecycle is a supervision concern (04-init) |

| **Permissions in Identity** | Permission policy is about "who can do what" |

| **Offline-first** | All local operations work without network |

| **Multi-user concurrent** | Multiple users can have active sessions simultaneously |

---

## Deliverables

### Specification Deliverables

| File | Phase | Action | Description |

|------|-------|--------|-------------|

| `docs/spec/v0.1.2/USERSPACE.md` | 0 | Update | Userspace layer overview |

| `docs/spec/v0.1.2/05-identity/README.md` | 1A | Update | Identity layer overview |

| `docs/spec/v0.1.2/05-identity/01-users.md` | 1A | Update | User primitive, home directories |

| `docs/spec/v0.1.2/05-identity/02-sessions.md` | 1A | Update | Session management |

| `docs/spec/v0.1.2/05-identity/03-zero-id.md` | 1A | Update | Zero-ID integration |

| `docs/spec/v0.1.2/05-identity/04-permissions.md` | 1A | Update | Capability policy |

| `docs/spec/v0.1.2/06-filesystem/README.md` | 2A | Update | Filesystem layer overview |

| `docs/spec/v0.1.2/06-filesystem/01-database.md` | 2A | Update | Two-database architecture |

| `docs/spec/v0.1.2/06-filesystem/02-vfs.md` | 2A | Update | Virtual filesystem spec |

| `docs/spec/v0.1.2/06-filesystem/03-storage.md` | 2A | Update | File operations, encryption |

| `docs/spec/v0.1.2/04-init/03-process-manager.md` | 5 | Update | Process lifecycle |

### Implementation Deliverables

| Crate | Phase | Description |

|-------|-------|-------------|

| `crates/zos-identity/` | 1B | User, session, keystore, UserService |

| `crates/zos-vfs/` | 2B | Inode, VfsService, IndexedDB backend |

| `crates/zos-apps/` (updates) | 3B | Identity and VFS integration |

| `crates/zos-desktop/` (updates) | 4B | Login UI, session management |

| `web/` (updates) | 4B | Frontend identity components |

---

## Task Dependency Order

```
PHASE 0: Foundation
├── phase0-userspace-readme
└── phase0-cleanup-old-runtime

PHASE 1: Identity Layer (depends on Phase 0)
│
├── PHASE 1A: Specification
│   ├── phase1a-identity-folder
│   ├── phase1a-identity-users-spec
│   ├── phase1a-identity-sessions-spec
│   ├── phase1a-identity-zero-id-spec
│   └── phase1a-identity-permissions-spec
│
└── PHASE 1B: Implementation (depends on 1A)
    ├── phase1b-identity-crate
    ├── phase1b-identity-types
    ├── phase1b-identity-sessions
    ├── phase1b-identity-keystore
    ├── phase1b-identity-service
    ├── phase1b-identity-ipc
    └── phase1b-identity-tests

PHASE 2: Filesystem Layer (depends on Phase 1B)
│
├── PHASE 2A: Specification
│   ├── phase2a-filesystem-folder
│   ├── phase2a-fs-database-spec
│   ├── phase2a-fs-vfs-spec
│   └── phase2a-fs-storage-spec
│
└── PHASE 2B: Implementation (depends on 2A)
    ├── phase2b-filesystem-crate
    ├── phase2b-vfs-types
    ├── phase2b-vfs-service
    ├── phase2b-vfs-indexeddb
    ├── phase2b-vfs-bootstrap
    ├── phase2b-vfs-ipc
    └── phase2b-vfs-tests

PHASE 3: Applications Layer (depends on Phase 2B)
│
├── PHASE 3A: Specification
│   └── phase3a-apps-review
│
└── PHASE 3B: Implementation (depends on 3A)
    ├── phase3b-apps-identity-integration
    ├── phase3b-apps-vfs-integration
    └── phase3b-apps-tests

PHASE 4: Desktop Layer (depends on Phase 3B)
│
├── PHASE 4A: Specification
│   └── phase4a-desktop-review
│
└── PHASE 4B: Implementation (depends on 4A)
    ├── phase4b-desktop-identity-ui
    ├── phase4b-desktop-session-mgmt
    └── phase4b-desktop-tests

PHASE 5: Integration (depends on Phase 4B)
├── phase5-move-process-to-init
├── phase5-end-to-end-tests
└── phase5-update-docs
```

---

## Implementation Details by Phase

### Phase 1B: Identity Implementation (zos-identity crate)

```
crates/zos-identity/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Re-exports, crate root
│   ├── types.rs            # User, UserStatus, UserPreferences
│   ├── session.rs          # LocalSession, SessionMetadata, RemoteAuthState
│   ├── keystore.rs         # LocalKeyStore, key management
│   ├── service.rs          # UserService trait and implementations
│   ├── ipc.rs              # IPC message definitions and handlers
│   └── error.rs            # IdentityError enum
└── tests/
    └── integration.rs
```

**Key Implementation Tasks:**

1. **Types** (`types.rs`): Define `User`, `UserStatus`, `UserPreferences` structs with serde serialization
2. **Sessions** (`session.rs`): Implement `LocalSession`, `SessionMetadata`, session lifecycle methods
3. **KeyStore** (`keystore.rs`): Implement `LocalKeyStore` with file path helpers for VFS integration
4. **Service** (`service.rs`): `UserService` trait with `create_user`, `get_user`, `authenticate`, `create_session`
5. **IPC** (`ipc.rs`): Message type constants (0x7000-0x704F), request/response encoding

### Phase 2B: Filesystem Implementation (zos-vfs crate)

```
crates/zos-vfs/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Re-exports, crate root
│   ├── types.rs            # Inode, InodeType, FilePermissions, DirEntry
│   ├── path.rs             # Path validation, normalization
│   ├── service.rs          # VfsService trait
│   ├── memory.rs           # In-memory VFS for testing
│   ├── indexeddb.rs        # IndexedDB backend (zos-userspace)
│   ├── bootstrap.rs        # init_filesystem, clean_tmp
│   ├── ipc.rs              # IPC message definitions
│   └── error.rs            # VfsError enum
└── tests/
    └── integration.rs
```

**Key Implementation Tasks:**

1. **Types** (`types.rs`): `Inode`, `InodeType`, `FilePermissions`, `DirEntry`, `StorageUsage`, `StorageQuota`
2. **Path** (`path.rs`): Validate paths, normalize, extract parent/name
3. **Service** (`service.rs`): `VfsService` trait with all file/directory operations
4. **Memory** (`memory.rs`): In-memory `HashMap`-based VFS for unit tests
5. **IndexedDB** (`indexeddb.rs`): Real backend using JS interop with `zos-userspace` database
6. **Bootstrap** (`bootstrap.rs`): Create `/system`, `/home`, `/tmp` on first boot

---

## Notes

- Each "B" phase (implementation) requires its corresponding "A" phase (specification) to be complete
- Implementation phases can only start after the previous layer's implementation is complete
- Within each phase, tasks can be parallelized where dependencies allow
- Phase 3 and 4 are lighter since most code already exists; focus is on integration