//! Async network syscalls for Zero OS
//!
//! These syscalls initiate async network (HTTP) operations and return a request_id
//! immediately. The result is delivered via MSG_NET_RESULT IPC message.
//!
//! Only the Network Service should use these - applications use IPC to Network Service.

#[cfg(not(target_arch = "wasm32"))]
use crate::error;
#[allow(unused_imports)]
use crate::SYS_NETWORK_FETCH;

#[cfg(target_arch = "wasm32")]
extern "C" {
    fn zos_syscall(syscall_num: u32, arg1: u32, arg2: u32, arg3: u32) -> i64;
    fn zos_send_bytes(ptr: *const u8, len: u32);
}

// ============================================================================
// Async Network Syscalls (for Network Service)
// ============================================================================

/// Start async HTTP fetch operation.
///
/// This syscall returns immediately with a request_id. When the operation
/// completes, the result is delivered via MSG_NET_RESULT IPC message.
///
/// # Arguments
/// - `request_json`: JSON-serialized HttpRequest bytes
///
/// # Returns
/// - `Ok(request_id)`: Request ID to match with result
/// - `Err(code)`: Failed to start operation
#[cfg(target_arch = "wasm32")]
pub fn network_fetch_async(request_json: &[u8]) -> Result<i64, i64> {
    unsafe {
        zos_send_bytes(request_json.as_ptr(), request_json.len() as u32);
        let result = zos_syscall(SYS_NETWORK_FETCH, request_json.len() as u32, 0, 0);
        if result >= 0 {
            Ok(result)
        } else {
            Err(result)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn network_fetch_async(_request_json: &[u8]) -> Result<i64, i64> {
    Err(error::E_NOSYS as i64)
}
