# Axiom: Verification Layer

> Axiom is the verification layer that records all syscalls and state changes. It maintains two logs: SysLog (audit trail) and CommitLog (deterministic replay).

## Overview

Axiom provides:

1. **SysLog**: Records every syscall (request + response) for audit purposes
2. **CommitLog**: Records state mutations for deterministic replay
3. **Sender Verification**: Ensures caller identity from trusted context

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              AXIOM                                          │
│                                                                             │
│  ┌─────────────────────────────┐  ┌─────────────────────────────────────┐  │
│  │         SysLog              │  │           CommitLog                 │  │
│  │                             │  │                                     │  │
│  │  • All syscall requests     │  │  • State mutations only             │  │
│  │  • All syscall responses    │  │  • Hash-chained                     │  │
│  │  • Audit trail              │  │  • Deterministic replay             │  │
│  │  • NOT used for replay      │  │  • Checkpoint-signed                │  │
│  │                             │  │                                     │  │
│  └─────────────────────────────┘  └─────────────────────────────────────┘  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Core Principle

**Everything that mutates kernel state happens via Commits.** A Commit is an atomic state change event. Commits are appended to an immutable CommitLog and applied to a deterministic kernel state machine.

**Two logs, two purposes:**

| Log | Purpose | Used for Replay |
|-----|---------|-----------------|
| SysLog | Audit trail ("what was asked, what was answered") | No |
| CommitLog | State mutations ("what actually changed") | Yes |

## Two-Log Model

### SysLog (Audit)

The SysLog records every syscall for audit purposes:

- All requests from processes
- All responses from kernel
- Complete audit trail
- NOT used for replay (can be deleted without losing state)

### CommitLog (State)

The CommitLog records actual state mutations:

- Only successful state changes
- Hash-chained for tamper-evidence
- Checkpoint-signed for verification
- Deterministic replay: `reduce(genesis, commits) -> state`

### Relationship

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              SysLog (Audit)                                 │
│  Records: Every syscall request and response                                │
│  Purpose: "What was asked, what was answered"                               │
│  NOT used for replay                                                        │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ causes
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           CommitLog (State)                                 │
│  Records: State mutations only                                              │
│  Purpose: "What actually changed"                                           │
│  Used for deterministic replay                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

A SysEvent may cause zero, one, or many Commits:

| SysEvent | Commits Generated |
|----------|-------------------|
| `CapGrant` (success) | `CapInserted` |
| `CapGrant` (failure) | None (state unchanged) |
| `Spawn` | `ProcessCreated`, `CapInserted` (multiple) |
| `Exit` | `ProcessExited`, `CapRemoved` (cleanup), `EndpointDestroyed` |

## Syscall Flow

```
Process A                  Axiom                      Kernel
   │                         │                          │
   │─── syscall(CapGrant) ──▶│                          │
   │                         │                          │
   │                         │ 1. Get sender from       │
   │                         │    trusted context       │
   │                         │                          │
   │                         │ 2. Create SysEvent       │
   │                         │    (request)             │
   │                         │                          │
   │                         │ 3. Append to SysLog      │
   │                         │    (async)               │
   │                         │                          │
   │                         │─── forward request ─────▶│
   │                         │                          │
   │                         │                          │ 4. Verify caps
   │                         │                          │
   │                         │                          │ 5. Execute
   │                         │                          │
   │                         │                          │ 6. Emit Commit(s)
   │                         │                          │    if successful
   │                         │                          │
   │                         │◀── result + Commits ─────│
   │                         │                          │
   │                         │ 7. Append Commits to     │
   │                         │    CommitLog (ordered)   │
   │                         │                          │
   │                         │ 8. Create SysEvent       │
   │                         │    (response)            │
   │                         │                          │
   │                         │ 9. Append to SysLog      │
   │                         │                          │
   │◀── result ──────────────│                          │
   │                         │                          │
```

## Sender Verification

**Critical:** The `sender` field in a SysEvent CANNOT be self-reported by the application. Both Axiom and Kernel verify sender identity from trusted context.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ HARDWARE/RUNTIME LEVEL (cannot be spoofed)                                  │
│                                                                             │
│ • Native: CPU context register holds current PID                            │
│ • WASM: Supervisor tracks which WASM instance made the call                 │
│ • The process CANNOT lie about who it is                                    │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ AXIOM verifies sender                                                       │
│                                                                             │
│ 1. Get caller PID from trusted context (NOT from request payload)           │
│ 2. Verify caller PID exists and is valid                                    │
│ 3. Set SysEvent.sender = verified PID                                       │
│ 4. Log SysEvent with verified sender                                        │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ KERNEL verifies sender                                                      │
│                                                                             │
│ 1. Get sender from SysEvent (set by Axiom)                                  │
│ 2. Verify sender matches the calling context                                │
│ 3. Look up sender's CSpace                                                  │
│ 4. Check capability in THAT CSpace (not any other)                          │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Separation of Concerns

| Layer | Responsibility | Owns |
|-------|----------------|------|
| **Axiom** | Verification + Recording | SysLog, CommitLog |
| **Kernel** | Capabilities + Execution | CSpaces, Scheduler, emits Commits |

### Axiom (Verification Layer)

- **Entry point** for all syscalls - nothing bypasses Axiom
- **Verifies** sender identity from trusted context (cannot be spoofed)
- **SysLog**: Records every syscall (request + response) for audit
- **CommitLog**: Records state changes for deterministic replay
- **Sequencing**: Assigns commit sequence numbers, maintains hash chain
- **Persistence**: Writes logs to storage (async)

### Kernel (Execution)

- **Capabilities**: CSpace tables define what each process can do
- **Verification**: Kernel verifies capabilities before execution
- **Scheduling**: Thread scheduler decides when syscalls run
- **Execution**: Processes syscall, emits Commits for state changes
- **Commits**: Kernel emits `Commit` events for every state mutation

## Concurrency Model

**Synchronous per core, parallel across cores.** Each core executes syscalls synchronously (one at a time, blocking until complete). Multiple cores execute in parallel. The CommitLog enforces a total ordering on state changes.

```
Core 1: syscall A ──▶ execute ──▶ return ──▶ syscall C ──▶ ...
                           │
                           └──▶ Commit(seq=42) ──┐
                                                 ├──▶ CommitLog (ordered)
Core 2: syscall B ──▶ execute ──▶ return ──▶ ...│
                           │                     │
                           └──▶ Commit(seq=43) ──┘
```

**Key points:**

- **Synchronous per core** - syscall blocks until result is returned
- **Parallel across cores** - multiple cores execute concurrently
- **Sequential commits** - CommitLog serializes all state changes
- **Deterministic replay** - commit order is the source of truth

## Determinism

**Same CommitLog = same state.** Because:

- CommitLog contains all state changes
- Each Commit is applied atomically
- Commit order is deterministic (sequence numbers)
- `reduce(genesis, commits) -> state` is a pure function

**SysLog is NOT needed for replay.** It's just the audit trail. You could delete the SysLog and still reconstruct state from CommitLog.

## Component Specifications

| Component | File | Description |
|-----------|------|-------------|
| SysLog | [01-syslog.md](01-syslog.md) | SysEvent types, audit trail |
| CommitLog | [02-commitlog.md](02-commitlog.md) | Commit types, hash chain |
| Replay | [03-replay.md](03-replay.md) | State reconstruction |

## Properties

1. **Tamper-Evident**: CommitLog is hash-chained; any modification is detectable.

2. **Verifiable**: Periodic checkpoints with signed state hashes enable verification.

3. **Deterministic**: Same CommitLog always produces same state.

4. **Auditable**: SysLog provides complete audit trail of all syscalls.

5. **Recoverable**: State can be reconstructed from CommitLog at any time.

## WASM Notes

### Storage

On WASM, both logs are persisted to IndexedDB:

```javascript
// Simplified persistence API
async function persistCommit(commit) {
    const tx = db.transaction('commit_log', 'readwrite');
    const store = tx.objectStore('commit_log');
    await store.add(commit);
    await tx.complete;
}
```

### Limitations

- **Async Commit**: IndexedDB writes are asynchronous, so there's a window where logged entries aren't durable.
- **No Hardware Protection**: The supervisor (JavaScript) must be trusted.

### Mitigations

- Critical operations can wait for IndexedDB transaction commit before returning.
- Logs can be periodically checksummed and stored in multiple locations.
