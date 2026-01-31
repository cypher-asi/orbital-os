//! Bump Allocator for Zero OS WASM Processes
//!
//! Provides a simple bump allocator with configurable heap size via const generic.
//! This eliminates code duplication across all WASM binaries that need an allocator.
//!
//! # Usage
//!
//! ```ignore
//! // At the crate root level:
//! zos_allocator::init!(1024 * 1024); // 1MB heap
//! ```
//!
//! # Heap Sizes by Binary
//!
//! | Binary | Heap Size | Rationale |
//! |--------|-----------|-----------|
//! | init | 4MB | Service registry, loading large binaries |
//! | idle | 64KB | Minimal - does nothing |
//! | pingpong | 1MB | Latency measurement with vectors |
//! | sender | 1MB | Message burst handling |
//! | receiver | 1MB | Message counting |
//! | memhog | 16MB | Memory stress testing |
//!
//! # Important: Heap Base
//!
//! The allocator uses the `__heap_base` linker symbol to determine where to start
//! allocating. This symbol is provided by wasm-ld and correctly accounts for the
//! data section and stack. Using a hardcoded value like 0x10000 can cause the
//! allocator to return addresses that overlap with static data, leading to corruption.

#![no_std]

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

// Import the __heap_base symbol from wasm-ld
// This tells us where the data section ends and the heap can begin
#[cfg(target_arch = "wasm32")]
extern "C" {
    static __heap_base: u8;
}

/// Get the heap base address from the linker symbol.
/// Falls back to 0x10000 on non-WASM targets.
#[cfg(target_arch = "wasm32")]
#[inline]
fn heap_base() -> usize {
    unsafe { &__heap_base as *const u8 as usize }
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
fn heap_base() -> usize {
    0x10000 // Fallback for non-WASM (not actually used)
}

/// Initialize the global allocator with the specified heap size in bytes.
///
/// This macro must be called exactly once at the crate root level.
/// It only activates on wasm32 targets.
///
/// # Example
///
/// ```ignore
/// zos_allocator::init!(1024 * 1024); // 1MB heap
/// ```
#[macro_export]
macro_rules! init {
    ($heap_size:expr) => {
        #[cfg(target_arch = "wasm32")]
        #[global_allocator]
        static ALLOCATOR: $crate::BumpAllocator<{ $heap_size }> = $crate::BumpAllocator::new();
    };
}

/// Bump allocator with configurable heap size.
///
/// The heap starts at `__heap_base` (determined by the linker) to properly
/// avoid conflicts with the WASM data section and stack.
///
/// This is a simple "bump pointer" allocator that:
/// - Allocates by incrementing a pointer
/// - Never deallocates (suitable for short-lived WASM processes)
/// - Is thread-safe via atomic operations
pub struct BumpAllocator<const SIZE: usize> {
    head: AtomicUsize,
}

impl<const SIZE: usize> BumpAllocator<SIZE> {
    /// Create a new bump allocator.
    pub const fn new() -> Self {
        Self {
            head: AtomicUsize::new(0),
        }
    }
}

// SAFETY: The allocator uses atomic operations for thread safety
unsafe impl<const SIZE: usize> Sync for BumpAllocator<SIZE> {}

unsafe impl<const SIZE: usize> GlobalAlloc for BumpAllocator<SIZE> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();
        let heap_start = heap_base();

        loop {
            let head = self.head.load(Ordering::Relaxed);
            let aligned = (heap_start + head + align - 1) & !(align - 1);
            let new_head = aligned - heap_start + size;

            if new_head > SIZE {
                return core::ptr::null_mut();
            }

            if self
                .head
                .compare_exchange_weak(head, new_head, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                return aligned as *mut u8;
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator doesn't deallocate - memory is reclaimed when process exits
    }
}
