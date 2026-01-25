//! System struct - combines Axiom verification layer and KernelCore execution layer.
//!
//! Per the architectural invariants (docs/invariants/invariants.md):
//!
//! - **Axiom** is the verification layer that gates all kernel access
//! - **KernelCore** is the execution layer that holds state and executes operations
//! - **System** combines both, providing the single entry point for all syscalls
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                          SYSTEM                              │
//! │                                                             │
//! │   ┌───────────────────────────────────────────────────┐     │
//! │   │                      AXIOM                         │     │
//! │   │   - Verification layer (sender identity)          │     │
//! │   │   - SysLog (audit trail)                          │     │
//! │   │   - CommitLog (state mutations)                   │     │
//! │   │   - THE entry point for all syscalls              │     │
//! │   └───────────────────────────────────────────────────┘     │
//! │                              │                               │
//! │                              │ (verified request)            │
//! │                              ▼                               │
//! │   ┌───────────────────────────────────────────────────┐     │
//! │   │                     KERNEL                         │     │
//! │   │   - Capabilities & CSpaces                        │     │
//! │   │   - Process state                                 │     │
//! │   │   - IPC endpoints                                 │     │
//! │   │   - Emits Commits for state changes               │     │
//! │   └───────────────────────────────────────────────────┘     │
//! │                                                             │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! All syscalls flow: `Process → System.process_syscall() → Axiom (log) → KernelCore (execute) → Axiom (record) → Process`

mod lifecycle;
mod metrics;

use alloc::vec::Vec;

use crate::capability::Permissions;
use crate::core::KernelCore;
use crate::error::KernelError;
use crate::ipc::{Endpoint, EndpointDetail, EndpointInfo, Message};
use crate::syscall::{RevokeNotification, Syscall, SyscallResult};
use crate::types::{CapSlot, EndpointId, Process, ProcessId, SystemMetrics};
use crate::CapabilitySpace;
use zos_axiom::{AxiomGateway, Commit, CommitLog, CommitType, SysLog};
use zos_hal::HAL;

/// System combines the Axiom verification layer with the KernelCore execution layer.
///
/// This is the correct architectural boundary:
/// - Axiom and Kernel are **separate components, separately instantiated**
/// - Axiom handles: verification, SysLog, CommitLog
/// - Kernel handles: capabilities, state, execution
/// - ALL kernel access MUST flow THROUGH Axiom - no direct kernel calls
pub struct System<H: HAL> {
    /// Axiom verification layer (SysLog + CommitLog)
    pub axiom: AxiomGateway,
    /// Kernel execution layer (state + capabilities)
    pub kernel: KernelCore<H>,
    /// Boot time (for uptime calculation)
    boot_time: u64,
}

impl<H: HAL> System<H> {
    /// Create a new System with the given HAL.
    pub fn new(hal: H) -> Self {
        let boot_time = hal.now_nanos();
        Self {
            axiom: AxiomGateway::new(boot_time),
            kernel: KernelCore::new(hal),
            boot_time,
        }
    }

    /// Get reference to HAL.
    pub fn hal(&self) -> &H {
        self.kernel.hal()
    }

    /// Get uptime in nanoseconds.
    pub fn uptime_nanos(&self) -> u64 {
        self.kernel.hal().now_nanos().saturating_sub(self.boot_time)
    }

    /// Get boot time.
    pub fn boot_time(&self) -> u64 {
        self.boot_time
    }

    // ========================================================================
    // Main Syscall Entry Point - ALL syscalls flow through here
    // ========================================================================

    /// Process a syscall through the Axiom verification layer.
    ///
    /// This is THE entry point for all syscalls. It:
    /// 1. Logs the request to SysLog
    /// 2. Executes via KernelCore
    /// 3. Records commits to CommitLog
    /// 4. Logs the response to SysLog
    /// 5. Returns (result_code, rich_result, response_data)
    ///
    /// # Invariant 9: Axiom Is the Single Syscall Gateway
    ///
    /// All syscalls follow: `Process → Axiom → Kernel → Axiom → Process`
    /// No bypass paths exist.
    pub fn process_syscall(
        &mut self,
        sender: ProcessId,
        syscall_num: u32,
        args: [u32; 4],
        data: &[u8],
    ) -> (i64, SyscallResult, Vec<u8>) {
        let timestamp = self.uptime_nanos();

        // 1. Log request to SysLog
        let req_id = self
            .axiom
            .syslog_mut()
            .log_request(sender.0, syscall_num, args, timestamp);

        // 2. Execute syscall via KernelCore
        let (result, commit_types) =
            execute_syscall_kernel_fn(&mut self.kernel, syscall_num, sender, args, data, timestamp);

        // 3. Record commits to CommitLog
        for ct in commit_types {
            self.axiom.append_internal_commit(ct, timestamp);
        }

        // 4. Get rich result and response data
        let (rich_result, response_data) =
            metrics::get_syscall_rich_result(&mut self.kernel, sender, syscall_num, args, data, result, timestamp);

        // 5. Log response to SysLog
        self.axiom
            .syslog_mut()
            .log_response(sender.0, req_id, result, timestamp);

        (result, rich_result, response_data)
    }

    // ========================================================================
    // Process Management (routed through Axiom)
    // ========================================================================

    /// Register a process and log the mutation.
    pub fn register_process(&mut self, name: &str) -> ProcessId {
        let timestamp = self.uptime_nanos();
        let (pid, commits) = self.kernel.register_process(name, timestamp);
        self.record_commits(commits, timestamp);
        pid
    }

    /// Register a process with a specific PID (for supervisor and special processes).
    pub fn register_process_with_pid(&mut self, pid: ProcessId, name: &str) -> ProcessId {
        let timestamp = self.uptime_nanos();
        let (result_pid, commits) = self.kernel.register_process_with_pid(pid, name, timestamp);
        self.record_commits(commits, timestamp);
        result_pid
    }

    /// Kill a process and log the mutation.
    pub fn kill_process(&mut self, pid: ProcessId) {
        let timestamp = self.uptime_nanos();
        let commits = self.kernel.kill_process(pid, timestamp);
        self.record_commits(commits, timestamp);
    }

    /// Record a process fault and terminate it.
    pub fn fault_process(&mut self, pid: ProcessId, reason: u32, description: alloc::string::String) {
        let timestamp = self.uptime_nanos();
        let commits = self.kernel.fault_process(pid, reason, description, timestamp);
        self.record_commits(commits, timestamp);
    }

    /// Get process info.
    pub fn get_process(&self, pid: ProcessId) -> Option<&Process> {
        self.kernel.get_process(pid)
    }

    /// List all processes.
    pub fn list_processes(&self) -> Vec<(ProcessId, &Process)> {
        self.kernel.list_processes()
    }

    // ========================================================================
    // Endpoint Management
    // ========================================================================

    /// Create an endpoint and log the mutation.
    pub fn create_endpoint(&mut self, owner: ProcessId) -> Result<(EndpointId, CapSlot), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.kernel.create_endpoint(owner, timestamp);
        self.record_commits(commits, timestamp);
        result
    }

    /// List all endpoints.
    pub fn list_endpoints(&self) -> Vec<EndpointInfo> {
        self.kernel.list_endpoints()
    }

    /// Get endpoint info.
    pub fn get_endpoint(&self, id: EndpointId) -> Option<&Endpoint> {
        self.kernel.get_endpoint(id)
    }

    /// Get detailed endpoint info.
    pub fn get_endpoint_detail(&self, id: EndpointId) -> Option<EndpointDetail> {
        self.kernel.get_endpoint_detail(id)
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
            self.kernel
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
        let (result, commits) = self.kernel.grant_capability_to_endpoint(
            owner_pid, endpoint_id, to_pid, perms, timestamp,
        );
        self.record_commits(commits, timestamp);
        result
    }

    /// Revoke capability and log the mutation.
    pub fn revoke_capability(&mut self, pid: ProcessId, slot: CapSlot) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.kernel.revoke_capability(pid, slot, timestamp);
        self.record_commits(commits, timestamp);
        result
    }

    /// Delete capability and log the mutation.
    pub fn delete_capability(&mut self, pid: ProcessId, slot: CapSlot) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.kernel.delete_capability(pid, slot, timestamp);
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
        let (result, commits) = self.kernel.derive_capability(pid, slot, new_perms, timestamp);
        self.record_commits(commits, timestamp);
        result
    }

    /// Get capability space for a process.
    pub fn get_cap_space(&self, pid: ProcessId) -> Option<&CapabilitySpace> {
        self.kernel.get_cap_space(pid)
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
        let (result, commit) = self.kernel.ipc_send(from_pid, endpoint_slot, tag, data, timestamp);
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
            self.kernel
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
        self.kernel.ipc_receive(pid, endpoint_slot, timestamp)
    }

    /// Receive IPC message with capability transfer.
    pub fn ipc_receive_with_caps(
        &mut self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
    ) -> Result<Option<(Message, Vec<CapSlot>)>, KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.kernel.ipc_receive_with_caps(pid, endpoint_slot, timestamp);
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
    #[deprecated(note = "Use ipc_send with proper capabilities instead. See method docs for migration path.")]
    pub fn send_to_process(
        &mut self,
        from_pid: ProcessId,
        to_pid: ProcessId,
        tag: u32,
        data: Vec<u8>,
    ) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        self.kernel.send_to_process(from_pid, to_pid, tag, data, timestamp)
    }

    // ========================================================================
    // Syscall Handling (higher-level API)
    // ========================================================================

    /// Handle a syscall from a process.
    pub fn handle_syscall(&mut self, pid: ProcessId, syscall: Syscall) -> SyscallResult {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.kernel.handle_syscall(pid, syscall, timestamp);
        self.record_commits(commits, timestamp);
        result
    }

    // ========================================================================
    // Memory Management
    // ========================================================================

    /// Allocate memory to a process.
    pub fn allocate_memory(&mut self, pid: ProcessId, bytes: usize) -> Result<usize, KernelError> {
        self.kernel.allocate_memory(pid, bytes)
    }

    /// Free memory from a process.
    pub fn free_memory(&mut self, pid: ProcessId, bytes: usize) -> Result<usize, KernelError> {
        self.kernel.free_memory(pid, bytes)
    }

    /// Update process memory size.
    pub fn update_process_memory(&mut self, pid: ProcessId, new_size: usize) {
        self.kernel.update_process_memory(pid, new_size)
    }

    // ========================================================================
    // Metrics and Monitoring
    // ========================================================================

    /// Get system-wide metrics.
    pub fn get_system_metrics(&self) -> SystemMetrics {
        self.kernel.get_system_metrics(self.uptime_nanos())
    }

    /// Get total system memory usage.
    pub fn total_memory(&self) -> usize {
        self.kernel.total_memory()
    }

    /// Get total message count in all endpoint queues.
    pub fn total_pending_messages(&self) -> usize {
        self.kernel.total_pending_messages()
    }

    // ========================================================================
    // CommitLog Access
    // ========================================================================

    /// Get reference to the commit log.
    pub fn commitlog(&self) -> &CommitLog {
        self.axiom.commitlog()
    }

    /// Get reference to the syslog.
    pub fn syslog(&self) -> &SysLog {
        self.axiom.syslog()
    }

    // ========================================================================
    // Private helpers
    // ========================================================================

    /// Record commits to the axiom gateway.
    fn record_commits(&mut self, commits: Vec<Commit>, timestamp: u64) {
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
    }
}

impl<H: HAL + Default> System<H> {
    /// Create a system for replay mode.
    pub fn new_for_replay() -> Self {
        let hal = H::default();
        Self {
            kernel: KernelCore::new(hal),
            axiom: AxiomGateway::new(0),
            boot_time: 0,
        }
    }
}

// ============================================================================
// Syscall Dispatch Implementation (moved from dispatch.rs)
// ============================================================================

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
        0x00..=0x07 => execute_basic_syscall(core, syscall_num, sender, args),
        0x11..=0x15 => {
            execute_process_syscall(core, syscall_num, sender, args, data, timestamp)
        }
        0x30 | 0x31 | 0x35 => execute_capability_syscall(core, syscall_num, sender, args, timestamp),
        0x40 | 0x41 => execute_ipc_syscall(core, syscall_num, sender, args, data, timestamp),
        0x70..=0x74 => execute_storage_syscall(core, syscall_num, sender, data),
        0x90 => execute_network_syscall(core, sender, data),
        _ => (-1, Vec::new()),
    }
}

fn execute_basic_syscall<H: HAL>(
    core: &KernelCore<H>,
    syscall_num: u32,
    sender: ProcessId,
    args: [u32; 4],
) -> (i64, Vec<CommitType>) {
    match syscall_num {
        0x00 => (0, Vec::new()),
        0x01 => (0, Vec::new()),
        0x02 => {
            let nanos = core.hal().now_nanos();
            let result = if args[0] == 0 {
                (nanos & 0xFFFFFFFF) as i64
            } else {
                ((nanos >> 32) & 0xFFFFFFFF) as i64
            };
            (result, Vec::new())
        }
        0x03 => (sender.0 as i64, Vec::new()),
        0x04 => (0, Vec::new()),
        0x05 => (0, Vec::new()),
        0x06 => {
            let millis = core.hal().wallclock_ms();
            let result = if args[0] == 0 {
                (millis & 0xFFFFFFFF) as i64
            } else {
                ((millis >> 32) & 0xFFFFFFFF) as i64
            };
            (result, Vec::new())
        }
        0x07 => (0, Vec::new()),
        _ => (-1, Vec::new()),
    }
}

fn execute_process_syscall<H: HAL>(
    core: &mut KernelCore<H>,
    syscall_num: u32,
    sender: ProcessId,
    args: [u32; 4],
    data: &[u8],
    timestamp: u64,
) -> (i64, Vec<CommitType>) {
    match syscall_num {
        0x11 => lifecycle::execute_exit(core, sender, timestamp),
        0x12 => (0, Vec::new()),
        0x13 => lifecycle::execute_kill_with_cap(core, sender, args, timestamp),
        0x14 => lifecycle::execute_register_process(core, sender, data, timestamp),
        0x15 => lifecycle::execute_create_endpoint_for(core, sender, args, timestamp),
        _ => (-1, Vec::new()),
    }
}

fn execute_capability_syscall<H: HAL>(
    core: &mut KernelCore<H>,
    syscall_num: u32,
    sender: ProcessId,
    args: [u32; 4],
    timestamp: u64,
) -> (i64, Vec<CommitType>) {
    match syscall_num {
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
        0x35 => {
            let (result, commits) = core.create_endpoint(sender, timestamp);
            let commit_types: Vec<CommitType> = commits.into_iter().map(|c| c.commit_type).collect();
            match result {
                Ok((eid, _slot)) => (eid.0 as i64, commit_types),
                Err(_) => (-1, commit_types),
            }
        }
        _ => (-1, Vec::new()),
    }
}

fn execute_ipc_syscall<H: HAL>(
    core: &mut KernelCore<H>,
    syscall_num: u32,
    sender: ProcessId,
    args: [u32; 4],
    data: &[u8],
    timestamp: u64,
) -> (i64, Vec<CommitType>) {
    match syscall_num {
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
        0x41 => {
            let slot = args[0];
            match core.ipc_has_message(sender, slot, timestamp) {
                Ok(true) => (1, Vec::new()),
                Ok(false) => (0, Vec::new()),
                Err(_) => (-1, Vec::new()),
            }
        }
        _ => (-1, Vec::new()),
    }
}

fn execute_storage_syscall<H: HAL>(
    core: &KernelCore<H>,
    syscall_num: u32,
    sender: ProcessId,
    data: &[u8],
) -> (i64, Vec<CommitType>) {
    match syscall_num {
        0x70 => execute_storage_read(core, sender, data),
        0x71 => execute_storage_write(core, sender, data),
        0x72 => execute_storage_delete(core, sender, data),
        0x73 => execute_storage_list(core, sender, data),
        0x74 => execute_storage_exists(core, sender, data),
        _ => (-1, Vec::new()),
    }
}

fn execute_storage_read<H: HAL>(
    core: &KernelCore<H>,
    sender: ProcessId,
    data: &[u8],
) -> (i64, Vec<CommitType>) {
    let key = match core::str::from_utf8(data) {
        Ok(k) => k,
        Err(_) => return (-1, Vec::new()),
    };
    match core.hal().storage_read_async(sender.0, key) {
        Ok(request_id) => (request_id as i64, Vec::new()),
        Err(_) => (-1, Vec::new()),
    }
}

fn execute_storage_write<H: HAL>(
    core: &KernelCore<H>,
    sender: ProcessId,
    data: &[u8],
) -> (i64, Vec<CommitType>) {
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

fn execute_storage_delete<H: HAL>(
    core: &KernelCore<H>,
    sender: ProcessId,
    data: &[u8],
) -> (i64, Vec<CommitType>) {
    let key = match core::str::from_utf8(data) {
        Ok(k) => k,
        Err(_) => return (-1, Vec::new()),
    };
    match core.hal().storage_delete_async(sender.0, key) {
        Ok(request_id) => (request_id as i64, Vec::new()),
        Err(_) => (-1, Vec::new()),
    }
}

fn execute_storage_list<H: HAL>(
    core: &KernelCore<H>,
    sender: ProcessId,
    data: &[u8],
) -> (i64, Vec<CommitType>) {
    let prefix = match core::str::from_utf8(data) {
        Ok(p) => p,
        Err(_) => return (-1, Vec::new()),
    };
    match core.hal().storage_list_async(sender.0, prefix) {
        Ok(request_id) => (request_id as i64, Vec::new()),
        Err(_) => (-1, Vec::new()),
    }
}

fn execute_storage_exists<H: HAL>(
    core: &KernelCore<H>,
    sender: ProcessId,
    data: &[u8],
) -> (i64, Vec<CommitType>) {
    let key = match core::str::from_utf8(data) {
        Ok(k) => k,
        Err(_) => return (-1, Vec::new()),
    };
    match core.hal().storage_exists_async(sender.0, key) {
        Ok(request_id) => (request_id as i64, Vec::new()),
        Err(_) => (-1, Vec::new()),
    }
}

fn execute_network_syscall<H: HAL>(
    core: &KernelCore<H>,
    sender: ProcessId,
    data: &[u8],
) -> (i64, Vec<CommitType>) {
    match core.hal().network_fetch_async(sender.0, data) {
        Ok(request_id) => (request_id as i64, Vec::new()),
        Err(_) => (-1, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zos_hal::TestHal;

    #[test]
    fn test_system_creation() {
        let system = System::new(TestHal::default());
        assert_eq!(system.syslog().len(), 0);
        assert_eq!(system.commitlog().len(), 1); // Genesis
    }

    #[test]
    fn test_system_process_registration() {
        let mut system = System::new(TestHal::default());
        let pid = system.register_process("test");
        assert!(pid.0 > 0);
        assert!(system.get_process(pid).is_some());
    }

    #[test]
    fn test_system_syscall_logs_to_syslog() {
        let mut system = System::new(TestHal::default());
        let pid = system.register_process("test");

        // Make a syscall
        let (_result, _rich, _data) = system.process_syscall(pid, 0x00, [0, 0, 0, 0], &[]);

        // SysLog should have request + response
        assert_eq!(system.syslog().len(), 2);
    }
}
