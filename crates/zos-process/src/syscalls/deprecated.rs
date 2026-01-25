//! Deprecated VFS syscall wrappers for Zero OS
//!
//! These functions are deprecated. Use zos_vfs::VfsClient for VFS operations.
//! VFS operations now go through the VFS IPC service, which maintains the
//! thin-supervisor architecture principle.

extern crate alloc;
use alloc::string::ToString;
use crate::error;
// Note: VFS syscalls are deprecated and not moved to zos-kernel
// These remain here for backward compatibility only
pub const SYS_VFS_READ: u32 = 0x80;
pub const SYS_VFS_WRITE: u32 = 0x81;
pub const SYS_VFS_MKDIR: u32 = 0x82;
pub const SYS_VFS_LIST: u32 = 0x83;
pub const SYS_VFS_DELETE: u32 = 0x84;
pub const SYS_VFS_EXISTS: u32 = 0x85;
use alloc::string::String;
use alloc::vec::Vec;

#[cfg(target_arch = "wasm32")]
extern "C" {
    fn zos_syscall(syscall_num: u32, arg1: u32, arg2: u32, arg3: u32) -> u32;
    fn zos_send_bytes(ptr: *const u8, len: u32);
    fn zos_recv_bytes(ptr: *mut u8, max_len: u32) -> u32;
}

// ============================================================================
// VFS Syscall Wrappers (DEPRECATED)
// ============================================================================

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
#[deprecated(
    since = "0.1.0",
    note = "Use zos_vfs::VfsClient::read_file() via VFS IPC service"
)]
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

#[deprecated(
    since = "0.1.0",
    note = "Use zos_vfs::VfsClient::read_file() via VFS IPC service"
)]
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
#[deprecated(
    since = "0.1.0",
    note = "Use zos_vfs::VfsClient::write_file() via VFS IPC service"
)]
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
        let result = zos_syscall(
            SYS_VFS_WRITE,
            path_bytes.len() as u32,
            content.len() as u32,
            0,
        );
        if result == 0 {
            Ok(())
        } else {
            Err(result)
        }
    }
}

#[deprecated(
    since = "0.1.0",
    note = "Use zos_vfs::VfsClient::write_file() via VFS IPC service"
)]
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
#[deprecated(
    since = "0.1.0",
    note = "Use zos_vfs::VfsClient::mkdir() via VFS IPC service"
)]
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

#[deprecated(
    since = "0.1.0",
    note = "Use zos_vfs::VfsClient::mkdir() via VFS IPC service"
)]
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
#[deprecated(
    since = "0.1.0",
    note = "Use zos_vfs::VfsClient::readdir() via VFS IPC service"
)]
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

#[deprecated(
    since = "0.1.0",
    note = "Use zos_vfs::VfsClient::readdir() via VFS IPC service"
)]
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
#[deprecated(
    since = "0.1.0",
    note = "Use zos_vfs::VfsClient::unlink() via VFS IPC service"
)]
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

#[deprecated(
    since = "0.1.0",
    note = "Use zos_vfs::VfsClient::unlink() via VFS IPC service"
)]
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
#[deprecated(
    since = "0.1.0",
    note = "Use zos_vfs::VfsClient::exists() via VFS IPC service"
)]
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

#[deprecated(
    since = "0.1.0",
    note = "Use zos_vfs::VfsClient::exists() via VFS IPC service"
)]
#[cfg(not(target_arch = "wasm32"))]
pub fn vfs_exists(_path: &str) -> Result<bool, u32> {
    Err(error::E_NOSYS)
}
