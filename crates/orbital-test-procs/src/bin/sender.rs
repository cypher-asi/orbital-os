//! Sender Test Process
//!
//! Sends configurable message bursts for IPC throughput testing.

#![cfg_attr(target_arch = "wasm32", no_std)]
#![cfg_attr(target_arch = "wasm32", no_main)]

#[cfg(target_arch = "wasm32")]
extern crate alloc;

#[cfg(target_arch = "wasm32")]
use alloc::vec;

#[cfg(not(target_arch = "wasm32"))]
use std::vec;

use orbital_process::{self as syscall};
use orbital_test_procs::{
    SenderStats, CMD_EXIT, CMD_QUERY, CMD_RESET, CMD_SEND_BURST, MSG_DATA, MSG_SENDER_STATS,
};

// Command endpoint slot
const CMD_ENDPOINT: u32 = 0;
// Report endpoint slot
const REPORT_ENDPOINT: u32 = 1;

/// Process entry point
#[no_mangle]
pub extern "C" fn _start() {
    let mut sent_count: u64 = 0;
    let mut sent_bytes: u64 = 0;
    let start_time = syscall::get_time();

    loop {
        if let Some(msg) = syscall::receive(CMD_ENDPOINT) {
            match msg.tag {
                CMD_SEND_BURST => {
                    // Parse: [count: u32, size: u32, target_slot: u32]
                    if msg.data.len() >= 12 {
                        let count = u32::from_le_bytes([
                            msg.data[0],
                            msg.data[1],
                            msg.data[2],
                            msg.data[3],
                        ]);
                        let size = u32::from_le_bytes([
                            msg.data[4],
                            msg.data[5],
                            msg.data[6],
                            msg.data[7],
                        ]) as usize;
                        let target_slot = u32::from_le_bytes([
                            msg.data[8],
                            msg.data[9],
                            msg.data[10],
                            msg.data[11],
                        ]);

                        let payload = vec![0x42u8; size];

                        for _ in 0..count {
                            if syscall::send(target_slot, MSG_DATA, &payload).is_ok() {
                                sent_count += 1;
                                sent_bytes += size as u64;
                            }
                        }

                        report_stats(sent_count, sent_bytes, start_time);
                    }
                }

                CMD_QUERY => {
                    report_stats(sent_count, sent_bytes, start_time);
                }

                CMD_RESET => {
                    sent_count = 0;
                    sent_bytes = 0;
                }

                CMD_EXIT => syscall::exit(0),
                _ => {}
            }
        }
        syscall::yield_now();
    }
}

fn report_stats(count: u64, bytes: u64, start: u64) {
    let elapsed = syscall::get_time() - start;
    let msgs_per_sec = if elapsed > 0 {
        count * 1_000_000_000 / elapsed
    } else {
        0
    };

    let stats = SenderStats {
        messages_sent: count,
        bytes_sent: bytes,
        elapsed_nanos: elapsed,
        msgs_per_sec,
    };

    let _ = syscall::send(REPORT_ENDPOINT, MSG_SENDER_STATS, &stats.to_bytes());
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
