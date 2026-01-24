//! KernelCore implementation - modular organization of kernel state and methods.
//!
//! This module contains the core kernel state and its method implementations,
//! split into logical submodules:
//!
//! - `process` - Process lifecycle (register, kill, fault)
//! - `endpoint` - Endpoint management (create, list, get)
//! - `capability` - Capability operations (grant, revoke, derive, delete)
//! - `ipc` - IPC send/receive operations
//! - `syscall` - Syscall dispatch and handling

mod capability;
mod endpoint;
mod ipc;
mod process;
mod syscall;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::error::KernelError;
use crate::ipc::Endpoint;
use crate::types::{EndpointId, Process, ProcessId, SystemMetrics};
use crate::{AxiomError, CapabilitySpace};
use zos_hal::HAL;

/// The kernel core holds all mutable state.
///
/// All mutation methods on KernelCore return `(Result, Vec<Commit>)` where
/// the commits describe the state mutations that occurred. The caller is
/// responsible for appending these commits to the CommitLog via AxiomGateway.
///
/// This pattern ensures all state-mutating operations flow through AxiomGateway,
/// making Axiom-bypass violations impossible at compile time.
pub struct KernelCore<H: HAL> {
    /// HAL reference for debug output only (no state changes)
    hal: H,
    /// Process table
    pub(crate) processes: BTreeMap<ProcessId, Process>,
    /// Capability spaces (per-process)
    pub(crate) cap_spaces: BTreeMap<ProcessId, CapabilitySpace>,
    /// IPC endpoints
    pub(crate) endpoints: BTreeMap<EndpointId, Endpoint>,
    /// Next process ID
    pub(crate) next_pid: u64,
    /// Next endpoint ID
    pub(crate) next_endpoint_id: u64,
    /// Next capability ID
    pub(crate) next_cap_id: u64,
    /// Total IPC messages since boot
    pub(crate) total_ipc_count: u64,
}

impl<H: HAL> KernelCore<H> {
    /// Create a new kernel core with the given HAL
    pub fn new(hal: H) -> Self {
        Self {
            hal,
            processes: BTreeMap::new(),
            cap_spaces: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            next_pid: 1,
            next_endpoint_id: 1,
            next_cap_id: 1,
            total_ipc_count: 0,
        }
    }

    /// Get a reference to the HAL (for debug output)
    pub fn hal(&self) -> &H {
        &self.hal
    }

    /// Generate next capability ID
    pub(crate) fn next_cap_id(&mut self) -> u64 {
        let id = self.next_cap_id;
        self.next_cap_id += 1;
        id
    }

    // ========================================================================
    // Read-only accessors
    // ========================================================================

    /// Get process info
    pub fn get_process(&self, pid: ProcessId) -> Option<&Process> {
        self.processes.get(&pid)
    }

    /// Get mutable process info
    pub fn get_process_mut(&mut self, pid: ProcessId) -> Option<&mut Process> {
        self.processes.get_mut(&pid)
    }

    /// Get all processes
    pub fn list_processes(&self) -> Vec<(ProcessId, &Process)> {
        self.processes.iter().map(|(&pid, p)| (pid, p)).collect()
    }

    /// Get capability space for a process
    pub fn get_cap_space(&self, pid: ProcessId) -> Option<&CapabilitySpace> {
        self.cap_spaces.get(&pid)
    }

    /// Get total system memory usage
    pub fn total_memory(&self) -> usize {
        self.processes.values().map(|p| p.metrics.memory_size).sum()
    }

    /// Get total message count in all endpoint queues
    pub fn total_pending_messages(&self) -> usize {
        self.endpoints
            .values()
            .map(|e| e.pending_messages.len())
            .sum()
    }

    /// Get system-wide metrics
    pub fn get_system_metrics(&self, uptime_ns: u64) -> SystemMetrics {
        SystemMetrics {
            process_count: self.processes.len(),
            total_memory: self.total_memory(),
            endpoint_count: self.endpoints.len(),
            total_pending_messages: self.total_pending_messages(),
            total_ipc_messages: self.total_ipc_count,
            uptime_ns,
        }
    }

    // ========================================================================
    // Memory management helpers
    // ========================================================================

    /// Allocate memory to a process (simulated)
    pub fn allocate_memory(&mut self, pid: ProcessId, bytes: usize) -> Result<usize, KernelError> {
        let proc = self
            .processes
            .get_mut(&pid)
            .ok_or(KernelError::ProcessNotFound)?;
        proc.metrics.memory_size += bytes;
        self.hal.debug_write(&alloc::format!(
            "[kernel] PID {} allocated {} bytes (total: {} bytes)",
            pid.0,
            bytes,
            proc.metrics.memory_size
        ));
        Ok(proc.metrics.memory_size)
    }

    /// Free memory from a process (simulated)
    pub fn free_memory(&mut self, pid: ProcessId, bytes: usize) -> Result<usize, KernelError> {
        let proc = self
            .processes
            .get_mut(&pid)
            .ok_or(KernelError::ProcessNotFound)?;
        proc.metrics.memory_size = proc.metrics.memory_size.saturating_sub(bytes);
        self.hal.debug_write(&alloc::format!(
            "[kernel] PID {} freed {} bytes (total: {} bytes)",
            pid.0,
            bytes,
            proc.metrics.memory_size
        ));
        Ok(proc.metrics.memory_size)
    }

    /// Update process memory size (called when WASM memory grows)
    pub fn update_process_memory(&mut self, pid: ProcessId, new_size: usize) {
        if let Some(proc) = self.processes.get_mut(&pid) {
            proc.metrics.memory_size = new_size;
        }
    }
}

/// Helper for mapping AxiomError to KernelError
pub(crate) fn map_axiom_error(e: AxiomError) -> KernelError {
    match e {
        AxiomError::InvalidSlot => KernelError::InvalidCapability,
        AxiomError::WrongType => KernelError::InvalidCapability,
        AxiomError::InsufficientRights => KernelError::PermissionDenied,
        AxiomError::Expired => KernelError::PermissionDenied,
        AxiomError::ObjectNotFound => KernelError::InvalidCapability,
    }
}
