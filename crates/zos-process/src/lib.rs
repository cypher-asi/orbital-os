//! Process-side syscall library for Zero OS
//!
//! This crate provides the syscall interface that processes use to
//! communicate with the kernel. On WASM, this uses imported functions
//! that are provided by the JavaScript host.
//!
//! # Syscall Numbers
//!
//! This crate defines both canonical (new) and legacy syscall numbers.
//! The kernel accepts both, so existing code continues to work.
//!
//! See `docs/new-spec/02-kernel/06-syscalls.md` for the full ABI specification.

#![no_std]
extern crate alloc;

// ============================================================================
// Module Organization
// ============================================================================

pub mod syscalls;
pub mod types;

// ============================================================================
// Re-exports for Convenience
// ============================================================================

// Re-export syscall constants from zos-ipc (the single source of truth)
pub use zos_ipc::syscall::*;

// Syscall error codes
pub mod error {
    /// Success
    pub const E_OK: u32 = 0;
    /// Permission denied
    pub const E_PERM: u32 = 1;
    /// Object not found
    pub const E_NOENT: u32 = 2;
    /// Invalid argument
    pub const E_INVAL: u32 = 3;
    /// Syscall not implemented
    pub const E_NOSYS: u32 = 4;
    /// Would block (try again)
    pub const E_AGAIN: u32 = 5;
    /// Out of memory
    pub const E_NOMEM: u32 = 6;
    /// Invalid capability slot
    pub const E_BADF: u32 = 7;
    /// Resource busy
    pub const E_BUSY: u32 = 8;
    /// Already exists
    pub const E_EXIST: u32 = 9;
    /// Buffer overflow
    pub const E_OVERFLOW: u32 = 10;
}

// Re-export types
pub use types::{CapInfo, ObjectType, Permissions, ProcessInfo, ReceivedMessage};

// Re-export core syscalls
pub use syscalls::{
    call, cap_delete, cap_derive, cap_grant, cap_inspect, cap_revoke, cap_revoke_from,
    console_write, create_endpoint, create_endpoint_for, debug, exit, get_pid, get_time,
    get_wallclock, kill, list_caps, list_processes, receive, receive_blocking, register_process,
    reply, send, send_with_caps, yield_now,
};

// Re-export storage syscalls
pub use syscalls::storage::{
    storage_delete_async, storage_exists_async, storage_list_async, storage_read_async,
    storage_write_async,
};

// Re-export network syscalls
pub use syscalls::network::network_fetch_async;

// Re-export deprecated VFS syscalls
#[allow(deprecated)]
pub use syscalls::deprecated::{vfs_delete, vfs_exists, vfs_list, vfs_mkdir, vfs_read, vfs_write};

// ============================================================================
// IPC Message Constants (re-exported from zos-ipc)
// ============================================================================

// Re-export all IPC modules for convenient access
pub use zos_ipc::{
    console, diagnostics, identity_cred, identity_key, identity_machine, identity_perm,
    identity_prefs, identity_query, identity_remote, identity_session, identity_user, identity_zid,
    init, kernel, net, permission, pm, revoke_reason, slots, storage, supervisor, vfs_dir,
    vfs_file, vfs_meta, vfs_quota,
};

/// Console input message tag - used by terminal for receiving keyboard input.
pub use zos_ipc::MSG_CONSOLE_INPUT;

// =============================================================================
// Init Service Protocol (for service discovery)
// =============================================================================

/// Register a service with init: data = [name_len: u8, name: [u8], endpoint_id_low: u32, endpoint_id_high: u32]
pub use zos_ipc::init::MSG_REGISTER_SERVICE;

/// Lookup a service: data = [name_len: u8, name: [u8]]
pub use zos_ipc::init::MSG_LOOKUP_SERVICE;

/// Lookup response: data = [found: u8, endpoint_id_low: u32, endpoint_id_high: u32]
pub use zos_ipc::init::MSG_LOOKUP_RESPONSE;

/// Request spawn: data = [name_len: u8, name: [u8]]
pub use zos_ipc::init::MSG_SPAWN_SERVICE;

/// Spawn response: data = [success: u8, pid: u32]
pub use zos_ipc::init::MSG_SPAWN_RESPONSE;

/// Service ready notification (service → init after registration complete)
pub use zos_ipc::init::MSG_SERVICE_READY;

// =============================================================================
// Capability Revocation Notification (IPC → Process)
// =============================================================================

/// Notification that a capability was revoked from this process
/// Payload: [slot: u32, object_type: u8, object_id: u64, reason: u8]
pub use zos_ipc::kernel::MSG_CAP_REVOKED;

/// Revocation reason: Supervisor/user explicitly revoked the capability
pub const REVOKE_REASON_EXPLICIT: u8 = zos_ipc::revoke_reason::EXPLICIT;
/// Revocation reason: Capability expired
pub const REVOKE_REASON_EXPIRED: u8 = zos_ipc::revoke_reason::EXPIRED;
/// Revocation reason: Source process exited
pub const REVOKE_REASON_PROCESS_EXIT: u8 = zos_ipc::revoke_reason::PROCESS_EXIT;

/// Well-known slot for init's endpoint (every process gets this at spawn)
pub use zos_ipc::slots::INIT_ENDPOINT_SLOT;

// =============================================================================
// Storage Result IPC (delivered from supervisor via HAL async storage)
// =============================================================================

/// Storage operation result delivered via IPC
/// Payload format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
pub use zos_ipc::storage::MSG_STORAGE_RESULT;

/// Storage result types
pub mod storage_result {
    pub use zos_ipc::storage::result::*;
}

// =============================================================================
// Supervisor → Init Protocol (0x2xxx range)
// =============================================================================

/// Supervisor requests Init to deliver console input to a terminal process.
/// Payload: [target_pid: u32, endpoint_slot: u32, data_len: u16, data: [u8]]
pub use zos_ipc::supervisor::MSG_SUPERVISOR_CONSOLE_INPUT;

/// Supervisor requests Init to terminate a process.
/// Payload: [target_pid: u32]
pub use zos_ipc::supervisor::MSG_SUPERVISOR_KILL_PROCESS;

/// Supervisor requests Init to route an IPC message to a process.
/// Payload: [target_pid: u32, endpoint_slot: u32, tag: u32, data_len: u16, data: [u8]]
pub use zos_ipc::supervisor::MSG_SUPERVISOR_IPC_DELIVERY;

// =============================================================================
// Supervisor → PermissionManager Protocol
// =============================================================================

/// Supervisor requests PermissionManager to revoke a capability from a process.
/// Payload: [target_pid: u32, slot: u32, reason: u8]
pub use zos_ipc::supervisor::MSG_SUPERVISOR_REVOKE_CAP;

// =============================================================================
// Permission Protocol (03-security.md)
// =============================================================================

/// Request Init to grant a capability to a process
pub use zos_ipc::permission::MSG_GRANT_PERMISSION;

/// Request Init to revoke a capability from a process
pub use zos_ipc::permission::MSG_REVOKE_PERMISSION;

/// Query what permissions a process has
pub use zos_ipc::permission::MSG_LIST_PERMISSIONS;

/// Response from Init with grant/revoke result
pub use zos_ipc::permission::MSG_PERMISSION_RESPONSE;
