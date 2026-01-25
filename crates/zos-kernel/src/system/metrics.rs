//! Metrics and introspection syscall handlers
//!
//! This module contains syscall handlers for system introspection:
//! - `format_caps_list()` - Format capability list for syscall response
//! - `format_process_list()` - Format process list for syscall response

use alloc::vec::Vec;

use crate::core::KernelCore;
use crate::error::KernelError;
use crate::syscall::{Syscall, SyscallResult};
use crate::types::ProcessId;
use zos_hal::HAL;

/// Get rich result and response data for a syscall.
///
/// This function routes syscalls to specialized formatters based on the syscall number.
#[allow(dead_code)] // Called from System::process_syscall
pub(in crate::system) fn get_syscall_rich_result<H: HAL>(
    kernel: &mut KernelCore<H>,
    sender: ProcessId,
    syscall_num: u32,
    args: [u32; 4],
    _data: &[u8],
    result: i64,
    timestamp: u64,
) -> (SyscallResult, Vec<u8>) {
    match syscall_num {
        0x04 => format_caps_list(kernel, sender, result, timestamp),
        0x05 => format_process_list(kernel, sender, result, timestamp),
        0x41 => format_receive_result(kernel, sender, args, result, timestamp),
        _ => default_rich_result(result),
    }
}

/// Format capability list for syscall 0x04 (LIST_CAPS).
///
/// Returns (SyscallResult, response_bytes) where response_bytes contains:
/// - u32: number of capabilities
/// - For each capability:
///   - u32: slot number
///   - u8: object type
///   - u64: object ID
pub(in crate::system) fn format_caps_list<H: HAL>(
    kernel: &mut KernelCore<H>,
    sender: ProcessId,
    result: i64,
    timestamp: u64,
) -> (SyscallResult, Vec<u8>) {
    let syscall = Syscall::ListCaps;
    let (rich_result, _) = kernel.handle_syscall(sender, syscall, timestamp);
    
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

/// Format process list for syscall 0x05 (LIST_PROCESSES).
///
/// Returns (SyscallResult, response_bytes) where response_bytes contains:
/// - u32: number of processes
/// - For each process:
///   - u32: process ID
///   - u16: name length
///   - bytes: process name (UTF-8)
pub(in crate::system) fn format_process_list<H: HAL>(
    kernel: &mut KernelCore<H>,
    sender: ProcessId,
    result: i64,
    timestamp: u64,
) -> (SyscallResult, Vec<u8>) {
    let syscall = Syscall::ListProcesses;
    let (rich_result, _) = kernel.handle_syscall(sender, syscall, timestamp);
    
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

/// Format IPC receive result for syscall 0x41 (IPC_RECEIVE).
pub(in crate::system) fn format_receive_result<H: HAL>(
    kernel: &mut KernelCore<H>,
    sender: ProcessId,
    args: [u32; 4],
    result: i64,
    timestamp: u64,
) -> (SyscallResult, Vec<u8>) {
    if result == 1 {
        let slot = args[0];
        let (recv_result, commits) = kernel.ipc_receive_with_caps(sender, slot, timestamp);
        let _ = commits;

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

/// Default rich result formatting for syscalls.
pub(in crate::system) fn default_rich_result(result: i64) -> (SyscallResult, Vec<u8>) {
    if result >= 0 {
        (SyscallResult::Ok(result as u64), Vec::new())
    } else {
        (SyscallResult::Err(KernelError::PermissionDenied), Vec::new())
    }
}
