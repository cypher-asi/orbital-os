//! Ping-Pong Test State Machine
//!
//! Automated IPC latency testing between worker processes.
//!
//! # Init-Routed IPC Usage
//!
//! This module routes commands through Init via MSG_SUPERVISOR_IPC_DELIVERY.
//! This preserves capability checks and keeps the supervisor as a thin boundary
//! layer while still allowing the pingpong test to coordinate processes.

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
    pub system: &'a mut zos_kernel::System<H>,
    pub write_console: &'a dyn Fn(&str),
    pub init_endpoint_slot: Option<u32>,
}

fn route_ipc_via_init<H: zos_hal::HAL>(
    ctx: &mut PingPongContext<'_, H>,
    target_pid: u64,
    endpoint_slot: u32,
    tag: u32,
    data: &[u8],
) -> Result<(), zos_kernel::KernelError> {
    let init_slot = match ctx.init_endpoint_slot {
        Some(slot) => slot,
        None => {
            (ctx.write_console)("[pingpong] Cannot route IPC: no Init capability\n");
            return Err(zos_kernel::KernelError::PermissionDenied);
        }
    };

    let mut payload = Vec::with_capacity(14 + data.len());
    payload.extend_from_slice(&(target_pid as u32).to_le_bytes());
    payload.extend_from_slice(&endpoint_slot.to_le_bytes());
    payload.extend_from_slice(&tag.to_le_bytes());
    payload.extend_from_slice(&(data.len() as u16).to_le_bytes());
    payload.extend_from_slice(data);

    let supervisor_pid = ProcessId(0);
    ctx.system.ipc_send(
        supervisor_pid,
        init_slot,
        zos_ipc::supervisor::MSG_SUPERVISOR_IPC_DELIVERY,
        payload,
    )
}

fn request_kill_via_init<H: zos_hal::HAL>(
    ctx: &mut PingPongContext<'_, H>,
    target_pid: ProcessId,
) -> Result<(), zos_kernel::KernelError> {
    let init_slot = match ctx.init_endpoint_slot {
        Some(slot) => slot,
        None => {
            (ctx.write_console)("[pingpong] Cannot request kill: no Init capability\n");
            return Err(zos_kernel::KernelError::PermissionDenied);
        }
    };

    let payload = (target_pid.0 as u32).to_le_bytes().to_vec();
    let supervisor_pid = ProcessId(0);
    ctx.system.ipc_send(
        supervisor_pid,
        init_slot,
        zos_ipc::supervisor::MSG_SUPERVISOR_KILL_PROCESS,
        payload,
    )
}

/// Progress the ping-pong test state machine
///
/// Returns the new state after processing
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
            match ctx.system.grant_capability(
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
            match ctx.system.grant_capability(
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

            // Put ponger in pong mode
            if let Err(e) = route_ipc_via_init(ctx, ponger_pid, 0, CMD_PONG_MODE, &[]) {
                (ctx.write_console)(&format!("  Error sending PONG_MODE: {:?}\n", e));
            }

            // Send ping command to pinger with iterations count
            let ping_data = iterations.to_le_bytes().to_vec();
            if let Err(e) = route_ipc_via_init(ctx, pinger_pid, 0, CMD_PING, &ping_data) {
                (ctx.write_console)(&format!("  Error sending PING cmd: {:?}\n", e));
            }

            // Move to running state
            let start_time = ctx.system.uptime_nanos();
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
            let elapsed = ctx.system.uptime_nanos() - start_time;
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

            // Send exit commands
            let _ = route_ipc_via_init(ctx, pinger_pid, 0, CMD_EXIT, &[]);
            let _ = route_ipc_via_init(ctx, ponger_pid, 0, CMD_EXIT, &[]);

            // Request kill through Init for proper auditing
            // HAL workers will be terminated by supervisor after receiving INIT:KILL_OK
            let _ = request_kill_via_init(ctx, ProcessId(pinger_pid));
            let _ = request_kill_via_init(ctx, ProcessId(ponger_pid));

            (ctx.write_console)(&format!(
                "  Kill requests sent for processes {} and {} (awaiting Init confirmation)\n",
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
