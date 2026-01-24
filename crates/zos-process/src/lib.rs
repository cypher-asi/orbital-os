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

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ============================================================================
// Canonical Syscall Numbers (new ABI)
// ============================================================================

/// Syscall number ranges:
/// - 0x00-0x0F: Misc (debug, info, time)
/// - 0x10-0x1F: Thread (create, exit, yield, sleep)
/// - 0x20-0x2F: Memory (map, unmap, protect)
/// - 0x30-0x3F: Capability (grant, revoke, transfer)
/// - 0x40-0x4F: IPC (send, receive, call, reply)
/// - 0x50-0x5F: IRQ (register, ack, mask)
/// - 0x60-0x6F: I/O (port read/write)
pub mod syscall {
    // === Misc (0x00 - 0x0F) ===
    /// Print debug message
    pub const SYS_DEBUG: u32 = 0x01;
    /// Get current time (arg: 0=low32, 1=high32)
    pub const SYS_GET_TIME: u32 = 0x02;
    /// Get own process ID
    pub const SYS_GET_PID: u32 = 0x03;
    /// List capabilities
    pub const SYS_LIST_CAPS: u32 = 0x04;
    /// List processes
    pub const SYS_LIST_PROCS: u32 = 0x05;
    /// Get wall-clock time in milliseconds since Unix epoch (arg: 0=low32, 1=high32)
    pub const SYS_GET_WALLCLOCK: u32 = 0x06;
    /// Write to console output (for terminal/shell output)
    /// The supervisor receives a notification callback after this syscall completes.
    pub const SYS_CONSOLE_WRITE: u32 = 0x07;

    // === Thread (0x10 - 0x1F) ===
    /// Create a new thread
    pub const SYS_THREAD_CREATE: u32 = 0x10;
    /// Exit current thread/process
    pub const SYS_EXIT: u32 = 0x11;
    /// Yield to scheduler
    pub const SYS_YIELD: u32 = 0x12;
    /// Kill a process (requires Process capability)
    pub const SYS_KILL: u32 = 0x13;
    /// Register a new process (Init-only syscall for spawn protocol)
    pub const SYS_REGISTER_PROCESS: u32 = 0x14;
    /// Create an endpoint for another process (Init-only syscall for spawn protocol)
    pub const SYS_CREATE_ENDPOINT_FOR: u32 = 0x15;
    /// Wait for thread to exit (reusing 0x16 to avoid conflict)
    pub const SYS_THREAD_JOIN: u32 = 0x16;
    /// Sleep for specified nanoseconds (legacy, now 0x15)
    pub const SYS_SLEEP: u32 = 0x15;

    // === Memory (0x20 - 0x2F) ===
    /// Map memory region
    pub const SYS_MMAP: u32 = 0x20;
    /// Unmap memory region
    pub const SYS_MUNMAP: u32 = 0x21;
    /// Change memory protection
    pub const SYS_MPROTECT: u32 = 0x22;

    // === Capability (0x30 - 0x3F) ===
    /// Grant capability to another process
    pub const SYS_CAP_GRANT: u32 = 0x30;
    /// Revoke a capability
    pub const SYS_CAP_REVOKE: u32 = 0x31;
    /// Delete a capability from own CSpace
    pub const SYS_CAP_DELETE: u32 = 0x32;
    /// Inspect a capability
    pub const SYS_CAP_INSPECT: u32 = 0x33;
    /// Derive a capability with reduced permissions
    pub const SYS_CAP_DERIVE: u32 = 0x34;
    /// Create an IPC endpoint
    pub const SYS_EP_CREATE: u32 = 0x35;

    // === IPC (0x40 - 0x4F) ===
    /// Send message to endpoint
    pub const SYS_SEND: u32 = 0x40;
    /// Receive message from endpoint
    pub const SYS_RECEIVE: u32 = 0x41;
    /// Send and wait for reply (RPC)
    pub const SYS_CALL: u32 = 0x42;
    /// Reply to a call
    pub const SYS_REPLY: u32 = 0x43;
    /// Send message with capabilities
    pub const SYS_SEND_CAP: u32 = 0x44;

    // === IRQ (0x50 - 0x5F) ===
    /// Register IRQ handler
    pub const SYS_IRQ_REGISTER: u32 = 0x50;
    /// Acknowledge IRQ
    pub const SYS_IRQ_ACK: u32 = 0x51;
    /// Mask IRQ
    pub const SYS_IRQ_MASK: u32 = 0x52;
    /// Unmask IRQ
    pub const SYS_IRQ_UNMASK: u32 = 0x53;

    // === I/O (0x60 - 0x6F) ===
    /// Read byte from I/O port
    pub const SYS_IO_IN8: u32 = 0x60;
    /// Read word from I/O port
    pub const SYS_IO_IN16: u32 = 0x61;
    /// Read dword from I/O port
    pub const SYS_IO_IN32: u32 = 0x62;
    /// Write byte to I/O port
    pub const SYS_IO_OUT8: u32 = 0x63;
    /// Write word to I/O port
    pub const SYS_IO_OUT16: u32 = 0x64;
    /// Write dword to I/O port
    pub const SYS_IO_OUT32: u32 = 0x65;

    // === Platform Storage (0x70 - 0x7F) ===
    // These are HAL-level key-value storage operations, NOT filesystem operations.
    // VfsService (userspace) uses these syscalls for persistence to IndexedDB/disk.
    // Applications should use zos_vfs::VfsClient for filesystem operations.
    //
    // All storage syscalls are ASYNC and return a request_id immediately.
    // The result is delivered via IPC to the requesting process.
    
    /// Read blob from platform storage (async - returns request_id)
    /// Args: key_len in data buffer
    /// Returns: request_id (response delivered via IPC with data)
    pub const SYS_STORAGE_READ: u32 = 0x70;
    
    /// Write blob to platform storage (async - returns request_id)
    /// Args: key_len, value_len in data buffer (key then value)
    /// Returns: request_id (response delivered via IPC with success/error)
    pub const SYS_STORAGE_WRITE: u32 = 0x71;
    
    /// Delete blob from platform storage (async - returns request_id)
    /// Args: key_len in data buffer
    /// Returns: request_id (response delivered via IPC with success/error)
    pub const SYS_STORAGE_DELETE: u32 = 0x72;
    
    /// List keys with prefix (async - returns request_id)
    /// Args: prefix_len in data buffer
    /// Returns: request_id (response delivered via IPC with key list)
    pub const SYS_STORAGE_LIST: u32 = 0x73;
    
    /// Check if key exists (async - returns request_id)
    /// Args: key_len in data buffer
    /// Returns: request_id (response delivered via IPC with bool)
    pub const SYS_STORAGE_EXISTS: u32 = 0x74;

    // =========================================================================
    // Deprecated VFS syscalls (kept for backward compatibility)
    // These are superseded by the VFS IPC service (zos_vfs::VfsClient)
    // =========================================================================

    /// DEPRECATED: Read file via kernel VFS
    #[deprecated(since = "0.1.0", note = "Use VFS IPC service via zos_vfs::VfsClient")]
    pub const SYS_VFS_READ: u32 = 0x80;

    /// DEPRECATED: Write file via kernel VFS
    #[deprecated(since = "0.1.0", note = "Use VFS IPC service via zos_vfs::VfsClient")]
    pub const SYS_VFS_WRITE: u32 = 0x81;

    /// DEPRECATED: Create directory via kernel VFS
    #[deprecated(since = "0.1.0", note = "Use VFS IPC service via zos_vfs::VfsClient")]
    pub const SYS_VFS_MKDIR: u32 = 0x82;

    /// DEPRECATED: List directory via kernel VFS
    #[deprecated(since = "0.1.0", note = "Use VFS IPC service via zos_vfs::VfsClient")]
    pub const SYS_VFS_LIST: u32 = 0x83;

    /// DEPRECATED: Delete file/directory via kernel VFS
    #[deprecated(since = "0.1.0", note = "Use VFS IPC service via zos_vfs::VfsClient")]
    pub const SYS_VFS_DELETE: u32 = 0x84;

    /// DEPRECATED: Check if path exists via kernel VFS
    #[deprecated(since = "0.1.0", note = "Use VFS IPC service via zos_vfs::VfsClient")]
    pub const SYS_VFS_EXISTS: u32 = 0x85;
}

// Re-export canonical syscalls at module level for convenience
pub use syscall::*;

// ============================================================================
// Error Codes
// ============================================================================

/// Syscall error codes.
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

// ============================================================================
// Message Tags (for IPC protocol)
// ============================================================================
// All IPC message constants are defined in zos-ipc as the single source of truth.
// We re-export them here for backward compatibility.

// Re-export all IPC modules for convenient access
pub use zos_ipc::{
    console, diagnostics, identity_cred, identity_key, identity_machine, identity_perm,
    identity_query, identity_remote, identity_session, identity_user, init, kernel, permission,
    pm, revoke_reason, slots, storage, supervisor, vfs_dir, vfs_file, vfs_meta, vfs_quota,
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
// These messages are sent from the supervisor to Init to route operations
// that need kernel access. The supervisor has no direct kernel access.

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
// Messages for permission management (Desktop/Supervisor -> Init)
// =============================================================================

/// Request Init to grant a capability to a process
pub use zos_ipc::permission::MSG_GRANT_PERMISSION;

/// Request Init to revoke a capability from a process
pub use zos_ipc::permission::MSG_REVOKE_PERMISSION;

/// Query what permissions a process has
pub use zos_ipc::permission::MSG_LIST_PERMISSIONS;

/// Response from Init with grant/revoke result
pub use zos_ipc::permission::MSG_PERMISSION_RESPONSE;

// =============================================================================
// Object Types (for capabilities)
// =============================================================================

/// Types of kernel objects that can be accessed via capabilities
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ObjectType {
    /// IPC endpoint
    Endpoint = 1,
    /// Console I/O
    Console = 2,
    /// Persistent storage (namespaced per-app)
    Storage = 3,
    /// Network access
    Network = 4,
    /// Process management (spawn, kill)
    Process = 5,
    /// Memory region
    Memory = 6,
}

impl ObjectType {
    /// Convert from u8
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(ObjectType::Endpoint),
            2 => Some(ObjectType::Console),
            3 => Some(ObjectType::Storage),
            4 => Some(ObjectType::Network),
            5 => Some(ObjectType::Process),
            6 => Some(ObjectType::Memory),
            _ => None,
        }
    }

    /// Get display name
    pub fn name(&self) -> &'static str {
        match self {
            ObjectType::Endpoint => "Endpoint",
            ObjectType::Console => "Console",
            ObjectType::Storage => "Storage",
            ObjectType::Network => "Network",
            ObjectType::Process => "Process",
            ObjectType::Memory => "Memory",
        }
    }
}

// ============================================================================
// External functions (provided by JavaScript host)
// ============================================================================

#[cfg(target_arch = "wasm32")]
extern "C" {
    /// Make a syscall to the kernel
    /// Returns a handle that can be used to retrieve the result
    fn zos_syscall(syscall_num: u32, arg1: u32, arg2: u32, arg3: u32) -> u32;

    /// Send bytes to the kernel (for syscall data)
    fn zos_send_bytes(ptr: *const u8, len: u32);

    /// Get bytes from the kernel (for syscall results)
    /// Returns the number of bytes written
    fn zos_recv_bytes(ptr: *mut u8, max_len: u32) -> u32;

    /// Yield to allow other processes to run
    fn zos_yield();

    /// Get the process's assigned PID
    fn zos_get_pid() -> u32;
}

// ============================================================================
// Syscall Wrappers
// ============================================================================

/// Get this process's PID
#[cfg(target_arch = "wasm32")]
pub fn get_pid() -> u32 {
    unsafe { zos_get_pid() }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn get_pid() -> u32 {
    0 // Mock for non-WASM
}

/// Print a debug message
#[cfg(target_arch = "wasm32")]
pub fn debug(msg: &str) {
    let bytes = msg.as_bytes();
    unsafe {
        zos_send_bytes(bytes.as_ptr(), bytes.len() as u32);
        zos_syscall(SYS_DEBUG, bytes.len() as u32, 0, 0);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn debug(_msg: &str) {
    // No-op for non-WASM
}

/// Write to console output (for terminal/shell output)
///
/// Unlike `debug()`, this is for user-visible console output that goes through
/// the supervisor to the UI. The supervisor receives a notification callback
/// after this syscall completes.
#[cfg(target_arch = "wasm32")]
pub fn console_write(text: &str) {
    let bytes = text.as_bytes();
    unsafe {
        zos_send_bytes(bytes.as_ptr(), bytes.len() as u32);
        zos_syscall(SYS_CONSOLE_WRITE, bytes.len() as u32, 0, 0);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn console_write(_text: &str) {
    // No-op for non-WASM
}

/// Send a message to an endpoint
#[cfg(target_arch = "wasm32")]
pub fn send(endpoint_slot: u32, tag: u32, data: &[u8]) -> Result<(), u32> {
    unsafe {
        zos_send_bytes(data.as_ptr(), data.len() as u32);
        let result = zos_syscall(SYS_SEND, endpoint_slot, tag, data.len() as u32);
        if result == 0 {
            Ok(())
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn send(_endpoint_slot: u32, _tag: u32, _data: &[u8]) -> Result<(), u32> {
    Ok(())
}

/// Receive a message from an endpoint (non-blocking)
#[cfg(target_arch = "wasm32")]
pub fn receive(endpoint_slot: u32) -> Option<ReceivedMessage> {
    let mut buffer = [0u8; 4096];
    unsafe {
        let result = zos_syscall(SYS_RECEIVE, endpoint_slot, 0, 0) as i32;
        if result <= 0 {
            // 0 = no message, negative = error (e.g., permission denied)
            return None;
        }
        // Get the message data
        let len = zos_recv_bytes(buffer.as_mut_ptr(), buffer.len() as u32);
        if len == 0 {
            return None;
        }
        // Parse message format:
        // [from_pid: u32][tag: u32][num_caps: u8][cap_slots: u32*num_caps][data: ...]
        // Minimum: 4 + 4 + 1 = 9 bytes
        if len < 9 {
            return None;
        }
        let from_pid = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        let tag = u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
        let num_caps = buffer[8] as usize;
        
        // Parse capability slots
        let cap_data_len = num_caps * 4;
        let data_start = 9 + cap_data_len;
        if (len as usize) < data_start {
            return None;
        }
        
        let mut cap_slots = Vec::with_capacity(num_caps);
        for i in 0..num_caps {
            let offset = 9 + i * 4;
            let slot = u32::from_le_bytes([
                buffer[offset],
                buffer[offset + 1],
                buffer[offset + 2],
                buffer[offset + 3],
            ]);
            cap_slots.push(slot);
        }
        
        let data = buffer[data_start..len as usize].to_vec();
        Some(ReceivedMessage {
            from_pid,
            tag,
            cap_slots,
            data,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn receive(_endpoint_slot: u32) -> Option<ReceivedMessage> {
    None
}

/// Receive a message, blocking until one arrives
#[cfg(target_arch = "wasm32")]
pub fn receive_blocking(endpoint_slot: u32) -> ReceivedMessage {
    loop {
        if let Some(msg) = receive(endpoint_slot) {
            return msg;
        }
        yield_now();
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn receive_blocking(_endpoint_slot: u32) -> ReceivedMessage {
    panic!("receive_blocking not supported outside WASM")
}

/// Get uptime in nanoseconds
#[cfg(target_arch = "wasm32")]
pub fn get_time() -> u64 {
    unsafe {
        let low = zos_syscall(SYS_GET_TIME, 0, 0, 0);
        let high = zos_syscall(SYS_GET_TIME, 1, 0, 0);
        ((high as u64) << 32) | (low as u64)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn get_time() -> u64 {
    0
}

/// Get wall-clock time in milliseconds since Unix epoch
///
/// This is real time-of-day (can jump due to NTP sync).
/// Use `get_time()` for durations and scheduling.
#[cfg(target_arch = "wasm32")]
pub fn get_wallclock() -> u64 {
    unsafe {
        let low = zos_syscall(SYS_GET_WALLCLOCK, 0, 0, 0);
        let high = zos_syscall(SYS_GET_WALLCLOCK, 1, 0, 0);
        ((high as u64) << 32) | (low as u64)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn get_wallclock() -> u64 {
    // Return current time for testing (using std)
    #[cfg(feature = "std")]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
    #[cfg(not(feature = "std"))]
    {
        0
    }
}

/// Yield to allow other processes to run
#[cfg(target_arch = "wasm32")]
pub fn yield_now() {
    unsafe {
        zos_yield();
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn yield_now() {}

/// Exit the process
#[cfg(target_arch = "wasm32")]
pub fn exit(code: i32) -> ! {
    unsafe {
        zos_syscall(SYS_EXIT, code as u32, 0, 0);
    }
    // The kernel should terminate us, but just in case:
    loop {
        yield_now();
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn exit(_code: i32) -> ! {
    panic!("exit called outside WASM")
}

/// Kill a process.
///
/// This syscall requires the caller to have a Process capability for the target
/// process with write permission, OR the caller must be Init (PID 1).
///
/// # Arguments
/// - `target_pid`: PID of the process to terminate
///
/// # Returns
/// - `Ok(())`: Process was terminated
/// - `Err(code)`: Error (e.g., permission denied, process not found)
#[cfg(target_arch = "wasm32")]
pub fn kill(target_pid: u32) -> Result<(), u32> {
    unsafe {
        let result = zos_syscall(SYS_KILL, target_pid, 0, 0);
        if result == 0 {
            Ok(())
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn kill(_target_pid: u32) -> Result<(), u32> {
    Err(error::E_NOSYS)
}

// ============================================================================
// Capability Syscall Wrappers
// ============================================================================

/// Permission flags for capability operations
#[derive(Clone, Copy, Debug, Default)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
    pub grant: bool,
}

impl Permissions {
    /// Full permissions
    pub fn full() -> Self {
        Self {
            read: true,
            write: true,
            grant: true,
        }
    }

    /// Read-only
    pub fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            grant: false,
        }
    }

    /// Write-only
    pub fn write_only() -> Self {
        Self {
            read: false,
            write: true,
            grant: false,
        }
    }

    /// Pack into a single byte
    pub fn to_byte(&self) -> u8 {
        let mut b = 0u8;
        if self.read {
            b |= 0x01;
        }
        if self.write {
            b |= 0x02;
        }
        if self.grant {
            b |= 0x04;
        }
        b
    }

    /// Unpack from a byte
    pub fn from_byte(b: u8) -> Self {
        Self {
            read: (b & 0x01) != 0,
            write: (b & 0x02) != 0,
            grant: (b & 0x04) != 0,
        }
    }
}

/// Grant a capability to another process
///
/// # Arguments
/// - `from_slot`: Source capability slot in caller's CSpace
/// - `to_pid`: Target process ID
/// - `perms`: Permissions to grant (attenuated from source)
///
/// # Returns
/// - `Ok(slot)`: Slot in target's CSpace where capability was placed
/// - `Err(code)`: Error code
#[cfg(target_arch = "wasm32")]
pub fn cap_grant(from_slot: u32, to_pid: u32, perms: Permissions) -> Result<u32, u32> {
    unsafe {
        let result = zos_syscall(SYS_CAP_GRANT, from_slot, to_pid, perms.to_byte() as u32);
        if result & 0x80000000 == 0 {
            Ok(result)
        } else {
            Err(result & 0x7FFFFFFF)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn cap_grant(_from_slot: u32, _to_pid: u32, _perms: Permissions) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}

/// Revoke a capability (requires grant permission)
///
/// # Arguments
/// - `slot`: Capability slot to revoke
///
/// # Returns
/// - `Ok(())`: Capability revoked
/// - `Err(code)`: Error code
#[cfg(target_arch = "wasm32")]
pub fn cap_revoke(slot: u32) -> Result<(), u32> {
    unsafe {
        let result = zos_syscall(SYS_CAP_REVOKE, slot, 0, 0);
        if result == 0 {
            Ok(())
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn cap_revoke(_slot: u32) -> Result<(), u32> {
    Err(error::E_NOSYS)
}

/// Revoke a capability from another process (privileged operation)
///
/// This is a privileged syscall for the PermissionManager (PID 2).
/// It allows revoking capabilities from any process's CSpace.
///
/// # Arguments
/// - `target_pid`: Process ID to revoke from
/// - `slot`: Capability slot to revoke
///
/// # Returns
/// - `Ok(())`: Capability revoked
/// - `Err(code)`: Error code
#[cfg(target_arch = "wasm32")]
pub fn cap_revoke_from(target_pid: u32, slot: u32) -> Result<(), u32> {
    unsafe {
        let result = zos_syscall(SYS_CAP_REVOKE, target_pid, slot, 0);
        if result == 0 {
            Ok(())
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn cap_revoke_from(_target_pid: u32, _slot: u32) -> Result<(), u32> {
    Err(error::E_NOSYS)
}

/// Delete a capability from own CSpace
///
/// # Arguments
/// - `slot`: Capability slot to delete
///
/// # Returns
/// - `Ok(())`: Capability deleted
/// - `Err(code)`: Error code
#[cfg(target_arch = "wasm32")]
pub fn cap_delete(slot: u32) -> Result<(), u32> {
    unsafe {
        let result = zos_syscall(SYS_CAP_DELETE, slot, 0, 0);
        if result == 0 {
            Ok(())
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn cap_delete(_slot: u32) -> Result<(), u32> {
    Err(error::E_NOSYS)
}

/// Inspect a capability
///
/// # Arguments
/// - `slot`: Capability slot to inspect
///
/// # Returns
/// - `Some(CapInfo)`: Capability information
/// - `None`: Slot is empty or invalid
#[cfg(target_arch = "wasm32")]
pub fn cap_inspect(slot: u32) -> Option<CapInfo> {
    let mut buffer = [0u8; 32];
    unsafe {
        let result = zos_syscall(SYS_CAP_INSPECT, slot, 0, 0);
        if result == 0 {
            return None;
        }
        // Get capability info from buffer
        let len = zos_recv_bytes(buffer.as_mut_ptr(), buffer.len() as u32);
        if len < 16 {
            return None;
        }
        // Parse: [object_type: u8, perms: u8, pad: u16, slot: u32, object_id: u64]
        let object_type = buffer[0];
        let perms = buffer[1];
        let object_id = u64::from_le_bytes([
            buffer[8], buffer[9], buffer[10], buffer[11], buffer[12], buffer[13], buffer[14],
            buffer[15],
        ]);
        Some(CapInfo {
            slot,
            object_type,
            object_id,
            can_read: (perms & 0x01) != 0,
            can_write: (perms & 0x02) != 0,
            can_grant: (perms & 0x04) != 0,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn cap_inspect(_slot: u32) -> Option<CapInfo> {
    None
}

/// Derive a capability with reduced permissions
///
/// Creates a new capability in caller's CSpace with attenuated permissions.
///
/// # Arguments
/// - `slot`: Source capability slot
/// - `new_perms`: Requested permissions (will be intersected with source)
///
/// # Returns
/// - `Ok(new_slot)`: Slot of the new derived capability
/// - `Err(code)`: Error code
#[cfg(target_arch = "wasm32")]
pub fn cap_derive(slot: u32, new_perms: Permissions) -> Result<u32, u32> {
    unsafe {
        let result = zos_syscall(SYS_CAP_DERIVE, slot, new_perms.to_byte() as u32, 0);
        if result & 0x80000000 == 0 {
            Ok(result)
        } else {
            Err(result & 0x7FFFFFFF)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn cap_derive(_slot: u32, _new_perms: Permissions) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}

/// Send a message with capabilities to transfer
///
/// # Arguments
/// - `endpoint_slot`: Capability slot for the destination endpoint
/// - `tag`: Application-defined message tag
/// - `data`: Message payload
/// - `cap_slots`: Capability slots to transfer (removed from caller's CSpace)
///
/// # Returns
/// - `Ok(())`: Message sent
/// - `Err(code)`: Error code
#[cfg(target_arch = "wasm32")]
pub fn send_with_caps(
    endpoint_slot: u32,
    tag: u32,
    data: &[u8],
    cap_slots: &[u32],
) -> Result<(), u32> {
    unsafe {
        // Send data first
        zos_send_bytes(data.as_ptr(), data.len() as u32);
        // Pack cap_slots into bytes and send
        if !cap_slots.is_empty() {
            let cap_bytes: Vec<u8> = cap_slots.iter().flat_map(|s| s.to_le_bytes()).collect();
            zos_send_bytes(cap_bytes.as_ptr(), cap_bytes.len() as u32);
        }
        let result = zos_syscall(
            SYS_SEND_CAP,
            endpoint_slot,
            tag,
            (data.len() as u32) | ((cap_slots.len() as u32) << 16),
        );
        if result == 0 {
            Ok(())
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn send_with_caps(
    _endpoint_slot: u32,
    _tag: u32,
    _data: &[u8],
    _cap_slots: &[u32],
) -> Result<(), u32> {
    Err(error::E_NOSYS)
}

/// Call - send a message and wait for reply (RPC pattern)
///
/// # Arguments
/// - `endpoint_slot`: Capability slot for the destination endpoint
/// - `tag`: Application-defined message tag
/// - `data`: Request payload
///
/// # Returns
/// - `Ok(ReceivedMessage)`: Reply message
/// - `Err(code)`: Error code
#[cfg(target_arch = "wasm32")]
pub fn call(endpoint_slot: u32, tag: u32, data: &[u8]) -> Result<ReceivedMessage, u32> {
    // Simple implementation: send then poll for reply
    send(endpoint_slot, tag, data)?;

    // Poll for reply (would block in a real implementation)
    loop {
        if let Some(msg) = receive(endpoint_slot) {
            return Ok(msg);
        }
        yield_now();
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn call(_endpoint_slot: u32, _tag: u32, _data: &[u8]) -> Result<ReceivedMessage, u32> {
    Err(error::E_NOSYS)
}

/// Reply to a call
///
/// # Arguments
/// - `caller_pid`: PID of the calling process
/// - `tag`: Reply message tag
/// - `data`: Reply payload
///
/// # Returns
/// - `Ok(())`: Reply sent
/// - `Err(code)`: Error code
#[cfg(target_arch = "wasm32")]
pub fn reply(caller_pid: u32, tag: u32, data: &[u8]) -> Result<(), u32> {
    unsafe {
        zos_send_bytes(data.as_ptr(), data.len() as u32);
        let result = zos_syscall(SYS_REPLY, caller_pid, tag, data.len() as u32);
        if result == 0 {
            Ok(())
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn reply(_caller_pid: u32, _tag: u32, _data: &[u8]) -> Result<(), u32> {
    Err(error::E_NOSYS)
}

/// Create an IPC endpoint
///
/// # Returns
/// - `Ok((endpoint_id, slot))`: Endpoint ID and capability slot
/// - `Err(code)`: Error code
#[cfg(target_arch = "wasm32")]
pub fn create_endpoint() -> Result<(u64, u32), u32> {
    unsafe {
        let result = zos_syscall(SYS_EP_CREATE, 0, 0, 0);
        if result & 0x80000000 == 0 {
            // Result format: high 32 = endpoint_id, low 32 = slot
            let high = zos_syscall(SYS_EP_CREATE, 1, 0, 0);
            let endpoint_id = ((high as u64) << 32) | (result as u64 >> 32);
            let slot = result & 0xFFFFFFFF;
            Ok((endpoint_id, slot))
        } else {
            Err(result & 0x7FFFFFFF)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn create_endpoint() -> Result<(u64, u32), u32> {
    Err(error::E_NOSYS)
}

// ============================================================================
// Init-Only Syscalls (Spawn Protocol)
// ============================================================================
//
// These syscalls can only be invoked by Init (PID 1) as part of the
// Init-driven spawn protocol. They ensure all process lifecycle management
// flows through Init, enabling proper audit logging via SysLog.

/// Register a new process in the kernel (Init-only syscall).
///
/// This is the first step of the Init-driven spawn protocol. Only Init (PID 1)
/// can call this syscall. Other processes will receive an error.
///
/// # Arguments
/// - `name`: Name of the process to register
///
/// # Returns
/// - `Ok(pid)`: The PID assigned to the new process
/// - `Err(code)`: Error code (e.g., permission denied if caller is not Init)
#[cfg(target_arch = "wasm32")]
pub fn register_process(name: &str) -> Result<u32, u32> {
    let bytes = name.as_bytes();
    unsafe {
        zos_send_bytes(bytes.as_ptr(), bytes.len() as u32);
        let result = zos_syscall(SYS_REGISTER_PROCESS, bytes.len() as u32, 0, 0) as i32;
        if result >= 0 {
            Ok(result as u32)
        } else {
            Err(error::E_PERM)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn register_process(_name: &str) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}

/// Create an endpoint for another process (Init-only syscall).
///
/// This syscall is part of the Init-driven spawn protocol. Only Init (PID 1)
/// can create endpoints for other processes. This enables Init to set up
/// the standard endpoint configuration during spawn.
///
/// # Arguments
/// - `target_pid`: PID of the process to create an endpoint for
///
/// # Returns
/// - `Ok((endpoint_id, slot))`: The created endpoint ID and slot
/// - `Err(code)`: Error code (e.g., permission denied if caller is not Init)
#[cfg(target_arch = "wasm32")]
pub fn create_endpoint_for(target_pid: u32) -> Result<(u64, u32), u32> {
    unsafe {
        let result = zos_syscall(SYS_CREATE_ENDPOINT_FOR, target_pid, 0, 0) as i64;
        if result >= 0 {
            // Result is packed: high 32 = slot, low 32 = endpoint_id
            let slot = (result >> 32) as u32;
            let endpoint_id = (result & 0xFFFFFFFF) as u64;
            Ok((endpoint_id, slot))
        } else {
            Err(error::E_PERM)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn create_endpoint_for(_target_pid: u32) -> Result<(u64, u32), u32> {
    Err(error::E_NOSYS)
}

// ============================================================================
// Async Platform Storage Syscalls (for VfsService)
// ============================================================================
//
// These syscalls initiate async storage operations and return a request_id
// immediately. The result is delivered via MSG_STORAGE_RESULT IPC message.
//
// Only VfsService should use these - applications use zos_vfs::VfsClient.

/// Start async storage read operation.
///
/// This syscall returns immediately with a request_id. When the operation
/// completes, the result is delivered via MSG_STORAGE_RESULT IPC message.
///
/// # Arguments
/// - `key`: Storage key to read
///
/// # Returns
/// - `Ok(request_id)`: Request ID to match with result
/// - `Err(code)`: Failed to start operation
#[cfg(target_arch = "wasm32")]
pub fn storage_read_async(key: &str) -> Result<u32, u32> {
    let key_bytes = key.as_bytes();
    unsafe {
        zos_send_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let result = zos_syscall(SYS_STORAGE_READ, key_bytes.len() as u32, 0, 0);
        if result as i32 >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn storage_read_async(_key: &str) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}

/// Start async storage write operation.
///
/// This syscall returns immediately with a request_id. When the operation
/// completes, the result is delivered via MSG_STORAGE_RESULT IPC message.
///
/// # Arguments
/// - `key`: Storage key to write
/// - `value`: Data to store
///
/// # Returns
/// - `Ok(request_id)`: Request ID to match with result
/// - `Err(code)`: Failed to start operation
#[cfg(target_arch = "wasm32")]
pub fn storage_write_async(key: &str, value: &[u8]) -> Result<u32, u32> {
    let key_bytes = key.as_bytes();
    // Data format: [key_len: u32, key: [u8], value: [u8]]
    let mut data = Vec::with_capacity(4 + key_bytes.len() + value.len());
    data.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
    data.extend_from_slice(key_bytes);
    data.extend_from_slice(value);
    
    unsafe {
        zos_send_bytes(data.as_ptr(), data.len() as u32);
        let result = zos_syscall(SYS_STORAGE_WRITE, key_bytes.len() as u32, value.len() as u32, 0);
        if result as i32 >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn storage_write_async(_key: &str, _value: &[u8]) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}

/// Start async storage delete operation.
///
/// This syscall returns immediately with a request_id. When the operation
/// completes, the result is delivered via MSG_STORAGE_RESULT IPC message.
///
/// # Arguments
/// - `key`: Storage key to delete
///
/// # Returns
/// - `Ok(request_id)`: Request ID to match with result
/// - `Err(code)`: Failed to start operation
#[cfg(target_arch = "wasm32")]
pub fn storage_delete_async(key: &str) -> Result<u32, u32> {
    let key_bytes = key.as_bytes();
    unsafe {
        zos_send_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let result = zos_syscall(SYS_STORAGE_DELETE, key_bytes.len() as u32, 0, 0);
        if result as i32 >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn storage_delete_async(_key: &str) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}

/// Start async storage list operation.
///
/// This syscall returns immediately with a request_id. When the operation
/// completes, the result is delivered via MSG_STORAGE_RESULT IPC message
/// with a JSON array of matching keys.
///
/// # Arguments
/// - `prefix`: Key prefix to match
///
/// # Returns
/// - `Ok(request_id)`: Request ID to match with result
/// - `Err(code)`: Failed to start operation
#[cfg(target_arch = "wasm32")]
pub fn storage_list_async(prefix: &str) -> Result<u32, u32> {
    let prefix_bytes = prefix.as_bytes();
    unsafe {
        zos_send_bytes(prefix_bytes.as_ptr(), prefix_bytes.len() as u32);
        let result = zos_syscall(SYS_STORAGE_LIST, prefix_bytes.len() as u32, 0, 0);
        if result as i32 >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn storage_list_async(_prefix: &str) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}

/// Start async storage exists check.
///
/// This syscall returns immediately with a request_id. When the operation
/// completes, the result is delivered via MSG_STORAGE_RESULT IPC message
/// with EXISTS_OK result type (data byte: 1=exists, 0=not exists).
///
/// # Arguments
/// - `key`: Storage key to check
///
/// # Returns
/// - `Ok(request_id)`: Request ID to match with result
/// - `Err(code)`: Failed to start operation
#[cfg(target_arch = "wasm32")]
pub fn storage_exists_async(key: &str) -> Result<u32, u32> {
    let key_bytes = key.as_bytes();
    unsafe {
        zos_send_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let result = zos_syscall(SYS_STORAGE_EXISTS, key_bytes.len() as u32, 0, 0);
        if result as i32 >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn storage_exists_async(_key: &str) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}

// ============================================================================
// VFS Syscall Wrappers (DEPRECATED)
// ============================================================================
//
// These functions are deprecated. Use zos_vfs::VfsClient for VFS operations.
// VFS operations now go through the VFS IPC service, which maintains the
// thin-supervisor architecture principle.

/// Read a file from the VFS.
///
/// # Deprecated
/// Use `zos_vfs::VfsClient::read_file()` instead.
///
/// # Arguments
/// - `path`: Path to the file to read
///
/// # Returns
/// - `Ok(Vec<u8>)`: File contents
/// - `Err(code)`: Error code
#[deprecated(since = "0.1.0", note = "Use zos_vfs::VfsClient::read_file() via VFS IPC service")]
#[cfg(target_arch = "wasm32")]
#[allow(deprecated)]
pub fn vfs_read(path: &str) -> Result<Vec<u8>, u32> {
    let path_bytes = path.as_bytes();
    unsafe {
        zos_send_bytes(path_bytes.as_ptr(), path_bytes.len() as u32);
        let result = zos_syscall(SYS_VFS_READ, path_bytes.len() as u32, 0, 0);
        if result == 0 {
            // Get the file contents
            let mut buffer = [0u8; 65536]; // 64KB max file size for now
            let len = zos_recv_bytes(buffer.as_mut_ptr(), buffer.len() as u32);
            Ok(buffer[..len as usize].to_vec())
        } else {
            Err(result)
        }
    }
}

#[deprecated(since = "0.1.0", note = "Use zos_vfs::VfsClient::read_file() via VFS IPC service")]
#[cfg(not(target_arch = "wasm32"))]
pub fn vfs_read(_path: &str) -> Result<Vec<u8>, u32> {
    Err(error::E_NOSYS)
}

/// Write a file to the VFS.
///
/// # Deprecated
/// Use `zos_vfs::VfsClient::write_file()` instead.
///
/// # Arguments
/// - `path`: Path to the file to write
/// - `content`: File contents to write
///
/// # Returns
/// - `Ok(())`: File written successfully
/// - `Err(code)`: Error code
#[deprecated(since = "0.1.0", note = "Use zos_vfs::VfsClient::write_file() via VFS IPC service")]
#[cfg(target_arch = "wasm32")]
#[allow(deprecated)]
pub fn vfs_write(path: &str, content: &[u8]) -> Result<(), u32> {
    // Send path length, then path, then content
    let path_bytes = path.as_bytes();
    let mut data = Vec::with_capacity(4 + path_bytes.len() + content.len());
    data.extend_from_slice(&(path_bytes.len() as u32).to_le_bytes());
    data.extend_from_slice(path_bytes);
    data.extend_from_slice(content);
    
    unsafe {
        zos_send_bytes(data.as_ptr(), data.len() as u32);
        let result = zos_syscall(SYS_VFS_WRITE, path_bytes.len() as u32, content.len() as u32, 0);
        if result == 0 {
            Ok(())
        } else {
            Err(result)
        }
    }
}

#[deprecated(since = "0.1.0", note = "Use zos_vfs::VfsClient::write_file() via VFS IPC service")]
#[cfg(not(target_arch = "wasm32"))]
pub fn vfs_write(_path: &str, _content: &[u8]) -> Result<(), u32> {
    Err(error::E_NOSYS)
}

/// Create a directory in the VFS.
///
/// # Deprecated
/// Use `zos_vfs::VfsClient::mkdir()` instead.
///
/// # Arguments
/// - `path`: Path to the directory to create
///
/// # Returns
/// - `Ok(())`: Directory created successfully
/// - `Err(code)`: Error code
#[deprecated(since = "0.1.0", note = "Use zos_vfs::VfsClient::mkdir() via VFS IPC service")]
#[cfg(target_arch = "wasm32")]
#[allow(deprecated)]
pub fn vfs_mkdir(path: &str) -> Result<(), u32> {
    let path_bytes = path.as_bytes();
    unsafe {
        zos_send_bytes(path_bytes.as_ptr(), path_bytes.len() as u32);
        let result = zos_syscall(SYS_VFS_MKDIR, path_bytes.len() as u32, 0, 0);
        if result == 0 {
            Ok(())
        } else {
            Err(result)
        }
    }
}

#[deprecated(since = "0.1.0", note = "Use zos_vfs::VfsClient::mkdir() via VFS IPC service")]
#[cfg(not(target_arch = "wasm32"))]
pub fn vfs_mkdir(_path: &str) -> Result<(), u32> {
    Err(error::E_NOSYS)
}

/// List directory contents.
///
/// # Deprecated
/// Use `zos_vfs::VfsClient::readdir()` instead.
///
/// # Arguments
/// - `path`: Path to the directory to list
///
/// # Returns
/// - `Ok(Vec<String>)`: List of entry names
/// - `Err(code)`: Error code
#[deprecated(since = "0.1.0", note = "Use zos_vfs::VfsClient::readdir() via VFS IPC service")]
#[cfg(target_arch = "wasm32")]
#[allow(deprecated)]
pub fn vfs_list(path: &str) -> Result<Vec<String>, u32> {
    let path_bytes = path.as_bytes();
    unsafe {
        zos_send_bytes(path_bytes.as_ptr(), path_bytes.len() as u32);
        let result = zos_syscall(SYS_VFS_LIST, path_bytes.len() as u32, 0, 0);
        if result == 0 {
            // Get the list data (count: u32, then name_len: u16, name: [u8] for each entry)
            let mut buffer = [0u8; 4096];
            let len = zos_recv_bytes(buffer.as_mut_ptr(), buffer.len() as u32);
            if len < 4 {
                return Ok(Vec::new());
            }
            
            let count = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
            let mut entries = Vec::with_capacity(count);
            let mut offset = 4;
            
            for _ in 0..count {
                if offset + 2 > len as usize {
                    break;
                }
                let name_len = u16::from_le_bytes([buffer[offset], buffer[offset + 1]]) as usize;
                offset += 2;
                if offset + name_len > len as usize {
                    break;
                }
                if let Ok(name) = core::str::from_utf8(&buffer[offset..offset + name_len]) {
                    entries.push(name.to_string());
                }
                offset += name_len;
            }
            
            Ok(entries)
        } else {
            Err(result)
        }
    }
}

#[deprecated(since = "0.1.0", note = "Use zos_vfs::VfsClient::readdir() via VFS IPC service")]
#[cfg(not(target_arch = "wasm32"))]
pub fn vfs_list(_path: &str) -> Result<Vec<String>, u32> {
    Err(error::E_NOSYS)
}

/// Delete a file or directory.
///
/// # Deprecated
/// Use `zos_vfs::VfsClient::unlink()` instead.
///
/// # Arguments
/// - `path`: Path to delete
///
/// # Returns
/// - `Ok(())`: Deleted successfully
/// - `Err(code)`: Error code
#[deprecated(since = "0.1.0", note = "Use zos_vfs::VfsClient::unlink() via VFS IPC service")]
#[cfg(target_arch = "wasm32")]
#[allow(deprecated)]
pub fn vfs_delete(path: &str) -> Result<(), u32> {
    let path_bytes = path.as_bytes();
    unsafe {
        zos_send_bytes(path_bytes.as_ptr(), path_bytes.len() as u32);
        let result = zos_syscall(SYS_VFS_DELETE, path_bytes.len() as u32, 0, 0);
        if result == 0 {
            Ok(())
        } else {
            Err(result)
        }
    }
}

#[deprecated(since = "0.1.0", note = "Use zos_vfs::VfsClient::unlink() via VFS IPC service")]
#[cfg(not(target_arch = "wasm32"))]
pub fn vfs_delete(_path: &str) -> Result<(), u32> {
    Err(error::E_NOSYS)
}

/// Check if a path exists in the VFS.
///
/// # Deprecated
/// Use `zos_vfs::VfsClient::exists()` instead.
///
/// # Arguments
/// - `path`: Path to check
///
/// # Returns
/// - `Ok(true)`: Path exists
/// - `Ok(false)`: Path does not exist
/// - `Err(code)`: Error code
#[deprecated(since = "0.1.0", note = "Use zos_vfs::VfsClient::exists() via VFS IPC service")]
#[cfg(target_arch = "wasm32")]
#[allow(deprecated)]
pub fn vfs_exists(path: &str) -> Result<bool, u32> {
    let path_bytes = path.as_bytes();
    unsafe {
        zos_send_bytes(path_bytes.as_ptr(), path_bytes.len() as u32);
        let result = zos_syscall(SYS_VFS_EXISTS, path_bytes.len() as u32, 0, 0);
        // Result: 0 = exists, 1 = not exists, other = error
        match result {
            0 => Ok(true),
            1 => Ok(false),
            _ => Err(result),
        }
    }
}

#[deprecated(since = "0.1.0", note = "Use zos_vfs::VfsClient::exists() via VFS IPC service")]
#[cfg(not(target_arch = "wasm32"))]
pub fn vfs_exists(_path: &str) -> Result<bool, u32> {
    Err(error::E_NOSYS)
}

// ============================================================================
// Types
// ============================================================================

/// A received IPC message
#[derive(Clone, Debug)]
pub struct ReceivedMessage {
    /// Sender's PID
    pub from_pid: u32,
    /// Message tag
    pub tag: u32,
    /// Capability slots containing transferred capabilities
    /// These are slots in the receiver's CSpace where the kernel installed
    /// capabilities that were transferred with this message.
    pub cap_slots: Vec<u32>,
    /// Message data
    pub data: Vec<u8>,
}

/// Capability info returned from list_caps
#[derive(Clone, Debug)]
pub struct CapInfo {
    pub slot: u32,
    pub object_type: u8,
    pub object_id: u64,
    pub can_read: bool,
    pub can_write: bool,
    pub can_grant: bool,
}

/// Process info returned from list_processes
#[derive(Clone, Debug)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub state: u8,
}

// ============================================================================
// List syscalls
// ============================================================================

/// List all capabilities in this process's capability space
#[cfg(target_arch = "wasm32")]
pub fn list_caps() -> Vec<CapInfo> {
    let mut buffer = [0u8; 4096];
    unsafe {
        let result = zos_syscall(SYS_LIST_CAPS, 0, 0, 0);
        if result != 0 {
            return Vec::new();
        }
        // Get the capability data
        let len = zos_recv_bytes(buffer.as_mut_ptr(), buffer.len() as u32);
        if len < 4 {
            return Vec::new();
        }
        // Parse: first 4 bytes = count, then for each cap: slot(4) + type(1) + object_id(8) = 13 bytes
        let count = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
        let mut caps = Vec::with_capacity(count);
        let mut offset = 4;
        for _ in 0..count {
            if offset + 13 > len as usize {
                break;
            }
            let slot = u32::from_le_bytes([
                buffer[offset],
                buffer[offset + 1],
                buffer[offset + 2],
                buffer[offset + 3],
            ]);
            let object_type = buffer[offset + 4];
            let object_id = u64::from_le_bytes([
                buffer[offset + 5],
                buffer[offset + 6],
                buffer[offset + 7],
                buffer[offset + 8],
                buffer[offset + 9],
                buffer[offset + 10],
                buffer[offset + 11],
                buffer[offset + 12],
            ]);
            // Note: permissions not included in kernel response yet
            // We'll need to extend the kernel response format
            caps.push(CapInfo {
                slot,
                object_type,
                object_id,
                can_read: true,  // placeholder
                can_write: true, // placeholder
                can_grant: false, // placeholder
            });
            offset += 13;
        }
        caps
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn list_caps() -> Vec<CapInfo> {
    Vec::new()
}

/// List all processes in the system
#[cfg(target_arch = "wasm32")]
pub fn list_processes() -> Vec<ProcessInfo> {
    let mut buffer = [0u8; 4096];
    unsafe {
        let result = zos_syscall(SYS_LIST_PROCS, 0, 0, 0);
        if result != 0 {
            return Vec::new();
        }
        // Get the process data
        let len = zos_recv_bytes(buffer.as_mut_ptr(), buffer.len() as u32);
        if len < 4 {
            return Vec::new();
        }
        // Parse: first 4 bytes = count, then for each proc: pid(4) + name_len(2) + name(variable)
        let count = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
        let mut procs = Vec::with_capacity(count);
        let mut offset = 4;
        for _ in 0..count {
            if offset + 6 > len as usize {
                break;
            }
            let pid = u32::from_le_bytes([
                buffer[offset],
                buffer[offset + 1],
                buffer[offset + 2],
                buffer[offset + 3],
            ]);
            let name_len = u16::from_le_bytes([buffer[offset + 4], buffer[offset + 5]]) as usize;
            offset += 6;
            if offset + name_len > len as usize {
                break;
            }
            let name = core::str::from_utf8(&buffer[offset..offset + name_len])
                .unwrap_or("???")
                .to_string();
            offset += name_len;
            procs.push(ProcessInfo {
                pid,
                name,
                state: 0, // Running (state not included in kernel response)
            });
        }
        procs
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn list_processes() -> Vec<ProcessInfo> {
    Vec::new()
}
// ============================================================================
// Console helpers
// ============================================================================

// Note: console_write() is now defined above as a syscall wrapper (SYS_CONSOLE_WRITE).
// The old IPC-based console_write(slot, text) has been removed.
