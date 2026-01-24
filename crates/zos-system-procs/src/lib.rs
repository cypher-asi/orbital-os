//! System Processes for Zero OS
//!
//! This crate provides system processes that ship with Zero OS:
//! - Memory allocation and isolation testing
//! - IPC throughput and latency benchmarking
//! - Process lifecycle utilities

#![no_std]
extern crate alloc;

// Note: Syscall implementations are imported from JavaScript host via zero-process crate

use alloc::vec::Vec;

/// System process types that can be spawned
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemProcType {
    /// Allocates memory on command, reports real usage
    MemoryHog,
    /// Sends configurable message bursts
    Sender,
    /// Receives and counts messages
    Receiver,
    /// Measures IPC round-trip latency
    PingPong,
    /// Does nothing (baseline for measurements)
    Idle,
}

impl SystemProcType {
    /// Get the process type from a string name
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "memhog" | "memory_hog" | "memoryhog" => Some(Self::MemoryHog),
            "sender" => Some(Self::Sender),
            "receiver" => Some(Self::Receiver),
            "pingpong" | "ping_pong" | "ping-pong" => Some(Self::PingPong),
            "idle" => Some(Self::Idle),
            _ => None,
        }
    }

    /// Get the name of the process type
    pub fn name(&self) -> &'static str {
        match self {
            Self::MemoryHog => "memhog",
            Self::Sender => "sender",
            Self::Receiver => "receiver",
            Self::PingPong => "pingpong",
            Self::Idle => "idle",
        }
    }

    /// Get a description of the process type
    pub fn description(&self) -> &'static str {
        match self {
            Self::MemoryHog => "Allocates memory on command, reports usage",
            Self::Sender => "Sends configurable message bursts",
            Self::Receiver => "Receives and counts messages",
            Self::PingPong => "Measures IPC round-trip latency",
            Self::Idle => "Does nothing (baseline)",
        }
    }
}

// ============================================================================
// Command Tags for Test Processes
// ============================================================================

/// Command to allocate memory (arg: size in bytes as u32 little-endian)
pub const CMD_ALLOC: u32 = 0x1001;
/// Command to free the last allocation
pub const CMD_FREE: u32 = 0x1002;
/// Command to free all allocations
pub const CMD_FREE_ALL: u32 = 0x1003;
/// Command to query current state
pub const CMD_QUERY: u32 = 0x1004;
/// Command to exit the process
pub const CMD_EXIT: u32 = 0x1005;
/// Command to reset statistics
pub const CMD_RESET: u32 = 0x1006;

/// Command to send a burst of messages
/// Data: [count: u32, size: u32, target_endpoint: u32]
pub const CMD_SEND_BURST: u32 = 0x2001;

/// Command to start ping test
/// Data: [target_endpoint: u32, iterations: u32]
pub const CMD_PING: u32 = 0x3001;
/// Command to switch to pong responder mode
pub const CMD_PONG_MODE: u32 = 0x3002;

// ============================================================================
// Response Tags
// ============================================================================

// Re-export diagnostic message constants from zos-ipc (the single source of truth)
pub use zos_process::diagnostics::{
    MSG_MEMORY_STATUS,
    MSG_SENDER_STATS,
    MSG_RECEIVER_STATS,
    MSG_LATENCY_STATS,
    MSG_PING,
    MSG_PONG,
    MSG_DATA,
};

// ============================================================================
// Statistics Structures
// ============================================================================

/// Memory status reported by memhog process
#[repr(C)]
#[derive(Clone, Debug, Default)]
pub struct MemoryStatus {
    /// Bytes allocated by our code
    pub allocated_by_us: u64,
    /// Number of allocations
    pub allocation_count: u32,
    /// Reserved for future use
    pub _reserved: u32,
}

impl MemoryStatus {
    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(&self.allocated_by_us.to_le_bytes());
        bytes.extend_from_slice(&self.allocation_count.to_le_bytes());
        bytes.extend_from_slice(&self._reserved.to_le_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 16 {
            return None;
        }
        Some(Self {
            allocated_by_us: u64::from_le_bytes(bytes[0..8].try_into().ok()?),
            allocation_count: u32::from_le_bytes(bytes[8..12].try_into().ok()?),
            _reserved: u32::from_le_bytes(bytes[12..16].try_into().ok()?),
        })
    }
}

/// Sender statistics
#[repr(C)]
#[derive(Clone, Debug, Default)]
pub struct SenderStats {
    /// Total messages sent
    pub messages_sent: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Elapsed time in nanoseconds
    pub elapsed_nanos: u64,
    /// Calculated messages per second
    pub msgs_per_sec: u64,
}

impl SenderStats {
    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32);
        bytes.extend_from_slice(&self.messages_sent.to_le_bytes());
        bytes.extend_from_slice(&self.bytes_sent.to_le_bytes());
        bytes.extend_from_slice(&self.elapsed_nanos.to_le_bytes());
        bytes.extend_from_slice(&self.msgs_per_sec.to_le_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 32 {
            return None;
        }
        Some(Self {
            messages_sent: u64::from_le_bytes(bytes[0..8].try_into().ok()?),
            bytes_sent: u64::from_le_bytes(bytes[8..16].try_into().ok()?),
            elapsed_nanos: u64::from_le_bytes(bytes[16..24].try_into().ok()?),
            msgs_per_sec: u64::from_le_bytes(bytes[24..32].try_into().ok()?),
        })
    }
}

/// Receiver statistics
#[repr(C)]
#[derive(Clone, Debug, Default)]
pub struct ReceiverStats {
    /// Total messages received
    pub messages_received: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Time of first message (nanos since start)
    pub first_msg_time: u64,
    /// Time of last message (nanos since start)
    pub last_msg_time: u64,
}

impl ReceiverStats {
    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32);
        bytes.extend_from_slice(&self.messages_received.to_le_bytes());
        bytes.extend_from_slice(&self.bytes_received.to_le_bytes());
        bytes.extend_from_slice(&self.first_msg_time.to_le_bytes());
        bytes.extend_from_slice(&self.last_msg_time.to_le_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 32 {
            return None;
        }
        Some(Self {
            messages_received: u64::from_le_bytes(bytes[0..8].try_into().ok()?),
            bytes_received: u64::from_le_bytes(bytes[8..16].try_into().ok()?),
            first_msg_time: u64::from_le_bytes(bytes[16..24].try_into().ok()?),
            last_msg_time: u64::from_le_bytes(bytes[24..32].try_into().ok()?),
        })
    }
}

/// Latency statistics from ping-pong test
#[repr(C)]
#[derive(Clone, Debug, Default)]
pub struct LatencyStats {
    /// Number of iterations
    pub iterations: u64,
    /// Minimum latency in nanoseconds
    pub min_ns: u64,
    /// Maximum latency in nanoseconds
    pub max_ns: u64,
    /// Average latency in nanoseconds
    pub avg_ns: u64,
    /// Median latency in nanoseconds
    pub median_ns: u64,
}

impl LatencyStats {
    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(40);
        bytes.extend_from_slice(&self.iterations.to_le_bytes());
        bytes.extend_from_slice(&self.min_ns.to_le_bytes());
        bytes.extend_from_slice(&self.max_ns.to_le_bytes());
        bytes.extend_from_slice(&self.avg_ns.to_le_bytes());
        bytes.extend_from_slice(&self.median_ns.to_le_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 40 {
            return None;
        }
        Some(Self {
            iterations: u64::from_le_bytes(bytes[0..8].try_into().ok()?),
            min_ns: u64::from_le_bytes(bytes[8..16].try_into().ok()?),
            max_ns: u64::from_le_bytes(bytes[16..24].try_into().ok()?),
            avg_ns: u64::from_le_bytes(bytes[24..32].try_into().ok()?),
            median_ns: u64::from_le_bytes(bytes[32..40].try_into().ok()?),
        })
    }

    /// Format as human-readable string
    pub fn format(&self) -> alloc::string::String {
        use alloc::format;
        format!(
            "Iterations: {}, Min: {}ns, Max: {}ns, Avg: {}ns, Median: {}ns",
            self.iterations, self.min_ns, self.max_ns, self.avg_ns, self.median_ns
        )
    }
}
