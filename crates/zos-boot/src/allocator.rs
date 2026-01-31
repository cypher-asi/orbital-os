//! Kernel heap allocator
//!
//! Uses a simple linked list allocator for the kernel heap.
//! This is a basic allocator suitable for early boot; can be replaced
//! with a more sophisticated one later.

use linked_list_allocator::LockedHeap;

/// Kernel heap size
/// 
/// Needs to be large enough to:
/// - Parse and instantiate WASM modules via wasmi (each ~2-3x binary size)
/// - Allocate WASM value stacks (up to 768KB per process)
/// - Allocate WASM linear memory (3MB per process × 6 processes = 18MB)
/// - Allocate process structures and IPC buffers (~5MB for large binaries)
/// - Run the kernel's data structures
/// 
/// 48MB to support running multiple WASM services during boot.
/// Services include: init, permission, vfs, keystore,
/// identity (~1.1MB), and time.
pub const HEAP_SIZE: usize = 48 * 1024 * 1024;

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
