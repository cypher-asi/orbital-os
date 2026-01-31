//! Custom getrandom implementation for QEMU
//!
//! This module provides a custom random number generator that uses the kernel's
//! SYS_RANDOM syscall (backed by RDRAND on x86_64) instead of wasm-bindgen's
//! JavaScript crypto API which isn't available in QEMU.
//!
//! To use this in WASM binaries for QEMU:
//! 1. Add `zos-process = { ..., features = ["custom-getrandom"] }` to Cargo.toml
//! 2. The getrandom crate will automatically use this implementation

use crate::SYS_RANDOM;

/// Maximum bytes per SYS_RANDOM call (kernel limit)
const MAX_RANDOM_BYTES: usize = 256;

/// Get random bytes from the kernel using SYS_RANDOM syscall
///
/// This uses the kernel's RDRAND-backed random number generator.
/// Returns the number of bytes written, or 0 on error.
#[cfg(target_arch = "wasm32")]
pub fn get_random_bytes(dest: &mut [u8]) -> usize {
    extern "C" {
        fn zos_syscall(syscall_num: u32, arg1: u32, arg2: u32, arg3: u32) -> u32;
        fn zos_recv_bytes(ptr: *mut u8, max_len: u32) -> u32;
    }

    let mut filled = 0;
    
    while filled < dest.len() {
        let remaining = dest.len() - filled;
        let chunk_size = core::cmp::min(remaining, MAX_RANDOM_BYTES);
        
        // Request random bytes from kernel
        let result = unsafe {
            zos_syscall(SYS_RANDOM, chunk_size as u32, 0, 0)
        };
        
        // Check for error
        if result == 0xFFFFFFFF || result == 0 {
            break;
        }
        
        // Retrieve the random bytes
        let bytes_to_read = core::cmp::min(result as usize, chunk_size);
        let bytes_received = unsafe {
            zos_recv_bytes(
                dest[filled..].as_mut_ptr(),
                bytes_to_read as u32
            )
        };
        
        filled += bytes_received as usize;
        
        // If we got fewer bytes than requested, stop
        if (bytes_received as usize) < bytes_to_read {
            break;
        }
    }
    
    filled
}

#[cfg(not(target_arch = "wasm32"))]
pub fn get_random_bytes(_dest: &mut [u8]) -> usize {
    0 // Not available on non-WASM
}

// ============================================================================
// Custom getrandom registration
// ============================================================================

/// Custom getrandom implementation for QEMU WASM
///
/// This is registered with the getrandom crate when the "custom-getrandom"
/// feature is enabled. It routes randomness requests through the kernel's
/// SYS_RANDOM syscall.
#[cfg(all(target_arch = "wasm32", feature = "custom-getrandom"))]
fn custom_getrandom(dest: &mut [u8]) -> Result<(), getrandom::Error> {
    let filled = get_random_bytes(dest);
    
    if filled == dest.len() {
        Ok(())
    } else {
        // Return an error code indicating the kernel RNG failed
        // Using error code 1 (UNSUPPORTED) since we can't get random data
        Err(getrandom::Error::UNSUPPORTED)
    }
}

// Register our custom getrandom implementation
#[cfg(all(target_arch = "wasm32", feature = "custom-getrandom"))]
getrandom::register_custom_getrandom!(custom_getrandom);
