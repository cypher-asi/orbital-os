# Virtual Memory Manager

> The VMM manages address spaces and memory protection. On WASM, this is minimal as the runtime handles linear memory.

## Overview

The Virtual Memory Manager provides:

1. **Address Spaces**: Per-process virtual memory mappings
2. **Memory Protection**: Read/write/execute permissions
3. **Capability Integration**: Memory regions as capability-protected objects

## WASM vs Native

| Feature            | WASM                        | Native (x86_64)              |
|--------------------|-----------------------------|------------------------------|
| Address space      | Linear memory (single)      | 48-bit virtual (per-process) |
| Page tables        | N/A (WASM runtime)          | 4-level x86_64 paging        |
| Protection         | WASM semantics              | Page table permissions       |
| Memory allocation  | `memory.grow`               | VMM + physical allocator     |
| Shared memory      | N/A                         | Shared page mappings         |

## WASM Memory Model (Phase 1)

On WASM, each process has a single linear memory managed by the WASM runtime:

```
┌─────────────────────────────────────────────────────────────────────┐
│                    WASM Linear Memory                                │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────────┐│
│  │                                                                ││
│  │  Address 0                                            Max addr ││
│  │  ├────────────────────────────────────────────────────────────┤││
│  │  │ Stack  │ Heap ────────────────────────────────▶│           │││
│  │  │  │     │                                       │ (unused)  │││
│  │  │  ▼     │                                       │           │││
│  │  ├────────┴───────────────────────────────────────┴───────────┤││
│  │                                                                ││
│  │  Size: pages × 64KB (WASM page = 64KB)                         ││
│  │  Max: 4GB (65536 pages)                                        ││
│  └────────────────────────────────────────────────────────────────┘│
│                                                                     │
│  Operations:                                                        │
│  - memory.grow(pages) → new_size or -1                             │
│  - memory.size → current pages                                     │
│  - Load/store at any valid address                                  │
└─────────────────────────────────────────────────────────────────────┘
```

### WASM Memory Tracking

The kernel tracks process memory size for metrics:

```rust
/// Update process memory size (called when WASM memory grows).
pub fn update_process_memory(kernel: &mut Kernel, pid: ProcessId, new_size: usize) {
    if let Some(proc) = kernel.processes.get_mut(&pid) {
        proc.metrics.memory_size = new_size;
    }
}

/// Get process memory size via HAL.
pub fn get_process_memory(kernel: &Kernel, pid: ProcessId) -> Option<usize> {
    let handle = kernel.process_handles.get(&pid)?;
    kernel.hal.get_process_memory_size(handle).ok()
}
```

### WASM VMM Syscalls

On WASM Phase 1, these syscalls are **not implemented** (return `ENOSYS`):

- `SYS_MMAP` (0x20)
- `SYS_MUNMAP` (0x21)
- `SYS_MPROTECT` (0x22)

Processes use standard WASM memory operations instead.

## Native Memory Model (Phase 2+)

On native x86_64 targets, full VMM is implemented.

### Address Space Layout

```
┌─────────────────────────────────────────────────────────────────────┐
│                    x86_64 Virtual Address Space                      │
│                    (48-bit, 256 TB)                                  │
│                                                                     │
│  0xFFFF_FFFF_FFFF_FFFF ┌─────────────────────────────────────────┐ │
│                        │           Kernel Space                   │ │
│                        │                                         │ │
│                        │  - Kernel code and data                 │ │
│                        │  - Kernel heap                          │ │
│                        │  - Direct physical mapping              │ │
│                        │  - Per-CPU data                         │ │
│                        │                                         │ │
│  0xFFFF_8000_0000_0000 ├─────────────────────────────────────────┤ │
│                        │           (non-canonical hole)          │ │
│  0x0000_7FFF_FFFF_FFFF ├─────────────────────────────────────────┤ │
│                        │           User Space                     │ │
│                        │                                         │ │
│                        │  - Process code (.text)                 │ │
│                        │  - Process data (.data, .bss)           │ │
│                        │  - Process heap                         │ │
│                        │  - Process stack                        │ │
│                        │  - Memory-mapped files                  │ │
│                        │  - Shared libraries                     │ │
│                        │                                         │ │
│  0x0000_0000_0000_0000 └─────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

### Page Table Structure

```rust
/// 4-level x86_64 page table.
///
/// Virtual address breakdown (48-bit):
/// - Bits 47-39: PML4 index (9 bits, 512 entries)
/// - Bits 38-30: PDPT index (9 bits, 512 entries)
/// - Bits 29-21: PD index (9 bits, 512 entries)
/// - Bits 20-12: PT index (9 bits, 512 entries)
/// - Bits 11-0: Page offset (12 bits, 4KB)

/// Page table entry flags.
pub mod PageFlags {
    pub const PRESENT: u64 = 1 << 0;
    pub const WRITABLE: u64 = 1 << 1;
    pub const USER: u64 = 1 << 2;
    pub const WRITE_THROUGH: u64 = 1 << 3;
    pub const NO_CACHE: u64 = 1 << 4;
    pub const ACCESSED: u64 = 1 << 5;
    pub const DIRTY: u64 = 1 << 6;
    pub const HUGE_PAGE: u64 = 1 << 7;
    pub const GLOBAL: u64 = 1 << 8;
    pub const NO_EXECUTE: u64 = 1 << 63;
}

/// Page table (512 entries × 8 bytes = 4KB).
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [u64; 512],
}
```

### Address Space Structure

```rust
/// Per-process address space.
pub struct AddressSpace {
    /// Root page table (PML4) physical address
    pub pml4_phys: PhysAddr,
    
    /// Memory regions (for tracking and cleanup)
    pub regions: BTreeMap<VirtAddr, MemoryRegion>,
    
    /// Total mapped pages
    pub mapped_pages: usize,
}

/// A contiguous memory region.
pub struct MemoryRegion {
    /// Starting virtual address
    pub base: VirtAddr,
    
    /// Size in bytes
    pub size: usize,
    
    /// Protection flags
    pub prot: MemoryProtection,
    
    /// Backing (anonymous, file-backed, shared)
    pub backing: MemoryBacking,
}

/// Memory protection flags.
#[derive(Clone, Copy, Debug)]
pub struct MemoryProtection {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

/// Memory backing type.
pub enum MemoryBacking {
    /// Anonymous memory (zero-filled on demand)
    Anonymous,
    /// Backed by physical frames
    Physical { frames: Vec<PhysAddr> },
    /// Shared with another process
    Shared { shared_id: u64 },
}
```

### VMM Operations

#### Map Memory

```rust
/// Map memory into an address space.
///
/// # Pre-conditions
/// - Caller has capability for address space with Write permission
/// - Region does not overlap existing mappings
/// - Physical frames available (if not demand-paged)
///
/// # Post-conditions
/// - Page tables updated
/// - Region added to address space
/// - Capability for region created
pub fn vmm_map(
    kernel: &mut Kernel,
    pid: ProcessId,
    vaddr: VirtAddr,
    size: usize,
    prot: MemoryProtection,
) -> Result<CapSlot, KernelError> {
    let addr_space = kernel.address_spaces.get_mut(&pid)
        .ok_or(KernelError::ProcessNotFound)?;
    
    // Check for overlap
    if addr_space.regions.range(..=vaddr).next_back()
        .map_or(false, |(_, r)| r.base + r.size > vaddr)
    {
        return Err(KernelError::InvalidArgument);
    }
    
    // Allocate physical frames
    let num_pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
    let frames = allocate_frames(num_pages)?;
    
    // Map in page tables
    for (i, &frame) in frames.iter().enumerate() {
        let page_vaddr = vaddr + i * PAGE_SIZE;
        map_page(addr_space.pml4_phys, page_vaddr, frame, prot)?;
    }
    
    // Add region
    let region = MemoryRegion {
        base: vaddr,
        size,
        prot,
        backing: MemoryBacking::Physical { frames },
    };
    addr_space.regions.insert(vaddr, region);
    addr_space.mapped_pages += num_pages;
    
    // Create capability
    let cap = create_memory_capability(kernel, pid, vaddr, size, prot)?;
    
    Ok(cap)
}
```

#### Unmap Memory

```rust
/// Unmap memory from an address space.
///
/// # Pre-conditions
/// - Capability for region with Write permission
/// - Region exists
///
/// # Post-conditions
/// - Page tables cleared
/// - Physical frames freed
/// - Region removed
/// - Capability invalidated
pub fn vmm_unmap(
    kernel: &mut Kernel,
    pid: ProcessId,
    vaddr: VirtAddr,
    size: usize,
) -> Result<(), KernelError> {
    let addr_space = kernel.address_spaces.get_mut(&pid)
        .ok_or(KernelError::ProcessNotFound)?;
    
    // Find and remove region
    let region = addr_space.regions.remove(&vaddr)
        .ok_or(KernelError::InvalidArgument)?;
    
    // Unmap pages
    let num_pages = (region.size + PAGE_SIZE - 1) / PAGE_SIZE;
    for i in 0..num_pages {
        let page_vaddr = vaddr + i * PAGE_SIZE;
        unmap_page(addr_space.pml4_phys, page_vaddr)?;
    }
    
    // Free physical frames
    if let MemoryBacking::Physical { frames } = region.backing {
        for frame in frames {
            free_frame(frame);
        }
    }
    
    addr_space.mapped_pages -= num_pages;
    
    Ok(())
}
```

#### Change Protection

```rust
/// Change memory protection for a region.
///
/// # Pre-conditions
/// - Capability for region with Write permission
/// - New protection is subset of original (can't add permissions)
///
/// # Post-conditions
/// - Page table permissions updated
/// - TLB flushed for affected range
pub fn vmm_protect(
    kernel: &mut Kernel,
    pid: ProcessId,
    vaddr: VirtAddr,
    size: usize,
    new_prot: MemoryProtection,
) -> Result<(), KernelError> {
    let addr_space = kernel.address_spaces.get_mut(&pid)
        .ok_or(KernelError::ProcessNotFound)?;
    
    let region = addr_space.regions.get_mut(&vaddr)
        .ok_or(KernelError::InvalidArgument)?;
    
    // Can only reduce permissions
    if (new_prot.read && !region.prot.read) ||
       (new_prot.write && !region.prot.write) ||
       (new_prot.execute && !region.prot.execute) {
        return Err(KernelError::PermissionDenied);
    }
    
    // Update page tables
    let num_pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
    for i in 0..num_pages {
        let page_vaddr = vaddr + i * PAGE_SIZE;
        update_page_protection(addr_space.pml4_phys, page_vaddr, new_prot)?;
    }
    
    // Flush TLB
    flush_tlb_range(vaddr, size);
    
    region.prot = new_prot;
    
    Ok(())
}
```

### Physical Frame Allocator

```rust
/// Physical frame allocator (buddy system).
pub struct FrameAllocator {
    /// Free lists by order (2^order pages)
    free_lists: [Vec<PhysAddr>; MAX_ORDER],
    /// Total available frames
    total_frames: usize,
    /// Currently free frames
    free_frames: usize,
}

impl FrameAllocator {
    /// Allocate `count` contiguous frames.
    pub fn allocate(&mut self, count: usize) -> Option<PhysAddr> {
        let order = count.next_power_of_two().trailing_zeros() as usize;
        self.allocate_order(order)
    }
    
    /// Allocate 2^order frames.
    fn allocate_order(&mut self, order: usize) -> Option<PhysAddr> {
        if let Some(frame) = self.free_lists[order].pop() {
            self.free_frames -= 1 << order;
            return Some(frame);
        }
        
        // Split larger block
        if order < MAX_ORDER - 1 {
            if let Some(large) = self.allocate_order(order + 1) {
                let buddy = large + ((1 << order) * PAGE_SIZE);
                self.free_lists[order].push(buddy);
                return Some(large);
            }
        }
        
        None
    }
    
    /// Free frames starting at `addr`.
    pub fn free(&mut self, addr: PhysAddr, count: usize) {
        let order = count.next_power_of_two().trailing_zeros() as usize;
        self.free_lists[order].push(addr);
        self.free_frames += count;
        // TODO: Coalesce buddies
    }
}
```

## Memory Capabilities

Memory regions are capability-protected:

```rust
/// Create a capability for a memory region.
fn create_memory_capability(
    kernel: &mut Kernel,
    pid: ProcessId,
    vaddr: VirtAddr,
    size: usize,
    prot: MemoryProtection,
) -> Result<CapSlot, KernelError> {
    let cap = Capability {
        id: kernel.next_cap_id(),
        object_type: ObjectType::Memory,
        object_id: vaddr as u64,  // Use vaddr as ID
        permissions: Permissions {
            read: prot.read,
            write: prot.write,
            grant: true,  // Owner can grant
        },
        generation: 0,
        expires_at: 0,
    };
    
    // Log creation
    kernel.axiom_log.append(
        pid,
        CapOperation::Create {
            cap_id: cap.id,
            object_type: ObjectType::Memory,
            object_id: vaddr as u64,
            holder: pid,
        },
        kernel.hal.now_nanos(),
    );
    
    let slot = kernel.cap_spaces.get_mut(&pid)
        .ok_or(KernelError::ProcessNotFound)?
        .insert(cap);
    
    Ok(slot)
}
```

## TLB Management

```rust
/// Flush TLB for a single page.
#[cfg(target_arch = "x86_64")]
pub fn flush_tlb_page(vaddr: VirtAddr) {
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) vaddr, options(nostack, preserves_flags));
    }
}

/// Flush TLB for a range.
pub fn flush_tlb_range(start: VirtAddr, size: usize) {
    let num_pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
    for i in 0..num_pages {
        flush_tlb_page(start + i * PAGE_SIZE);
    }
}

/// Flush entire TLB (CR3 reload).
#[cfg(target_arch = "x86_64")]
pub fn flush_tlb_all() {
    unsafe {
        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack));
        core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nomem, nostack));
    }
}
```

## LOC Budget

Target: ~800 LOC for VMM subsystem.

| Component           | Estimated LOC | WASM Phase 1 |
|---------------------|---------------|--------------|
| Address space struct| ~50           | Stub         |
| Page table ops      | ~200          | N/A          |
| Map/unmap/protect   | ~250          | N/A          |
| Frame allocator     | ~200          | N/A          |
| TLB management      | ~50           | N/A          |
| Memory capability   | ~50           | Partial      |
| **Total**           | **~800**      | ~100         |
