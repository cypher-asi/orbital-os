//! WASM-specific runtime for Orbital OS processes
//!
//! This crate implements the syscall interface using native WASM atomics.
//! It is linked into process binaries when targeting wasm32.
//!
//! # Architecture
//!
//! Each process has a SharedArrayBuffer-backed mailbox for communicating with
//! the supervisor. The mailbox layout is:
//!
//! | Offset | Size | Field |
//! |--------|------|-------|
//! | 0 | 4 | status (0=idle, 1=pending, 2=ready) |
//! | 4 | 4 | syscall_num |
//! | 8 | 4 | arg0 |
//! | 12 | 4 | arg1 |
//! | 16 | 4 | arg2 |
//! | 20 | 4 | result |
//! | 24 | 4 | data_len |
//! | 28 | 4068 | data buffer |
//!
//! Syscall flow:
//! 1. Process writes syscall params to mailbox
//! 2. Process sets status to PENDING
//! 3. Process calls memory.atomic.wait32 (blocks until status changes)
//! 4. Supervisor reads syscall, processes it, writes result
//! 5. Supervisor sets status to READY and calls Atomics.notify()
//! 6. Process wakes up, reads result, sets status to IDLE

#![no_std]

// Status values
const STATUS_IDLE: i32 = 0;
const STATUS_PENDING: i32 = 1;
#[allow(dead_code)]
const STATUS_READY: i32 = 2; // Used by supervisor, kept for documentation

// Mailbox field offsets (in i32 units)
const OFFSET_STATUS: usize = 0;
const OFFSET_SYSCALL_NUM: usize = 1;
const OFFSET_ARG0: usize = 2;
const OFFSET_ARG1: usize = 3;
const OFFSET_ARG2: usize = 4;
const OFFSET_RESULT: usize = 5;
const OFFSET_DATA_LEN: usize = 6;
#[allow(dead_code)]
const OFFSET_DATA: usize = 7; // Start of data buffer (in i32 units = byte offset 28)

/// Base address of the syscall mailbox in linear memory
/// Set by __orbital_rt_init during process startup
static mut MAILBOX_BASE: *mut i32 = core::ptr::null_mut();

/// Initialize the WASM runtime with the mailbox address
///
/// # Safety
/// Must be called exactly once during process initialization, before any syscalls.
/// The mailbox_addr must point to valid shared memory.
#[no_mangle]
pub unsafe extern "C" fn __orbital_rt_init(mailbox_addr: usize) {
    MAILBOX_BASE = mailbox_addr as *mut i32;
}

/// Make a syscall using native WASM atomics
///
/// This function:
/// 1. Writes syscall parameters to the shared mailbox
/// 2. Sets status to PENDING
/// 3. Blocks using memory.atomic.wait32 until the supervisor processes it
/// 4. Reads and returns the result
///
/// # Safety
/// This is safe to call from WASM process code after __orbital_rt_init has been called.
#[no_mangle]
pub extern "C" fn orbital_syscall(syscall_num: u32, arg0: u32, arg1: u32, arg2: u32) -> i32 {
    unsafe {
        if MAILBOX_BASE.is_null() {
            return -1; // Not initialized
        }

        let base = MAILBOX_BASE;

        // Write syscall parameters using atomic stores
        atomic_store(base.add(OFFSET_SYSCALL_NUM), syscall_num as i32);
        atomic_store(base.add(OFFSET_ARG0), arg0 as i32);
        atomic_store(base.add(OFFSET_ARG1), arg1 as i32);
        atomic_store(base.add(OFFSET_ARG2), arg2 as i32);

        // Set status to PENDING (this signals the supervisor)
        atomic_store(base.add(OFFSET_STATUS), STATUS_PENDING);

        // Wait for supervisor to process the syscall
        // This blocks until status is no longer PENDING
        loop {
            // wait returns: 0 = woken by notify, 1 = value changed, 2 = timeout
            let _ = memory_atomic_wait32(base.add(OFFSET_STATUS), STATUS_PENDING, -1);

            // Check if status changed from PENDING
            let status = atomic_load(base.add(OFFSET_STATUS));
            if status != STATUS_PENDING {
                break;
            }
            // Spurious wakeup, wait again
        }

        // Read the result
        let result = atomic_load(base.add(OFFSET_RESULT));

        // Reset status to IDLE
        atomic_store(base.add(OFFSET_STATUS), STATUS_IDLE);

        result
    }
}

/// Send bytes to the syscall data buffer
///
/// Must be called before orbital_syscall when the syscall needs data.
#[no_mangle]
pub unsafe extern "C" fn orbital_send_bytes(ptr: *const u8, len: u32) -> u32 {
    if MAILBOX_BASE.is_null() || ptr.is_null() {
        return 0;
    }

    let max_len = 4068u32; // Maximum data buffer size
    let actual_len = if len > max_len { max_len } else { len };

    // Get pointer to data buffer (byte offset 28)
    let data_ptr = (MAILBOX_BASE as *mut u8).add(28);

    // Copy data to mailbox buffer
    for i in 0..actual_len as usize {
        let byte = *ptr.add(i);
        // Use atomic store for each byte to ensure visibility
        core::ptr::write_volatile(data_ptr.add(i), byte);
    }

    // Store data length
    atomic_store(MAILBOX_BASE.add(OFFSET_DATA_LEN), actual_len as i32);

    actual_len
}

/// Receive bytes from the syscall result buffer
///
/// Called after orbital_syscall to retrieve result data.
#[no_mangle]
pub unsafe extern "C" fn orbital_recv_bytes(ptr: *mut u8, max_len: u32) -> u32 {
    if MAILBOX_BASE.is_null() || ptr.is_null() {
        return 0;
    }

    // Read data length
    let data_len = atomic_load(MAILBOX_BASE.add(OFFSET_DATA_LEN)) as u32;
    let actual_len = if data_len > max_len {
        max_len
    } else {
        data_len
    };

    // Get pointer to data buffer (byte offset 28)
    let data_ptr = (MAILBOX_BASE as *const u8).add(28);

    // Copy data from mailbox buffer
    for i in 0..actual_len as usize {
        let byte = core::ptr::read_volatile(data_ptr.add(i));
        *ptr.add(i) = byte;
    }

    actual_len
}

/// Yield the current process's time slice
///
/// Does a short wait to allow the scheduler to run other processes.
#[no_mangle]
pub extern "C" fn orbital_yield() {
    unsafe {
        if MAILBOX_BASE.is_null() {
            return;
        }

        // Do a wait with 0 timeout on a dummy location
        // This yields to the browser's event loop briefly
        let yield_addr = MAILBOX_BASE.add(15); // Unused location in mailbox
        memory_atomic_wait32(yield_addr, 0, 0);
    }
}

/// Get the process's PID (stored by supervisor during init)
#[no_mangle]
pub extern "C" fn orbital_get_pid() -> u32 {
    unsafe {
        if MAILBOX_BASE.is_null() {
            return 0;
        }
        // PID is stored at a known offset (we'll use offset 14)
        atomic_load(MAILBOX_BASE.add(14)) as u32
    }
}

// ============================================================================
// Native WASM atomic intrinsics
// ============================================================================

/// Atomic store (i32)
#[inline(always)]
unsafe fn atomic_store(ptr: *mut i32, val: i32) {
    #[cfg(target_arch = "wasm32")]
    {
        core::arch::wasm32::i32_atomic_store(ptr as *mut i32, val);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        core::ptr::write_volatile(ptr, val);
    }
}

/// Atomic load (i32)
#[inline(always)]
unsafe fn atomic_load(ptr: *const i32) -> i32 {
    #[cfg(target_arch = "wasm32")]
    {
        core::arch::wasm32::i32_atomic_load(ptr as *mut i32)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        core::ptr::read_volatile(ptr)
    }
}

/// Atomic wait (blocks until value at ptr != expected, or timeout)
/// Returns: 0 = woken by notify, 1 = value mismatch, 2 = timeout
#[inline(always)]
#[allow(unused_variables)]
unsafe fn memory_atomic_wait32(ptr: *mut i32, expected: i32, timeout_ns: i64) -> i32 {
    #[cfg(target_arch = "wasm32")]
    {
        core::arch::wasm32::memory_atomic_wait32(ptr, expected, timeout_ns)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Non-WASM fallback: just check value
        if core::ptr::read_volatile(ptr) != expected {
            1 // Value mismatch
        } else {
            2 // Timeout (can't actually wait)
        }
    }
}
