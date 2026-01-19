//! Memory Hog Test Process
//!
//! Allocates memory on command and reports real WASM memory usage.
//! Used to test memory isolation between processes.

#![cfg_attr(target_arch = "wasm32", no_std)]
#![cfg_attr(target_arch = "wasm32", no_main)]

#[cfg(target_arch = "wasm32")]
extern crate alloc;

#[cfg(target_arch = "wasm32")]
use alloc::vec;
#[cfg(target_arch = "wasm32")]
use alloc::vec::Vec;

#[cfg(not(target_arch = "wasm32"))]
use std::vec;
#[cfg(not(target_arch = "wasm32"))]
use std::vec::Vec;

use orbital_process::{self as syscall, ReceivedMessage};
use orbital_test_procs::{
    MemoryStatus, CMD_ALLOC, CMD_EXIT, CMD_FREE, CMD_FREE_ALL, CMD_QUERY, MSG_MEMORY_STATUS,
};

// Command endpoint slot (assigned by supervisor)
const CMD_ENDPOINT: u32 = 0;
// Report endpoint slot
const REPORT_ENDPOINT: u32 = 1;

/// Stored allocations
static mut ALLOCATIONS: Vec<Vec<u8>> = Vec::new();

/// Process entry point
#[no_mangle]
pub extern "C" fn _start() {
    let mut total_allocated: usize = 0;

    // Main loop: wait for commands and execute them
    loop {
        // Wait for command (blocking receive)
        if let Some(msg) = syscall::receive(CMD_ENDPOINT) {
            total_allocated = handle_command(msg, total_allocated);
        }
        syscall::yield_now();
    }
}

fn handle_command(msg: ReceivedMessage, mut total_allocated: usize) -> usize {
    match msg.tag {
        CMD_ALLOC => {
            // Allocate requested bytes
            if msg.data.len() >= 4 {
                let size = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]])
                    as usize;
                let chunk = vec![0xABu8; size]; // Fill with pattern
                total_allocated += size;

                unsafe {
                    ALLOCATIONS.push(chunk);
                }

                report_memory_status(total_allocated);
            }
        }

        CMD_FREE => {
            // Free last allocation
            if let Some(chunk) = unsafe { ALLOCATIONS.pop() } {
                total_allocated = total_allocated.saturating_sub(chunk.len());
            }
            report_memory_status(total_allocated);
        }

        CMD_FREE_ALL => {
            unsafe {
                ALLOCATIONS.clear();
            }
            total_allocated = 0;
            report_memory_status(total_allocated);
        }

        CMD_QUERY => {
            report_memory_status(total_allocated);
        }

        CMD_EXIT => {
            syscall::exit(0);
        }

        _ => {}
    }

    total_allocated
}

fn report_memory_status(allocated: usize) {
    // Get real WASM memory stats
    #[cfg(target_arch = "wasm32")]
    let wasm_pages = core::arch::wasm32::memory_size(0);
    #[cfg(not(target_arch = "wasm32"))]
    let wasm_pages = 1; // Default for non-WASM

    let status = MemoryStatus {
        allocated_by_us: allocated as u64,
        allocation_count: unsafe { ALLOCATIONS.len() } as u32,
        _reserved: wasm_pages as u32,
    };

    let _ = syscall::send(REPORT_ENDPOINT, MSG_MEMORY_STATUS, &status.to_bytes());
}

// Native main (for cargo check/test)
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("This binary is meant to run as WASM in Orbital OS");
}

// ============================================================================
// Required for no_std WASM
// ============================================================================

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
        alloc::format!("PANIC: {}", s)
    } else {
        alloc::string::String::from("PANIC: unknown")
    };
    syscall::debug(&msg);
    syscall::exit(1);
}

#[cfg(target_arch = "wasm32")]
mod allocator {
    use core::alloc::{GlobalAlloc, Layout};

    struct BumpAllocator {
        head: core::sync::atomic::AtomicUsize,
    }

    #[global_allocator]
    static ALLOCATOR: BumpAllocator = BumpAllocator {
        head: core::sync::atomic::AtomicUsize::new(0),
    };

    const HEAP_START: usize = 0x10000; // 64KB offset
    const HEAP_SIZE: usize = 16 * 1024 * 1024; // 16MB heap for memhog

    unsafe impl GlobalAlloc for BumpAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let size = layout.size();
            let align = layout.align();

            loop {
                let head = self.head.load(core::sync::atomic::Ordering::Relaxed);
                let aligned = (HEAP_START + head + align - 1) & !(align - 1);
                let new_head = aligned - HEAP_START + size;

                if new_head > HEAP_SIZE {
                    return core::ptr::null_mut();
                }

                if self
                    .head
                    .compare_exchange_weak(
                        head,
                        new_head,
                        core::sync::atomic::Ordering::SeqCst,
                        core::sync::atomic::Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    return aligned as *mut u8;
                }
            }
        }

        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
            // Bump allocator doesn't deallocate
        }
    }
}
