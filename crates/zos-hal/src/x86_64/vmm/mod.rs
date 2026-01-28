//! Virtual Memory Manager for x86_64
//!
//! Provides 4-level page table management, physical frame allocation,
//! and per-process address spaces.
//!
//! # Architecture
//!
//! x86_64 uses 48-bit virtual addresses with 4-level page tables:
//! - PML4 (Page Map Level 4): 512 entries, bits 47-39
//! - PDPT (Page Directory Pointer Table): 512 entries, bits 38-30  
//! - PD (Page Directory): 512 entries, bits 29-21
//! - PT (Page Table): 512 entries, bits 20-12
//! - Page offset: 12 bits (4KB pages)
//!
//! # Memory Layout
//!
//! ```text
//! 0xFFFF_FFFF_FFFF_FFFF  ┌─────────────────────────────────────┐
//!                        │           Kernel Space              │
//!                        │  - Kernel code and data             │
//!                        │  - Kernel heap                      │
//!                        │  - Physical memory mapping          │
//! 0xFFFF_8000_0000_0000  ├─────────────────────────────────────┤
//!                        │        (non-canonical hole)         │
//! 0x0000_7FFF_FFFF_FFFF  ├─────────────────────────────────────┤
//!                        │           User Space                │
//!                        │  - Process code (.text)             │
//!                        │  - Process data (.data, .bss)       │
//!                        │  - Process heap                     │
//!                        │  - Process stack                    │
//! 0x0000_0000_0000_0000  └─────────────────────────────────────┘
//! ```

pub mod address_space;
pub mod frame_allocator;
pub mod page_table;
pub mod tlb;

pub use address_space::{AddressSpace, MemoryBacking, MemoryProtection, MemoryRegion};
pub use frame_allocator::FrameAllocator;
pub use page_table::{PageFlags, PageTable, PageTableEntry};

use spin::Mutex;
use x86_64::structures::paging::PhysFrame;
use x86_64::{PhysAddr, VirtAddr};

/// Page size (4KB)
pub const PAGE_SIZE: usize = 4096;

/// Kernel space start address (higher half)
pub const KERNEL_SPACE_START: u64 = 0xFFFF_8000_0000_0000;

/// User space end address
pub const USER_SPACE_END: u64 = 0x0000_7FFF_FFFF_FFFF;

/// Global frame allocator
static FRAME_ALLOCATOR: Mutex<Option<FrameAllocator>> = Mutex::new(None);

/// Physical memory offset (set during init)
static mut PHYS_MEM_OFFSET: u64 = 0;

/// Descriptor for a memory region from the bootloader
#[derive(Debug, Clone, Copy)]
pub struct MemoryRegionDescriptor {
    /// Physical start address
    pub start: u64,
    /// Region size in bytes
    pub size: u64,
    /// Region type
    pub kind: MemoryRegionKind,
}

/// Types of memory regions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRegionKind {
    /// Usable RAM
    Usable,
    /// Reserved by firmware/BIOS
    Reserved,
    /// ACPI reclaimable
    AcpiReclaimable,
    /// ACPI NVS
    AcpiNvs,
    /// Bad memory
    BadMemory,
    /// Bootloader/kernel reserved
    BootloaderReserved,
    /// Kernel code/data
    Kernel,
    /// Unknown
    Unknown,
}

/// Initialize the VMM with physical memory information
///
/// # Arguments
/// * `physical_memory_offset` - Virtual address where physical memory is mapped
/// * `memory_regions` - Memory map from bootloader
///
/// # Safety
/// Must be called only once during kernel initialization.
/// The physical_memory_offset must be valid.
pub unsafe fn init(physical_memory_offset: u64, memory_regions: &[MemoryRegionDescriptor]) {
    PHYS_MEM_OFFSET = physical_memory_offset;
    
    // Initialize frame allocator with usable regions
    let mut allocator = FrameAllocator::new();
    
    for region in memory_regions {
        if region.kind == MemoryRegionKind::Usable {
            allocator.add_region(region.start, region.size);
        }
    }
    
    *FRAME_ALLOCATOR.lock() = Some(allocator);
}

/// Get the physical memory offset
pub fn phys_mem_offset() -> u64 {
    unsafe { PHYS_MEM_OFFSET }
}

/// Convert a physical address to a virtual address using the offset mapping
pub fn phys_to_virt(phys: PhysAddr) -> VirtAddr {
    VirtAddr::new(phys.as_u64() + unsafe { PHYS_MEM_OFFSET })
}

/// Allocate a physical frame
pub fn allocate_frame() -> Option<PhysFrame> {
    let mut allocator = FRAME_ALLOCATOR.lock();
    allocator.as_mut()?.allocate_frame()
}

/// Free a physical frame
pub fn free_frame(frame: PhysFrame) {
    if let Some(allocator) = FRAME_ALLOCATOR.lock().as_mut() {
        allocator.free_frame(frame);
    }
}

/// Get frame allocator statistics
pub fn frame_stats() -> Option<(usize, usize)> {
    let allocator = FRAME_ALLOCATOR.lock();
    allocator.as_ref().map(|a| (a.free_frames(), a.total_frames()))
}

/// Create a new address space (returns PML4 physical address)
pub fn create_address_space() -> Option<AddressSpace> {
    AddressSpace::new()
}

/// Test VMM isolation between two address spaces
///
/// Creates two address spaces, writes to one, and verifies the other is unaffected.
/// Returns true if isolation is working correctly.
pub fn test_isolation() -> bool {
    crate::serial_println!("VMM: Testing address space isolation...");
    
    // Create two address spaces
    let mut space_a = match create_address_space() {
        Some(s) => s,
        None => {
            crate::serial_println!("VMM: Failed to create address space A");
            return false;
        }
    };
    
    let mut space_b = match create_address_space() {
        Some(s) => s,
        None => {
            crate::serial_println!("VMM: Failed to create address space B");
            return false;
        }
    };
    
    crate::serial_println!("VMM: Created address spaces A (PML4: {:?}) and B (PML4: {:?})",
        space_a.pml4_phys(), space_b.pml4_phys());
    
    // Test address for mapping (user space, page-aligned)
    let test_vaddr = VirtAddr::new(0x1000_0000);
    let prot = MemoryProtection::read_write();
    
    // Map a page in space A
    if space_a.map_page(test_vaddr, prot).is_err() {
        crate::serial_println!("VMM: Failed to map page in space A");
        return false;
    }
    crate::serial_println!("VMM: Mapped page at {:?} in space A", test_vaddr);
    
    // Write a test value to space A
    let test_value: u64 = 0xDEADBEEF_CAFEBABE;
    unsafe {
        let ptr = space_a.translate(test_vaddr)
            .map(|p| phys_to_virt(p).as_mut_ptr::<u64>());
        
        if let Some(ptr) = ptr {
            *ptr = test_value;
            crate::serial_println!("VMM: Wrote 0x{:X} to space A", test_value);
        } else {
            crate::serial_println!("VMM: Failed to translate address in space A");
            return false;
        }
    }
    
    // Map the same virtual address in space B
    if space_b.map_page(test_vaddr, prot).is_err() {
        crate::serial_println!("VMM: Failed to map page in space B");
        return false;
    }
    crate::serial_println!("VMM: Mapped page at {:?} in space B", test_vaddr);
    
    // Read from space B (should be different physical frame, so different value)
    let value_b = unsafe {
        space_b.translate(test_vaddr)
            .map(|p| phys_to_virt(p).as_ptr::<u64>())
            .map(|ptr| *ptr)
    };
    
    // Value in space B should NOT be our test value (it's a different frame)
    match value_b {
        Some(v) if v == test_value => {
            crate::serial_println!("VMM: ISOLATION FAILED! Space B has same value as A");
            false
        }
        Some(v) => {
            crate::serial_println!("VMM: Space B has value 0x{:X} (different from A)", v);
            crate::serial_println!("VMM: Isolation test PASSED!");
            true
        }
        None => {
            crate::serial_println!("VMM: Failed to read from space B");
            false
        }
    }
}
