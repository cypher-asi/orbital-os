//! Ping-Pong Test Process
//!
//! Measures IPC round-trip latency between two worker processes.
//!
//! Slot layout:
//! - Slot 0: Own endpoint (receives commands, pings, or pongs depending on mode)
//! - Slot 1: Granted capability to other process's endpoint (for sending)

#![cfg_attr(target_arch = "wasm32", no_std)]
#![cfg_attr(target_arch = "wasm32", no_main)]

#[cfg(target_arch = "wasm32")]
extern crate alloc;

#[cfg(target_arch = "wasm32")]
use alloc::format;
#[cfg(target_arch = "wasm32")]
use alloc::vec::Vec;

#[cfg(not(target_arch = "wasm32"))]
use std::format;
#[cfg(not(target_arch = "wasm32"))]
use std::vec::Vec;

use orbital_process::{self as syscall};
use orbital_test_procs::{CMD_EXIT, CMD_PING, CMD_PONG_MODE, MSG_PING, MSG_PONG};

// Slot 0: Own endpoint for receiving
const MY_ENDPOINT: u32 = 0;
// Slot 1: Granted capability to peer's endpoint for sending
const PEER_ENDPOINT: u32 = 1;

/// Process entry point
#[no_mangle]
pub extern "C" fn _start() {
    syscall::debug("pingpong: started, waiting for commands on slot 0");

    loop {
        if let Some(msg) = syscall::receive(MY_ENDPOINT) {
            match msg.tag {
                CMD_PING => {
                    // Run ping test: Parse [iterations: u32]
                    // We send pings to PEER_ENDPOINT (slot 1) and receive pongs on MY_ENDPOINT (slot 0)
                    let iterations = if msg.data.len() >= 4 {
                        u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]])
                    } else {
                        100 // default
                    };

                    syscall::debug(&format!(
                        "pingpong: starting PING mode, {} iterations",
                        iterations
                    ));
                    run_ping_mode(iterations);
                }

                CMD_PONG_MODE => {
                    syscall::debug("pingpong: entering PONG mode");
                    run_pong_mode();
                    syscall::debug("pingpong: exited PONG mode");
                }

                CMD_EXIT => {
                    syscall::debug("pingpong: received EXIT command");
                    syscall::exit(0);
                }

                _ => {
                    syscall::debug(&format!("pingpong: unknown command tag 0x{:x}", msg.tag));
                }
            }
        }
        syscall::yield_now();
    }
}

/// Ping mode: Send pings and measure round-trip time
fn run_ping_mode(iterations: u32) {
    let mut latencies: Vec<u64> = Vec::with_capacity(iterations as usize);
    let mut timeouts = 0u32;

    for i in 0..iterations {
        let start = syscall::get_time();

        // Send ping to peer (slot 1)
        if syscall::send(PEER_ENDPOINT, MSG_PING, &[]).is_err() {
            syscall::debug(&format!("pingpong: failed to send ping {}", i));
            continue;
        }

        // Wait for pong on our endpoint (slot 0) with timeout
        let mut received = false;
        let timeout_ns = 100_000_000; // 100ms timeout

        loop {
            if let Some(pong) = syscall::receive(MY_ENDPOINT) {
                if pong.tag == MSG_PONG {
                    let elapsed = syscall::get_time() - start;
                    latencies.push(elapsed);
                    received = true;
                    break;
                } else if pong.tag == CMD_EXIT {
                    syscall::debug("pingpong: received EXIT during ping test");
                    return;
                }
            }

            // Check timeout
            let now = syscall::get_time();
            if now - start > timeout_ns {
                timeouts += 1;
                break;
            }

            syscall::yield_now();
        }

        if !received && timeouts > 10 {
            syscall::debug(&format!(
                "pingpong: too many timeouts ({}), aborting",
                timeouts
            ));
            break;
        }
    }

    // Report results
    report_latencies(&latencies, timeouts);
}

/// Pong mode: Respond to pings
fn run_pong_mode() {
    let mut pong_count = 0u64;

    loop {
        if let Some(msg) = syscall::receive(MY_ENDPOINT) {
            match msg.tag {
                MSG_PING => {
                    // Send pong back to peer (slot 1)
                    if syscall::send(PEER_ENDPOINT, MSG_PONG, &[]).is_ok() {
                        pong_count += 1;
                    }
                }
                CMD_EXIT => {
                    syscall::debug(&format!("pingpong: PONG mode handled {} pings", pong_count));
                    return;
                }
                _ => {}
            }
        }
        syscall::yield_now();
    }
}

fn report_latencies(latencies: &[u64], timeouts: u32) {
    if latencies.is_empty() {
        syscall::debug(&format!(
            "pingpong: no successful pings (timeouts: {})",
            timeouts
        ));
        return;
    }

    let count = latencies.len() as u64;
    let sum: u64 = latencies.iter().sum();
    let min = *latencies.iter().min().unwrap_or(&0);
    let max = *latencies.iter().max().unwrap_or(&0);
    let avg = if count > 0 { sum / count } else { 0 };

    // Calculate median
    let mut sorted = latencies.to_vec();
    sorted.sort();
    let median = if count > 0 {
        sorted[count as usize / 2]
    } else {
        0
    };

    // Report via debug syscall so it appears in console
    syscall::debug("========== PING-PONG RESULTS ==========");
    syscall::debug(&format!("  Iterations: {} (timeouts: {})", count, timeouts));
    syscall::debug(&format!(
        "  Min:    {:>10} ns ({:.3} µs)",
        min,
        min as f64 / 1000.0
    ));
    syscall::debug(&format!(
        "  Max:    {:>10} ns ({:.3} µs)",
        max,
        max as f64 / 1000.0
    ));
    syscall::debug(&format!(
        "  Avg:    {:>10} ns ({:.3} µs)",
        avg,
        avg as f64 / 1000.0
    ));
    syscall::debug(&format!(
        "  Median: {:>10} ns ({:.3} µs)",
        median,
        median as f64 / 1000.0
    ));
    syscall::debug("========================================");
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
