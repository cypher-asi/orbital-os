//! Syscall dispatch module
//!
//! This module contains the low-level syscall dispatch logic used by the supervisor.
//! It handles:
//! - Raw syscall execution from process workers
//! - Syscall argument parsing and result encoding
//! - Rich result construction for complex syscalls (list caps, list procs, receive)

use alloc::vec::Vec;

use crate::capability::Permissions;
use crate::error::KernelError;
use crate::syscall::{Syscall, SyscallResult};
use crate::types::ProcessId;
use crate::{Kernel, KernelCore};
use zos_axiom::CommitType;
use zos_hal::HAL;

/// Execute a raw syscall from a process.
///
/// This is the main entry point for syscall dispatch from the supervisor.
/// It handles:
/// 1. Logging the request to SysLog
/// 2. Executing the syscall
/// 3. Recording any commits to the CommitLog
/// 4. Constructing the rich result and response data
/// 5. Logging the response to SysLog
///
/// Returns (result_code, rich_result, response_data).
pub fn execute_raw_syscall<H: HAL>(
    kernel: &mut Kernel<H>,
    sender: ProcessId,
    syscall_num: u32,
    args: [u32; 4],
    data: &[u8],
) -> (i64, SyscallResult, Vec<u8>) {
    let timestamp = kernel.uptime_nanos();

    // Log request to SysLog
    let req_id = kernel.axiom.syslog_mut().log_request(sender.0, syscall_num, args, timestamp);

    // Execute syscall
    let (result, commit_types) =
        execute_syscall_kernel_fn(&mut kernel.core, syscall_num, sender, args, data, timestamp);

    // Log commits
    for ct in commit_types {
        kernel.axiom.append_internal_commit(ct, timestamp);
    }

    // Get rich result and response data
    let (rich_result, response_data) =
        get_syscall_rich_result(kernel, sender, syscall_num, args, data, result);

    // Log response to SysLog
    kernel
        .axiom
        .syslog_mut()
        .log_response(sender.0, req_id, result, timestamp);

    (result, rich_result, response_data)
}

/// Execute the kernel-side syscall operation.
///
/// Returns (result_code, commits).
fn execute_syscall_kernel_fn<H: HAL>(
    core: &mut KernelCore<H>,
    syscall_num: u32,
    sender: ProcessId,
    args: [u32; 4],
    data: &[u8],
    timestamp: u64,
) -> (i64, Vec<CommitType>) {
    match syscall_num {
        // NOP
        0x00 => (0, Vec::new()),

        // SYS_DEBUG - Just returns 0, supervisor handles the message
        0x01 => (0, Vec::new()),

        // SYS_GET_TIME
        0x02 => {
            let nanos = core.hal().now_nanos();
            let result = if args[0] == 0 {
                (nanos & 0xFFFFFFFF) as i64
            } else {
                ((nanos >> 32) & 0xFFFFFFFF) as i64
            };
            (result, Vec::new())
        }

        // SYS_GET_PID
        0x03 => (sender.0 as i64, Vec::new()),

        // SYS_LIST_CAPS
        0x04 => (0, Vec::new()),

        // SYS_LIST_PROCS
        0x05 => (0, Vec::new()),

        // SYS_GET_WALLCLOCK
        0x06 => {
            let millis = core.hal().wallclock_ms();
            let result = if args[0] == 0 {
                (millis & 0xFFFFFFFF) as i64
            } else {
                ((millis >> 32) & 0xFFFFFFFF) as i64
            };
            (result, Vec::new())
        }

        // SYS_CONSOLE_WRITE
        // The supervisor receives the console output data directly via the raw syscall
        // interface and forwards it to the UI. No kernel buffering needed.
        0x07 => (0, Vec::new()),

        // SYS_EXIT
        0x11 => {
            let commits = core.kill_process(sender, timestamp);
            let commit_types: Vec<CommitType> = commits.into_iter().map(|c| c.commit_type).collect();
            (0, commit_types)
        }

        // SYS_YIELD
        0x12 => (0, Vec::new()),

        // SYS_KILL - Kill a process (requires Process capability)
        0x13 => {
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

        // SYS_REGISTER_PROCESS (0x14) - Register a new process (Init-only)
        // This syscall is part of the Init-driven spawn protocol.
        // Only Init (PID 1) can register processes to ensure all spawn operations
        // flow through Init and are properly audited via SysLog.
        // Data payload: process name as UTF-8 string
        // Returns: new PID on success, -1 on failure
        0x14 => {
            // Only Init (PID 1) can register processes
            if sender.0 != 1 {
                return (-1, Vec::new());
            }
            let name = core::str::from_utf8(data).unwrap_or("unknown");
            let (pid, commits) = core.register_process(name, timestamp);
            let commit_types = commits.into_iter().map(|c| c.commit_type).collect();
            (pid.0 as i64, commit_types)
        }

        // SYS_CREATE_ENDPOINT_FOR (0x15) - Create an endpoint for another process (Init-only)
        // This syscall is part of the Init-driven spawn protocol.
        // Only Init (PID 1) can create endpoints for other processes.
        // Args: [target_pid: u32]
        // Returns: endpoint_id on success, -1 on failure
        0x15 => {
            // Only Init (PID 1) can create endpoints for other processes
            if sender.0 != 1 {
                return (-1, Vec::new());
            }
            let target_pid = ProcessId(args[0] as u64);
            let (result, commits) = core.create_endpoint(target_pid, timestamp);
            let commit_types: Vec<CommitType> = commits.into_iter().map(|c| c.commit_type).collect();
            match result {
                Ok((eid, slot)) => {
                    // Pack endpoint_id and slot into result
                    // High 32 bits: slot, low 32 bits: endpoint_id
                    let packed = ((slot as i64) << 32) | (eid.0 as i64 & 0xFFFFFFFF);
                    (packed, commit_types)
                }
                Err(_) => (-1, commit_types),
            }
        }

        // SYS_CAP_GRANT
        0x30 => {
            let from_slot = args[0];
            let to_pid = ProcessId(args[1] as u64);
            let perms = Permissions::from_byte(args[2] as u8);

            match core.grant_capability(sender, from_slot, to_pid, perms, timestamp) {
                (Ok(new_slot), commits) => {
                    let commit_types: Vec<CommitType> =
                        commits.into_iter().map(|c| c.commit_type).collect();
                    (new_slot as i64, commit_types)
                }
                (Err(_), _) => (-1, Vec::new()),
            }
        }

        // SYS_CAP_REVOKE
        0x31 => {
            let target_pid = ProcessId(args[0] as u64);
            let slot = args[1];

            match core.delete_capability(target_pid, slot, timestamp) {
                (Ok(()), commits) => {
                    let commit_types: Vec<CommitType> =
                        commits.into_iter().map(|c| c.commit_type).collect();
                    (0, commit_types)
                }
                (Err(_), _) => (-1, Vec::new()),
            }
        }

        // SYS_EP_CREATE
        0x35 => {
            let (result, commits) = core.create_endpoint(sender, timestamp);
            let commit_types: Vec<CommitType> = commits.into_iter().map(|c| c.commit_type).collect();
            match result {
                Ok((eid, _slot)) => (eid.0 as i64, commit_types),
                Err(_) => (-1, commit_types),
            }
        }

        // SYS_SEND
        0x40 => {
            let slot = args[0];
            let tag = args[1];
            let (result, commit) = core.ipc_send(sender, slot, tag, data.to_vec(), timestamp);
            let commit_types: Vec<CommitType> = commit.into_iter().map(|c| c.commit_type).collect();
            match result {
                Ok(()) => (0, commit_types),
                Err(_) => (-1, commit_types),
            }
        }

        // SYS_RECEIVE
        0x41 => {
            let slot = args[0];
            match core.ipc_has_message(sender, slot, timestamp) {
                Ok(true) => (1, Vec::new()),  // Message available
                Ok(false) => (0, Vec::new()), // No message (WouldBlock)
                Err(_) => (-1, Vec::new()),
            }
        }

        // === Platform Storage (0x70 - 0x7F) ===
        // These syscalls start async storage operations via the HAL.
        // Results are delivered via IPC (MSG_STORAGE_RESULT).

        // SYS_STORAGE_READ (0x70) - Start async read
        0x70 => {
            let key = match core::str::from_utf8(data) {
                Ok(k) => k,
                Err(_) => return (-1, Vec::new()),
            };
            match core.hal().storage_read_async(sender.0, key) {
                Ok(request_id) => (request_id as i64, Vec::new()),
                Err(_) => (-1, Vec::new()),
            }
        }

        // SYS_STORAGE_WRITE (0x71) - Start async write
        0x71 => {
            // Data format: [key_len: u32, key: [u8], value: [u8]]
            if data.len() < 4 {
                return (-1, Vec::new());
            }
            let key_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
            if data.len() < 4 + key_len {
                return (-1, Vec::new());
            }
            let key = match core::str::from_utf8(&data[4..4 + key_len]) {
                Ok(k) => k,
                Err(_) => return (-1, Vec::new()),
            };
            let value = &data[4 + key_len..];
            match core.hal().storage_write_async(sender.0, key, value) {
                Ok(request_id) => (request_id as i64, Vec::new()),
                Err(_) => (-1, Vec::new()),
            }
        }

        // SYS_STORAGE_DELETE (0x72) - Start async delete
        0x72 => {
            let key = match core::str::from_utf8(data) {
                Ok(k) => k,
                Err(_) => return (-1, Vec::new()),
            };
            match core.hal().storage_delete_async(sender.0, key) {
                Ok(request_id) => (request_id as i64, Vec::new()),
                Err(_) => (-1, Vec::new()),
            }
        }

        // SYS_STORAGE_LIST (0x73) - Start async list
        0x73 => {
            let prefix = match core::str::from_utf8(data) {
                Ok(p) => p,
                Err(_) => return (-1, Vec::new()),
            };
            match core.hal().storage_list_async(sender.0, prefix) {
                Ok(request_id) => (request_id as i64, Vec::new()),
                Err(_) => (-1, Vec::new()),
            }
        }

        // SYS_STORAGE_EXISTS (0x74) - Start async exists check
        0x74 => {
            let key = match core::str::from_utf8(data) {
                Ok(k) => k,
                Err(_) => return (-1, Vec::new()),
            };
            match core.hal().storage_exists_async(sender.0, key) {
                Ok(request_id) => (request_id as i64, Vec::new()),
                Err(_) => (-1, Vec::new()),
            }
        }

        // Unknown syscall
        _ => (-1, Vec::new()),
    }
}

/// Get rich result and response data for a syscall.
///
/// Some syscalls (like list caps, list procs, receive) return complex data
/// that needs to be serialized into a response buffer for the process.
fn get_syscall_rich_result<H: HAL>(
    kernel: &mut Kernel<H>,
    sender: ProcessId,
    syscall_num: u32,
    args: [u32; 4],
    _data: &[u8],
    result: i64,
) -> (SyscallResult, Vec<u8>) {
    match syscall_num {
        // SYS_LIST_CAPS
        0x04 => {
            let syscall = Syscall::ListCaps;
            let timestamp = kernel.uptime_nanos();
            let (rich_result, _) = kernel.core.handle_syscall(sender, syscall, timestamp);
            if let SyscallResult::CapList(ref caps) = rich_result {
                let mut bytes = Vec::new();
                bytes.extend_from_slice(&(caps.len() as u32).to_le_bytes());
                for (slot, cap) in caps {
                    bytes.extend_from_slice(&slot.to_le_bytes());
                    bytes.push(cap.object_type as u8);
                    bytes.extend_from_slice(&cap.object_id.to_le_bytes());
                }
                (rich_result, bytes)
            } else {
                (SyscallResult::Ok(result as u64), Vec::new())
            }
        }

        // SYS_LIST_PROCS
        0x05 => {
            let syscall = Syscall::ListProcesses;
            let timestamp = kernel.uptime_nanos();
            let (rich_result, _) = kernel.core.handle_syscall(sender, syscall, timestamp);
            if let SyscallResult::ProcessList(ref procs) = rich_result {
                let mut bytes = Vec::new();
                bytes.extend_from_slice(&(procs.len() as u32).to_le_bytes());
                for (proc_pid, name, _state) in procs {
                    bytes.extend_from_slice(&(proc_pid.0 as u32).to_le_bytes());
                    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
                    bytes.extend_from_slice(name.as_bytes());
                }
                (rich_result, bytes)
            } else {
                (SyscallResult::Ok(result as u64), Vec::new())
            }
        }

        // SYS_RECEIVE
        0x41 => {
            if result == 1 {
                let slot = args[0];
                let timestamp = kernel.uptime_nanos();
                let (recv_result, commits) = kernel.core.ipc_receive_with_caps(sender, slot, timestamp);

                for commit in commits {
                    kernel
                        .axiom
                        .append_internal_commit(commit.commit_type, timestamp);
                }

                match recv_result {
                    Ok(Some((msg, installed_slots))) => {
                        let mut msg_bytes = Vec::new();
                        msg_bytes.extend_from_slice(&(msg.from.0 as u32).to_le_bytes());
                        msg_bytes.extend_from_slice(&msg.tag.to_le_bytes());
                        msg_bytes.push(installed_slots.len() as u8);
                        for cap_slot in &installed_slots {
                            msg_bytes.extend_from_slice(&cap_slot.to_le_bytes());
                        }
                        msg_bytes.extend_from_slice(&msg.data);
                        (SyscallResult::Message(msg), msg_bytes)
                    }
                    _ => (SyscallResult::Ok(result as u64), Vec::new()),
                }
            } else if result == 0 {
                (SyscallResult::WouldBlock, Vec::new())
            } else {
                (SyscallResult::Err(KernelError::PermissionDenied), Vec::new())
            }
        }

        // Default
        _ => {
            if result >= 0 {
                (SyscallResult::Ok(result as u64), Vec::new())
            } else {
                (SyscallResult::Err(KernelError::PermissionDenied), Vec::new())
            }
        }
    }
}
