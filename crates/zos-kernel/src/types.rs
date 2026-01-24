//! Core kernel types
//!
//! This module contains the fundamental types used throughout the kernel:
//! - Process and endpoint identifiers
//! - Process state and metrics
//! - System-wide metrics

use alloc::string::String;

// Re-export types from zos-axiom to maintain backwards compatibility
pub use zos_axiom::{CapSlot, ObjectType};

/// Process identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

/// IPC endpoint identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EndpointId(pub u64);

/// Process state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    /// Process is running
    Running,
    /// Process is blocked waiting for IPC
    Blocked,
    /// Process has exited
    Zombie,
}

/// Process descriptor
pub struct Process {
    /// Process ID
    pub pid: ProcessId,
    /// Process name
    pub name: String,
    /// Current state
    pub state: ProcessState,
    /// Detailed metrics for this process
    pub metrics: ProcessMetrics,
}

/// Per-process resource tracking
#[derive(Clone, Debug, Default)]
pub struct ProcessMetrics {
    /// Memory size (bytes)
    pub memory_size: usize,
    /// Messages sent
    pub ipc_sent: u64,
    /// Messages received
    pub ipc_received: u64,
    /// Bytes sent via IPC
    pub ipc_bytes_sent: u64,
    /// Bytes received via IPC
    pub ipc_bytes_received: u64,
    /// Syscalls made
    pub syscall_count: u64,
    /// Time of last activity (nanos since boot)
    pub last_active_ns: u64,
    /// Process start time (nanos since boot)
    pub start_time_ns: u64,
}

/// Per-endpoint tracking
#[derive(Clone, Debug, Default)]
pub struct EndpointMetrics {
    /// Messages currently queued
    pub queue_depth: usize,
    /// Total messages ever sent to this endpoint
    pub total_messages: u64,
    /// Total bytes received
    pub total_bytes: u64,
    /// High water mark (max queue depth seen)
    pub queue_high_water: usize,
}

/// System-wide metrics
#[derive(Clone, Debug)]
pub struct SystemMetrics {
    /// Process count
    pub process_count: usize,
    /// Total memory across all processes
    pub total_memory: usize,
    /// Endpoint count
    pub endpoint_count: usize,
    /// Total pending messages
    pub total_pending_messages: usize,
    /// Total IPC messages since boot
    pub total_ipc_messages: u64,
    /// Uptime in nanoseconds
    pub uptime_ns: u64,
}
