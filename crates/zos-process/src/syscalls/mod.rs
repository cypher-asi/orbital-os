//! Core syscall wrappers for Zero OS

extern crate alloc;
#[cfg(target_arch = "wasm32")]
use alloc::string::ToString;
use crate::error;
// Import syscall numbers (re-exported from zos-ipc at crate root)
#[allow(unused_imports)]
use crate::{
    SYS_CALL, SYS_CAP_DELETE, SYS_CAP_DERIVE, SYS_CAP_GRANT, SYS_CAP_INSPECT, SYS_CAP_LIST,
    SYS_CAP_REVOKE, SYS_CONSOLE_WRITE, SYS_CREATE_ENDPOINT, SYS_CREATE_ENDPOINT_FOR, SYS_DEBUG,
    SYS_DELETE_ENDPOINT, SYS_EXIT, SYS_KILL, SYS_LOAD_BINARY, SYS_PS, SYS_RECV, SYS_REGISTER_PROCESS,
    SYS_REPLY, SYS_SEND, SYS_SEND_CAP, SYS_SPAWN_PROCESS, SYS_TIME, SYS_WALLCLOCK, SYS_YIELD,
};
use crate::types::{CapInfo, Permissions, ProcessInfo, ReceivedMessage};
use alloc::vec::Vec;

pub mod keystore;
pub mod network;
pub mod storage;

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
// Basic Process Syscalls
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

/// Get uptime in nanoseconds
#[cfg(target_arch = "wasm32")]
pub fn get_time() -> u64 {
    unsafe {
        let low = zos_syscall(SYS_TIME, 0, 0, 0);
        let high = zos_syscall(SYS_TIME, 1, 0, 0);
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
        let low = zos_syscall(SYS_WALLCLOCK, 0, 0, 0);
        let high = zos_syscall(SYS_WALLCLOCK, 1, 0, 0);
        ((high as u64) << 32) | (low as u64)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn get_wallclock() -> u64 {
    // Return mock timestamp for native testing
    // This crate is no_std, so we don't have access to system time
    // Real time comes from HAL in actual runtime
    1737504000000
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
// IPC Syscalls
// ============================================================================

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

/// Receive a message from an endpoint (non-blocking).
///
/// # Returns
/// - `Ok(msg)`: Successfully received a message
/// - `Err(RecvError::NoMessage)`: No message available (try again later)
/// - `Err(RecvError::PermissionDenied)`: No permission to receive on this endpoint
/// - `Err(RecvError::InvalidEndpoint)`: Invalid endpoint slot
/// - `Err(RecvError::ParseError)`: Message data was malformed
#[cfg(target_arch = "wasm32")]
pub fn receive(endpoint_slot: u32) -> Result<ReceivedMessage, error::RecvError> {
    use error::RecvError;

    // Buffer sized to support large IPC messages (e.g., PQ hybrid keys ~6KB)
    let mut buffer = [0u8; 16384];
    unsafe {
        let result = zos_syscall(SYS_RECV, endpoint_slot, 0, 0) as i32;
        
        if result <= 0 {
            // 0 = no message, negative = error code
            return Err(RecvError::from_code(result));
        }
        
        // CRITICAL: Get the message data BEFORE any debug logging!
        // Debug logging makes a SYS_DEBUG syscall which clears the mailbox buffer.
        let len = zos_recv_bytes(buffer.as_mut_ptr(), buffer.len() as u32);
        
        if len == 0 {
            // This should never happen - if SYS_RECV returned success, there should be data
            return Err(RecvError::ParseError);
        }

        // Parse message format:
        // [from_pid: u32][tag: u32][num_caps: u8][cap_slots: u32*num_caps][data: ...]
        // Minimum: 4 + 4 + 1 = 9 bytes
        if len < 9 {
            return Err(RecvError::ParseError);
        }
        let from_pid = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        let tag = u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
        let num_caps = buffer[8] as usize;

        // Parse capability slots with overflow check
        let cap_data_len = num_caps.checked_mul(4).ok_or(RecvError::ParseError)?;
        let data_start = 9usize.checked_add(cap_data_len).ok_or(RecvError::ParseError)?;
        if (len as usize) < data_start {
            return Err(RecvError::ParseError);
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
        Ok(ReceivedMessage {
            from_pid,
            tag,
            cap_slots,
            data,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn receive(_endpoint_slot: u32) -> Result<ReceivedMessage, error::RecvError> {
    Err(error::RecvError::NoMessage)
}

/// Receive a message from an endpoint (legacy, returns Option).
///
/// **Deprecated**: Prefer `receive()` which returns `Result<_, RecvError>` for
/// better error handling.
#[cfg(target_arch = "wasm32")]
pub fn receive_opt(endpoint_slot: u32) -> Option<ReceivedMessage> {
    receive(endpoint_slot).ok()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn receive_opt(_endpoint_slot: u32) -> Option<ReceivedMessage> {
    None
}

/// Receive a message, blocking until one arrives.
///
/// This polls `receive()` in a loop, yielding between attempts.
/// Returns immediately on non-recoverable errors (permission denied, invalid endpoint).
#[cfg(target_arch = "wasm32")]
pub fn receive_blocking(endpoint_slot: u32) -> Result<ReceivedMessage, error::RecvError> {
    use error::RecvError;
    
    loop {
        match receive(endpoint_slot) {
            Ok(msg) => return Ok(msg),
            Err(RecvError::NoMessage) => yield_now(), // Keep polling
            Err(e) => return Err(e), // Non-recoverable error
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn receive_blocking(_endpoint_slot: u32) -> Result<ReceivedMessage, error::RecvError> {
    Err(error::RecvError::NoMessage)
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
        if let Ok(msg) = receive(endpoint_slot) {
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

// ============================================================================
// Capability Syscalls
// ============================================================================

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
/// This is a privileged syscall for the PermissionService (PID 2).
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

/// Create an IPC endpoint
///
/// # Returns
/// - `Ok((endpoint_id, slot))`: Endpoint ID and capability slot
/// - `Err(code)`: Error code
///
/// # ABI Format
/// The kernel returns a packed u64: `(slot << 32) | (endpoint_id & 0xFFFFFFFF)`
/// This is consistent with `create_endpoint_for`.
#[cfg(target_arch = "wasm32")]
pub fn create_endpoint() -> Result<(u64, u32), u32> {
    unsafe {
        let result = zos_syscall(SYS_CREATE_ENDPOINT, 0, 0, 0) as i64;
        if result >= 0 {
            // Kernel returns packed: (slot << 32) | endpoint_id
            // Unpack: slot in high 32 bits, endpoint_id in low 32 bits
            let slot = (result >> 32) as u32;
            let endpoint_id = (result & 0xFFFFFFFF) as u64;
            Ok((endpoint_id, slot))
        } else {
            Err((-result) as u32)
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
///
/// # ABI Format
/// The kernel returns a packed i64: `(slot << 32) | (endpoint_id & 0xFFFFFFFF)`
/// This is consistent with `create_endpoint`.
#[cfg(target_arch = "wasm32")]
pub fn create_endpoint_for(target_pid: u32) -> Result<(u64, u32), u32> {
    unsafe {
        let result = zos_syscall(SYS_CREATE_ENDPOINT_FOR, target_pid, 0, 0) as i64;
        if result >= 0 {
            // Kernel returns packed: (slot << 32) | endpoint_id
            // Unpack: slot in high 32 bits, endpoint_id in low 32 bits
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

/// Load a binary by name from platform storage (Init-only syscall).
///
/// This syscall is part of the pure microkernel spawn protocol. Init uses this
/// to load service binaries before spawning them with `spawn_process`.
///
/// # Platform Behavior
/// - **QEMU**: Returns embedded binary data
/// - **WASM**: Returns `NotSupported` error (use Supervisor async flow instead)
///
/// # Arguments
/// - `name`: Binary name (e.g., "permission_service", "vfs_service")
///
/// # Returns
/// - `Ok(Vec<u8>)`: Binary data
/// - `Err(code)`: Error code
///   - `PERMISSION_DENIED (-4)`: Caller is not Init
///   - `NOT_FOUND (-2)`: Binary not found
///   - `NOT_SUPPORTED (-3)`: Platform doesn't support sync loading
#[cfg(target_arch = "wasm32")]
pub fn load_binary(name: &str) -> Result<Vec<u8>, i32> {
    use crate::SYS_LOAD_BINARY;
    
    let bytes = name.as_bytes();
    unsafe {
        zos_send_bytes(bytes.as_ptr(), bytes.len() as u32);
        let result = zos_syscall(SYS_LOAD_BINARY, bytes.len() as u32, 0, 0) as i32;
        
        if result < 0 {
            // Negative result is an error code
            return Err(result);
        }
        
        // Positive result is the binary size
        let binary_size = result as usize;
        let mut buffer = alloc::vec![0u8; binary_size];
        let received = zos_recv_bytes(buffer.as_mut_ptr(), binary_size as u32) as usize;
        
        if received != binary_size {
            // Truncation - shouldn't happen but handle gracefully
            buffer.truncate(received);
        }
        
        Ok(buffer)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_binary(_name: &str) -> Result<Vec<u8>, i32> {
    // NOT_SUPPORTED error code
    Err(-3)
}

/// Spawn a process from binary data (Init-only syscall).
///
/// This syscall is part of the pure microkernel spawn protocol. Init uses this
/// to spawn processes after loading their binary with `load_binary`.
///
/// # Arguments
/// - `name`: Process name for debugging/identification
/// - `binary`: WASM binary data
///
/// # Returns
/// - `Ok(pid)`: PID of the spawned process
/// - `Err(code)`: Error code
///   - `PERMISSION_DENIED (-4)`: Caller is not Init
///   - `INVALID_ARGUMENT (-5)`: Missing or invalid payload
///   - `SPAWN_FAILED (-6)`: HAL failed to spawn process
#[cfg(target_arch = "wasm32")]
pub fn spawn_process(name: &str, binary: &[u8]) -> Result<u32, i32> {
    use crate::SYS_SPAWN_PROCESS;
    
    // Build payload: [name_len: u32 (LE), name: [u8], binary: [u8]]
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len() as u32;
    
    let mut payload = Vec::with_capacity(4 + name_bytes.len() + binary.len());
    payload.extend_from_slice(&name_len.to_le_bytes());
    payload.extend_from_slice(name_bytes);
    payload.extend_from_slice(binary);
    
    unsafe {
        zos_send_bytes(payload.as_ptr(), payload.len() as u32);
        let result = zos_syscall(SYS_SPAWN_PROCESS, payload.len() as u32, 0, 0) as i32;
        
        if result < 0 {
            Err(result)
        } else {
            Ok(result as u32)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_process(_name: &str, _binary: &[u8]) -> Result<u32, i32> {
    // NOT_SUPPORTED error code
    Err(-3)
}

// ============================================================================
// Introspection Syscalls
// ============================================================================

/// List all capabilities in this process's capability space
#[cfg(target_arch = "wasm32")]
pub fn list_caps() -> Vec<CapInfo> {
    let mut buffer = [0u8; 4096];
    unsafe {
        let result = zos_syscall(SYS_CAP_LIST, 0, 0, 0);
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
                can_read: true,   // placeholder
                can_write: true,  // placeholder
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
        let result = zos_syscall(SYS_PS, 0, 0, 0);
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
