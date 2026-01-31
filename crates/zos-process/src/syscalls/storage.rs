//! Async platform storage syscalls for Zero OS
//!
//! These syscalls initiate async storage operations and return a request_id
//! immediately. The result is delivered via MSG_STORAGE_RESULT IPC message.
//!
//! Only VfsService should use these - applications use zos_vfs::VfsClient.

#[cfg(not(target_arch = "wasm32"))]
use crate::error;
#[allow(unused_imports)]
use crate::{
    SYS_STORAGE_BATCH_WRITE, SYS_STORAGE_DELETE, SYS_STORAGE_EXISTS, SYS_STORAGE_LIST,
    SYS_STORAGE_READ, SYS_STORAGE_WRITE,
};
#[allow(unused_imports)]
use alloc::vec::Vec;

#[cfg(target_arch = "wasm32")]
extern "C" {
    fn zos_syscall(syscall_num: u32, arg1: u32, arg2: u32, arg3: u32) -> i64;
    fn zos_send_bytes(ptr: *const u8, len: u32);
}

// ============================================================================
// Async Platform Storage Syscalls (for VfsService)
// ============================================================================

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
pub fn storage_read_async(key: &str) -> Result<i64, i64> {
    let key_bytes = key.as_bytes();
    unsafe {
        zos_send_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let result = zos_syscall(SYS_STORAGE_READ, key_bytes.len() as u32, 0, 0);
        if result >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn storage_read_async(_key: &str) -> Result<i64, i64> {
    Err(error::E_NOSYS as i64)
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
pub fn storage_write_async(key: &str, value: &[u8]) -> Result<i64, i64> {
    let key_bytes = key.as_bytes();
    // Data format: [key_len: u32, key: [u8], value: [u8]]
    let mut data = Vec::with_capacity(4 + key_bytes.len() + value.len());
    data.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
    data.extend_from_slice(key_bytes);
    data.extend_from_slice(value);

    unsafe {
        zos_send_bytes(data.as_ptr(), data.len() as u32);
        let result = zos_syscall(
            SYS_STORAGE_WRITE,
            key_bytes.len() as u32,
            value.len() as u32,
            0,
        );
        if result >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn storage_write_async(_key: &str, _value: &[u8]) -> Result<i64, i64> {
    Err(error::E_NOSYS as i64)
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
pub fn storage_delete_async(key: &str) -> Result<i64, i64> {
    let key_bytes = key.as_bytes();
    unsafe {
        zos_send_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let result = zos_syscall(SYS_STORAGE_DELETE, key_bytes.len() as u32, 0, 0);
        if result >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn storage_delete_async(_key: &str) -> Result<i64, i64> {
    Err(error::E_NOSYS as i64)
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
pub fn storage_list_async(prefix: &str) -> Result<i64, i64> {
    let prefix_bytes = prefix.as_bytes();
    unsafe {
        zos_send_bytes(prefix_bytes.as_ptr(), prefix_bytes.len() as u32);
        let result = zos_syscall(SYS_STORAGE_LIST, prefix_bytes.len() as u32, 0, 0);
        if result >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn storage_list_async(_prefix: &str) -> Result<i64, i64> {
    Err(error::E_NOSYS as i64)
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
pub fn storage_exists_async(key: &str) -> Result<i64, i64> {
    let key_bytes = key.as_bytes();
    unsafe {
        zos_send_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let result = zos_syscall(SYS_STORAGE_EXISTS, key_bytes.len() as u32, 0, 0);
        if result >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn storage_exists_async(_key: &str) -> Result<i64, i64> {
    Err(error::E_NOSYS as i64)
}

/// Start async batch storage write operation.
///
/// This syscall writes multiple key-value pairs in a single IndexedDB transaction,
/// significantly reducing round-trip latency compared to individual writes.
/// Used by VFS mkdir with create_parents=true to write all parent inodes atomically.
///
/// # Arguments
/// - `items`: Array of (key, value) pairs to write
///
/// # Returns
/// - `Ok(request_id)`: Request ID to match with result
/// - `Err(code)`: Failed to start operation
#[cfg(target_arch = "wasm32")]
pub fn storage_batch_write_async(items: &[(&str, &[u8])]) -> Result<i64, i64> {
    // Data format: [count: u32, (key_len: u32, key: [u8], value_len: u32, value: [u8])*]
    let mut data = Vec::new();
    data.extend_from_slice(&(items.len() as u32).to_le_bytes());

    for (key, value) in items {
        let key_bytes = key.as_bytes();
        data.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
        data.extend_from_slice(key_bytes);
        data.extend_from_slice(&(value.len() as u32).to_le_bytes());
        data.extend_from_slice(value);
    }

    unsafe {
        zos_send_bytes(data.as_ptr(), data.len() as u32);
        let result = zos_syscall(
            SYS_STORAGE_BATCH_WRITE,
            items.len() as u32,
            0,
            0,
        );
        if result >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn storage_batch_write_async(_items: &[(&str, &[u8])]) -> Result<i64, i64> {
    Err(error::E_NOSYS as i64)
}
