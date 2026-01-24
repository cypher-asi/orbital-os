# Microkernel System Design — Architectural Invariants

*(Including Axiom, Supervisor, and Kernel Size Constraints)*

---

## Scope and Applicability

These invariants are **non-negotiable** and apply to **all targets**: WASM, QEMU, and baremetal.

There are no target-specific exceptions to the security model. While targets differ in **hardware capabilities** (e.g., preemptive scheduling, memory protection), the **architectural invariants** defined here must be enforced by software on every platform.

```
┌──────────────┐      IPC       ┌──────────────┐      Axiom      ┌──────────┐
│  Supervisor  │ ───────────▶  │   Process    │ ─────────────▶  │  Kernel  │
│  (JS/Native) │               │ (Init, etc.) │                 │          │
└──────────────┘               └──────────────┘                 └──────────┘
```

The supervisor communicates with the system via **IPC to processes**, which then interact with Axiom and the Kernel. The supervisor never has direct kernel access.

---

## 1. Fundamental Axioms (System Truths)

1. **All Authority Flows Through Axiom**

   * No process, service, or supervisor may directly invoke the kernel
   * **Axiom is the sole syscall entry point**
   * All kernel interaction is mediated, verified, and recorded by Axiom

2. **Kernel State Is Mutated Only by Commits**

   * All kernel state changes occur as **atomic Commits**
   * Commits are appended to an immutable **CommitLog**
   * Kernel state is defined as:

     ```
     reduce(genesis_state, CommitLog) → current_state
     ```

3. **Everything That Matters Is Observable**

   * Every syscall request and response is recorded in **SysLog**
   * Every state mutation is recorded in **CommitLog**
   * No hidden or implicit kernel behavior exists

---

## 2. Kernel Invariants (Non-Negotiable)

4. **Kernel Minimality**

   * The kernel implements *only*:

     * IPC
     * Scheduling
     * Address spaces
     * Capability enforcement
     * Commit emission
   * No filesystems, identity, policy, or drivers live in kernel space

5. **Hard Kernel Size Limit**

   * **The kernel must remain ≤ 3,000 lines of code**
   * Excludes:

     * Tests
     * Comments
     * Build scripts
   * Includes:

     * All executable kernel logic
   * Any feature that threatens this limit **must move to userspace**

6. **Small Kernel Is a Security Property**

   * Kernel size is a **first-class invariant**, not an optimization
   * The size limit exists to ensure:

     * Auditability
     * Formal reasoning
     * Verifiability
     * Reduced attack surface

7. **No Policy in the Kernel**

   * Kernel does **not** interpret:

     * Paths
     * User identities
     * Permissions
     * Security labels
   * Kernel enforces mechanism only

8. **Implicit Capability Enforcement**

   * Kernel never answers "is this allowed?"
   * Capability checks are implicit during execution
   * If execution occurs, authorization was valid

---

## 3. Axiom Invariants (Verification & Recording Layer)

9. **Axiom Is the Single Syscall Gateway**

   * All syscalls follow:

     ```
     Process → Axiom → Kernel → Axiom → Process
     ```
   * No bypass paths exist

10. **Sender Identity Is Non-Spoofable**

    * Sender identity is derived from **trusted execution context**
    * Never from request payloads
    * Both Axiom and Kernel independently verify sender identity

11. **Two-Log Model Is Mandatory**

    * **SysLog**:

      * Records all syscall requests and responses
      * Audit-only, not used for replay
    * **CommitLog**:

      * Records only successful state mutations
      * Hash-chained, ordered
      * Used for deterministic replay

12. **SysEvents ≠ State Changes**

    * A syscall may generate:

      * Zero commits
      * One commit
      * Many commits
    * Only commits mutate kernel state

---

## 4. Supervisor Invariants (Thin Boundary Rule)

13. **Supervisor Is a Thin Transport Boundary**

    * Supervisor only:

      * Relays data between **web client ↔ processes**
      * Uses **standard IPC calls** to communicate with processes
    * Performs:

      * No policy decisions
      * No authority checks
      * No state mutation
      * No kernel calls

14. **Supervisor Has No Ambient Authority**

    * Holds no privileged capabilities
    * Cannot mint, modify, or revoke capabilities
    * Cannot fabricate or alter SysEvents or Commits

15. **Supervisor Is Not a Security Boundary**

    * Security does **not** rely on supervisor correctness
    * Supervisor compromise cannot:

      * Access kernel
      * Escalate privileges
      * Mutate state
    * At worst: drop, delay, or reorder messages

16. **Supervisor Cannot Bypass Axiom**

    * Supervisor may not:

      * Call kernel directly
      * Inject commits
      * Forge sender identity

---

## 5. Capability Model Invariants

17. **Capabilities Are Primitive**

    * Kernel capabilities reference only kernel objects:

      * IPC endpoints
      * Processes
      * Memory regions
      * IRQ handlers
      * I/O ports
      * Console
    * Rights are fixed bitmasks (read, write, grant)

18. **No Kernel-Level Semantic Capabilities**

    * No `FileCap`, `SocketCap`, or `AdminCap` in kernel
    * All semantic authority lives in userspace

19. **Delegation Over Ambient Authority**

    * No implicit privileges
    * All authority is explicitly delegated
    * "Root" is not special — only more capable

---

## 6. IPC, Data Flow & Memory Invariants

20. **All Cross-Domain Interaction Uses IPC**

    * No shared global state
    * No direct cross-process memory access
    * No hidden call paths

21. **Sync vs Async Is Explicit**

    * Sync IPC blocks
    * Async IPC uses notifications
    * No implicit concurrency

22. **Kernel Never Returns Data**

    * Kernel routes messages only
    * Data flows via:

      * IPC payloads (small)
      * Shared memory (bulk)
      * Capability transfer

---

## 7. Filesystem & Namespace Invariants

23. **Filesystems Are Userspace Services**

    * Path resolution, namespaces, permissions live outside kernel
    * File handles are protocol-level constructs

24. **Per-Process Namespaces**

    * No global `/`
    * Each process sees only explicitly granted namespaces

---

## 8. Determinism & Verifiability Invariants

25. **CommitLog Is the Source of Truth**

    * Same CommitLog → same system state
    * Commit ordering defines reality

26. **SysLog Is Discardable**

    * SysLog may be deleted without affecting correctness
    * CommitLog alone is sufficient for recovery

27. **System State Is Reconstructible**

    * Full system state can be rebuilt from:

      * Genesis state
      * CommitLog

---

## 10. Storage Access Invariants

28. **All Storage Access Through HAL**

    * Process storage operations use syscalls which route through HAL
    * Supervisor bootstrap uses HAL's `bootstrap_storage_*` methods or internal async vfs module (before processes exist)
    * React UI may read from ZosStorage caches (read-only, caches populated via HAL)
    * No direct IndexedDB access outside HAL/ZosStorage implementation
    * Single `ZosStorage` JS object is the only IndexedDB interface

29. **Unified ZosStorage Object**

    * `VfsStorage` and `ZosStorage` are consolidated into a single `ZosStorage` object
    * All IndexedDB operations flow through this single object
    * Provides three access patterns:
      * **Runtime Path**: VFS Service → syscall → HAL → ZosStorage
      * **Bootstrap Path**: Supervisor Boot → HAL/vfs → ZosStorage
      * **Read-Only Path**: React Components → ZosStorageClient → ZosStorage sync caches

30. **Bootstrap Storage Exception**

    * The supervisor may use async vfs module functions during bootstrap
    * This is acceptable because:
      * Bootstrap happens once at system start
      * HAL trait methods are synchronous but IndexedDB is async
      * After Init starts, all storage access goes through processes
    * The vfs module is internal to zos-supervisor-web

---

## 11. Target Capabilities

The following hardware capabilities vary by target but do **not** affect the architectural invariants above:

| Capability | WASM | QEMU | Baremetal |
|------------|------|------|-----------|
| Scheduling | Cooperative | Preemptive | Preemptive |
| Memory protection | Software (WASM sandbox) | Hardware (page tables) | Hardware (MMU) |
| Process isolation | Web Workers | Hardware VMM | Hardware MMU |
| Multi-threading | No (single-threaded workers) | Yes | Yes |
| IRQ handling | N/A (event-based) | Hardware (IOAPIC) | Hardware (APIC) |

**Key principle**: Even without hardware enforcement, the software architecture must uphold all invariants. The WASM target enforces isolation through the WASM sandbox and correct software design, not through hardware protection.

---

## One-Sentence Mental Model

> **The Supervisor only transports, Axiom verifies and records, the kernel executes and emits commits — and the kernel remains small enough to be fully understood.**

---

## Why the 3,000 LOC Invariant Matters

This single rule:

* Forces *everything interesting* into userspace
* Keeps the kernel auditable by one human
* Makes formal methods realistic
* Prevents "just one more feature" creep

---

## Appendix: Current Implementation Violations

The following are known violations in the current codebase that must be fixed to comply with these invariants:

| Violation | Location | Invariant Violated | Status | Required Fix |
|-----------|----------|-------------------|--------|--------------|
| ~~`Supervisor.revoke_capability()`~~ | `zos-supervisor-web/src/supervisor/mod.rs` | 14, 16 | **FIXED** | ~~Route through PermissionManager process via IPC~~ |
| ~~`kernel.deliver_console_input()`~~ | `zos-kernel/src/kernel_impl.rs` | 13, 16 | **FIXED** | ~~Use IPC with capability granted by Init~~ |
| ~~`kernel.deliver_supervisor_ipc()`~~ | `zos-kernel/src/kernel_impl.rs` | 13, 16 | **FIXED** | ~~Method removed; routes via Init~~ |
| Direct `kernel.kill_process()` | `zos-supervisor-web/src/supervisor/mod.rs` | 13, 16 | **NOT FIXED** | Route kill requests through Init via `MSG_SUPERVISOR_KILL_PROCESS` |

### Fixed Violations

1. **`Supervisor.revoke_capability()`**: Now routes through PermissionManager (PID 2) via IPC using `MSG_SUPERVISOR_REVOKE_CAP (0x2020)`. The supervisor sends an IPC message to PM, which performs the capability deletion and notifies the affected process.

2. **`kernel.deliver_console_input()`**: Method removed. Console input now uses capability-checked IPC routed through Init via `MSG_SUPERVISOR_CONSOLE_INPUT (0x2001)`.

3. **`kernel.deliver_supervisor_ipc()`**: Method removed. IPC delivery now routes through Init via `MSG_SUPERVISOR_IPC_DELIVERY (0x2003)`.

### Remaining Violation

**Direct `kernel.kill_process()` calls**: The supervisor still directly calls `kernel.kill_process()` in multiple locations:

- `crates/zos-supervisor-web/src/supervisor/mod.rs` (multiple locations)
- `crates/zos-supervisor-web/src/supervisor/spawn.rs`
- `crates/zos-supervisor-web/src/pingpong.rs`

**Required Fix**: Route kill requests through Init via `MSG_SUPERVISOR_KILL_PROCESS (0x2002)`, which already exists. The Init process already has a handler (`handle_supervisor_kill_process`) that invokes `syscall::kill()`.

---

### IPC Protocol Consolidation

All IPC message constants are now centralized in the `zos-ipc` crate as the single source of truth. This eliminates duplicate constant definitions and ensures consistent values across all crates.

Key fix: The `MSG_SUPERVISOR_REVOKE_CAP` constant was incorrectly defined as `0x2010` in the supervisor, which conflicted with `MSG_REQUEST_CAPABILITY`. It has been corrected to use `0x2020` from `zos-ipc`.

These are **implementation bugs**, not acceptable deviations. The supervisor must never have direct kernel access on any target.
