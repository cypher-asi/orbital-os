//! Syscall definitions and types
//!
//! This module contains:
//! - Canonical syscall number constants (ABI)
//! - Syscall enum for type-safe dispatch
//! - Syscall result types

use alloc::string::String;
use alloc::vec::Vec;

use crate::capability::{Capability, Permissions};
use crate::error::KernelError;
use crate::ipc::Message;
use crate::types::{ObjectType, ProcessId, ProcessState};
use zos_axiom::CapSlot;

// ============================================================================
// Canonical Syscall Numbers (ABI)
// ============================================================================

/// Debug print syscall
pub const SYS_DEBUG: u32 = 0x01;
/// Yield/cooperative scheduling hint
pub const SYS_YIELD: u32 = 0x02;
/// Exit process
pub const SYS_EXIT: u32 = 0x03;
/// Get current time (nanos since boot)
pub const SYS_TIME: u32 = 0x04;
/// Console write syscall - write text to console output
/// The supervisor receives a callback notification after this syscall completes.
pub const SYS_CONSOLE_WRITE: u32 = 0x07;

// Console input message tag (supervisor -> terminal input endpoint)
// Re-exported from zos-ipc (the single source of truth)
pub use zos_ipc::MSG_CONSOLE_INPUT;

// Capability revocation notification message tag (supervisor -> process input endpoint)
// Re-exported from zos-ipc (the single source of truth)
pub use zos_ipc::kernel::MSG_CAP_REVOKED;

/// Create an IPC endpoint
pub const SYS_CREATE_ENDPOINT: u32 = 0x11;
/// Delete an endpoint
pub const SYS_DELETE_ENDPOINT: u32 = 0x12;
/// Kill a process (requires Process capability with kill permission)
pub const SYS_KILL: u32 = 0x13;
/// Register a new process (Init-only syscall for spawn protocol)
pub const SYS_REGISTER_PROCESS: u32 = 0x14;
/// Create an endpoint for another process (Init-only syscall for spawn protocol)
pub const SYS_CREATE_ENDPOINT_FOR: u32 = 0x15;

/// Grant a capability to another process
pub const SYS_CAP_GRANT: u32 = 0x30;
/// Revoke a capability (requires grant permission)
pub const SYS_CAP_REVOKE: u32 = 0x31;
/// Delete a capability from own CSpace
pub const SYS_CAP_DELETE: u32 = 0x32;
/// Inspect a capability (get info)
pub const SYS_CAP_INSPECT: u32 = 0x33;
/// Derive a new capability with reduced permissions
pub const SYS_CAP_DERIVE: u32 = 0x34;
/// List all capabilities
pub const SYS_CAP_LIST: u32 = 0x35;

/// Send a message
pub const SYS_SEND: u32 = 0x40;
/// Receive a message
pub const SYS_RECV: u32 = 0x41;
/// Call (send + wait for reply)
pub const SYS_CALL: u32 = 0x42;
/// Reply to a call
pub const SYS_REPLY: u32 = 0x43;
/// Send with capability transfer
pub const SYS_SEND_CAP: u32 = 0x44;

/// List all processes (supervisor only)
pub const SYS_PS: u32 = 0x50;

/// Syscall request from a process
#[derive(Clone, Debug)]
pub enum Syscall {
    /// Print debug message (SYS_DEBUG 0x01)
    Debug { msg: String },
    /// Create a new IPC endpoint (SYS_CREATE_ENDPOINT 0x11)
    CreateEndpoint,
    /// Send a message to an endpoint (SYS_SEND 0x40)
    Send {
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
    },
    /// Receive a message from an endpoint (SYS_RECV 0x41)
    Receive { endpoint_slot: CapSlot },
    /// List this process's capabilities (SYS_CAP_LIST 0x35)
    ListCaps,
    /// List all processes (SYS_PS 0x50)
    ListProcesses,
    /// Exit process (SYS_EXIT 0x03)
    Exit { code: i32 },
    /// Get current time (SYS_TIME 0x04)
    GetTime,
    /// Yield CPU (SYS_YIELD 0x02)
    Yield,

    // === Capability syscalls ===
    /// Grant capability to another process (SYS_CAP_GRANT 0x30)
    CapGrant {
        from_slot: CapSlot,
        to_pid: ProcessId,
        permissions: Permissions,
    },
    /// Revoke a capability (SYS_CAP_REVOKE 0x31)
    CapRevoke { slot: CapSlot },
    /// Delete capability from own CSpace (SYS_CAP_DELETE 0x32)
    CapDelete { slot: CapSlot },
    /// Inspect a capability (SYS_CAP_INSPECT 0x33)
    CapInspect { slot: CapSlot },
    /// Derive capability with reduced permissions (SYS_CAP_DERIVE 0x34)
    CapDerive {
        slot: CapSlot,
        new_permissions: Permissions,
    },

    // === Enhanced IPC syscalls ===
    /// Send with capability transfer (SYS_SEND_CAP 0x44)
    SendWithCaps {
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
        cap_slots: Vec<CapSlot>,
    },
    /// Call (send + wait for reply) (SYS_CALL 0x42)
    Call {
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
    },
    /// Reply to a call (SYS_REPLY 0x43)
    Reply {
        caller_pid: ProcessId,
        tag: u32,
        data: Vec<u8>,
    },
    /// Kill a process (SYS_KILL 0x13 - requires Process capability)
    Kill { target_pid: ProcessId },
}

/// Information about a capability (returned by CapInspect)
#[derive(Clone, Debug)]
pub struct CapInfo {
    /// Capability ID
    pub id: u64,
    /// Object type
    pub object_type: ObjectType,
    /// Object ID
    pub object_id: u64,
    /// Permissions
    pub permissions: Permissions,
    /// Generation (for revocation)
    pub generation: u32,
    /// Expiration (0 = never)
    pub expires_at: u64,
}

impl From<&Capability> for CapInfo {
    fn from(cap: &Capability) -> Self {
        Self {
            id: cap.id,
            object_type: cap.object_type,
            object_id: cap.object_id,
            permissions: cap.permissions,
            generation: cap.generation,
            expires_at: cap.expires_at,
        }
    }
}

/// Information about a revoked capability for notification delivery
#[derive(Clone, Debug)]
pub struct RevokeNotification {
    /// Process ID of the affected process
    pub pid: ProcessId,
    /// Capability slot that was revoked
    pub slot: CapSlot,
    /// Object type of the revoked capability
    pub object_type: u8,
    /// Object ID of the revoked capability
    pub object_id: u64,
    /// Reason for revocation
    pub reason: u8,
}

impl RevokeNotification {
    /// Create an empty notification (for cases where cap didn't exist)
    pub fn empty() -> Self {
        Self {
            pid: ProcessId(0),
            slot: 0,
            object_type: 0,
            object_id: 0,
            reason: 0,
        }
    }

    /// Check if this notification has valid data
    pub fn is_valid(&self) -> bool {
        self.object_type != 0
    }
}

/// Syscall result
#[derive(Clone, Debug)]
pub enum SyscallResult {
    /// Success with optional value
    Ok(u64),
    /// Error occurred
    Err(KernelError),
    /// Message received
    Message(Message),
    /// Message received with installed capability slots
    MessageWithCaps(Message, Vec<CapSlot>),
    /// Would block (no message available)
    WouldBlock,
    /// Capability info (from inspect)
    CapInfo(CapInfo),
    /// Capability list
    CapList(Vec<(CapSlot, Capability)>),
    /// Process list
    ProcessList(Vec<(ProcessId, String, ProcessState)>),
}
