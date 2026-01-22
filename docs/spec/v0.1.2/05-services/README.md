# Runtime Services

> System services that run in user-space and provide OS functionality.

## Overview

Runtime services run in user-space (outside the kernel) and provide OS functionality via IPC. This architecture provides:

1. **Isolation**: Service bugs don't crash the kernel
2. **Updatability**: Services can be updated without reboot
3. **Flexibility**: Different policy implementations possible
4. **Security**: Services are sandboxed with limited capabilities

## Services

| Service | Description | Specification |
|---------|-------------|---------------|
| Network | HTTP, WebSocket, DNS | [01-network.md](01-network.md) |

## Note on Reorganization

This layer previously contained more services. These have been reorganized:

| Original | New Location | Rationale |
|----------|--------------|-----------|
| Process Manager | [04-init/03-process-manager.md](../04-init/03-process-manager.md) | Supervision responsibility |
| Permissions | [05-identity/04-permissions.md](../05-identity/04-permissions.md) | Identity-related |
| Identity | [05-identity/](../05-identity/) | New identity layer |
| Storage | [06-filesystem/](../06-filesystem/) | New filesystem layer |

The Network service remains as a standalone runtime service because it:
- Doesn't fit cleanly in the identity or filesystem layers
- Provides generic network access for all applications
- Is optional (some ZOS instances may not have network)

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Runtime Services                                       │
│                                                                              │
│  ┌──────────────────┐                                                       │
│  │  Network Service │                                                       │
│  │                  │                                                       │
│  │  • HTTP/Fetch    │                                                       │
│  │  • WebSocket     │                                                       │
│  │  • DNS           │                                                       │
│  │  • Policy        │                                                       │
│  └────────┬─────────┘                                                       │
│           │                                                                  │
└───────────┼──────────────────────────────────────────────────────────────────┘
            │
            │  Syscalls via Axiom
            ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Kernel                                              │
│                                                                              │
│  Capabilities │ Threads │ VMM │ IPC │ Interrupts                            │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Service Discovery

Applications discover services via init:

```rust
// Look up network service
let network_ep = lookup_service("network")?;

// Make HTTP request
let request = HttpRequest {
    method: HttpMethod::Get,
    url: "https://api.example.com/data".to_string(),
    headers: vec![],
    body: None,
    timeout_ms: 30000,
};
send(network_ep, MSG_NET_REQUEST, &encode(&request));
let response = receive_reply();
```

## Service Capabilities

Each service receives specific capabilities from init:

| Service | Capabilities |
|---------|--------------|
| Network | Fetch API access, socket access (native) |

## Related Specifications

- [../USERSPACE.md](../USERSPACE.md) - Userspace layer overview
- [../04-init/02-supervision.md](../04-init/02-supervision.md) - Service supervision
- [../04-init/03-process-manager.md](../04-init/03-process-manager.md) - Process management
