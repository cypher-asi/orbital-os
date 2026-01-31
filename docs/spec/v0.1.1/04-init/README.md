# 04 - Init Process

## Overview

The init process (PID 1) is the first user-space process spawned by the kernel. In v0.1.1, init has a minimal role:

- **Bootstrap**: Spawn PermissionService and initial services
- **Service Registry**: Maintain name → endpoint mapping for service discovery
- **Idle**: After bootstrap, enter minimal loop handling service messages

Permission management has been delegated to PermissionService (PID 2).

## Bootstrap Sequence

```
Init starts (PID 1)
       │
       ▼
┌──────────────────────────┐
│  Initialize service      │
│  registry (BTreeMap)     │
└──────────────┬───────────┘
               │
               ▼
┌──────────────────────────┐
│  Spawn PermissionService │
│  (via INIT:SPAWN:)       │
└──────────────┬───────────┘
               │
               ▼
┌──────────────────────────┐
│  Enter idle loop         │
│  • Handle IPC messages   │
│  • Service registration  │
│  • Service lookups       │
└──────────────────────────┘
```

## Service Protocol

### Registration

Services register with init to be discoverable:

```
MSG_REGISTER_SERVICE (0x1000)
┌────────────┬─────────────┬──────────────────┬───────────────────┐
│  name_len  │    name     │ endpoint_id_low  │ endpoint_id_high  │
│   (u8)     │   (bytes)   │      (u32)       │       (u32)       │
└────────────┴─────────────┴──────────────────┴───────────────────┘
```

Example registration:
```rust
let name = b"terminal";
let mut data = vec![name.len() as u8];
data.extend_from_slice(name);
data.extend_from_slice(&(endpoint_id as u32).to_le_bytes());
data.extend_from_slice(&((endpoint_id >> 32) as u32).to_le_bytes());
send(INIT_ENDPOINT_SLOT, MSG_REGISTER_SERVICE, &data)?;
```

### Lookup

Processes can discover services by name:

```
MSG_LOOKUP_SERVICE (0x1001)
┌────────────┬─────────────┐
│  name_len  │    name     │
│   (u8)     │   (bytes)   │
└────────────┴─────────────┘

MSG_LOOKUP_RESPONSE (0x1002)
┌─────────┬──────────────────┬───────────────────┐
│  found  │ endpoint_id_low  │ endpoint_id_high  │
│  (u8)   │      (u32)       │       (u32)       │
└─────────┴──────────────────┴───────────────────┘
```

### Spawn Request

Processes can request init to spawn new services:

```
MSG_SPAWN_SERVICE (0x1003)
┌────────────┬─────────────┐
│  name_len  │    name     │
│   (u8)     │   (bytes)   │
└────────────┴─────────────┘
```

Init forwards the spawn request to the supervisor via debug channel:
```rust
debug(&format!("INIT:SPAWN:{}", name));
```

### Ready Notification

Services signal readiness after registration:

```
MSG_SERVICE_READY (0x1005)
(empty payload)
```

## Capability Slots

Init's well-known capability slots:

| Slot | Purpose |
|------|---------|
| 0 | Init's main endpoint (for receiving service messages) |

Note: Console output uses `SYS_CONSOLE_WRITE` syscall (no slot needed).

## Implementation

```rust
struct Init {
    services: BTreeMap<String, ServiceInfo>,
    endpoint_slot: u32,
    boot_complete: bool,
}

struct ServiceInfo {
    pid: u32,
    endpoint_id: u64,
    ready: bool,
}

impl Init {
    fn run(&mut self) {
        self.log("Zero OS Init Process starting (PID 1)");
        self.boot_sequence();
        
        loop {
            if let Some(msg) = receive(self.endpoint_slot) {
                self.handle_message(&msg);
            }
            yield_now();
        }
    }
    
    fn boot_sequence(&mut self) {
        // Spawn PermissionService (PID 2)
        debug("INIT:SPAWN:permission");
        
        // Terminal is spawned per-window by Desktop
        self.boot_complete = true;
    }
    
    fn handle_message(&mut self, msg: &ReceivedMessage) {
        match msg.tag {
            MSG_REGISTER_SERVICE => self.handle_register(msg),
            MSG_LOOKUP_SERVICE => self.handle_lookup(msg),
            MSG_SERVICE_READY => self.handle_ready(msg),
            MSG_SPAWN_SERVICE => self.handle_spawn_request(msg),
            _ => self.log(&format!("Unknown message tag: 0x{:x}", msg.tag)),
        }
    }
}
```

## Console Output

Init uses `SYS_CONSOLE_WRITE` for logging:

```rust
fn log(&self, msg: &str) {
    console_write(&format!("[init] {}\n", msg));
}
```

This routes output through the supervisor to the console callback.

## Supervisor Communication

Init communicates with the supervisor via debug messages for privileged operations:

| Message | Purpose |
|---------|---------|
| `INIT:SPAWN:{name}` | Request supervisor to spawn a process |
| `INIT:GRANT:{pid}:{slot}:{type}:{id}:{perms}` | Request capability grant |
| `INIT:REVOKE:{pid}:{slot}` | Request capability revoke |
| `INIT:PERM_RESPONSE:{...}` | Permission operation result |

## Compliance Checklist

### Source Files
- `crates/zos-init/src/lib.rs` - Init process implementation

### Key Invariants
- [ ] Init is always PID 1
- [ ] Init endpoint slot is always 0
- [ ] Service names are unique
- [ ] Spawn requests go through supervisor
- [ ] Boot sequence completes before handling messages

### Differences from v0.1.0
- Terminal no longer auto-spawned (Desktop handles it)
- Console output via SYS_CONSOLE_WRITE syscall
- Permission management delegated to PermissionService
- Minimal idle loop (not event-driven)
