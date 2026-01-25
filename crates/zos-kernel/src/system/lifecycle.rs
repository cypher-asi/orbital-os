//! Process lifecycle syscall handlers
//!
//! This module contains syscall handlers for process lifecycle management:
//! - `execute_exit()` - Handle process exit syscall
//! - `execute_kill_with_cap()` - Handle kill process with capability check
//! - `execute_register_process()` - Handle process registration
//! - `execute_create_endpoint_for()` - Handle endpoint creation for another process

use alloc::vec::Vec;

use crate::core::KernelCore;
use crate::types::ProcessId;
use zos_axiom::CommitType;
use zos_hal::HAL;

/// Execute process exit syscall (0x11).
///
/// Terminates the calling process and returns its commits.
pub(in crate::system) fn execute_exit<H: HAL>(
    core: &mut KernelCore<H>,
    sender: ProcessId,
    timestamp: u64,
) -> (i64, Vec<CommitType>) {
    let commits = core.kill_process(sender, timestamp);
    let commit_types: Vec<CommitType> = commits.into_iter().map(|c| c.commit_type).collect();
    (0, commit_types)
}

/// Execute kill process syscall with capability check (0x13).
///
/// Kills a target process if the sender has the appropriate capability.
/// Returns success (0) or error (-1).
pub(in crate::system) fn execute_kill_with_cap<H: HAL>(
    core: &mut KernelCore<H>,
    sender: ProcessId,
    args: [u32; 4],
    timestamp: u64,
) -> (i64, Vec<CommitType>) {
    let target_pid = ProcessId(args[0] as u64);

    match core.kill_process_with_cap_check(sender, target_pid, timestamp) {
        (Ok(()), commits) => {
            let commit_types: Vec<CommitType> =
                commits.into_iter().map(|c| c.commit_type).collect();
            (0, commit_types)
        }
        (Err(_), _) => (-1, Vec::new()),
    }
}

/// Execute register process syscall (0x14).
///
/// Creates a new process. Only init (PID 1) can call this.
/// Returns the new process ID or -1 on error.
pub(in crate::system) fn execute_register_process<H: HAL>(
    core: &mut KernelCore<H>,
    sender: ProcessId,
    data: &[u8],
    timestamp: u64,
) -> (i64, Vec<CommitType>) {
    // Only init can register processes
    if sender.0 != 1 {
        return (-1, Vec::new());
    }

    let name = core::str::from_utf8(data).unwrap_or("unknown");
    let (pid, commits) = core.register_process(name, timestamp);
    let commit_types = commits.into_iter().map(|c| c.commit_type).collect();

    (pid.0 as i64, commit_types)
}

/// Execute create endpoint for another process syscall (0x15).
///
/// Creates an endpoint for a target process. Only init (PID 1) can call this.
/// Returns packed (slot << 32 | endpoint_id) or -1 on error.
pub(in crate::system) fn execute_create_endpoint_for<H: HAL>(
    core: &mut KernelCore<H>,
    sender: ProcessId,
    args: [u32; 4],
    timestamp: u64,
) -> (i64, Vec<CommitType>) {
    // Only init can create endpoints for other processes
    if sender.0 != 1 {
        return (-1, Vec::new());
    }

    let target_pid = ProcessId(args[0] as u64);
    let (result, commits) = core.create_endpoint(target_pid, timestamp);
    let commit_types: Vec<CommitType> = commits.into_iter().map(|c| c.commit_type).collect();

    match result {
        Ok((eid, slot)) => {
            // Pack endpoint ID and slot into a single i64
            let packed = ((slot as i64) << 32) | (eid.0 as i64 & 0xFFFFFFFF);
            (packed, commit_types)
        }
        Err(_) => (-1, commit_types),
    }
}
