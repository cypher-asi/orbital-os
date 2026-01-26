//! Async keystore syscalls for Zero OS
//!
//! These syscalls initiate async keystore operations and return a request_id
//! immediately. The result is delivered via MSG_KEYSTORE_RESULT IPC message.
//!
//! Only VfsService should use these - applications use VFS IPC with /keys/ paths.

#[cfg(not(target_arch = "wasm32"))]
use crate::error;
#[allow(unused_imports)]
use crate::{
    SYS_KEYSTORE_DELETE, SYS_KEYSTORE_EXISTS, SYS_KEYSTORE_LIST, SYS_KEYSTORE_READ,
    SYS_KEYSTORE_WRITE,
};
#[allow(unused_imports)]
use alloc::vec::Vec;

#[cfg(target_arch = "wasm32")]
extern "C" {
    fn zos_syscall(syscall_num: u32, arg1: u32, arg2: u32, arg3: u32) -> u32;
    fn zos_send_bytes(ptr: *const u8, len: u32);
}

// ============================================================================
// Async Keystore Syscalls (for VfsService)
// ============================================================================

/// Start async keystore read operation.
///
/// This syscall returns immediately with a request_id. When the operation
/// completes, the result is delivered via MSG_KEYSTORE_RESULT IPC message.
///
/// # Arguments
/// - `key`: Keystore path to read (e.g., "/keys/{user_id}/identity/public_keys.json")
///
/// # Returns
/// - `Ok(request_id)`: Request ID to match with result
/// - `Err(code)`: Failed to start operation
#[cfg(target_arch = "wasm32")]
pub fn keystore_read_async(key: &str) -> Result<u32, u32> {
    let key_bytes = key.as_bytes();
    unsafe {
        zos_send_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let result = zos_syscall(SYS_KEYSTORE_READ, key_bytes.len() as u32, 0, 0);
        if result as i32 >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn keystore_read_async(_key: &str) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}

/// Start async keystore write operation.
///
/// This syscall returns immediately with a request_id. When the operation
/// completes, the result is delivered via MSG_KEYSTORE_RESULT IPC message.
///
/// # Arguments
/// - `key`: Keystore path to write (e.g., "/keys/{user_id}/identity/public_keys.json")
/// - `value`: Data to store
///
/// # Returns
/// - `Ok(request_id)`: Request ID to match with result
/// - `Err(code)`: Failed to start operation
#[cfg(target_arch = "wasm32")]
pub fn keystore_write_async(key: &str, value: &[u8]) -> Result<u32, u32> {
    let key_bytes = key.as_bytes();
    // Data format: [key_len: u32, key: [u8], value: [u8]]
    let mut data = Vec::with_capacity(4 + key_bytes.len() + value.len());
    data.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
    data.extend_from_slice(key_bytes);
    data.extend_from_slice(value);

    unsafe {
        zos_send_bytes(data.as_ptr(), data.len() as u32);
        let result = zos_syscall(
            SYS_KEYSTORE_WRITE,
            key_bytes.len() as u32,
            value.len() as u32,
            0,
        );
        if result as i32 >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn keystore_write_async(_key: &str, _value: &[u8]) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}

/// Start async keystore delete operation.
///
/// This syscall returns immediately with a request_id. When the operation
/// completes, the result is delivered via MSG_KEYSTORE_RESULT IPC message.
///
/// # Arguments
/// - `key`: Keystore path to delete
///
/// # Returns
/// - `Ok(request_id)`: Request ID to match with result
/// - `Err(code)`: Failed to start operation
#[cfg(target_arch = "wasm32")]
pub fn keystore_delete_async(key: &str) -> Result<u32, u32> {
    let key_bytes = key.as_bytes();
    unsafe {
        zos_send_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let result = zos_syscall(SYS_KEYSTORE_DELETE, key_bytes.len() as u32, 0, 0);
        if result as i32 >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn keystore_delete_async(_key: &str) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}

/// Start async keystore list operation.
///
/// This syscall returns immediately with a request_id. When the operation
/// completes, the result is delivered via MSG_KEYSTORE_RESULT IPC message
/// with a JSON array of matching paths.
///
/// # Arguments
/// - `prefix`: Path prefix to match (e.g., "/keys/{user_id}/identity/machine")
///
/// # Returns
/// - `Ok(request_id)`: Request ID to match with result
/// - `Err(code)`: Failed to start operation
#[cfg(target_arch = "wasm32")]
pub fn keystore_list_async(prefix: &str) -> Result<u32, u32> {
    let prefix_bytes = prefix.as_bytes();
    unsafe {
        zos_send_bytes(prefix_bytes.as_ptr(), prefix_bytes.len() as u32);
        let result = zos_syscall(SYS_KEYSTORE_LIST, prefix_bytes.len() as u32, 0, 0);
        if result as i32 >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn keystore_list_async(_prefix: &str) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}

/// Start async keystore exists check.
///
/// This syscall returns immediately with a request_id. When the operation
/// completes, the result is delivered via MSG_KEYSTORE_RESULT IPC message
/// with EXISTS_OK result type (data byte: 1=exists, 0=not exists).
///
/// # Arguments
/// - `key`: Keystore path to check
///
/// # Returns
/// - `Ok(request_id)`: Request ID to match with result
/// - `Err(code)`: Failed to start operation
#[cfg(target_arch = "wasm32")]
pub fn keystore_exists_async(key: &str) -> Result<u32, u32> {
    let key_bytes = key.as_bytes();
    unsafe {
        zos_send_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let result = zos_syscall(SYS_KEYSTORE_EXISTS, key_bytes.len() as u32, 0, 0);
        if result as i32 >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn keystore_exists_async(_key: &str) -> Result<u32, u32> {
    Err(error::E_NOSYS)
}
