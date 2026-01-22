# Userspace Layer

> The higher-order concept encompassing identity, filesystem, applications, and desktop.

## Overview

**Userspace** is the collective term for layers 05-08 of Zero OS, representing all user-facing functionality above the kernel and init layers. Userspace provides the abstractions that applications and users interact with: identity management, filesystem services, application lifecycle, and desktop/compositor.

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

## Design Principles

### 1. Offline-First

All userspace functionality works fully offline. Network connectivity enhances capabilities but is never required for local operations:

- Users can authenticate locally without a server
- Files are stored locally and optionally synced
- Applications run without network access unless explicitly required

### 2. File-Based Identity

All identity material is stored as files within the user's home directory:

```
/home/{user_id}/.zos/
├── identity/           # Cryptographic keys and user record
├── sessions/           # Active session files
├── credentials/        # Linked credentials
├── tokens/             # Token families
└── config/             # User preferences
```

This enables:
- Backup/restore of complete identity
- Multi-device sync via filesystem
- Inspection and debugging via standard tools

### 3. Two-Database Architecture

ZOS uses two separate IndexedDB databases to maintain clear separation of concerns:

| Database | Purpose | Access |
|----------|---------|--------|
| `zos-kernel` | Kernel and system state (processes, capabilities, IPC) | Kernel-only |
| `zos-userspace` | Virtual filesystem (inodes, file content) | VFS service |

See [06-filesystem/01-database.md](06-filesystem/01-database.md) for details.

### 4. Capability-Gated Access

All userspace services are accessed via IPC with capability-based authorization:

```
Application ──IPC──► Identity Service ──IPC──► VFS Service
     │                      │                        │
     └── capability ────────┴── capability ──────────┘
```

### 5. Multi-User Concurrent

Multiple users can have active sessions simultaneously. Each user has isolated:
- Home directory
- Session tokens
- Capability grants
- Application state

## Layer Specifications

### Layer 05: Identity

User and session management, Zero-ID integration, permissions.

| File | Description |
|------|-------------|
| [05-identity/README.md](05-identity/README.md) | Layer overview |
| [05-identity/01-users.md](05-identity/01-users.md) | User primitive, home directories |
| [05-identity/02-sessions.md](05-identity/02-sessions.md) | Local/remote sessions |
| [05-identity/03-zero-id.md](05-identity/03-zero-id.md) | Zero-ID integration, key storage |
| [05-identity/04-permissions.md](05-identity/04-permissions.md) | Capability policy enforcement |

### Layer 06: Filesystem

Virtual filesystem with hierarchical paths and user home directories.

| File | Description |
|------|-------------|
| [06-filesystem/README.md](06-filesystem/README.md) | Layer overview |
| [06-filesystem/01-database.md](06-filesystem/01-database.md) | Two-database architecture |
| [06-filesystem/02-vfs.md](06-filesystem/02-vfs.md) | Virtual filesystem, inodes |
| [06-filesystem/03-storage.md](06-filesystem/03-storage.md) | File operations, encryption |

### Layer 07: Applications

Sandboxed application model and lifecycle.

See [07-applications/README.md](07-applications/README.md).

### Layer 08: Desktop

Window management, input routing, and compositor.

See [08-desktop/README.md](08-desktop/README.md).

## Canonical Filesystem Hierarchy

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

## Boot Sequence

1. **Kernel boot**: HAL initializes, kernel starts
2. **Init spawn**: Kernel spawns init process with elevated capabilities
3. **VFS initialization**: VFS service creates root filesystem structure
4. **Identity service start**: Loads user registry, prepares for logins
5. **Desktop start**: Compositor initializes, shows login screen
6. **User login**: Identity validates credentials, creates session
7. **Home bootstrap**: If new user, create home directory structure
8. **Desktop session**: Load user preferences, start desktop shell

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

## Related Specifications

- [02-axiom/README.md](02-axiom/README.md) - Verification layer (SysLog + CommitLog)
- [03-kernel/README.md](03-kernel/README.md) - Microkernel (capabilities, IPC, threads)
- [04-init/README.md](04-init/README.md) - Bootstrap and supervision
