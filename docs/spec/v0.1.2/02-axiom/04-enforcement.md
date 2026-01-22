# Axiom Boundary Enforcement

> All kernel state mutations must flow through Axiom. The kernel must reject direct access.

## Overview

The Axiom layer is the **single entry point** for all state-mutating operations in Zero OS. This document specifies:

1. The architectural invariants that Axiom enforces
2. The boundary between Axiom and Kernel
3. Enforcement mechanisms (compile-time and runtime)
4. The public API contract

## Design Invariants

### Invariant 1: All Syscalls Flow Through Axiom

Every syscall from a process must pass through `AxiomGateway.syscall()`:

```
Process → Supervisor → AxiomGateway.syscall() → Kernel → Commits → AxiomGateway
```

This ensures:
- All requests are logged to SysLog (audit)
- All state mutations are logged to CommitLog (replay)
- Sender identity is verified from trusted context

### Invariant 2: Kernel State is Private

The `KernelCore` struct containing mutable state is not directly accessible:

```rust
pub struct Kernel<H: HAL> {
    core: KernelCore<H>,      // Private - no external access
    axiom: AxiomGateway,      // Public gateway for mutations
    boot_time: u64,
}
```

Only the `Kernel` wrapper's public methods can access `core`, and those methods route through Axiom.

### Invariant 3: Commits for All State Mutations

Every operation that changes kernel state must produce a `Commit`:

| Operation | CommitType |
|-----------|------------|
| Process created | `ProcessCreated` |
| Process exited | `ProcessExited` |
| Process faulted | `ProcessFaulted` |
| Capability inserted | `CapInserted` |
| Capability removed | `CapRemoved` |
| Capability granted | `CapGranted` |
| Endpoint created | `EndpointCreated` |
| Endpoint destroyed | `EndpointDestroyed` |
| Message sent (optional) | `MessageSent` |

### Invariant 4: Deterministic Replay

Given the same `CommitLog`, replaying commits always produces the same state:

```
state = reduce(genesis, commits)
state_hash(replay(commits)) == state_hash(original)
```

### Invariant 5: Capability Checks via axiom_check()

All capability-gated operations must call `axiom_check()` before execution:

```rust
pub fn axiom_check<'a>(
    cspace: &'a CapabilitySpace,
    slot: CapSlot,
    required: &Permissions,
    expected_type: Option<ObjectType>,
    current_time: u64,
) -> Result<&'a Capability, AxiomError>;
```

This function:
1. Validates slot exists
2. Checks object type matches
3. Verifies permissions are sufficient
4. Checks expiration

## CommitType Coverage

### Implemented CommitTypes

```rust
pub enum CommitType {
    // Process lifecycle
    Genesis,
    ProcessCreated { pid, parent, name },
    ProcessExited { pid, code },
    ProcessFaulted { pid, reason, description },
    
    // Capability mutations
    CapInserted { pid, slot, cap_id, object_type, object_id, perms },
    CapRemoved { pid, slot },
    CapGranted { from_pid, to_pid, from_slot, to_slot, new_cap_id, perms },
    
    // Endpoint lifecycle
    EndpointCreated { id, owner },
    EndpointDestroyed { id },
    
    // IPC events (optional, for audit)
    MessageSent { from_pid, to_endpoint, tag, size },
}
```

### Message Logging Policy

`MessageSent` commits are **optional** because:

1. Message content is potentially large and sensitive
2. Message queues are volatile (not part of replayed state)
3. Only metadata is logged for audit purposes

When enabled, `MessageSent` provides:
- Full IPC audit trail
- Message rate tracking
- Debugging visibility

## Syscall → Commit Mapping

| Syscall | Commits Produced | Notes |
|---------|------------------|-------|
| `SYS_EXIT` (0x11) | `ProcessExited` | Via `kill_process` |
| `SYS_EP_CREATE` (0x35) | `EndpointCreated`, `CapInserted` | Creates endpoint + capability |
| `SYS_SEND` (0x40) | `MessageSent` (optional) | Metadata only if enabled |
| `SYS_RECEIVE` (0x41) | None | Read-only operation |
| `SYS_CAP_GRANT` (0x30) | `CapGranted`, `CapInserted` | Source + destination |
| `SYS_CAP_REVOKE` (0x31) | `CapRemoved` | Removes from CSpace |
| `SYS_DEBUG` (0x01) | None | Side-effect only |
| `SYS_GET_TIME` (0x02) | None | Read-only |
| `SYS_GET_PID` (0x03) | None | Read-only |
| `SYS_YIELD` (0x12) | None | No state change |

## Public API Contract

### Allowed External Access

```rust
impl<H: HAL> Kernel<H> {
    // State-mutating (routes through Axiom)
    pub fn dispatch_syscall(...) -> SyscallResult;
    pub fn handle_syscall(...) -> SyscallResult;
    
    // Read-only inspection
    pub fn get_process(&self, pid: ProcessId) -> Option<&Process>;
    pub fn list_processes(&self) -> Vec<(ProcessId, String)>;
    pub fn get_cap_space(&self, pid: ProcessId) -> Option<&CapabilitySpace>;
    pub fn commitlog(&self) -> &CommitLog;
    pub fn syslog(&self) -> &SysLog;
    pub fn uptime_nanos(&self) -> u64;
    
    // HAL access (for process management)
    pub fn hal(&self) -> &H;
}
```

### Forbidden Direct Access

The following patterns violate the Axiom boundary:

```rust
// BAD: Direct kernel core access
kernel.core.processes.insert(...);
kernel.core.cap_spaces.get_mut(...);

// BAD: Bypassing Axiom for state changes
kernel.core.grant_capability(...);
kernel.core.kill_process(...);

// BAD: Direct CommitLog manipulation
kernel.axiom.commitlog_mut().append(...);
```

## Enforcement Mechanisms

### Compile-Time Enforcement

The `KernelCore` struct is defined with `pub(crate)` visibility:

```rust
pub(crate) struct KernelCore<H: HAL> {
    hal: H,
    processes: BTreeMap<ProcessId, Process>,
    cap_spaces: BTreeMap<ProcessId, CapabilitySpace>,
    endpoints: BTreeMap<EndpointId, Endpoint>,
    // ...
}
```

External crates cannot access `KernelCore` directly.

### Runtime Enforcement

Debug builds include assertions that verify Axiom context:

```rust
#[cfg(debug_assertions)]
fn verify_axiom_context(&self) {
    // Panic if called outside Axiom gateway
}
```

## Replay Correctness

### Replayable Trait

```rust
pub trait Replayable {
    fn replay_genesis(&mut self) -> ReplayResult<()>;
    fn replay_create_process(...) -> ReplayResult<()>;
    fn replay_exit_process(...) -> ReplayResult<()>;
    fn replay_process_faulted(...) -> ReplayResult<()>;
    fn replay_insert_capability(...) -> ReplayResult<()>;
    fn replay_remove_capability(...) -> ReplayResult<()>;
    fn replay_cap_granted(...) -> ReplayResult<()>;
    fn replay_create_endpoint(...) -> ReplayResult<()>;
    fn replay_destroy_endpoint(...) -> ReplayResult<()>;
    fn replay_message_sent(...) -> ReplayResult<()>;
    fn state_hash(&self) -> [u8; 32];
}
```

### Replay Verification

```rust
pub fn replay_and_verify<R: Replayable>(
    state: &mut R,
    commits: &[Commit],
    expected_hash: [u8; 32],
) -> ReplayResult<()>;
```

This function:
1. Applies all commits in sequence
2. Computes final state hash
3. Verifies hash matches expected value

## Hash Chain Integrity

The CommitLog maintains a hash chain:

```
Genesis(hash=H0) → Commit1(prev=H0, hash=H1) → Commit2(prev=H1, hash=H2) → ...
```

Verification:
```rust
pub fn verify_integrity(&self) -> bool {
    for commit in &self.commits {
        if commit.prev_commit != expected_prev { return false; }
        if compute_hash(commit) != commit.id { return false; }
        expected_prev = commit.id;
    }
    true
}
```

## SysLog Filtering Policy

For performance, not all syscalls are logged to SysLog. The policy is:

### Always Logged (State-Mutating or Auditable)

| Syscall | Reason |
|---------|--------|
| `SYS_DEBUG` (0x01) | Audit trail for debug output |
| `SYS_EXIT` (0x11) | State-mutating (process termination) |
| `SYS_EP_CREATE` (0x35) | State-mutating (endpoint creation) |
| `SYS_SEND` (0x40) | Communication audit trail |
| `SYS_CAP_GRANT` (0x30) | State-mutating (capability transfer) |
| `SYS_CAP_REVOKE` (0x31) | State-mutating (capability removal) |
| All others | Default to logged for safety |

### Not Logged (Read-Only)

| Syscall | Reason |
|---------|--------|
| `NOP` (0x00) | No-op, no side effects |
| `SYS_GET_TIME` (0x02) | Read-only |
| `SYS_GET_PID` (0x03) | Read-only |
| `SYS_LIST_CAPS` (0x04) | Read-only |
| `SYS_LIST_PROCS` (0x05) | Read-only |
| `SYS_GET_WALLCLOCK` (0x06) | Read-only |
| `SYS_YIELD` (0x12) | No state change |
| `SYS_RECEIVE` (0x41) | Read-only (polling) |

### Rationale

1. **Read-only operations** don't affect state, so excluding them:
   - Reduces SysLog size significantly (time queries are frequent)
   - Doesn't affect replay correctness
   - Improves performance

2. **All state-mutating operations** are logged because:
   - Required for complete audit trail
   - May be useful for debugging
   - Default-to-log ensures nothing is missed

## WASM Notes

### IndexedDB Persistence

On WASM targets, commits are persisted to IndexedDB:

- `commit_to_js()` serializes commits
- `persistEntries()` batches writes
- `loadAll()` restores on page reload

### JS Supervisor Minimization

The JavaScript supervisor should only:
- Manage Web Worker lifecycle
- Transfer SharedArrayBuffer for syscall mailbox
- Poll for pending syscalls

All syscall processing logic resides in Rust.

## Security Considerations

### Capability Bypass Prevention

All paths that access kernel resources must:
1. Obtain capability from CSpace
2. Call `axiom_check()` to verify authority
3. Log the operation to SysLog

### Audit Trail Completeness

The SysLog captures:
- All syscall requests (including failed ones)
- All syscall responses
- Timestamp and sender for each event

### Replay Attack Prevention

The hash chain ensures:
- Commits cannot be reordered
- Commits cannot be modified
- Commits cannot be removed
