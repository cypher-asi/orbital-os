//! Process lifecycle management for KernelCore.
//!
//! This module contains methods for:
//! - Registering new processes
//! - Killing processes (with and without capability checks)
//! - Recording process faults

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::KernelError;
use crate::types::{ObjectType, Process, ProcessId, ProcessMetrics, ProcessState};
use crate::CapabilitySpace;
use zos_axiom::{Commit, CommitType};
use zos_hal::HAL;

use super::KernelCore;

impl<H: HAL> KernelCore<H> {
    /// Register a process (used by supervisor to register spawned workers).
    ///
    /// Returns (ProcessId, Vec<Commit>) - the commits describe the mutation.
    pub fn register_process(&mut self, name: &str, timestamp: u64) -> (ProcessId, Vec<Commit>) {
        self.register_process_with_parent(name, ProcessId(0), timestamp)
    }

    /// Register a process with a specific parent (for fork/spawn tracking).
    ///
    /// Returns (ProcessId, Vec<Commit>) - the commits describe the mutation.
    pub fn register_process_with_parent(
        &mut self,
        name: &str,
        parent: ProcessId,
        timestamp: u64,
    ) -> (ProcessId, Vec<Commit>) {
        let pid = ProcessId(self.next_pid);
        self.next_pid += 1;

        let process = self.create_process_entry(pid, name, timestamp);
        self.processes.insert(pid, process);
        self.cap_spaces.insert(pid, CapabilitySpace::new());

        self.hal.debug_write(&alloc::format!(
            "[kernel] Registered process: {} (PID {})",
            name,
            pid.0
        ));

        let commit = self.create_process_commit(pid, parent.0, name, timestamp);
        (pid, vec![commit])
    }

    /// Register a process with a specific PID (used for supervisor and special processes).
    ///
    /// Returns (ProcessId, Vec<Commit>) - the commits describe the mutation.
    /// If the PID already exists, returns the existing PID without creating a new process.
    pub fn register_process_with_pid(
        &mut self,
        pid: ProcessId,
        name: &str,
        timestamp: u64,
    ) -> (ProcessId, Vec<Commit>) {
        // If process with this PID already exists, return it
        if self.processes.contains_key(&pid) {
            self.hal.debug_write(&alloc::format!(
                "[kernel] Process {} (PID {}) already exists",
                name,
                pid.0
            ));
            return (pid, vec![]);
        }

        // Update next_pid if necessary to avoid collisions
        if pid.0 >= self.next_pid {
            self.next_pid = pid.0 + 1;
        }

        // Supervisor processes have no initial memory allocation
        let process = Process {
            pid,
            name: String::from(name),
            state: ProcessState::Running,
            metrics: ProcessMetrics {
                memory_size: 0,
                ipc_sent: 0,
                ipc_received: 0,
                ipc_bytes_sent: 0,
                ipc_bytes_received: 0,
                syscall_count: 0,
                last_active_ns: timestamp,
                start_time_ns: timestamp,
            },
        };
        self.processes.insert(pid, process);
        self.cap_spaces.insert(pid, CapabilitySpace::new());

        self.hal.debug_write(&alloc::format!(
            "[kernel] Registered process: {} (PID {})",
            name,
            pid.0
        ));

        let commit = self.create_process_commit(pid, 0, name, timestamp);
        (pid, vec![commit])
    }

    /// Kill a process with capability check.
    ///
    /// This is the syscall-accessible version of kill_process. It verifies that
    /// the caller has a Process capability for the target PID with write permission
    /// before performing the kill.
    ///
    /// Init (PID 1) is granted implicit permission to kill any process.
    ///
    /// Returns (Result<(), KernelError>, Vec<Commit>) - the result and commits.
    pub fn kill_process_with_cap_check(
        &mut self,
        caller: ProcessId,
        target_pid: ProcessId,
        timestamp: u64,
    ) -> (Result<(), KernelError>, Vec<Commit>) {
        // Check if target process exists
        if !self.processes.contains_key(&target_pid) {
            return (Err(KernelError::ProcessNotFound), Vec::new());
        }

        // Init (PID 1) has implicit permission to kill any process
        if caller.0 == 1 {
            self.hal.debug_write(&alloc::format!(
                "[kernel] Init (PID 1) killing process PID {} (implicit permission)",
                target_pid.0
            ));
            let commits = self.kill_process(target_pid, timestamp);
            return (Ok(()), commits);
        }

        // For other processes, check for Process capability with write permission
        if !self.has_kill_permission(caller, target_pid) {
            self.hal.debug_write(&alloc::format!(
                "[kernel] Kill denied: PID {} lacks Process capability for PID {}",
                caller.0,
                target_pid.0
            ));
            return (Err(KernelError::PermissionDenied), Vec::new());
        }

        // Permission check passed - perform the kill
        self.hal.debug_write(&alloc::format!(
            "[kernel] PID {} killing process PID {} (capability verified)",
            caller.0,
            target_pid.0
        ));
        let commits = self.kill_process(target_pid, timestamp);

        (Ok(()), commits)
    }

    /// Kill a process and clean up its resources.
    ///
    /// Returns Vec<Commit> describing the mutations.
    pub fn kill_process(&mut self, pid: ProcessId, timestamp: u64) -> Vec<Commit> {
        let mut commits = Vec::new();

        // Remove the process and create exit commit
        if let Some(proc) = self.processes.remove(&pid) {
            self.hal.debug_write(&alloc::format!(
                "[kernel] Killed process: {} (PID {})",
                proc.name,
                pid.0
            ));

            commits.push(Commit {
                id: [0u8; 32],
                prev_commit: [0u8; 32],
                seq: 0,
                timestamp,
                commit_type: CommitType::ProcessExited { pid: pid.0, code: -1 },
                caused_by: None,
            });
        }

        // Remove its capability space
        self.cap_spaces.remove(&pid);

        // Remove endpoints owned by this process and create destruction commits
        commits.extend(self.cleanup_process_endpoints(pid, timestamp));

        commits
    }

    /// Record a process fault and terminate it.
    ///
    /// This is used when a process crashes, performs an invalid syscall,
    /// or otherwise faults. The fault is recorded in the commit log before
    /// the process is terminated.
    ///
    /// # Fault reason codes:
    /// - 1: Invalid syscall
    /// - 2: Memory access violation
    /// - 3: Capability violation
    /// - 4: Panic / abort
    /// - 5: Timeout / watchdog
    /// - 0xFF: Unknown / unspecified
    pub fn fault_process(
        &mut self,
        pid: ProcessId,
        reason: u32,
        description: String,
        timestamp: u64,
    ) -> Vec<Commit> {
        let mut commits = Vec::new();

        // Only record fault if process exists
        if self.processes.contains_key(&pid) {
            self.hal.debug_write(&alloc::format!(
                "[kernel] Process {} faulted: {} (reason {})",
                pid.0, description, reason
            ));

            commits.push(Commit {
                id: [0u8; 32],
                prev_commit: [0u8; 32],
                seq: 0,
                timestamp,
                commit_type: CommitType::ProcessFaulted {
                    pid: pid.0,
                    reason,
                    description,
                },
                caused_by: None,
            });
        }

        // Now kill the process (adds ProcessExited and EndpointDestroyed commits)
        commits.extend(self.kill_process(pid, timestamp));

        commits
    }

    // ========================================================================
    // Private helper methods
    // ========================================================================

    /// Create a process entry with standard metrics initialization
    fn create_process_entry(&self, pid: ProcessId, name: &str, timestamp: u64) -> Process {
        Process {
            pid,
            name: String::from(name),
            state: ProcessState::Running,
            metrics: ProcessMetrics {
                memory_size: 65536, // Initial 64KB (1 WASM page)
                ipc_sent: 0,
                ipc_received: 0,
                ipc_bytes_sent: 0,
                ipc_bytes_received: 0,
                syscall_count: 0,
                last_active_ns: timestamp,
                start_time_ns: timestamp,
            },
        }
    }

    /// Create a ProcessCreated commit
    fn create_process_commit(
        &self,
        pid: ProcessId,
        parent: u64,
        name: &str,
        timestamp: u64,
    ) -> Commit {
        Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::ProcessCreated {
                pid: pid.0,
                parent,
                name: String::from(name),
            },
            caused_by: None,
        }
    }

    /// Check if caller has permission to kill target process
    fn has_kill_permission(&self, caller: ProcessId, target: ProcessId) -> bool {
        self.cap_spaces.get(&caller).map_or(false, |cspace| {
            cspace.slots.values().any(|cap| {
                cap.object_type == ObjectType::Process
                    && cap.object_id == target.0
                    && cap.permissions.write
            })
        })
    }

    /// Clean up endpoints owned by a process and return destruction commits
    fn cleanup_process_endpoints(&mut self, pid: ProcessId, timestamp: u64) -> Vec<Commit> {
        let owned_endpoints: Vec<_> = self
            .endpoints
            .iter()
            .filter(|(_, ep)| ep.owner == pid)
            .map(|(id, _)| *id)
            .collect();

        owned_endpoints
            .into_iter()
            .filter_map(|eid| {
                self.endpoints.remove(&eid).map(|_| Commit {
                    id: [0u8; 32],
                    prev_commit: [0u8; 32],
                    seq: 0,
                    timestamp,
                    commit_type: CommitType::EndpointDestroyed { id: eid.0 },
                    caused_by: None,
                })
            })
            .collect()
    }
}
