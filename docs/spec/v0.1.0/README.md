# Zero OS Specification v0.1.0

> WASM-first capability-based microkernel with deterministic replay.

## Layer Index

### Core Layers (00-04)

| Layer | Name | Description | Status |
|-------|------|-------------|--------|
| 00 | [Boot](00-boot/) | Reset vector, early init (browser-hosted WASM) | ✓ |
| 01 | [HAL](01-hal/) | Hardware abstraction layer | ✓ |
| 02 | [Axiom](02-axiom/) | Verification layer (SysLog + CommitLog) | ✓ |
| 03 | [Kernel](03-kernel/) | Microkernel (capabilities, threads, IPC) | ✓ |
| 04 | [Init](04-init/) | Bootstrap, supervision, process manager | ✓ |

### Userspace Layers (05-08)

| Layer | Name | Description | Status |
|-------|------|-------------|--------|
| 05 | [Identity](05-identity/) | Users, sessions, Zero-ID, permissions | ✓ |
| 06 | [Filesystem](06-filesystem/) | VFS, storage services, home directories | ✓ |
| 07 | [Applications](07-applications/) | Sandboxed application model | ✓ |
| 08 | [Desktop](08-desktop/) | Window management, compositor | ✓ |

See **[USERSPACE.md](USERSPACE.md)** for the unified userspace layer overview.

## Architecture Diagram

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
│  Layer 01: Hardware Abstraction     [01-hal/]                                │
│            Platform-specific: WASM/QEMU/Bare Metal                           │
├─────────────────────────────────────────────────────────────────────────────┤
│  Layer 00: Boot                     [00-boot/]                               │
│            Reset vector, early init (WASM: handled by browser)               │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Key Specifications

### Core

| Specification | Description |
|---------------|-------------|
| [02-axiom/02-commitlog.md](02-axiom/02-commitlog.md) | Commit types and hash chain |
| [03-kernel/03-capabilities.md](03-kernel/03-capabilities.md) | Capability system |
| [03-kernel/06-syscalls.md](03-kernel/06-syscalls.md) | Syscall ABI |
| [04-init/03-process-manager.md](04-init/03-process-manager.md) | Process lifecycle management |

### Userspace

| Specification | Description |
|---------------|-------------|
| [05-identity/01-users.md](05-identity/01-users.md) | User primitive, home directories |
| [05-identity/02-sessions.md](05-identity/02-sessions.md) | Session management |
| [05-identity/04-permissions.md](05-identity/04-permissions.md) | Permission system |
| [06-filesystem/02-vfs.md](06-filesystem/02-vfs.md) | Virtual filesystem |
| [06-filesystem/03-storage.md](06-filesystem/03-storage.md) | Storage and encryption |

## Reading Order

For implementers, the recommended reading order is:

1. **[USERSPACE.md](USERSPACE.md)** - Userspace layer overview
2. **[02-axiom/README.md](02-axiom/README.md)** - Verification layer
3. **[03-kernel/README.md](03-kernel/README.md)** - Kernel overview
4. **[04-init/README.md](04-init/README.md)** - Bootstrap and supervision
5. **[05-identity/README.md](05-identity/README.md)** - Identity layer
6. **[06-filesystem/README.md](06-filesystem/README.md)** - Filesystem layer
7. **[07-applications/README.md](07-applications/README.md)** - Application model
8. **[08-desktop/README.md](08-desktop/README.md)** - Desktop compositor

## Implementation Status

| Crate | Layer | Status |
|-------|-------|--------|
| `zos-hal` | 01 | Complete |
| `zos-axiom` | 02 | Complete |
| `zos-kernel` | 03 | Complete |
| `zos-init` | 04 | Complete |
| `zos-identity` | 05 | Complete |
| `zos-vfs` | 06 | Complete |
| `zos-apps` | 07 | Complete |
| `zos-desktop` | 08 | Complete |
| `zos-supervisor-web` | All | Complete |
