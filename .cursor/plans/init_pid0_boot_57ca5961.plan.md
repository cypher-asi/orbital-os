---
name: Init PID0 Boot
overview: Refactor boot and spawn flow so Init runs as PID 0, Supervisor is PID 1, and Init issues IPC capabilities to Supervisor via the Init-driven spawn protocol.
todos:
  - id: pid-swap-kernel
    content: Swap init/supervisor PID assumptions in kernel.
    status: pending
  - id: boot-flow
    content: Bootstrap init first; init registers supervisor.
    status: pending
  - id: init-driven-spawn
    content: Implement init-driven spawn + cap grants.
    status: pending
  - id: supervisor-updates
    content: Update supervisor PID usage and grant handling.
    status: pending
  - id: docs-tests
    content: Update docs/comments/tests for PID0 init.
    status: pending
---

# Init-First Boot and Init-Driven Spawn

## Goals

- Init is PID 0 and the first userspace process to boot.
- Supervisor becomes PID 1 and only uses IPC capabilities (no direct kernel calls beyond bootstrap).
- Process registration and endpoint creation move to Init-driven spawn, enabling Init to issue Supervisor’s IPC capabilities.

## Key Files

- [crates/zos-kernel/src/system.rs](crates/zos-kernel/src/system.rs)
- [crates/zos-kernel/src/core/process.rs](crates/zos-kernel/src/core/process.rs)
- [crates/zos-kernel/src/core/endpoint.rs](crates/zos-kernel/src/core/endpoint.rs)
- [crates/zos-supervisor-web/src/supervisor/boot.rs](crates/zos-supervisor-web/src/supervisor/boot.rs)
- [crates/zos-supervisor-web/src/supervisor/spawn.rs](crates/zos-supervisor-web/src/supervisor/spawn.rs)
- [crates/zos-supervisor-web/src/supervisor/mod.rs](crates/zos-supervisor-web/src/supervisor/mod.rs)
- [crates/zos-supervisor-web/src/syscall/mod.rs](crates/zos-supervisor-web/src/syscall/mod.rs)
- [crates/zos-init/src/lib.rs](crates/zos-init/src/lib.rs)
- [crates/zos-process/src/lib.rs](crates/zos-process/src/lib.rs)

## Plan

1. **Swap PID assumptions in kernel + syscall gatekeeping.**

   - Update Init-only syscall checks in `execute_syscall_kernel_fn` to allow sender PID 0 instead of 1.
   - Update implicit kill permission in `KernelCore::kill_process_with_cap_check` to use PID 0.
   - Update comments/docs in `zos-process` and `zos-init` to reflect Init PID 0.

2. **Bootstrap flow: spawn Init first and defer Supervisor registration to Init.**

   - In `Supervisor::boot()`, remove `initialize_supervisor_process()` and instead spawn Init with PID 0 (bootstrap exception).
   - Track “supervisor_pid” as unknown until Init registers it and tells Supervisor the assigned PID + init capability slot.
   - Update log strings and invariants text to match the new order.

3. **Init self-setup and Supervisor registration.**

   - In `Init::run()` (or early boot sequence), create Init’s own endpoints via `SYS_EP_CREATE` so it can receive supervisor IPC.
   - Register the Supervisor via `SYS_REGISTER_PROCESS` (expected PID 1) and grant Supervisor a write-only cap to Init’s endpoint.
   - Emit a debug message to Supervisor with assigned PID + init-cap slot (e.g., `INIT:SUPERVISOR_READY:pid:slot`), since Supervisor already parses debug strings.
   - Update all “supervisor PID 0” checks in Init handlers to expect PID 1.

4. **Enable Init to issue IPC capabilities during spawn.**

   - Adjust kernel endpoint creation so Init can grant capabilities it creates:
     - When Init (PID 0) calls `SYS_CREATE_ENDPOINT_FOR`, also install a grant-capable management cap in Init’s CSpace (or return a secondary slot so Init can re-grant).
   - Document this behavior in `zos-kernel` and `zos-process` comments so spawn flow is explicit.

5. **Implement full Init-driven spawn in Supervisor.**

   - In `Supervisor::complete_spawn()`, stop direct `system.register_process()` and `system.create_endpoint()` calls.
   - Instead, send `MSG_SUPERVISOR_SPAWN_PROCESS` to Init, store a pending spawn record with the wasm binary, and wait for `SPAWN:RESPONSE`.
   - After PID is assigned, request endpoint creation via `MSG_SUPERVISOR_CREATE_ENDPOINT` for the expected slots (init/terminal/general), and wait for `ENDPOINT:RESPONSE` to map slot → endpoint.
   - Request capability grants via `MSG_SUPERVISOR_GRANT_CAP` so Init issues Supervisor’s caps (init/pm/terminal) rather than Supervisor self-granting.
   - Once the spawn pipeline completes, call `hal.spawn_with_pid()` using the PID assigned by Init.

6. **Update Supervisor PID usage and init grant handling.**

   - Replace hard-coded `ProcessId(0)` for Supervisor with PID 1 throughout `supervisor/mod.rs`, `spawn.rs`, and `syscall/mod.rs`.
   - Update init grant/revoke handlers to use Init PID 0.
   - Add parsing for the new `INIT:SUPERVISOR_READY` debug message to set `supervisor_pid` and `init_endpoint_slot`.

7. **Tests + invariants cleanup.**

   - Update kernel tests referencing PID 1 as Init.
   - Refresh comments/notes in `boot.rs`, `spawn.rs`, and `zos-process` that currently document Init PID 1 and the old bootstrap exception.

## Open Risks / Notes

- The Init-driven spawn protocol currently uses debug messages for responses; we will keep that interface and extend with a “Supervisor ready” message to avoid IPC dependency.
- If required, we can later migrate Init→Supervisor responses to IPC, but this plan keeps changes localized.

## Targeted Reference Snippets

- Init-only syscall gatekeeping in `System::process_syscall`:
```110:137:crates/zos-kernel/src/system.rs
    pub fn process_syscall(
        &mut self,
        sender: ProcessId,
        syscall_num: u32,
        args: [u32; 4],
        data: &[u8],
    ) -> (i64, SyscallResult, Vec<u8>) {
        // ...
    }
```

- Implicit Init kill permission in `KernelCore::kill_process_with_cap_check`:
```114:136:crates/zos-kernel/src/core/process.rs
    /// Init (PID 1) is granted implicit permission to kill any process.
    pub fn kill_process_with_cap_check(
        &mut self,
        caller: ProcessId,
        target_pid: ProcessId,
        timestamp: u64,
    ) -> (Result<(), KernelError>, Vec<Commit>) {
        // ...
    }
```

- Current bootstrap logic in `Supervisor::boot()`:
```60:87:crates/zos-supervisor-web/src/supervisor/boot.rs
    #[wasm_bindgen]
    pub fn boot(&mut self) {
        log("[supervisor] Booting Zero OS kernel...");
        // ...
        // Initialize supervisor as a kernel process (PID 0)
        self.initialize_supervisor_process();
        // ...
    }
```