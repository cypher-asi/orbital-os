# Identity Layer

> User management, sessions, Zero-ID integration, and permission policies.

## Overview

The Identity layer provides user-facing identity management for Zero OS. Unlike traditional OS identity systems, ZOS Identity is:

1. **Offline-first**: Local authentication works without network
2. **File-based**: All identity material stored as files in user home directory
3. **Zero-ID integrated**: Cryptographic identity backed by Zero-ID protocol
4. **Capability-aware**: Permission policies control capability grants

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            Identity Layer                                     │
│                                                                              │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────────┐ │
│  │   User Service  │  │ Session Service │  │   Permission Service        │ │
│  │                 │  │                 │  │                             │ │
│  │  • Create user  │  │  • Login        │  │  • Check policy             │ │
│  │  • Get user     │  │  • Logout       │  │  • Capability history       │ │
│  │  • List users   │  │  • Validate     │  │  • Grant permissions        │ │
│  │  • Update user  │  │  • Refresh      │  │  • Revoke permissions       │ │
│  └────────┬────────┘  └────────┬────────┘  └─────────────┬───────────────┘ │
│           │                    │                          │                  │
│           └────────────────────┼──────────────────────────┘                  │
│                                │                                             │
│                     ┌──────────▼──────────┐                                 │
│                     │   Zero-ID Store     │                                 │
│                     │                     │                                 │
│                     │  • Local key store  │                                 │
│                     │  • Credentials      │                                 │
│                     │  • Token families   │                                 │
│                     └──────────┬──────────┘                                 │
│                                │                                             │
└────────────────────────────────┼─────────────────────────────────────────────┘
                                 │
                                 ▼
                    ┌────────────────────────┐
                    │   VFS (06-filesystem)  │
                    │                        │
                    │  /home/{user_id}/.zos/ │
                    └────────────────────────┘
```

## Files

| File | Description |
|------|-------------|
| [01-users.md](01-users.md) | User primitive, status, home directories |
| [02-sessions.md](02-sessions.md) | Local and remote session management |
| [03-zero-id.md](03-zero-id.md) | Zero-ID integration, key storage paths |
| [04-permissions.md](04-permissions.md) | Capability policy enforcement |

## Core Concepts

### User

A ZOS user is backed by a Zero-ID identity. The user record contains:

- **id**: UUID matching the Zero-ID identity_id
- **display_name**: Human-readable name for UI
- **status**: Active, Offline, or Suspended
- **default_namespace_id**: Default namespace for resources
- **created_at / last_active_at**: Timestamps

See [01-users.md](01-users.md) for details.

### Session

A session represents an authenticated user context. Sessions are local by default and can optionally be linked to remote authentication:

- **LocalSession**: Works fully offline
- **RemoteAuthState**: Optional remote server link for sync/federation

See [02-sessions.md](02-sessions.md) for details.

### Zero-ID Integration

Identity material is stored as files in the user's home directory:

```
/home/{user_id}/.zos/identity/
├── user.json                    # User record
├── public_keys.json             # Public key material
├── private_keys.enc             # Encrypted private keys
└── machine/
    └── {machine_id}.json        # Machine-specific keys
```

See [03-zero-id.md](03-zero-id.md) for details.

### Permissions

The permission system controls capability grants based on user identity and app manifests:

- **Policy rules**: Define what capabilities can be granted
- **Process classification**: System, Runtime, Application
- **Capability history**: Audit trail via Axiom log

See [04-permissions.md](04-permissions.md) for details.

## IPC Protocol

The Identity layer exposes three service endpoints:

### User Service Messages

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

## Service Discovery

Applications discover the Identity service via init:

```rust
// Look up identity service
let identity_ep = lookup_service("identity")?;

// Query current user
send(identity_ep, MSG_WHOAMI, &[]);
let response = receive_reply();
let user_info = decode::<WhoamiResponse>(&response.data);
```

## Security Model

1. **Local-first authentication**: Users authenticate against locally-stored keys
2. **Optional remote verification**: Can link to remote server for additional verification
3. **Session isolation**: Each session has isolated capability grants
4. **Capability attenuation**: Permissions can only be reduced, never increased
5. **Audit trail**: All permission changes logged to Axiom SysLog

## WASM Implementation

The Identity service runs as a WASM module:

```rust
#![no_std]
extern crate alloc;
extern crate zero_process;

use zero_process::*;

#[no_mangle]
pub extern "C" fn _start() {
    debug("identity: starting");
    
    // Load identity data from VFS
    let users = load_user_registry();
    
    let service_ep = create_endpoint();
    register_service("identity", service_ep);
    send_ready();
    
    loop {
        let msg = receive_blocking(service_ep);
        match msg.tag {
            user_msg::MSG_CREATE_USER => handle_create_user(msg),
            user_msg::MSG_LOGIN_CHALLENGE => handle_login(msg),
            user_msg::MSG_WHOAMI => handle_whoami(msg),
            // ...
        }
    }
}
```

## Related Specifications

- [../USERSPACE.md](../USERSPACE.md) - Userspace layer overview
- [../06-filesystem/README.md](../06-filesystem/README.md) - VFS for identity storage
- [../04-init/02-supervision.md](../04-init/02-supervision.md) - Service supervision
- [../03-kernel/03-capabilities.md](../03-kernel/03-capabilities.md) - Capability system
