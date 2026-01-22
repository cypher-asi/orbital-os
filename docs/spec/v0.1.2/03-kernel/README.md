# Microkernel Specification

> The Zero microkernel is a capability-based kernel designed for formal verification. It executes syscalls and emits Commits for all state changes.

## Architecture: Axiom-Gated Kernel

Every syscall flows through the Axiom verification layer before reaching kernel primitives. The kernel emits Commits for all state mutations:

```
┌─────────────────────────────────────────────────────────────────┐
│                         SYSCALL ENTRY                           │
│                      (06-syscalls.md)                           │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     AXIOM (VERIFICATION)                        │
│                      (../02-axiom/)                             │
│                                                                 │
│   • Verify sender from trusted context                          │
│   • Log SysEvent to SysLog                                      │
│   • Forward to kernel                                           │
└─────────────────────────────┬───────────────────────────────────┘
                              │ (verified request)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                         KERNEL                                  │
│                                                                 │
│   • Check capability in caller's CSpace                         │
│   • Execute operation                                           │
│   • Emit Commit(s) for state changes                           │
│                                                                 │
│         ┌───────────┬───────────┬───────────┐                  │
│         │  THREADS  │    VMM    │    IPC    │                  │
│         │01-threads │ 02-vmm    │  04-ipc   │                  │
│         └───────────┴───────────┴───────────┘                  │
│                         │                                       │
│                         ▼                                       │
│                  ┌───────────────┐                              │
│                  │  INTERRUPTS   │                              │
│                  │05-interrupts  │                              │
│                  └───────────────┘                              │
│                                                                 │
└─────────────────────────────┬───────────────────────────────────┘
                              │ (result + Commits)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     AXIOM (RECORDING)                           │
│                                                                 │
│   • Append Commits to CommitLog                                 │
│   • Log response SysEvent to SysLog                            │
│   • Return result to caller                                     │
└─────────────────────────────────────────────────────────────────┘
```

## Separation of Concerns

| Layer | Responsibility | Owns |
|-------|----------------|------|
| **Axiom** | Verification + Recording | SysLog, CommitLog |
| **Kernel** | Capabilities + Execution | CSpaces, Scheduler, emits Commits |

The kernel's role:
- **Capabilities**: CSpace tables define what each process can do
- **Verification**: Kernel verifies capabilities before execution
- **Scheduling**: Thread scheduler decides when syscalls run
- **Execution**: Processes syscall, emits Commits for state changes
- **Commits**: Every state mutation generates a Commit event

## Formal Verification Target

The kernel is designed to be formally verifiable. Total code budget: **<3000 LOC**.

### LOC Budget by Component

| Component     | File                | Budget LOC | Responsibility                      |
|---------------|---------------------|------------|-------------------------------------|
| Capabilities  | `03-capabilities.md`| ~700       | CSpace tables, capability checking  |
| Threads       | `01-threads.md`     | ~500       | TCBs, scheduling, context switch    |
| VMM           | `02-vmm.md`         | ~800       | Address spaces, memory management   |
| IPC           | `04-ipc.md`         | ~400       | Message passing, cap transfer       |
| Interrupts    | `05-interrupts.md`  | ~200       | IRQ routing, handlers               |
| Syscalls      | `06-syscalls.md`    | ~100       | Entry/exit, ABI marshaling          |
| **Total**     |                     | **~2700**  |                                     |

### Why Formal Verification?

A formally verified kernel provides:

1. **Guaranteed Memory Safety**: No buffer overflows, use-after-free, or data races.
2. **Capability Integrity**: Capabilities cannot be forged, only derived through authorized operations.
3. **Deterministic Replay**: CommitLog enables exact state reconstruction.
4. **Deadlock Freedom**: Thread and IPC operations terminate or block predictably.

## Key Invariants

These properties must hold at all times:

### 1. Kernel Emits Commits for All State Changes

> Every state mutation generates a Commit that is recorded by Axiom.

```rust
// Kernel processes syscall and emits commits
fn handle_cap_grant(
    state: &mut State,
    event_id: EventId,
    req: CapGrantRequest,
) -> Result<(CapSlot, Vec<Commit>), Error> {
    // 1. Check capability
    let cap = state.cspaces[req.from_pid].get(req.slot)?;
    if !cap.permissions.grant {
        return Err(Error::PermissionDenied);  // No commits on failure
    }
    
    // 2. Execute operation
    let new_slot = state.cspaces[req.to_pid].next_free_slot();
    let new_cap = cap.attenuate(req.perms);
    state.cspaces[req.to_pid].insert(new_slot, new_cap.clone());
    
    // 3. Emit commit for this state change
    let commit = Commit {
        commit_type: CommitType::CapInserted {
            pid: req.to_pid,
            slot: new_slot,
            cap: new_cap,
        },
        caused_by: Some(event_id),
        ..
    };
    
    Ok((new_slot, vec![commit]))
}
```

### 2. Capability Integrity

> Capabilities are unforgeable and can only be:
> - Created by the kernel (at object creation)
> - Derived through grant (with attenuation)
> - Transferred via IPC
> - Revoked by holder with grant permission

### 3. No Privilege Escalation

> Derived capabilities never exceed the permissions of their source.

```rust
fn derive_capability(source: &Capability, new_perms: Permissions) -> Capability {
    Capability {
        permissions: Permissions {
            read: source.permissions.read && new_perms.read,
            write: source.permissions.write && new_perms.write,
            grant: source.permissions.grant && new_perms.grant,
        },
        // ... other fields
    }
}
```

### 4. Memory Isolation

> A process can only access memory it has a valid capability for.

### 5. Deterministic Execution

> Same sequence of syscalls produces same sequence of Commits.

## Verification-Friendly Coding Style

The kernel follows these rules to enable formal verification:

### No Dynamic Allocation in Core Paths

```rust
// BAD: Allocates on heap
fn process_message(msg: &[u8]) -> Vec<u8> {
    msg.to_vec()  // Allocation!
}

// GOOD: Fixed-size buffers
fn process_message(msg: &[u8], out: &mut [u8; MAX_MSG_SIZE]) -> usize {
    let len = msg.len().min(MAX_MSG_SIZE);
    out[..len].copy_from_slice(&msg[..len]);
    len
}
```

### No Recursion

```rust
// BAD: Could overflow stack
fn traverse(node: &Node) {
    for child in node.children() {
        traverse(child);
    }
}

// GOOD: Iterative with explicit stack
fn traverse(root: &Node) {
    let mut stack = ArrayVec::<&Node, MAX_DEPTH>::new();
    stack.push(root);
    while let Some(node) = stack.pop() {
        // Process node
        for child in node.children() {
            stack.try_push(child).expect("depth exceeded");
        }
    }
}
```

### Pre/Post Conditions Documented

```rust
/// Send a message to an endpoint.
///
/// # Pre-conditions
/// - `cap` must reference a valid endpoint
/// - `cap.permissions.write` must be true
/// - `msg.len() <= MAX_MESSAGE_SIZE`
///
/// # Post-conditions
/// - Message is queued at endpoint
/// - CapInserted commit emitted if caps transferred
/// - Sender's ipc_sent incremented
fn ipc_send(cap: &Capability, msg: &[u8]) -> Result<((), Vec<Commit>), IpcError> {
    // ...
}
```

### Bounded Loops

```rust
// BAD: Unbounded
while !queue.is_empty() {
    process(queue.pop());
}

// GOOD: Bounded with explicit limit
for _ in 0..MAX_QUEUE_DEPTH {
    if let Some(item) = queue.pop() {
        process(item);
    } else {
        break;
    }
}
```

## Kernel State

The kernel maintains the following state (reconstructed from CommitLog at boot):

```rust
pub struct Kernel<H: HAL> {
    /// Platform abstraction layer
    hal: H,
    
    /// Process table: PID -> Process
    processes: BTreeMap<ProcessId, Process>,
    
    /// Capability spaces: PID -> CSpace
    cap_spaces: BTreeMap<ProcessId, CapabilitySpace>,
    
    /// IPC endpoints: EID -> Endpoint
    endpoints: BTreeMap<EndpointId, Endpoint>,
    
    /// ID generators (reconstructed from max IDs in commits)
    next_pid: u64,
    next_eid: u64,
    next_cap_id: u64,
    
    /// Boot time for uptime calculation
    boot_time: u64,
}
```

Note: The kernel does NOT own the logs. Axiom owns the SysLog and CommitLog.

## Component Specifications

| Component       | File                                  | Description                    |
|-----------------|---------------------------------------|--------------------------------|
| Threads         | [01-threads.md](01-threads.md)        | TCB, scheduling, states        |
| VMM             | [02-vmm.md](02-vmm.md)                | Address spaces, memory         |
| Capabilities    | [03-capabilities.md](03-capabilities.md) | CSpace, capability checking |
| IPC             | [04-ipc.md](04-ipc.md)                | Message passing                |
| Interrupts      | [05-interrupts.md](05-interrupts.md)  | IRQ handling                   |
| Syscalls        | [06-syscalls.md](06-syscalls.md)      | ABI definition                 |

## Related Specifications

| Spec | Location | Description |
|------|----------|-------------|
| Axiom | [../02-axiom/](../02-axiom/) | Verification layer (SysLog + CommitLog) |
| HAL | [../01-hal/](../01-hal/) | Hardware abstraction |
| Init | [../04-init/](../04-init/) | Bootstrap and supervision |

## WASM-Specific Considerations

On the WASM target:

1. **No preemption**: Processes yield cooperatively
2. **Single-threaded**: Each Web Worker is a single thread
3. **No interrupts**: Async event handling instead
4. **No VMM**: Linear memory only, managed by WASM runtime
5. **External supervisor**: JavaScript supervisor coordinates processes

See individual component specs for WASM-specific details.
