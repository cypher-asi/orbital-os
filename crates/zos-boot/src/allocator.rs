//! Kernel heap allocator
//!
//! Uses a simple linked list allocator for the kernel heap.
//! This is a basic allocator suitable for early boot; can be replaced
//! with a more sophisticated one later.

use linked_list_allocator::LockedHeap;

/// Kernel heap size
/// 
/// Needs to be large enough to:
/// - Parse and instantiate WASM modules via wasmi
/// - Allocate process structures and IPC buffers
/// - Run the kernel's data structures
/// 
/// 4MB is a reasonable starting point for running multiple WASM processes.
pub const HEAP_SIZE: usize = 4 * 1024 * 1024;

/// The kernel heap allocator
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Kernel heap memory region
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

/// Initialize the heap allocator
///
/// # Safety
/// Must be called only once during kernel initialization.
pub unsafe fn init() {
    // Use raw pointer to avoid creating a reference to mutable static
    let heap_ptr = &raw const HEAP;
    let heap_start = (*heap_ptr).as_ptr() as usize;
    ALLOCATOR.lock().init(heap_start as *mut u8, HEAP_SIZE);
}
