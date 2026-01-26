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
   * **Axiom is the verification layer through which all syscalls must pass**
   * Axiom and KernelCore are **separate components** - Axiom gates access to KernelCore
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

**Architecture: System Struct**

The `System<H>` struct combines Axiom (verification layer) and KernelCore (execution layer):

```
┌─────────────────────────────────────────────────────────────┐
│                          SYSTEM                              │
│                                                             │
│   ┌───────────────────────────────────────────────────┐     │
│   │                      AXIOM                         │     │
│   │   - Verification layer (sender identity)          │     │
│   │   - SysLog (audit trail)                          │     │
│   │   - CommitLog (state mutations)                   │     │
│   │   - THE entry point for all syscalls              │     │
│   └───────────────────────────────────────────────────┘     │
│                              │                               │
│                              │ (verified request)            │
│                              ▼                               │
│   ┌───────────────────────────────────────────────────┐     │
│   │                   KERNEL CORE                      │     │
│   │   - Capabilities & CSpaces                        │     │
│   │   - Process state                                 │     │
│   │   - IPC endpoints                                 │     │
│   │   - Emits Commits for state changes               │     │
│   └───────────────────────────────────────────────────┘     │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

The System struct instantiates Axiom and KernelCore separately:

```rust
pub struct System<H: HAL> {
    pub axiom: AxiomGateway,  // Verification layer
    pub kernel: KernelCore<H>, // Execution layer
}
```

This architecture ensures Axiom and Kernel remain **separate concerns** with no coupling or ownership relationship.

9. **Axiom Is the Single Syscall Gateway**

   * Axiom does **not** own the kernel; rather, it **gates all access** to the kernel
   * Axiom and KernelCore are separate components combined in the `System` struct
   * The syscall flow is:

     ```
     Process → System.process_syscall() → Axiom (verify, log request) →
     KernelCore (execute, emit commits) → Axiom (record commits, log response) → Process
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
    * React UI may read from ZosStorage/ZosKeystore caches (read-only, caches populated via HAL)
    * No direct IndexedDB access outside HAL/ZosStorage/ZosKeystore implementation
    * Two physically isolated IndexedDB databases:
      * `zos-storage`: General filesystem data (via ZosStorage)
      * `zos-keystore`: Cryptographic key material (via ZosKeystore)

29. **Dual Storage Objects (Filesystem + Keystore)**

    Storage is split into two physically isolated IndexedDB databases with separate JavaScript interfaces:

    | Database | JS Object | Purpose | Access Pattern |
    |----------|-----------|---------|----------------|
    | `zos-storage` | `ZosStorage` | General filesystem (files, directories, metadata) | VFS IPC → storage syscalls |
    | `zos-keystore` | `ZosKeystore` | Cryptographic key material only | Keystore syscalls (direct) |

    **Rationale for Physical Isolation:**

    * Key material never exposed to filesystem path traversal attacks
    * Separate access control - keystore syscalls vs storage syscalls
    * Independent lifecycle (keys persist across filesystem wipes)
    * Reduced attack surface - VFS bugs cannot leak key material

30. **Bootstrap Storage Exception**

    * The supervisor may use async vfs module functions during bootstrap
    * This is acceptable because:
      * Bootstrap happens once at system start
      * HAL trait methods are synchronous but IndexedDB is async
      * After Init starts, all storage access goes through processes
    * The vfs module is internal to zos-supervisor

31. **Filesystem Hierarchy Enforcement**

    All **filesystem** read/write operations must flow through VFS:

    ```
    ┌─────────────────────────────────────────────────────────────┐
    │                    Application/Service                       │
    │                   (identity, time, apps)                     │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ VFS IPC Protocol
                         │ (MSG_VFS_READ, MSG_VFS_WRITE, etc.)
                         ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                    VFS Service (PID 4)                       │
    │              ONLY process with storage syscalls              │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ Storage Syscalls
                         │ (SYS_STORAGE_READ, SYS_STORAGE_WRITE)
                         ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                    Supervisor (Main Thread)                  │
    │              system.process_syscall(pid, syscall)            │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ Axiom Gateway Entry Point
                         ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                   Axiom (Verification Layer)                 │
    │    - Logs request to SysLog                                  │
    │    - Verifies sender identity                                │
    │    - Calls kernel function                                   │
    │    - Records commits to CommitLog                            │
    │    - Logs response to SysLog                                 │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ kernel_fn callback
                         ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                 Kernel (Execution Layer)                     │
    │    execute_storage_read/write(core, sender, data)           │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ HAL trait call
                         ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                         HAL (WasmHal)                        │
    │    do_storage_read_async(pid, key) → request_id             │
    │    - Generates unique request_id                             │
    │    - Tracks pending_requests[request_id] = pid               │
    │    - Calls JavaScript FFI                                    │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ JavaScript FFI call
                         │ (returns request_id immediately)
                         ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                  ZosStorage (JavaScript)                     │
    │    startRead(request_id, key)                                │
    │    - Async IndexedDB operation                               │
    │    - supervisor.notify_storage_read_complete(request_id)     │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ IndexedDB async operation
                         ▼
                  IndexedDB (zos-storage)
    ```

    **Layer Responsibilities:**

    * **Services/Apps**: All processes needing filesystem storage MUST use VFS IPC protocol
    * **VFS Service**: The ONLY process authorized to make storage syscalls
    * **Supervisor**: Routes syscalls through Axiom gateway
    * **Axiom**: Verification and audit layer - logs all requests/responses, records commits
    * **Kernel**: Executes storage syscall logic, calls HAL methods
    * **HAL**: Platform abstraction - tracks pending requests, calls JavaScript
    * **ZosStorage**: JavaScript object interfacing with `zos-storage` IndexedDB

    **Rationale:**

    * Single point of control for all filesystem I/O
    * Enables consistent permission checking, quota enforcement, encryption
    * Simplifies audit trail for data access
    * Allows VFS to implement filesystem semantics (paths, directories, metadata)

    **Async Pattern:**

    VFS IPC is non-blocking and event-driven with two separate paths:

    **Request Path (Synchronous but Non-Blocking):**

    1. VFS calls `storage_read_async()` syscall
    2. Flows through: Supervisor → Axiom → Kernel → HAL → ZosStorage.startRead()
    3. Returns `request_id` immediately (e.g., 42)
    4. VFS stores `pending_ops[42] = {client_pid, path}`
    5. VFS yields (returns from message handler)

    **Callback Path (Asynchronous IPC Message):**

    1. IndexedDB operation completes (10ms, 100ms, whatever)
    2. ZosStorage calls `supervisor.notify_storage_read_complete(42, data)`
    3. Supervisor routes `MSG_STORAGE_RESULT` via IPC to VFS
    4. VFS receives IPC message, looks up `pending_ops[42]`
    5. VFS sends `MSG_VFS_READ_RESPONSE` to original client

    **Critical:** The callback does NOT go back through Axiom/Kernel - it's delivered as a standard IPC message.

    This push-based async pattern prevents deadlock:

    * No process blocks waiting for I/O
    * Request returns immediately with tracking ID
    * Callback arrives as separate IPC message (push notification)
    * Services maintain pending operation context keyed by request_id
    * Multiple storage operations can be in-flight simultaneously

    **Exception:** Bootstrap operations before VFS Service exists may use HAL's `bootstrap_storage_*` methods (see invariant 30).

32. **Keystore Hierarchy Enforcement (Cryptographic Key Material)**

    Cryptographic key operations use **dedicated keystore syscalls** that bypass VFS entirely:

    ```
    ┌─────────────────────────────────────────────────────────────┐
    │                    Identity Service (PID 5)                  │
    │              Authorized to use keystore syscalls             │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ Keystore Syscalls (Direct - No VFS)
                         │ (SYS_KEYSTORE_READ, SYS_KEYSTORE_WRITE, etc.)
                         ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                    Supervisor (Main Thread)                  │
    │              system.process_syscall(pid, syscall)            │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ Axiom Gateway Entry Point
                         ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                   Axiom (Verification Layer)                 │
    │    - Logs request to SysLog                                  │
    │    - Verifies sender identity (PID check)                    │
    │    - Calls kernel function                                   │
    │    - Records commits to CommitLog                            │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ kernel_fn callback
                         ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                 Kernel (Execution Layer)                     │
    │    execute_keystore_read/write(core, sender, data)          │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ HAL trait call
                         ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                         HAL (WasmHal)                        │
    │    do_keystore_read_async(pid, key) → request_id            │
    │    - Generates unique request_id                             │
    │    - Tracks pending_keystore_requests[request_id] = pid      │
    │    - Calls JavaScript FFI                                    │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ JavaScript FFI call
                         ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                  ZosKeystore (JavaScript)                    │
    │    startRead(request_id, key)                                │
    │    - Async IndexedDB operation                               │
    │    - supervisor.notify_keystore_read_complete(request_id)    │
    └────────────────────┬────────────────────────────────────────┘
                         │
                         │ IndexedDB async operation
                         ▼
                  IndexedDB (zos-keystore)
    ```

    **Why Keystore Bypasses VFS:**

    * **Security Isolation**: Key material never passes through VFS, eliminating path traversal and filesystem-level attack vectors
    * **Reduced Attack Surface**: VFS bugs (path parsing, permission checks, directory traversal) cannot leak cryptographic keys
    * **Physical Separation**: `zos-keystore` is a separate IndexedDB database from `zos-storage`
    * **Direct Access**: Identity Service needs fast, direct key access without VFS overhead
    * **No Filesystem Semantics**: Keys don't need directories, permissions, or metadata - just key-value storage

    **Access Control:**

    * Keystore syscalls are restricted to authorized processes (currently Identity Service, PID 5)
    * Axiom logs all keystore operations to SysLog for audit
    * React UI may read from `ZosKeystore.keyCache` (read-only, synchronous)

    **Keystore Path Format:**

    Keystore paths follow a convention for human readability but are NOT filesystem paths:

    ```
    /keys/{user_id}/identity/public_keys.json
    /keys/{user_id}/identity/machine/{machine_id}.json
    ```

    These paths are **keystore keys**, not VFS paths. They are never routed through VFS.

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

## 12. Protocol Constants Consolidation

33. **Single Source of Truth for All Constants**

    * The `zos-ipc` crate is the **single source of truth** for:
      * Syscall numbers (Process → Kernel operations)
      * IPC message tags (Process ↔ Process communication)
      * Protocol constants, ranges, and enumerations
    * No duplicate constant definitions are allowed anywhere else
    * Both `zos-kernel` and `zos-process` re-export from `zos-ipc`

34. **Constant Organization in zos-ipc**

    * **Syscalls** (`zos_ipc::syscall` module):
      * 0x01-0x0F: Misc (debug, time, info)
      * 0x10-0x1F: Process (create, exit, kill)
      * 0x30-0x3F: Capability (grant, revoke, inspect)
      * 0x40-0x4F: IPC (send, receive, call, reply)
      * 0x50-0x5F: System (list processes)
      * 0x60-0x6F: Keystore (cryptographic key storage - bypasses VFS)
      * 0x70-0x7F: Platform Storage (async ops - VFS only)
      * 0x90-0x9F: Network (async HTTP)
    * **IPC Messages** (various modules):
      * 0x0001-0x000F: Console/System
      * 0x0080-0x008F: Keystore results (MSG_KEYSTORE_RESULT)
      * 0x1000-0x100F: Init service
      * 0x2000-0x200F: Supervisor → Init
      * 0x2010-0x201F: PermissionService
      * 0x3000-0x30FF: Kernel notifications
      * 0x7000-0x70FF: Identity service
      * 0x8000-0x80FF: VFS service
      * 0x9000-0x901F: Network service

    **Rationale:**
    
    * Eliminates duplicate definitions across crates
    * Single point to update when adding new syscalls/messages
    * Prevents constant value conflicts
    * Makes protocol versioning and evolution easier
    * Both syscalls and IPC messages are part of the kernel interface
    * Keystore syscalls (0x60-0x6F) are separate from storage syscalls (0x70-0x7F) to enforce separation at the syscall boundary

---

## Appendix: Current Implementation Violations

The following are known violations in the current codebase that must be fixed to comply with these invariants:

| Violation | Location | Invariant Violated | Status | Required Fix |
|-----------|----------|-------------------|--------|--------------|
| ~~`Supervisor.revoke_capability()`~~ | `zos-supervisor/src/supervisor/mod.rs` | 14, 16 | **FIXED** | ~~Route through PermissionService process via IPC~~ |
| ~~`kernel.deliver_console_input()`~~ | `zos-kernel/src/kernel_impl.rs` | 13, 16 | **FIXED** | ~~Use IPC with capability granted by Init~~ |
| ~~`kernel.deliver_supervisor_ipc()`~~ | `zos-kernel/src/kernel_impl.rs` | 13, 16 | **FIXED** | ~~Method removed; routes via Init~~ |
| ~~Kernel owns Axiom~~ | `zos-kernel/src/kernel.rs` | 1, 9 | **FIXED** | ~~System struct separates Axiom and KernelCore~~ |
| ~~Direct `kernel.kill_process()`~~ | `zos-supervisor/src/supervisor/mod.rs` | 13, 16 | **FIXED** | ~~All kills route via `MSG_SUPERVISOR_KILL_PROCESS` (Init PID 1 exception documented)~~ |
| ~~`identity_service` direct storage syscalls~~ | `zos-apps/src/bin/identity_service/service.rs` | 31 | **FIXED** | ~~Now uses VFS IPC via `vfs_async` module~~ |
| ~~`time_service` direct storage syscalls~~ | `zos-apps/src/bin/time_service.rs` | 31 | **FIXED** | ~~Now uses VFS IPC via `vfs_async` module~~ |

### Architectural Changes

**Axiom/Kernel Boundary Refactor**: The architecture has been refactored so that Axiom and KernelCore are **separate components**:

- **Before**: `Kernel` struct owned `AxiomGateway` (inverted relationship)
- **After**: `System<H>` struct holds both `Axiom` and `KernelCore` separately

The `System` struct is the canonical entry point for all kernel operations.

**Direct KernelCore Access Violation**: Any code that calls `KernelCore` methods directly without going through `System.process_syscall()` violates the verification boundary. All syscalls must flow through Axiom to ensure proper audit logging and commit recording.

### Fixed Violations

1. **`Supervisor.revoke_capability()`**: Now routes through PermissionService (PID 2) via IPC using `MSG_SUPERVISOR_REVOKE_CAP (0x2020)`. The supervisor sends an IPC message to PS, which performs the capability deletion and notifies the affected process.

2. **`kernel.deliver_console_input()`**: Method removed. Console input now uses capability-checked IPC routed through Init via `MSG_SUPERVISOR_CONSOLE_INPUT (0x2001)`.

3. **`kernel.deliver_supervisor_ipc()`**: Method removed. IPC delivery now routes through Init via `MSG_SUPERVISOR_IPC_DELIVERY (0x2003)`.

4. **`time_service` direct storage syscalls**: Now uses VFS IPC via `vfs_async::send_read_request()` and `vfs_async::send_write_request()` per Invariant 31.

5. **`identity_service` direct storage syscalls**: Now uses VFS IPC via the `start_vfs_*` helper methods (`start_vfs_read`, `start_vfs_write`, `start_vfs_exists`, `start_vfs_mkdir`, `start_vfs_readdir`, `start_vfs_delete`) per Invariant 31. All handlers have been refactored to use VFS IPC instead of direct storage syscalls.

### Init (PID 1) Exception

**Direct `kernel.kill_process()` for Init**: The supervisor has a `kill_process_direct()` method that is used **only** for:

1. Terminating Init itself (PID 1) - Init cannot kill itself via IPC
2. Bootstrap failures before Init is fully spawned

This is an **architectural necessity**, not a violation. Init is the IPC routing hub and cannot route messages to itself. All other process kills properly route through Init via `MSG_SUPERVISOR_KILL_PROCESS (0x2002)`.

**Implementation**: All kill requests use `kill_process_via_init()` which sends IPC to Init, except for the documented Init edge case. This ensures proper audit logging via SysLog for all killable processes.

---

### IPC Protocol Consolidation

All IPC message constants are now centralized in the `zos-ipc` crate as the single source of truth. This eliminates duplicate constant definitions and ensures consistent values across all crates.

Key fix: The `MSG_SUPERVISOR_REVOKE_CAP` constant was incorrectly defined as `0x2010` in the supervisor, which conflicted with `MSG_REQUEST_CAPABILITY`. It has been corrected to use `0x2020` from `zos-ipc`.

These are **implementation bugs**, not acceptable deviations. The supervisor must never have direct kernel access on any target.
