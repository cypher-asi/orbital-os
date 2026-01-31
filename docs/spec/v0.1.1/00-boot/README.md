# 00 - Boot Sequence

## Overview

Zero OS boots via a JavaScript supervisor that orchestrates kernel initialization and process spawning. The boot sequence establishes the minimal infrastructure needed to run user-space processes.

## WASM Bootstrap Sequence

```
Browser loads page
       │
       ▼
┌──────────────────────────┐
│   Load supervisor WASM   │  ← zos_supervisor_web.js
└──────────────┬───────────┘
               │
               ▼
┌──────────────────────────┐
│  Supervisor::new()       │
│  • Create WasmHal        │
│  • Create Kernel         │
│  • Initialize Axiom      │
└──────────────┬───────────┘
               │
               ▼
┌──────────────────────────┐
│  Supervisor::boot()      │
│  • Register supervisor   │
│  • Create init endpoint  │
│  • Request init spawn    │
└──────────────┬───────────┘
               │
               ▼
┌──────────────────────────┐
│  JS fetches init.wasm    │
└──────────────┬───────────┘
               │
               ▼
┌──────────────────────────┐
│  complete_spawn(init)    │
│  • Create Web Worker     │
│  • Register process      │
│  • Send Init message     │
└──────────────┬───────────┘
               │
               ▼
┌──────────────────────────┐
│  Init process runs       │
│  • Service registry      │
│  • Spawn core services   │
└──────────────────────────┘
```

## Supervisor Initialization

```javascript
// Typical supervisor setup in React
const supervisor = new Supervisor();
supervisor.set_console_callback(onConsoleOutput);
supervisor.set_spawn_callback(async (type, name) => {
    const wasm = await fetch(`/processes/${type}.wasm`);
    const binary = await wasm.arrayBuffer();
    supervisor.complete_spawn(name, new Uint8Array(binary));
});
supervisor.boot();
```

## Boot Process (Supervisor.boot)

1. **Register Supervisor Process**: The supervisor registers itself as PID 0 for audit logging (but does not use IPC endpoints).

2. **Create Init Endpoint**: Create the well-known endpoint for init (endpoint ID 1).

3. **Request Init Spawn**: Request JavaScript to fetch and spawn the init process.

## Init Process Bootstrap

When init starts (PID 1):

1. Creates its service registry
2. Spawns core services:
   - `permission` (PID 2) - capability authority
3. Enters idle loop handling service messages

Note: Terminal processes are spawned on-demand by the Desktop component, not during boot.

## Service Discovery

Services register with init using IPC:

```
MSG_REGISTER_SERVICE (0x1000)
├── name_len: u8
├── name: [u8; name_len]
├── endpoint_id_low: u32
└── endpoint_id_high: u32
```

Other processes can lookup services:

```
MSG_LOOKUP_SERVICE (0x1001)
├── name_len: u8
└── name: [u8; name_len]

MSG_LOOKUP_RESPONSE (0x1002)
├── found: u8 (0 or 1)
├── endpoint_id_low: u32
└── endpoint_id_high: u32
```

## Compliance Checklist

### Source Files
- `crates/zos-supervisor/src/supervisor/boot.rs` - Boot sequence
- `crates/zos-init/src/lib.rs` - Init process

### Key Invariants
- [ ] Supervisor PID is always 0
- [ ] Init PID is always 1
- [ ] PermissionService PID is always 2
- [ ] Init endpoint ID is always 1
- [ ] All boot operations logged to SysLog

### Differences from v0.1.0
- Terminal is no longer auto-spawned during boot
- Supervisor uses privileged kernel APIs instead of IPC
- Console output uses SYS_CONSOLE_WRITE syscall
