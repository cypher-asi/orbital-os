//! Process spawning logic
//!
//! Handles spawning new processes and capability setup.
//!
//! # Architecture: Init-Driven Spawn Protocol
//!
//! Per the architectural invariants, all process lifecycle management should
//! flow through Init (PID 1) to ensure proper audit logging via SysLog.
//!
//! ## Current Implementation (Transitional)
//!
//! The current `complete_spawn()` uses direct kernel calls for process
//! registration and endpoint creation. This is a **transitional** approach
//! while we build out the async Init-driven spawn infrastructure.
//!
//! Direct kernel calls are acceptable during this transition because:
//! - Kernel methods still log commits to Axiom
//! - The supervisor is a trusted boundary component
//!
//! ## Target Architecture (Init-Driven)
//!
//! The target flow routes all operations through Init:
//!
//! ```text
//! Web → Supervisor: spawn("terminal")
//! Supervisor → Init: MSG_SUPERVISOR_SPAWN_PROCESS
//! Init → Kernel: SYS_REGISTER_PROCESS (via Axiom)
//! Init → Supervisor: MSG_SUPERVISOR_SPAWN_RESPONSE(pid)
//! Supervisor: Start worker with PID
//! ```
//!
//! ## Bootstrap Exception
//!
//! Init itself (PID 1) is created via direct kernel calls during bootstrap.
//! This is the **only** allowed direct kernel call for process creation.
//! See `boot.rs` for the bootstrap documentation.
//!
//! # Spawn State Machine
//!
//! For proper spawn tracking and timeout handling, each spawn operation goes
//! through a state machine:
//!
//! ```text
//! Requested → WaitingForBinary → WaitingForPid → WaitingForEndpoint → WaitingForCaps → Ready
//!     ↓              ↓                  ↓               ↓                   ↓            ↓
//!  Timeout       Timeout           Timeout          Timeout             Timeout      Success
//!     ↓              ↓                  ↓               ↓                   ↓
//!  Failed         Failed            Failed          Failed              Failed
//! ```

mod capabilities;
mod state;

pub use state::SpawnTracker;

use wasm_bindgen::prelude::*;
use zos_hal::HAL;
use zos_kernel::ProcessId;

use super::{log, Supervisor};
use crate::pingpong;

#[wasm_bindgen]
impl Supervisor {
    /// Complete spawning a process with the WASM binary.
    ///
    /// Called by JS after fetching the WASM file.
    ///
    /// # Current Implementation (Transitional)
    ///
    /// This method currently uses direct kernel calls for process registration
    /// and endpoint creation. This is a transitional implementation.
    ///
    /// For Init (name="init"), this is the **bootstrap exception** - the only
    /// allowed direct kernel call for process creation. See `boot.rs`.
    ///
    /// For other processes, this should eventually be migrated to use the
    /// Init-driven spawn protocol (MSG_SUPERVISOR_SPAWN_PROCESS).
    ///
    /// # Target Architecture
    ///
    /// The target implementation will:
    /// 1. Send MSG_SUPERVISOR_SPAWN_PROCESS to Init
    /// 2. Wait for MSG_SUPERVISOR_SPAWN_RESPONSE with assigned PID
    /// 3. Spawn worker with that PID
    ///
    /// This requires async coordination which is tracked for future work.
    ///
    /// # Spawn Tracking
    ///
    /// Uses SpawnTracker to track spawn operations for:
    /// - Timeout detection (spawns that take too long)
    /// - State correlation (matching responses to requests)
    /// - Cleanup of completed/failed spawns
    #[wasm_bindgen]
    pub fn complete_spawn(&mut self, name: &str, wasm_binary: &[u8]) -> u64 {
        log(&format!(
            "[supervisor] complete_spawn called for '{}', {} bytes",
            name,
            wasm_binary.len()
        ));

        // Start tracking this spawn operation
        let current_time = self.system.hal().wallclock_ms();
        let request_id = self.spawn_tracker.start_spawn(name, current_time);

        // Mark binary as received (transitioning from WaitingForBinary to WaitingForPid)
        if let Some(spawn) = self.spawn_tracker.get_mut(request_id) {
            spawn.binary_received();
        }

        let process_pid = self.register_process_for_spawn(name);

        // Update spawn state with assigned PID
        if let Some(spawn) = self.spawn_tracker.get_mut(request_id) {
            spawn.pid_assigned(process_pid.0);
        }

        match self.spawn_worker_for_process(process_pid, name, wasm_binary) {
            Ok(handle) => {
                log(&format!(
                    "[supervisor] Spawned Worker '{}' with PID {}",
                    name, handle.id
                ));

                // Track init spawn and grant capabilities
                self.setup_process_capabilities(process_pid, name);

                // Mark spawn as ready (all capabilities granted)
                if let Some(spawn) = self.spawn_tracker.get_mut(request_id) {
                    // Note: endpoint_created() and caps_granted() are called here for completeness
                    // In the future Init-driven flow, these would be called as responses arrive
                    spawn.endpoint_created(0); // Placeholder endpoint ID
                    spawn.caps_granted();
                }

                // Clean up completed spawns periodically
                let _ = self.spawn_tracker.cleanup_completed();

                // Check if this is part of an automated pingpong test
                let pid = process_pid.0;
                self.on_process_spawned(name, pid);

                pid
            }
            Err(e) => {
                self.write_console(&format!("Error spawning {}: {:?}\n", name, e));
                log(&format!("[supervisor] Failed to spawn {}: {:?}", name, e));

                // Mark spawn as failed
                if let Some(spawn) = self.spawn_tracker.get_mut(request_id) {
                    spawn.fail(format!("Worker spawn failed: {:?}", e));
                }

                self.cleanup_failed_spawn(process_pid, name);
                0
            }
        }
    }

    /// Check for timed-out spawn operations and clean them up.
    ///
    /// This should be called periodically (e.g., from poll_syscalls) to detect
    /// and handle spawn operations that have been pending for too long.
    pub(crate) fn check_spawn_timeouts(&mut self) {
        let current_time = self.system.hal().wallclock_ms();
        let timed_out = self.spawn_tracker.timeout_spawns(current_time);

        for spawn in timed_out {
            log(&format!(
                "[supervisor] Spawn timed out: {} (request_id={}, state={:?})",
                spawn.proc_name, spawn.request_id, spawn.state
            ));

            // If we have a PID, clean up the failed process
            if let Some(pid) = spawn.state.pid() {
                self.cleanup_failed_spawn(ProcessId(pid), &spawn.proc_name);
            }
        }
    }

    fn register_process_for_spawn(&mut self, name: &str) -> ProcessId {
        // TRANSITIONAL: Direct system call for process registration.
        // For Init, this is the bootstrap exception (see boot.rs).
        // For other processes, this should migrate to Init-driven spawn.
        let process_pid = self.system.register_process(name);
        log(&format!(
            "[supervisor] System assigned PID {} for '{}'",
            process_pid.0, name
        ));

        // Create endpoints for the process based on its role
        self.setup_process_endpoints(process_pid, name);

        process_pid
    }

    fn spawn_worker_for_process(
        &mut self,
        process_pid: ProcessId,
        name: &str,
        wasm_binary: &[u8],
    ) -> Result<crate::worker::WasmProcessHandle, zos_hal::HalError> {
        self.system
            .hal()
            .spawn_with_pid(process_pid.0, name, wasm_binary)
    }

    /// Set up endpoints for a process based on its role
    fn setup_process_endpoints(&mut self, process_pid: ProcessId, name: &str) {
        if name == "init" {
            // Init gets: slot 0 = init endpoint, slot 1 = console output
            if let Ok((eid, slot)) = self.system.create_endpoint(process_pid) {
                log(&format!(
                    "[supervisor] Created init endpoint {} at slot {} for init",
                    eid.0, slot
                ));
            }
            if let Ok((eid, slot)) = self.system.create_endpoint(process_pid) {
                log(&format!(
                    "[supervisor] Created console output endpoint {} at slot {} for init",
                    eid.0, slot
                ));
            }
        } else if name == "terminal" {
            self.setup_terminal_endpoints(process_pid);
        } else {
            // Other processes get two endpoints: output (slot 0) and input (slot 1)
            // This matches the app_main! macro which expects:
            // - Slot 0: UI output endpoint
            // - Slot 1: Input endpoint (for receiving messages)
            if let Ok((eid, slot)) = self.system.create_endpoint(process_pid) {
                log(&format!(
                    "[supervisor] Created output endpoint {} at slot {} for {}",
                    eid.0, slot, name
                ));
            }
            if let Ok((eid, slot)) = self.system.create_endpoint(process_pid) {
                log(&format!(
                    "[supervisor] Created input endpoint {} at slot {} for {}",
                    eid.0, slot, name
                ));
            }
        }
    }

    /// Set up terminal endpoints
    ///
    /// Terminal only needs its own input endpoint for receiving console input
    /// from the supervisor. Console output goes through SYS_CONSOLE_WRITE syscall.
    fn setup_terminal_endpoints(&mut self, process_pid: ProcessId) {
        // Terminal endpoint setup:
        // - Slot 0: Terminal's own endpoint (for general IPC if needed)
        // - Slot 1: Terminal's input endpoint (receives console input from supervisor)
        //
        // Note: Terminal does NOT need a capability to supervisor's endpoint.
        // Console output uses SYS_CONSOLE_WRITE syscall, which the supervisor
        // handles directly during syscall processing (no kernel buffering).

        // Create terminal's primary endpoint at slot 0
        if let Ok((eid, slot)) = self.system.create_endpoint(process_pid) {
            log(&format!(
                "[supervisor] Created terminal endpoint {} at slot {} for terminal",
                eid.0, slot
            ));
        }

        // Create terminal's input endpoint at slot 1
        // Supervisor will be granted a capability to this endpoint for console input
        if let Ok((input_eid, slot)) = self.system.create_endpoint(process_pid) {
            log(&format!(
                "[supervisor] Created terminal input endpoint {} at slot {} for terminal",
                input_eid.0, slot
            ));

            // Note: Supervisor capability to this endpoint is granted in
            // grant_terminal_capabilities() after endpoint creation.
        }
    }

    /// Clean up after a spawn failure.
    fn cleanup_failed_spawn(&mut self, process_pid: ProcessId, name: &str) {
        // Remove any supervisor-tracked state
        self.cleanup_process_state(process_pid.0);

        // Init cannot be killed via IPC; spawn failure during bootstrap uses direct cleanup.
        if name == "init" || !self.init_spawned {
            self.kill_process_direct(process_pid);
            return;
        }

        // Route non-init cleanup through Init for audit logging.
        self.kill_process_via_init(process_pid);
    }

    /// Called when a process is successfully spawned
    pub(crate) fn on_process_spawned(&mut self, name: &str, pid: u64) {
        // Check if we're waiting for this spawn as part of the pingpong test
        let (new_state, should_spawn_ponger) =
            pingpong::on_process_spawned(&self.pingpong_test, name, pid);

        if should_spawn_ponger {
            self.write_console(&format!("  Pinger spawned as PID {}\n", pid));
            self.pingpong_test = new_state;
            self.request_spawn("pingpong", "pp_ponger");
        } else if matches!(new_state, pingpong::PingPongTestState::SettingUpCaps { .. }) {
            self.write_console(&format!("  Ponger spawned as PID {}\n", pid));
            self.pingpong_test = new_state;
            self.progress_pingpong_test();
        } else {
            // Normal spawn, just report
            self.write_console(&format!("Spawned Worker '{}' as PID {}\n", name, pid));

            // NOTE: Terminal is now spawned per-window by the Desktop component
            // (no longer auto-spawned after init to enable process isolation)
            if name == "init" {
                log("[supervisor] Init started - terminal will be spawned per-window");
            }
        }
    }
}
