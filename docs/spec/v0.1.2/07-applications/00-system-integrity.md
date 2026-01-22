# System Integrity

> **ALL state-mutating operations MUST flow through Axiom Gateway.**

## Overview

This document specifies the core architectural invariant of Zero OS: the Axiom-first architecture. Every state-mutating operation must pass through the Axiom Gateway to ensure deterministic replay and complete audit trails.

## The Core Invariant

```
┌─────────────────────────────────────────────────────────────────┐
│                     CORRECT: Axiom-First                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   Any Operation ──► AxiomGateway ──► Kernel                     │
│                         │                                       │
│                    ┌────┴────┐                                  │
│                    │         │                                  │
│                 SysLog   CommitLog                              │
│                (audit)   (replay)                               │
│                                                                 │
├─────────────────────────────────────────────────────────────────┤
│                     WRONG: Direct Kernel Access                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   ❌ supervisor.kernel.send_to_process()  ← BYPASSES AXIOM     │
│   ❌ supervisor.kernel.terminate_process() ← BYPASSES AXIOM    │
│   ❌ Any direct kernel mutation without CommitLog entry         │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Why This Matters

| Property | Without Axiom | With Axiom |
|----------|---------------|------------|
| **Deterministic Replay** | Broken - mutations not captured | Complete - all mutations in CommitLog |
| **Audit Trail** | Incomplete - operations lost | Complete - all operations in SysLog |
| **Security Analysis** | Blind spots in system behavior | Full visibility |
| **Debugging** | Missing state transitions | Reproducible state at any point |

## Architectural Enforcement

> **The kernel MUST NOT allow direct calls. It can ONLY accept calls via Axiom.**

This is not a coding convention—it must be **architecturally enforced** so that bypassing Axiom is impossible at compile time.

### Design: Kernel Interior Pattern

The Kernel struct is split into two parts:

1. **`KernelInterior`** (private) - Contains all state-mutating operations
2. **`AxiomGateway`** (public) - The ONLY way to access `KernelInterior`

```rust
/// The Kernel is ONLY accessible through AxiomGateway.
/// Direct access to state-mutating operations is architecturally impossible.
pub struct Kernel<H: HAL> {
    /// Private interior - cannot be accessed directly
    interior: KernelInterior<H>,
    
    /// The ONLY public entry point for state mutations
    pub axiom: AxiomGateway,
}

/// Private kernel interior - all state-mutating operations live here.
/// This struct is NOT pub, so external code cannot call these methods directly.
struct KernelInterior<H: HAL> {
    hal: H,
    processes: BTreeMap<ProcessId, Process>,
    cap_spaces: BTreeMap<ProcessId, CapabilitySpace>,
    endpoints: BTreeMap<EndpointId, Endpoint>,
    // ...
}

impl<H: HAL> KernelInterior<H> {
    /// These methods are only callable from within AxiomGateway closures
    fn register_process(&mut self, name: &str) -> (ProcessId, Vec<Commit>) { ... }
    fn kill_process(&mut self, pid: ProcessId) -> (Result<()>, Vec<Commit>) { ... }
    fn create_endpoint(&mut self, owner: ProcessId) -> (EndpointId, Vec<Commit>) { ... }
    fn grant_capability(&mut self, ...) -> (Result<CapSlot>, Vec<Commit>) { ... }
    fn ipc_send(&mut self, ...) -> (Result<()>, Vec<Commit>) { ... }
    // All operations return their commits
}

impl<H: HAL> Kernel<H> {
    /// Read-only operations are still public (no state mutation)
    pub fn get_process(&self, pid: ProcessId) -> Option<&Process> { ... }
    pub fn uptime_nanos(&self) -> u64 { ... }
    pub fn list_processes(&self) -> Vec<ProcessId> { ... }
    
    /// The ONLY way to mutate state - through Axiom
    pub fn axiom(&mut self) -> &mut AxiomGateway { &mut self.axiom }
}
```

### AxiomGateway as the Sole Entry Point

```rust
impl AxiomGateway {
    /// Execute a syscall through Axiom.
    /// This is the ONLY way to mutate kernel state.
    pub fn syscall<F, R>(
        &mut self,
        interior: &mut KernelInterior,
        caller_pid: u64,
        syscall_num: u32,
        args: [u32; 4],
        timestamp: u64,
        operation: F,
    ) -> R
    where
        F: FnOnce(&mut KernelInterior) -> (R, Vec<Commit>),
    {
        // 1. Log request to SysLog
        let request_id = self.syslog.log_request(caller_pid, syscall_num, args, timestamp);
        
        // 2. Execute operation (returns result + commits)
        let (result, commits) = operation(interior);
        
        // 3. Append commits to CommitLog
        for commit in &commits {
            self.commitlog.append(commit.clone(), timestamp);
        }
        
        // 4. Log response to SysLog
        self.syslog.log_response(request_id, &result, timestamp);
        
        result
    }
    
    /// For supervisor-initiated internal operations (not process syscalls)
    pub fn internal_operation<F, R>(
        &mut self,
        interior: &mut KernelInterior,
        timestamp: u64,
        operation: F,
    ) -> R
    where
        F: FnOnce(&mut KernelInterior) -> (R, Vec<Commit>),
    {
        let (result, commits) = operation(interior);
        for commit in &commits {
            self.commitlog.append(commit.clone(), timestamp);
        }
        result
    }
}
```

### Why This Is a Security Feature

| Without Enforcement | With Enforcement |
|---------------------|------------------|
| Developers can accidentally bypass Axiom | Bypass is **compile-time impossible** |
| Code review must catch violations | Rust type system prevents violations |
| ~40 violations exist today | Zero violations possible |
| Trust-based security | Architecture-based security |

## Required Infrastructure

### New Commit Types

```rust
pub enum CommitType {
    // ... existing variants ...
    
    /// Process crashed/faulted
    ProcessFaulted { pid: u64, reason: String },
    
    /// Message sent (for IPC audit)
    MessageSent { from: u64, to: u64, tag: u32, size: usize },
}
```

### New Syscall

```rust
/// Syscall for supervisor-initiated sends (not from a process)
pub const SYS_SUPERVISOR_SEND: u32 = 0x70;
```

### Kernel Interior Methods

All mutation methods move to `KernelInterior` and return commits:

```rust
impl<H: HAL> KernelInterior<H> {
    fn register_process(&mut self, name: &str) -> (ProcessId, Vec<Commit>);
    fn kill_process(&mut self, pid: ProcessId) -> (Result<()>, Vec<Commit>);
    fn create_endpoint(&mut self, owner: ProcessId) -> (EndpointId, Vec<Commit>);
    fn grant_capability(&mut self, ...) -> (Result<CapSlot>, Vec<Commit>);
    fn ipc_send(&mut self, from: ProcessId, to: ProcessId, tag: u32, data: Vec<u8>) 
        -> (Result<()>, Vec<Commit>);
    fn mark_process_faulted(&mut self, pid: ProcessId, reason: &str) -> Vec<Commit>;
}
```

## Violation Categories

The following categories of Axiom-bypass violations exist in the codebase:

### Category 1: Process Lifecycle (HIGH PRIORITY)

Direct calls to `register_process` and `kill_process` without CommitLog entries.

| Operation | Issue |
|-----------|-------|
| `kernel.register_process(name)` | Creates process without ProcessCreated commit |
| `kernel.kill_process(pid)` | Destroys process without ProcessTerminated commit |

**Fix**: All process lifecycle operations must go through `axiom.internal_operation()`.

### Category 2: Endpoint Management (HIGH PRIORITY)

Direct calls to `create_endpoint` without CommitLog entries.

| Operation | Issue |
|-----------|-------|
| `kernel.create_endpoint(owner_pid)` | Creates endpoint without EndpointCreated commit |

**Fix**: Endpoint creation must return `EndpointCreated` commit via Axiom.

### Category 3: Capability Management (HIGH PRIORITY)

Direct calls to `grant_capability` without proper Axiom flow.

| Operation | Issue |
|-----------|-------|
| `kernel.grant_capability(...)` | Grants capability without CapabilityGranted commit |

**Fix**: All capability grants must flow through Init (PID 1) as the permission authority, using `SYS_CAP_GRANT` syscall which goes through Axiom.

### Category 4: Syscall Handling (ARCHITECTURAL)

Manual SysLog calls and missing CommitLog entries in syscall dispatch.

| Operation | Issue |
|-----------|-------|
| `kernel.log_syscall_request(...)` | Manual logging bypasses Axiom flow |
| `kernel.handle_syscall(...)` | May create state without CommitLog |
| `kernel.log_syscall_response(...)` | Manual logging bypasses Axiom flow |

**Fix**: Refactor `Supervisor.do_syscall()` to use `axiom.syscall()` wrapper.

### Category 5: IPC Operations (MEDIUM PRIORITY)

Direct calls to `send_to_process` bypassing Axiom.

| Operation | Issue |
|-----------|-------|
| `kernel.send_to_process(...)` | Sends message without MessageSent commit |
| `kernel.ipc_receive(...)` | Receive is read-only, acceptable |

**Fix**: All sends must go through Axiom to generate `MessageSent` commits.

## Fix Pattern

### Before (Bypasses Axiom - Will Not Compile After Refactor)

```rust
// ❌ COMPILE ERROR: `kill_process` is not accessible on Kernel
self.kernel.kill_process(pid);

// ❌ COMPILE ERROR: `register_process` is not accessible on Kernel  
let pid = self.kernel.register_process(name);

// ❌ COMPILE ERROR: manual log calls don't exist
let request_id = self.kernel.log_syscall_request(pid, syscall_num, args4);
```

### After (Correct - The ONLY Way That Compiles)

```rust
// ✅ Process syscall - through Axiom
let timestamp = self.kernel.uptime_nanos();
let result = self.kernel.axiom.syscall(
    &mut self.kernel.interior,
    pid.0,
    syscall_num,
    args4,
    timestamp,
    |interior| interior.handle_syscall(pid, syscall),
);

// ✅ Supervisor-initiated operation - through Axiom
let timestamp = self.kernel.uptime_nanos();
let pid = self.kernel.axiom.internal_operation(
    &mut self.kernel.interior,
    timestamp,
    |interior| interior.register_process(name),
);

// ✅ Kill process - through Axiom
self.kernel.axiom.internal_operation(
    &mut self.kernel.interior,
    timestamp,
    |interior| interior.kill_process(pid),
);
```

## Implementation Steps

1. **Create `KernelInterior`** - Move all state and mutation methods to private struct
2. **Make mutation methods return commits** - Every operation returns `(Result, Vec<Commit>)`
3. **Remove public mutation methods from `Kernel`** - Only read-only methods remain public
4. **Update all callers** - Must go through `kernel.axiom().syscall(...)` or `kernel.axiom().internal_operation(...)`
5. **Compile** - Any bypass attempt is now a compile error

## Verification

### Replay Test

1. Boot system, perform operations
2. Save CommitLog
3. Replay from genesis
4. Verify identical state

### Audit Test

Verify every state-mutating operation appears in SysLog with:
- Request timestamp
- Caller PID
- Operation type
- Result

### Compile-Time Test

After refactoring, any attempt to call mutation methods directly on `Kernel` will fail to compile.

## Summary

| Category | Count | Priority |
|----------|-------|----------|
| Process lifecycle | 10 | HIGH |
| Endpoint management | 5 | HIGH |
| Capability management | 8 | HIGH |
| Syscall handling | ~12 | HIGH (architectural) |
| IPC operations | 3 | MEDIUM |
| Memory operations | 2 | MEDIUM |
| **Total** | **~40** | |

The Kernel Interior pattern ensures that the Axiom-first invariant is architecturally enforced, making violations impossible at compile time rather than relying on code review or conventions.
