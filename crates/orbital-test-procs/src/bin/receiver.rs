//! Receiver Test Process
//!
//! Receives and counts messages for IPC throughput testing.

#![cfg_attr(target_arch = "wasm32", no_std)]
#![cfg_attr(target_arch = "wasm32", no_main)]

#[cfg(target_arch = "wasm32")]
extern crate alloc;

use orbital_process::{self as syscall};
use orbital_test_procs::{
    ReceiverStats, CMD_EXIT, CMD_QUERY, CMD_RESET, MSG_DATA, MSG_RECEIVER_STATS,
};

// Command endpoint slot
const CMD_ENDPOINT: u32 = 0;
// Data endpoint slot (receives messages from sender)
const DATA_ENDPOINT: u32 = 1;
// Report endpoint slot
const REPORT_ENDPOINT: u32 = 2;

/// Process entry point
#[no_mangle]
pub extern "C" fn _start() {
    let mut received_count: u64 = 0;
    let mut received_bytes: u64 = 0;
    let mut first_msg_time: Option<u64> = None;
    let mut last_msg_time: u64 = 0;

    loop {
        // Check for commands (non-blocking)
        if let Some(cmd) = syscall::receive(CMD_ENDPOINT) {
            match cmd.tag {
                CMD_QUERY => {
                    report_stats(
                        received_count,
                        received_bytes,
                        first_msg_time,
                        last_msg_time,
                    );
                }
                CMD_RESET => {
                    received_count = 0;
                    received_bytes = 0;
                    first_msg_time = None;
                    last_msg_time = 0;
                }
                CMD_EXIT => syscall::exit(0),
                _ => {}
            }
        }

        // Check for data messages (non-blocking)
        if let Some(msg) = syscall::receive(DATA_ENDPOINT) {
            if msg.tag == MSG_DATA {
                let now = syscall::get_time();
                if first_msg_time.is_none() {
                    first_msg_time = Some(now);
                }
                last_msg_time = now;

                received_count += 1;
                received_bytes += msg.data.len() as u64;
            }
        }

        syscall::yield_now();
    }
}

fn report_stats(count: u64, bytes: u64, first: Option<u64>, last: u64) {
    let stats = ReceiverStats {
        messages_received: count,
        bytes_received: bytes,
        first_msg_time: first.unwrap_or(0),
        last_msg_time: last,
    };

    let _ = syscall::send(REPORT_ENDPOINT, MSG_RECEIVER_STATS, &stats.to_bytes());
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

    const HEAP_START: usize = 0x10000;
    const HEAP_SIZE: usize = 1024 * 1024; // 1MB heap

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
