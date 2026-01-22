# Init Process

> The init process is the first user-space process, responsible for bootstrapping the system.

## Overview

Init is the ancestor of all user-space processes. It runs with elevated capabilities and is responsible for:

1. **Bootstrap**: Starting essential services
2. **Supervision**: Monitoring and restarting failed services
3. **Shutdown**: Coordinating clean system shutdown

## Files

| File                                      | Description                    |
|-------------------------------------------|--------------------------------|
| [01-bootstrap.md](01-bootstrap.md)        | System bootstrap sequence      |
| [02-supervision.md](02-supervision.md)    | Service supervision model      |
| [03-process-manager.md](03-process-manager.md) | Process lifecycle management |

## Init Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                           Init Process                               │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                     Capabilities                             │   │
│  │                                                              │   │
│  │  • ProcessManager endpoint (full access)                     │   │
│  │  • Console endpoint (read/write)                             │   │
│  │  • Storage endpoint (read/write)                             │   │
│  │  • Network endpoint (read/write) [if available]              │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                     Service Table                            │   │
│  │                                                              │   │
│  │  Name        │ PID │ State    │ Restart Policy              │   │
│  │  ────────────┼─────┼──────────┼──────────────────────       │   │
│  │  terminal    │ 2   │ Running  │ Always                      │   │
│  │  storage     │ 3   │ Running  │ Always                      │   │
│  │  network     │ 4   │ Running  │ OnFailure                   │   │
│  │  app1        │ 5   │ Stopped  │ Never                       │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  Main Loop:                                                         │
│  1. Wait for messages (service requests, status updates)           │
│  2. Handle spawn/stop/restart requests                             │
│  3. Monitor service health                                          │
│  4. Restart failed services per policy                              │
└─────────────────────────────────────────────────────────────────────┘
```

## Init Lifecycle

```
       Kernel spawns init
              │
              ▼
    ┌──────────────────┐
    │   Init starts    │
    │                  │
    │  1. Create IPC   │
    │     endpoint     │
    │  2. Register as  │
    │     supervisor   │
    └────────┬─────────┘
             │
             ▼
    ┌──────────────────┐
    │  Bootstrap       │
    │                  │
    │  Start services: │
    │  - terminal      │
    │  - storage       │
    │  - network       │
    └────────┬─────────┘
             │
             ▼
    ┌──────────────────┐
    │  Supervision     │◀────────┐
    │  Loop            │         │
    │                  │         │
    │  Wait for:       │         │
    │  - Messages      │         │ (loop forever)
    │  - Child exit    │         │
    │  - Restart timer │─────────┘
    └────────┬─────────┘
             │
             ▼ (shutdown signal)
    ┌──────────────────┐
    │  Shutdown        │
    │                  │
    │  1. Signal all   │
    │     services     │
    │  2. Wait/force   │
    │     terminate    │
    │  3. Exit         │
    └──────────────────┘
```

## Init Capabilities

Init receives special capabilities from the kernel at spawn:

| Capability Type | Object          | Permissions | Purpose                    |
|-----------------|-----------------|-------------|----------------------------|
| Endpoint        | Own endpoint    | RWG         | Receive requests           |
| Console         | Console         | RW          | Debug output               |
| Process         | All processes   | RWG         | Manage processes           |

## WASM Implementation

On WASM, init is a regular WASM module running in a Web Worker:

```rust
// crates/zos-init/src/lib.rs

#![no_std]
#![no_main]

extern crate alloc;
extern crate Zero_process;

use Zero_process::{debug, receive_blocking, send, yield_now};

#[no_mangle]
pub extern "C" fn _start() {
    debug("init: starting");
    
    // Create our IPC endpoint
    let my_endpoint = create_endpoint();
    
    // Bootstrap essential services
    let terminal_pid = spawn_service("terminal");
    let storage_pid = spawn_service("storage");
    
    // Main supervision loop
    loop {
        match receive(my_endpoint) {
            Some(msg) => handle_message(msg),
            None => yield_now(),
        }
        
        // Check for dead children and restart if needed
        check_children();
    }
}

fn spawn_service(name: &str) -> u32 {
    debug(&format!("init: spawning {}", name));
    // Request kernel to spawn service
    // ...
}

fn check_children() {
    // Monitor child processes
    // Restart per policy
}
```
