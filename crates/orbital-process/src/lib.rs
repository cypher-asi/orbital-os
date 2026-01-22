//! Process-side syscall library for Orbital OS
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
    /// Sleep for specified nanoseconds
    pub const SYS_SLEEP: u32 = 0x13;
    /// Wait for thread to exit
    pub const SYS_THREAD_JOIN: u32 = 0x14;

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

/// Console input message tag - used by terminal for receiving keyboard input.
pub const MSG_CONSOLE_INPUT: u32 = 0x0002;

// =============================================================================
// Init Service Protocol (for service discovery)
// =============================================================================

/// Register a service with init: data = [name_len: u8, name: [u8], endpoint_id_low: u32, endpoint_id_high: u32]
pub const MSG_REGISTER_SERVICE: u32 = 0x1000;

/// Lookup a service: data = [name_len: u8, name: [u8]]
pub const MSG_LOOKUP_SERVICE: u32 = 0x1001;

/// Lookup response: data = [found: u8, endpoint_id_low: u32, endpoint_id_high: u32]
pub const MSG_LOOKUP_RESPONSE: u32 = 0x1002;

/// Request spawn: data = [name_len: u8, name: [u8]]
pub const MSG_SPAWN_SERVICE: u32 = 0x1003;

/// Spawn response: data = [success: u8, pid: u32]
pub const MSG_SPAWN_RESPONSE: u32 = 0x1004;

/// Service ready notification (service → init after registration complete)
pub const MSG_SERVICE_READY: u32 = 0x1005;

// =============================================================================
// Capability Revocation Notification (IPC → Process)
// =============================================================================

/// Notification that a capability was revoked from this process
/// Payload: [slot: u32, object_type: u8, object_id: u64, reason: u8]
pub const MSG_CAP_REVOKED: u32 = 0x3010;

/// Revocation reason: Supervisor/user explicitly revoked the capability
pub const REVOKE_REASON_EXPLICIT: u8 = 1;
/// Revocation reason: Capability expired
pub const REVOKE_REASON_EXPIRED: u8 = 2;
/// Revocation reason: Source process exited
pub const REVOKE_REASON_PROCESS_EXIT: u8 = 3;

/// Well-known slot for init's endpoint (every process gets this at spawn)
pub const INIT_ENDPOINT_SLOT: u32 = 2;

// =============================================================================
// Permission Protocol (03-security.md)
// Messages for permission management (Desktop/Supervisor -> Init)
// =============================================================================

/// Request Init to grant a capability to a process
pub const MSG_GRANT_PERMISSION: u32 = 0x1010;

/// Request Init to revoke a capability from a process
pub const MSG_REVOKE_PERMISSION: u32 = 0x1011;

/// Query what permissions a process has
pub const MSG_LIST_PERMISSIONS: u32 = 0x1012;

/// Response from Init with grant/revoke result
pub const MSG_PERMISSION_RESPONSE: u32 = 0x1013;

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
    fn orbital_syscall(syscall_num: u32, arg1: u32, arg2: u32, arg3: u32) -> u32;

    /// Send bytes to the kernel (for syscall data)
    fn orbital_send_bytes(ptr: *const u8, len: u32);

    /// Get bytes from the kernel (for syscall results)
    /// Returns the number of bytes written
    fn orbital_recv_bytes(ptr: *mut u8, max_len: u32) -> u32;

    /// Yield to allow other processes to run
    fn orbital_yield();

    /// Get the process's assigned PID
    fn orbital_get_pid() -> u32;
}

// ============================================================================
// Syscall Wrappers
// ============================================================================

/// Get this process's PID
#[cfg(target_arch = "wasm32")]
pub fn get_pid() -> u32 {
    unsafe { orbital_get_pid() }
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
        orbital_send_bytes(bytes.as_ptr(), bytes.len() as u32);
        orbital_syscall(SYS_DEBUG, bytes.len() as u32, 0, 0);
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
        orbital_send_bytes(bytes.as_ptr(), bytes.len() as u32);
        orbital_syscall(SYS_CONSOLE_WRITE, bytes.len() as u32, 0, 0);
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
        orbital_send_bytes(data.as_ptr(), data.len() as u32);
        let result = orbital_syscall(SYS_SEND, endpoint_slot, tag, data.len() as u32);
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
        let result = orbital_syscall(SYS_RECEIVE, endpoint_slot, 0, 0) as i32;
        if result <= 0 {
            // 0 = no message, negative = error (e.g., permission denied)
            return None;
        }
        // Get the message data
        let len = orbital_recv_bytes(buffer.as_mut_ptr(), buffer.len() as u32);
        if len == 0 {
            return None;
        }
        // Parse: first 4 bytes = from_pid, next 4 = tag, rest = data
        if len < 8 {
            return None;
        }
        let from_pid = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        let tag = u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
        let data = buffer[8..len as usize].to_vec();
        Some(ReceivedMessage {
            from_pid,
            tag,
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
        let low = orbital_syscall(SYS_GET_TIME, 0, 0, 0);
        let high = orbital_syscall(SYS_GET_TIME, 1, 0, 0);
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
        let low = orbital_syscall(SYS_GET_WALLCLOCK, 0, 0, 0);
        let high = orbital_syscall(SYS_GET_WALLCLOCK, 1, 0, 0);
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
        orbital_yield();
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn yield_now() {}

/// Exit the process
#[cfg(target_arch = "wasm32")]
pub fn exit(code: i32) -> ! {
    unsafe {
        orbital_syscall(SYS_EXIT, code as u32, 0, 0);
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
        let result = orbital_syscall(SYS_CAP_GRANT, from_slot, to_pid, perms.to_byte() as u32);
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
        let result = orbital_syscall(SYS_CAP_REVOKE, slot, 0, 0);
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
        let result = orbital_syscall(SYS_CAP_DELETE, slot, 0, 0);
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
        let result = orbital_syscall(SYS_CAP_INSPECT, slot, 0, 0);
        if result == 0 {
            return None;
        }
        // Get capability info from buffer
        let len = orbital_recv_bytes(buffer.as_mut_ptr(), buffer.len() as u32);
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
        let result = orbital_syscall(SYS_CAP_DERIVE, slot, new_perms.to_byte() as u32, 0);
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
        orbital_send_bytes(data.as_ptr(), data.len() as u32);
        // Pack cap_slots into bytes and send
        if !cap_slots.is_empty() {
            let cap_bytes: Vec<u8> = cap_slots.iter().flat_map(|s| s.to_le_bytes()).collect();
            orbital_send_bytes(cap_bytes.as_ptr(), cap_bytes.len() as u32);
        }
        let result = orbital_syscall(
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
        orbital_send_bytes(data.as_ptr(), data.len() as u32);
        let result = orbital_syscall(SYS_REPLY, caller_pid, tag, data.len() as u32);
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
        let result = orbital_syscall(SYS_EP_CREATE, 0, 0, 0);
        if result & 0x80000000 == 0 {
            // Result format: high 32 = endpoint_id, low 32 = slot
            let high = orbital_syscall(SYS_EP_CREATE, 1, 0, 0);
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
// Types
// ============================================================================

/// A received IPC message
#[derive(Clone, Debug)]
pub struct ReceivedMessage {
    /// Sender's PID
    pub from_pid: u32,
    /// Message tag
    pub tag: u32,
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
        let result = orbital_syscall(SYS_LIST_CAPS, 0, 0, 0);
        if result != 0 {
            return Vec::new();
        }
        // Get the capability data
        let len = orbital_recv_bytes(buffer.as_mut_ptr(), buffer.len() as u32);
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
        let result = orbital_syscall(SYS_LIST_PROCS, 0, 0, 0);
        if result != 0 {
            return Vec::new();
        }
        // Get the process data
        let len = orbital_recv_bytes(buffer.as_mut_ptr(), buffer.len() as u32);
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
