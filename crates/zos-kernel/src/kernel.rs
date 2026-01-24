//! Kernel wrapper implementation.
//!
//! The Kernel struct is a thin wrapper around KernelCore that provides:
//! - Read-only access to kernel state via public accessor methods
//! - The `axiom` gateway for all state-mutating operations
//!
//! All mutations MUST flow through the Kernel's public methods, which ensure
//! proper audit logging and commit recording via axiom.

use alloc::string::String;
use alloc::vec::Vec;

use crate::core::KernelCore;
use crate::error::KernelError;
use crate::ipc::{Endpoint, EndpointDetail, EndpointInfo, Message};
use crate::syscall::{RevokeNotification, Syscall, SyscallResult};
use crate::types::{CapSlot, EndpointId, Process, ProcessId, SystemMetrics};
use crate::{CapabilitySpace, Permissions};
use zos_axiom::{AxiomGateway, CommitLog, SysLog};
use zos_hal::HAL;

/// The kernel, generic over HAL implementation.
pub struct Kernel<H: HAL> {
    /// The kernel core holding all mutable state
    /// Note: pub(crate) for dispatch module access
    pub(crate) core: KernelCore<H>,
    /// Axiom gateway (SysLog + CommitLog) - entry point for all mutations
    pub axiom: AxiomGateway,
    /// Boot time (for uptime calculation)
    boot_time: u64,
}

impl<H: HAL> Kernel<H> {
    /// Create a new kernel with the given HAL
    pub fn new(hal: H) -> Self {
        let boot_time = hal.now_nanos();
        Self {
            core: KernelCore::new(hal),
            axiom: AxiomGateway::new(boot_time),
            boot_time,
        }
    }

    /// Get reference to HAL
    pub fn hal(&self) -> &H {
        self.core.hal()
    }

    /// Get uptime in nanoseconds
    pub fn uptime_nanos(&self) -> u64 {
        self.core.hal().now_nanos().saturating_sub(self.boot_time)
    }

    /// Get boot time
    pub fn boot_time(&self) -> u64 {
        self.boot_time
    }

    // ========================================================================
    // Process Management
    // ========================================================================

    /// Register a process and log the mutation.
    pub fn register_process(&mut self, name: &str) -> ProcessId {
        let timestamp = self.uptime_nanos();
        let (pid, commits) = self.core.register_process(name, timestamp);
        self.record_commits(commits, timestamp);
        pid
    }

    /// Register a process with a specific PID (for supervisor and special processes).
    pub fn register_process_with_pid(&mut self, pid: ProcessId, name: &str) -> ProcessId {
        let timestamp = self.uptime_nanos();
        let (result_pid, commits) = self.core.register_process_with_pid(pid, name, timestamp);
        self.record_commits(commits, timestamp);
        result_pid
    }

    /// Kill a process and log the mutation.
    pub fn kill_process(&mut self, pid: ProcessId) {
        let timestamp = self.uptime_nanos();
        let commits = self.core.kill_process(pid, timestamp);
        self.record_commits(commits, timestamp);
    }

    /// Record a process fault and terminate it.
    pub fn fault_process(&mut self, pid: ProcessId, reason: u32, description: String) {
        let timestamp = self.uptime_nanos();
        let commits = self.core.fault_process(pid, reason, description, timestamp);
        self.record_commits(commits, timestamp);
    }

    /// Get process info
    pub fn get_process(&self, pid: ProcessId) -> Option<&Process> {
        self.core.get_process(pid)
    }

    /// List all processes
    pub fn list_processes(&self) -> Vec<(ProcessId, &Process)> {
        self.core.list_processes()
    }

    // ========================================================================
    // Endpoint Management
    // ========================================================================

    /// Create an endpoint and log the mutation.
    pub fn create_endpoint(&mut self, owner: ProcessId) -> Result<(EndpointId, CapSlot), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.create_endpoint(owner, timestamp);
        self.record_commits(commits, timestamp);
        result
    }

    /// List all endpoints
    pub fn list_endpoints(&self) -> Vec<EndpointInfo> {
        self.core.list_endpoints()
    }

    /// Get endpoint info
    pub fn get_endpoint(&self, id: EndpointId) -> Option<&Endpoint> {
        self.core.get_endpoint(id)
    }

    /// Get detailed endpoint info
    pub fn get_endpoint_detail(&self, id: EndpointId) -> Option<EndpointDetail> {
        self.core.get_endpoint_detail(id)
    }

    // ========================================================================
    // Capability Management
    // ========================================================================

    /// Grant capability and log the mutation.
    pub fn grant_capability(
        &mut self,
        from_pid: ProcessId,
        from_slot: CapSlot,
        to_pid: ProcessId,
        perms: Permissions,
    ) -> Result<CapSlot, KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) =
            self.core
                .grant_capability(from_pid, from_slot, to_pid, perms, timestamp);
        self.record_commits(commits, timestamp);
        result
    }

    /// Grant capability to a specific endpoint directly.
    pub fn grant_capability_to_endpoint(
        &mut self,
        owner_pid: ProcessId,
        endpoint_id: EndpointId,
        to_pid: ProcessId,
        perms: Permissions,
    ) -> Result<CapSlot, KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.grant_capability_to_endpoint(
            owner_pid, endpoint_id, to_pid, perms, timestamp,
        );
        self.record_commits(commits, timestamp);
        result
    }

    /// Revoke capability and log the mutation.
    pub fn revoke_capability(&mut self, pid: ProcessId, slot: CapSlot) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.revoke_capability(pid, slot, timestamp);
        self.record_commits(commits, timestamp);
        result
    }

    /// Delete capability and log the mutation.
    pub fn delete_capability(&mut self, pid: ProcessId, slot: CapSlot) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.delete_capability(pid, slot, timestamp);
        self.record_commits(commits, timestamp);
        result
    }

    /// Delete a capability and return information for notification.
    pub fn delete_capability_with_notification(
        &mut self,
        pid: ProcessId,
        slot: CapSlot,
        reason: u8,
    ) -> Result<RevokeNotification, KernelError> {
        // Get cap info before deletion
        let cap_info = self
            .get_cap_space(pid)
            .and_then(|cs| cs.get(slot))
            .map(|cap| (cap.object_type as u8, cap.object_id));

        // Perform the deletion
        self.delete_capability(pid, slot)?;

        // Build notification
        if let Some((object_type, object_id)) = cap_info {
            Ok(RevokeNotification {
                pid,
                slot,
                object_type,
                object_id,
                reason,
            })
        } else {
            Ok(RevokeNotification::empty())
        }
    }

    /// Derive capability and log the mutation.
    pub fn derive_capability(
        &mut self,
        pid: ProcessId,
        slot: CapSlot,
        new_perms: Permissions,
    ) -> Result<CapSlot, KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.derive_capability(pid, slot, new_perms, timestamp);
        self.record_commits(commits, timestamp);
        result
    }

    /// Get capability space for a process
    pub fn get_cap_space(&self, pid: ProcessId) -> Option<&CapabilitySpace> {
        self.core.get_cap_space(pid)
    }

    // ========================================================================
    // IPC Operations
    // ========================================================================

    /// Send IPC message and log the mutation.
    pub fn ipc_send(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
    ) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commit) = self.core.ipc_send(from_pid, endpoint_slot, tag, data, timestamp);
        if let Some(c) = commit {
            self.axiom.append_internal_commit(c.commit_type, timestamp);
        }
        result
    }

    /// Send IPC message with capability transfer.
    pub fn ipc_send_with_caps(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
        cap_slots: &[CapSlot],
    ) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) =
            self.core
                .ipc_send_with_caps(from_pid, endpoint_slot, tag, data, cap_slots, timestamp);
        self.record_commits(commits, timestamp);
        result
    }

    /// Receive IPC message.
    pub fn ipc_receive(
        &mut self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
    ) -> Result<Option<Message>, KernelError> {
        let timestamp = self.uptime_nanos();
        self.core.ipc_receive(pid, endpoint_slot, timestamp)
    }

    /// Receive IPC message with capability transfer.
    pub fn ipc_receive_with_caps(
        &mut self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
    ) -> Result<Option<(Message, Vec<CapSlot>)>, KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.ipc_receive_with_caps(pid, endpoint_slot, timestamp);
        self.record_commits(commits, timestamp);
        result
    }

    // ========================================================================
    // Syscall Handling
    // ========================================================================

    /// Handle a syscall from a process.
    pub fn handle_syscall(&mut self, pid: ProcessId, syscall: Syscall) -> SyscallResult {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.handle_syscall(pid, syscall, timestamp);
        self.record_commits(commits, timestamp);
        result
    }

    // ========================================================================
    // Deprecated APIs
    // ========================================================================

    /// Send a message to a process's first endpoint (for testing only).
    ///
    /// **DEPRECATED: This function BYPASSES capability checks.**
    ///
    /// This method violates the capability-based security model by allowing
    /// messages to be sent without proper endpoint capabilities. It exists
    /// only for legacy test compatibility (pingpong test).
    ///
    /// # Migration Path
    ///
    /// New code should:
    /// 1. Use `create_endpoint()` to create endpoints
    /// 2. Use `grant_capability()` to share endpoint access
    /// 3. Use `ipc_send()` with the granted capability slot
    ///
    /// The pingpong test should be updated to use proper IPC, after which
    /// this method can be removed entirely.
    ///
    /// # Internal Note
    ///
    /// The underlying `core.send_to_process()` is still used internally by
    /// the Reply syscall handler (SYS_REPLY 0x43) for RPC responses. That
    /// internal usage is acceptable because Reply sends to a known caller
    /// that initiated the Call. This public wrapper is what's deprecated.
    #[deprecated(note = "Use ipc_send with proper capabilities instead. See method docs for migration path.")]
    pub fn send_to_process(
        &mut self,
        from_pid: ProcessId,
        to_pid: ProcessId,
        tag: u32,
        data: Vec<u8>,
    ) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        self.core.send_to_process(from_pid, to_pid, tag, data, timestamp)
    }

    // ========================================================================
    // Memory Management
    // ========================================================================

    /// Allocate memory to a process
    pub fn allocate_memory(&mut self, pid: ProcessId, bytes: usize) -> Result<usize, KernelError> {
        self.core.allocate_memory(pid, bytes)
    }

    /// Free memory from a process
    pub fn free_memory(&mut self, pid: ProcessId, bytes: usize) -> Result<usize, KernelError> {
        self.core.free_memory(pid, bytes)
    }

    /// Update process memory size
    pub fn update_process_memory(&mut self, pid: ProcessId, new_size: usize) {
        self.core.update_process_memory(pid, new_size)
    }

    // ========================================================================
    // Metrics and Monitoring
    // ========================================================================

    /// Get system-wide metrics
    pub fn get_system_metrics(&self) -> SystemMetrics {
        self.core.get_system_metrics(self.uptime_nanos())
    }

    /// Get total system memory usage
    pub fn total_memory(&self) -> usize {
        self.core.total_memory()
    }

    /// Get total message count in all endpoint queues
    pub fn total_pending_messages(&self) -> usize {
        self.core.total_pending_messages()
    }

    // ========================================================================
    // CommitLog Access
    // ========================================================================

    /// Get reference to the commit log
    pub fn commitlog(&self) -> &CommitLog {
        self.axiom.commitlog()
    }

    /// Get reference to the syslog
    pub fn syslog(&self) -> &SysLog {
        self.axiom.syslog()
    }

    // ========================================================================
    // Raw Syscall Execution (for supervisor)
    // ========================================================================

    /// Execute a raw syscall and return (result, rich_result, response_data).
    pub fn execute_raw_syscall(
        &mut self,
        sender: ProcessId,
        syscall_num: u32,
        args: [u32; 4],
        data: &[u8],
    ) -> (i64, SyscallResult, Vec<u8>) {
        crate::dispatch::execute_raw_syscall(self, sender, syscall_num, args, data)
    }

    // ========================================================================
    // Private helpers
    // ========================================================================

    /// Record commits to the axiom gateway
    fn record_commits(&mut self, commits: Vec<zos_axiom::Commit>, timestamp: u64) {
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
    }
}

impl<H: HAL + Default> Kernel<H> {
    /// Create a kernel for replay mode.
    pub fn new_for_replay() -> Self {
        let hal = H::default();
        Self {
            core: KernelCore::new(hal),
            axiom: AxiomGateway::new(0),
            boot_time: 0,
        }
    }
}
