# Phase 1: WASM (Browser-Hosted)

> **Goal**: Demonstrate capability-based microkernel with deterministic replay running in a web browser.

## Overview

Phase 1 implements Zero OS running entirely in the browser using WebAssembly. This phase establishes the core architecture and proves the fundamental invariants work before adding hardware complexity.

### Platform Characteristics

- **Processes**: Web Workers (one per process)
- **Memory**: WASM linear memory (no MMU/VMM needed)
- **Scheduling**: Cooperative (no preemption)
- **Time**: `performance.now()` for nanosecond timestamps
- **Entropy**: `crypto.getRandomValues()`
- **Storage**: IndexedDB for logs
- **IPC**: SharedArrayBuffer + Atomics for syscall mailbox
- **Debug**: `console.log`

### Current Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Browser (index.html)                          │
│                                                                 │
│  ┌───────────────────────────────────────────────────────────┐ │
│  │               Supervisor (Rust/WASM + JavaScript)          │ │
│  │                                                           │ │
│  │  • zos-supervisor: Rust WASM supervisor                      │ │
│  │  • Spawns Web Workers for each process                    │ │
│  │  • Polls SharedArrayBuffer mailboxes for syscalls         │ │
│  │  • Routes IPC messages between workers                    │ │
│  │  • Persists Axiom log to IndexedDB                        │ │
│  └───────────────────────────────────────────────────────────┘ │
│         │         │         │         │                         │
│         ▼         ▼         ▼         ▼                         │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐              │
│  │ Worker  │ │ Worker  │ │ Worker  │ │ Worker  │              │
│  │(terminal)│ │ (idle)  │ │(sender) │ │(receiver)│              │
│  │  WASM   │ │  WASM   │ │  WASM   │ │  WASM   │              │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘              │
│      │             │             │             │                │
│      │             │             │             │                │
│      └─────────────┴─────────────┴─────────────┘                │
│                       │                                         │
│                       ▼                                         │
│            ┌─────────────────────┐                              │
│            │  Zero-kernel     │                              │
│            │  (in supervisor)    │                              │
│            │                     │                              │
│            │  • Capability system│                              │
│            │  • IPC endpoints    │                              │
│            │  • Process table    │                              │
│            │  • Axiom log        │                              │
│            └─────────────────────┘                              │
└─────────────────────────────────────────────────────────────────┘
```

## Implementation Stages

Phase 1 is divided into eight stages, each building on the previous:

| Stage | Name | Status | Focus |
|-------|------|--------|-------|
| [1.1](stage-1.1-minimal-kernel.md) | Minimal Kernel + Debug | ✅ **COMPLETE** | Basic syscall working |
| [1.2](stage-1.2-axiom-layer.md) | Axiom Layer | ✅ **COMPLETE** | SysLog + CommitLog infrastructure |
| [1.3](stage-1.3-capabilities-ipc.md) | Capabilities + IPC | ✅ **COMPLETE** | Core capability system, message passing |
| [1.4](stage-1.4-process-management.md) | Process Management | ✅ **COMPLETE** | Multiple processes communicating |
| [1.5](stage-1.5-init-services.md) | Init + Services | ✅ **COMPLETE** | Bootstrap, service discovery |
| [1.6](stage-1.6-replay-testing.md) | Replay + Testing | ✅ **COMPLETE** | Deterministic replay verification |
| [1.7](stage-1.7-web-ui.md) | Web UI | ✅ **COMPLETE** | Browser interface for inspection |
| [1.8](stage-1.8-desktop-environment.md) | Desktop Environment | ❌ **TODO** | Full desktop with windows and workspaces |

## Current Implementation Status

### ✅ What's Complete

1. **Kernel Core** (`Zero-kernel`)
   - Process registration, state management, metrics
   - Full capability system (CSpace, permissions, grant, revoke, delete, derive)
   - IPC endpoints with message queuing
   - IPC with capability transfer
   - Axiom log for capability mutations (with hash chain)
   - Extensive unit tests (50+ tests)

2. **HAL Layer** (`Zero-hal`)
   - HAL trait with process management, time, memory, random, debug
   - Mock HAL integrated into `Zero-kernel` for testing

3. **Process Support** (`zos-process`)
   - Process-side syscall library with full ABI
   - Syscall runtime provided by `worker.js` using SharedArrayBuffer + Atomics

4. **Browser Supervisor** (`zos-supervisor`, `worker.js`)
   - Web Worker process isolation
   - SharedArrayBuffer mailbox polling
   - Full dashboard UI (processes, memory, endpoints, IPC traffic, Axiom log)
   - IndexedDB persistence for Axiom log

5. **Userspace Processes** (`zos-apps`, `zos-system-procs`)
   - Terminal app implementing ZeroApp trait
   - Clock, Calculator apps with IPC protocol
   - PermissionService (PID 2)
   - idle, memhog, sender, receiver, pingpong test processes

6. **Axiom Layer** (`Zero-axiom`)
   - SysLog for syscall audit trail (request + response)
   - CommitLog for deterministic state mutations
   - AxiomGateway entry point for syscalls
   - CommitType enum for all state mutations
   - FNV-1a hash chain with integrity verification
   - 24 unit tests passing

5. **Init + Services** (Stage 1.5)
   - ✅ Formal init process (PID 1) in `zos-init` crate
   - ✅ Service registry with registration protocol
   - ✅ Bootstrap sequence: kernel → init → terminal
   - ✅ Terminal registers with init on startup

### ❌ What's Missing

1. **Desktop Environment** (Stage 1.8)
   - No WebGPU engine with infinite canvas
   - No window management (move, resize, z-order)
   - No input routing to windows
   - No React presentation layer

## Core Invariants

These properties must hold at every stage:

### 1. Capability Integrity ✅

- Capabilities only created by kernel
- Derived capabilities have permissions ≤ source
- Capability checks happen before every privileged operation
- No process can forge a capability

### 2. Sender Verification ✅

- Supervisor verifies sender from Worker context
- Kernel trusts sender PID from supervisor
- Processes cannot lie about their identity

### 3. Commit Atomicity ✅

- ✅ Axiom log is append-only with hash chain
- ✅ CommitLog records all state mutations
- ✅ Genesis commit at boot
- ✅ Process, endpoint, and capability mutations logged

### 4. Deterministic Replay ✅

- ✅ CommitLog infrastructure complete
- ✅ `apply_commit()` / `replay()` functions implemented
- ✅ `state_hash()` for deterministic state hashing
- ✅ `replay_and_verify()` for hash verification
- ✅ 12 replay tests passing

## File Structure (Current)

```
crates/
  Zero-axiom/           # Axiom verification layer (SysLog, CommitLog, Gateway)
  Zero-hal/             # HAL trait definition
  Zero-kernel/          # Kernel with capabilities, IPC, Axiom integration (includes mock HAL for tests)
  zos-process/         # Process-side syscall library
  zos-apps/            # Userspace apps (Terminal, Clock, Calculator, PermissionService)
  zos-system-procs/    # System processes (idle, memhog, etc.)

apps/
  zos-supervisor/             # Browser supervisor
    src/
      lib.rs               # Rust WASM supervisor
    www/
      index.html           # Full dashboard UI
      worker.js            # Web Worker bootstrap
      processes/           # Compiled WASM binaries
      pkg/                 # wasm-pack output

```

## Dependencies

### Rust Crates (Current)

```toml
[workspace.dependencies]
Zero-axiom = { path = "crates/Zero-axiom" }
Zero-hal = { path = "crates/Zero-hal" }
Zero-kernel = { path = "crates/Zero-kernel" }
zos-process = { path = "crates/zos-process" }

wasm-bindgen = "0.2"
js-sys = "0.3"
web-sys = "0.3"
serde = { version = "1.0", features = ["derive", "alloc"] }
serde_json = { version = "1.0", features = ["alloc"] }
```

## Build & Run

```bash
# Build everything
make build

# Build and run dev server
make dev

# Run tests
make test

# Clean
make clean
```

## Success Criteria for Phase 1

Phase 1 is complete when:

1. ✅ **Kernel boots** in browser and spawns processes
2. ✅ **Multiple processes** running in Web Workers
3. ✅ **Terminal service** can receive input and echo output
4. ✅ **Capability system** working: grants, revokes, attenuation
5. ✅ **IPC system** working: processes can send/receive messages
6. ✅ **Axiom layer** logs all state mutations (SysLog + CommitLog complete)
7. ✅ **Deterministic replay** works: same CommitLog produces same state
8. ✅ **Web UI** shows processes, capabilities, and logs
9. ✅ **Tests pass** (72 tests: 28 axiom + 44 kernel including 12 replay tests)
10. ✅ **Core invariants** verified (all 4 invariants verified)
11. ❌ **Desktop environment** with infinite canvas, windows, and workspaces

## Related Documentation

- [Spec: Axiom](../../spec/02-axiom/README.md) - Verification layer
- [Spec: Kernel](../../spec/03-kernel/README.md) - Microkernel architecture  
- [Spec: HAL](../../spec/01-hal/README.md) - Hardware abstraction
- [Spec: Desktop](../../spec/08-desktop/README.md) - Desktop environment
- [Rust Conventions](../../../.cursor/cursor_rules_rust.md) - Code quality rules

## Next Phase

After Phase 1 is complete, proceed to [Phase 2: QEMU](../phase-2-qemu/README.md) to add:

- Hardware VMM (virtual memory)
- Preemptive scheduling
- Interrupt handling
- VirtIO devices
