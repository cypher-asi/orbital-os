//! Ping-Pong Test State Machine
//!
//! Automated IPC latency testing between worker processes.
//!
//! # Deprecated API Usage
//!
//! This module uses the deprecated `send_to_process()` kernel method to send
//! commands to test processes. This bypasses capability checks and violates
//! the capability-based security model.
//!
//! ## Why This Is Still Here
//!
//! The pingpong test was written before the Init-driven spawn protocol was
//! implemented. It uses `send_to_process()` because:
//! 1. The supervisor doesn't have capability slots to the test processes' endpoints
//! 2. Setting up proper capability routing would require changes to the test setup
//!
//! ## Migration Plan
//!
//! To fix this properly, the pingpong test should:
//! 1. Grant supervisor capabilities to test process endpoints during spawn
//! 2. Use `kernel.ipc_send()` with proper capability slots
//! 3. Or route commands through Init using MSG_SUPERVISOR_IPC_DELIVERY
//!
//! This is tracked as technical debt to be addressed in a future cleanup.

use zos_kernel::ProcessId;

/// Command tags for pingpong test processes (must match zos-test-procs)
pub(crate) const CMD_PING: u32 = 0x3001;
pub(crate) const CMD_PONG_MODE: u32 = 0x3002;
pub(crate) const CMD_EXIT: u32 = 0x1005;

/// State of the automated ping-pong test
#[derive(Clone, Debug, Default)]
pub(crate) enum PingPongTestState {
    /// No test running
    #[default]
    Idle,
    /// Waiting for pinger process to spawn
    /// Note: Entry point (start_pingpong_test) removed with shell, but state machine kept for future use
    #[allow(dead_code)]
    WaitingForPinger { iterations: u32 },
    /// Waiting for ponger process to spawn
    WaitingForPonger { iterations: u32, pinger_pid: u64 },
    /// Both processes spawned, setting up capabilities
    SettingUpCaps {
        iterations: u32,
        pinger_pid: u64,
        ponger_pid: u64,
    },
    /// Capabilities granted, sending commands to start test
    StartingTest {
        iterations: u32,
        pinger_pid: u64,
        ponger_pid: u64,
    },
    /// Test running, waiting for completion
    Running {
        #[allow(dead_code)]
        iterations: u32,
        pinger_pid: u64,
        ponger_pid: u64,
        start_time: u64,
    },
    /// Test complete, cleaning up
    Cleanup { pinger_pid: u64, ponger_pid: u64 },
}

impl PingPongTestState {
    /// Check if a test is currently running
    /// Note: Currently unused since shell was removed, but kept for future test automation
    #[allow(dead_code)]
    pub(crate) fn is_idle(&self) -> bool {
        matches!(self, PingPongTestState::Idle)
    }
}

/// Context needed for ping-pong test operations
pub(crate) struct PingPongContext<'a, H: zos_hal::HAL> {
    pub kernel: &'a mut zos_kernel::Kernel<H>,
    pub write_console: &'a dyn Fn(&str),
    #[allow(dead_code)]
    pub request_spawn: &'a dyn Fn(&str, &str),
}

/// Progress the ping-pong test state machine
///
/// Returns the new state after processing
///
/// NOTE: Uses deprecated send_to_process() for testing purposes.
#[allow(deprecated)]
pub(crate) fn progress_pingpong_test<H: zos_hal::HAL>(
    state: &PingPongTestState,
    ctx: &mut PingPongContext<'_, H>,
) -> PingPongTestState {
    match state.clone() {
        PingPongTestState::SettingUpCaps {
            iterations,
            pinger_pid,
            ponger_pid,
        } => {
            (ctx.write_console)("  Setting up IPC capabilities...\n");

            let pinger = ProcessId(pinger_pid);
            let ponger = ProcessId(ponger_pid);

            // Grant pinger's endpoint (slot 0) to ponger (so ponger can send pongs back)
            match ctx.kernel.grant_capability(
                pinger,
                0,
                ponger,
                zos_kernel::Permissions {
                    read: false,
                    write: true,
                    grant: false,
                },
            ) {
                Ok(_slot) => {
                    // Successfully granted
                }
                Err(e) => {
                    (ctx.write_console)(&format!("  Error granting pinger->ponger cap: {:?}\n", e));
                }
            }

            // Grant ponger's endpoint (slot 0) to pinger (so pinger can send pings)
            match ctx.kernel.grant_capability(
                ponger,
                0,
                pinger,
                zos_kernel::Permissions {
                    read: false,
                    write: true,
                    grant: false,
                },
            ) {
                Ok(_slot) => {
                    // Successfully granted
                }
                Err(e) => {
                    (ctx.write_console)(&format!("  Error granting ponger->pinger cap: {:?}\n", e));
                }
            }

            // Move to starting test
            PingPongTestState::StartingTest {
                iterations,
                pinger_pid,
                ponger_pid,
            }
        }

        PingPongTestState::StartingTest {
            iterations,
            pinger_pid,
            ponger_pid,
        } => {
            (ctx.write_console)(&format!(
                "  Starting test with {} iterations...\n",
                iterations
            ));

            let pinger = ProcessId(pinger_pid);
            let ponger = ProcessId(ponger_pid);

            // Put ponger in pong mode
            if let Err(e) = ctx
                .kernel
                .send_to_process(ProcessId(2), ponger, CMD_PONG_MODE, vec![])
            {
                (ctx.write_console)(&format!("  Error sending PONG_MODE: {:?}\n", e));
            }

            // Send ping command to pinger with iterations count
            let ping_data = iterations.to_le_bytes().to_vec();
            if let Err(e) = ctx
                .kernel
                .send_to_process(ProcessId(2), pinger, CMD_PING, ping_data)
            {
                (ctx.write_console)(&format!("  Error sending PING cmd: {:?}\n", e));
            }

            // Move to running state
            let start_time = ctx.kernel.uptime_nanos();
            (ctx.write_console)("  Test running... (watch for results from processes)\n");

            PingPongTestState::Running {
                iterations,
                pinger_pid,
                ponger_pid,
                start_time,
            }
        }

        PingPongTestState::Running {
            iterations: _,
            pinger_pid,
            ponger_pid,
            start_time,
        } => {
            // Check if enough time has passed (timeout after 30 seconds)
            let elapsed = ctx.kernel.uptime_nanos() - start_time;
            let elapsed_secs = elapsed / 1_000_000_000;

            if elapsed_secs >= 30 {
                (ctx.write_console)("\n  Test timed out after 30 seconds.\n");
                PingPongTestState::Cleanup {
                    pinger_pid,
                    ponger_pid,
                }
            } else {
                // Otherwise, keep running - test results will be printed by processes
                state.clone()
            }
        }

        PingPongTestState::Cleanup {
            pinger_pid,
            ponger_pid,
        } => {
            (ctx.write_console)("  Cleaning up test processes...\n");

            let pinger = ProcessId(pinger_pid);
            let ponger = ProcessId(ponger_pid);

            // Send exit commands
            // NOTE: Uses deprecated send_to_process() - see module docs for migration plan
            let _ = ctx
                .kernel
                .send_to_process(ProcessId(2), pinger, CMD_EXIT, vec![]);
            let _ = ctx
                .kernel
                .send_to_process(ProcessId(2), ponger, CMD_EXIT, vec![]);

            // Kill processes in kernel
            // NOTE: Direct kernel.kill_process() for test cleanup.
            // In production code, this should route through Init.
            ctx.kernel.kill_process(pinger);
            ctx.kernel.kill_process(ponger);

            (ctx.write_console)(&format!(
                "  Killed processes {} and {}\n",
                pinger_pid, ponger_pid
            ));
            (ctx.write_console)("Ping-pong test complete.\nzero> ");

            PingPongTestState::Idle
        }

        _ => state.clone(),
    }
}

/// Handle process spawn notification for pingpong test
///
/// Returns (new_state, should_spawn_ponger)
pub(crate) fn on_process_spawned(
    state: &PingPongTestState,
    name: &str,
    pid: u64,
) -> (PingPongTestState, bool) {
    match state {
        PingPongTestState::WaitingForPinger { iterations } if name == "pp_pinger" => {
            // Now spawn the ponger
            (
                PingPongTestState::WaitingForPonger {
                    iterations: *iterations,
                    pinger_pid: pid,
                },
                true,
            )
        }
        PingPongTestState::WaitingForPonger {
            iterations,
            pinger_pid,
        } if name == "pp_ponger" => {
            // Both spawned, set up capabilities
            (
                PingPongTestState::SettingUpCaps {
                    iterations: *iterations,
                    pinger_pid: *pinger_pid,
                    ponger_pid: pid,
                },
                false,
            )
        }
        _ => (state.clone(), false),
    }
}

/// Check if pingpong test completed (when process prints results)
pub(crate) fn check_pingpong_complete(
    state: &PingPongTestState,
    pid: u64,
) -> Option<PingPongTestState> {
    if let PingPongTestState::Running {
        pinger_pid,
        ponger_pid,
        ..
    } = state
    {
        // The pinger process prints the results, so when we see results from it, cleanup
        if pid == *pinger_pid {
            return Some(PingPongTestState::Cleanup {
                pinger_pid: *pinger_pid,
                ponger_pid: *ponger_pid,
            });
        }
    }
    None
}
