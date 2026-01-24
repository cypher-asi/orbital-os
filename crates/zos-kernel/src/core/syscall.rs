//! Syscall handling for KernelCore.
//!
//! This module contains the main syscall dispatcher and category-specific handlers.

use alloc::vec;
use alloc::vec::Vec;

use crate::error::KernelError;
use crate::syscall::{CapInfo, Syscall, SyscallResult};
use crate::types::{ProcessId, ProcessState};
use zos_axiom::{Commit, CommitType};
use zos_hal::HAL;

use super::KernelCore;

impl<H: HAL> KernelCore<H> {
    /// Handle a syscall from a process.
    ///
    /// Returns (SyscallResult, Vec<Commit>) - the result and any commits generated.
    pub fn handle_syscall(
        &mut self,
        from_pid: ProcessId,
        syscall: Syscall,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        // Update syscall metrics
        self.update_syscall_metrics(from_pid, timestamp);

        match syscall {
            // Debug syscalls
            Syscall::Debug { msg } => self.handle_debug(from_pid, msg),

            // Endpoint syscalls
            Syscall::CreateEndpoint => self.handle_create_endpoint(from_pid, timestamp),

            // IPC syscalls
            Syscall::Send { endpoint_slot, tag, data } => {
                self.handle_send(from_pid, endpoint_slot, tag, data, timestamp)
            }
            Syscall::Receive { endpoint_slot } => {
                self.handle_receive(from_pid, endpoint_slot, timestamp)
            }
            Syscall::SendWithCaps { endpoint_slot, tag, data, cap_slots } => {
                self.handle_send_with_caps(from_pid, endpoint_slot, tag, data, cap_slots, timestamp)
            }
            Syscall::Call { endpoint_slot, tag, data } => {
                self.handle_call(from_pid, endpoint_slot, tag, data, timestamp)
            }
            Syscall::Reply { caller_pid, tag, data } => {
                self.handle_reply(from_pid, caller_pid, tag, data, timestamp)
            }

            // Capability syscalls
            Syscall::ListCaps => self.handle_list_caps(from_pid),
            Syscall::CapGrant { from_slot, to_pid, permissions } => {
                self.handle_cap_grant(from_pid, from_slot, to_pid, permissions, timestamp)
            }
            Syscall::CapRevoke { slot } => self.handle_cap_revoke(from_pid, slot, timestamp),
            Syscall::CapDelete { slot } => self.handle_cap_delete(from_pid, slot, timestamp),
            Syscall::CapInspect { slot } => self.handle_cap_inspect(from_pid, slot),
            Syscall::CapDerive { slot, new_permissions } => {
                self.handle_cap_derive(from_pid, slot, new_permissions, timestamp)
            }

            // Process syscalls
            Syscall::ListProcesses => self.handle_list_processes(),
            Syscall::Exit { code } => self.handle_exit(from_pid, code, timestamp),
            Syscall::Kill { target_pid } => self.handle_kill(from_pid, target_pid, timestamp),

            // Misc syscalls
            Syscall::GetTime => (SyscallResult::Ok(timestamp), vec![]),
            Syscall::Yield => (SyscallResult::Ok(0), vec![]),
        }
    }

    // ========================================================================
    // Debug syscalls
    // ========================================================================

    fn handle_debug(&self, from_pid: ProcessId, msg: alloc::string::String) -> (SyscallResult, Vec<Commit>) {
        self.hal.debug_write(&alloc::format!("[PID {}] {}", from_pid.0, msg));
        (SyscallResult::Ok(0), vec![])
    }

    // ========================================================================
    // Endpoint syscalls
    // ========================================================================

    fn handle_create_endpoint(
        &mut self,
        from_pid: ProcessId,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        let (result, commits) = self.create_endpoint(from_pid, timestamp);
        let syscall_result = match result {
            Ok((eid, slot)) => SyscallResult::Ok((eid.0 << 32) | (slot as u64)),
            Err(e) => SyscallResult::Err(e),
        };
        (syscall_result, commits)
    }

    // ========================================================================
    // IPC syscalls
    // ========================================================================

    fn handle_send(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: u32,
        tag: u32,
        data: Vec<u8>,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        let (result, commit) = self.ipc_send(from_pid, endpoint_slot, tag, data, timestamp);
        let commits = commit.into_iter().collect();
        let syscall_result = match result {
            Ok(()) => SyscallResult::Ok(0),
            Err(e) => SyscallResult::Err(e),
        };
        (syscall_result, commits)
    }

    fn handle_receive(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: u32,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        let result = self.ipc_receive(from_pid, endpoint_slot, timestamp);
        let syscall_result = match result {
            Ok(Some(msg)) => SyscallResult::Message(msg),
            Ok(None) => SyscallResult::WouldBlock,
            Err(e) => SyscallResult::Err(e),
        };
        (syscall_result, vec![])
    }

    fn handle_send_with_caps(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: u32,
        tag: u32,
        data: Vec<u8>,
        cap_slots: Vec<u32>,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        let (result, commits) =
            self.ipc_send_with_caps(from_pid, endpoint_slot, tag, data, &cap_slots, timestamp);
        let syscall_result = match result {
            Ok(()) => SyscallResult::Ok(0),
            Err(e) => SyscallResult::Err(e),
        };
        (syscall_result, commits)
    }

    fn handle_call(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: u32,
        tag: u32,
        data: Vec<u8>,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        // Call = send + block for reply
        let (result, commit) = self.ipc_send(from_pid, endpoint_slot, tag, data, timestamp);
        let commits = commit.into_iter().collect();
        let syscall_result = match result {
            Ok(()) => SyscallResult::WouldBlock,
            Err(e) => SyscallResult::Err(e),
        };
        (syscall_result, commits)
    }

    fn handle_reply(
        &mut self,
        from_pid: ProcessId,
        caller_pid: ProcessId,
        tag: u32,
        data: Vec<u8>,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        let result = self.send_to_process(from_pid, caller_pid, tag, data, timestamp);
        let syscall_result = match result {
            Ok(()) => SyscallResult::Ok(0),
            Err(e) => SyscallResult::Err(e),
        };
        (syscall_result, vec![])
    }

    // ========================================================================
    // Capability syscalls
    // ========================================================================

    fn handle_list_caps(&self, from_pid: ProcessId) -> (SyscallResult, Vec<Commit>) {
        let caps = self
            .cap_spaces
            .get(&from_pid)
            .map(|cs| cs.list())
            .unwrap_or_default();
        (SyscallResult::CapList(caps), vec![])
    }

    fn handle_cap_grant(
        &mut self,
        from_pid: ProcessId,
        from_slot: u32,
        to_pid: ProcessId,
        permissions: crate::Permissions,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        let (result, commits) =
            self.grant_capability(from_pid, from_slot, to_pid, permissions, timestamp);
        let syscall_result = match result {
            Ok(new_slot) => SyscallResult::Ok(new_slot as u64),
            Err(e) => SyscallResult::Err(e),
        };
        (syscall_result, commits)
    }

    fn handle_cap_revoke(
        &mut self,
        from_pid: ProcessId,
        slot: u32,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        let (result, commits) = self.revoke_capability(from_pid, slot, timestamp);
        let syscall_result = match result {
            Ok(()) => SyscallResult::Ok(0),
            Err(e) => SyscallResult::Err(e),
        };
        (syscall_result, commits)
    }

    fn handle_cap_delete(
        &mut self,
        from_pid: ProcessId,
        slot: u32,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        let (result, commits) = self.delete_capability(from_pid, slot, timestamp);
        let syscall_result = match result {
            Ok(()) => SyscallResult::Ok(0),
            Err(e) => SyscallResult::Err(e),
        };
        (syscall_result, commits)
    }

    fn handle_cap_inspect(&self, from_pid: ProcessId, slot: u32) -> (SyscallResult, Vec<Commit>) {
        let result = match self.cap_spaces.get(&from_pid) {
            Some(cspace) => match cspace.get(slot) {
                Some(cap) => SyscallResult::CapInfo(CapInfo::from(cap)),
                None => SyscallResult::Err(KernelError::InvalidCapability),
            },
            None => SyscallResult::Err(KernelError::ProcessNotFound),
        };
        (result, vec![])
    }

    fn handle_cap_derive(
        &mut self,
        from_pid: ProcessId,
        slot: u32,
        new_permissions: crate::Permissions,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        let (result, commits) = self.derive_capability(from_pid, slot, new_permissions, timestamp);
        let syscall_result = match result {
            Ok(new_slot) => SyscallResult::Ok(new_slot as u64),
            Err(e) => SyscallResult::Err(e),
        };
        (syscall_result, commits)
    }

    // ========================================================================
    // Process syscalls
    // ========================================================================

    fn handle_list_processes(&self) -> (SyscallResult, Vec<Commit>) {
        let procs: Vec<_> = self
            .processes
            .iter()
            .map(|(pid, p)| (*pid, p.name.clone(), p.state))
            .collect();
        (SyscallResult::ProcessList(procs), vec![])
    }

    fn handle_exit(
        &mut self,
        from_pid: ProcessId,
        code: i32,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        if let Some(proc) = self.processes.get_mut(&from_pid) {
            proc.state = ProcessState::Zombie;
        }

        let commit = Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::ProcessExited {
                pid: from_pid.0,
                code,
            },
            caused_by: None,
        };

        (SyscallResult::Ok(code as u64), vec![commit])
    }

    fn handle_kill(
        &mut self,
        from_pid: ProcessId,
        target_pid: ProcessId,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        let (result, commits) = self.kill_process_with_cap_check(from_pid, target_pid, timestamp);
        let syscall_result = match result {
            Ok(()) => SyscallResult::Ok(0),
            Err(e) => SyscallResult::Err(e),
        };
        (syscall_result, commits)
    }

    // ========================================================================
    // Helper methods
    // ========================================================================

    fn update_syscall_metrics(&mut self, from_pid: ProcessId, timestamp: u64) {
        if let Some(proc) = self.processes.get_mut(&from_pid) {
            proc.metrics.syscall_count += 1;
            proc.metrics.last_active_ns = timestamp;
        }
    }
}
