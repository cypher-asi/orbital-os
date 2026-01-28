//! Per-process Address Space management
//!
//! Each process has its own virtual address space with a unique PML4 table.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use x86_64::structures::paging::PhysFrame;
use x86_64::{PhysAddr, VirtAddr};

use super::page_table::{self, PageFlags, PageTable};
use super::{allocate_frame, free_frame, phys_to_virt, PAGE_SIZE};

/// Memory protection flags
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryProtection {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub user: bool,
}

impl MemoryProtection {
    /// Create read-only protection
    pub const fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            execute: false,
            user: true,
        }
    }

    /// Create read-write protection
    pub const fn read_write() -> Self {
        Self {
            read: true,
            write: true,
            execute: false,
            user: true,
        }
    }

    /// Create read-execute protection
    pub const fn read_execute() -> Self {
        Self {
            read: true,
            write: false,
            execute: true,
            user: true,
        }
    }

    /// Create read-write-execute protection (for code loading)
    pub const fn read_write_execute() -> Self {
        Self {
            read: true,
            write: true,
            execute: true,
            user: true,
        }
    }

    /// Create kernel-only protection
    pub const fn kernel() -> Self {
        Self {
            read: true,
            write: true,
            execute: true,
            user: false,
        }
    }

    /// Convert to page table flags
    pub fn to_page_flags(&self) -> u64 {
        let mut flags = PageFlags::PRESENT;
        
        if self.write {
            flags |= PageFlags::WRITABLE;
        }
        if self.user {
            flags |= PageFlags::USER;
        }
        if !self.execute {
            flags |= PageFlags::NO_EXECUTE;
        }
        
        flags
    }
}

/// Memory backing type
#[derive(Clone, Debug)]
pub enum MemoryBacking {
    /// Anonymous memory (zero-filled on demand)
    Anonymous,
    /// Backed by physical frames
    Physical { frames: Vec<PhysFrame> },
    /// Shared with another process
    Shared { shared_id: u64 },
}

/// A contiguous memory region
#[derive(Clone, Debug)]
pub struct MemoryRegion {
    /// Starting virtual address
    pub base: VirtAddr,
    /// Size in bytes
    pub size: usize,
    /// Protection flags
    pub prot: MemoryProtection,
    /// Backing (anonymous, physical, shared)
    pub backing: MemoryBacking,
}

/// Per-process address space
pub struct AddressSpace {
    /// Root page table (PML4) physical address
    pml4_phys: PhysAddr,
    /// PML4 frame (for cleanup)
    pml4_frame: PhysFrame,
    /// Memory regions (for tracking and cleanup)
    regions: BTreeMap<u64, MemoryRegion>,
    /// Total mapped pages
    mapped_pages: usize,
    /// Frames allocated for page tables (for cleanup)
    table_frames: Vec<PhysFrame>,
}

impl AddressSpace {
    /// Create a new address space with empty mappings
    pub fn new() -> Option<Self> {
        // Allocate the PML4 table
        let pml4_frame = allocate_frame()?;
        let pml4_phys = pml4_frame.start_address();
        
        // Zero the PML4 table
        unsafe {
            let pml4 = phys_to_virt(pml4_phys).as_mut_ptr::<PageTable>();
            (*pml4).clear();
        }
        
        Some(Self {
            pml4_phys,
            pml4_frame,
            regions: BTreeMap::new(),
            mapped_pages: 0,
            table_frames: Vec::new(),
        })
    }

    /// Get the PML4 physical address (for loading into CR3)
    pub fn pml4_phys(&self) -> PhysAddr {
        self.pml4_phys
    }

    /// Get the number of mapped pages
    pub fn mapped_pages(&self) -> usize {
        self.mapped_pages
    }

    /// Map a single page at the given virtual address
    ///
    /// Allocates a new physical frame and maps it.
    pub fn map_page(&mut self, vaddr: VirtAddr, prot: MemoryProtection) -> Result<(), &'static str> {
        // Allocate a physical frame for the page
        let frame = allocate_frame().ok_or("Failed to allocate frame for page")?;
        let paddr = frame.start_address();
        
        // Zero the frame
        unsafe {
            let ptr = phys_to_virt(paddr).as_mut_ptr::<u8>();
            core::ptr::write_bytes(ptr, 0, PAGE_SIZE);
        }
        
        // Map in page tables
        let flags = prot.to_page_flags();
        
        unsafe {
            page_table::map_page(self.pml4_phys, vaddr, paddr, flags, || {
                allocate_frame().map(|f| {
                    self.table_frames.push(f);
                    f.start_address()
                })
            })?;
        }
        
        // Track the region
        let region = MemoryRegion {
            base: vaddr,
            size: PAGE_SIZE,
            prot,
            backing: MemoryBacking::Physical { frames: alloc::vec![frame] },
        };
        self.regions.insert(vaddr.as_u64(), region);
        self.mapped_pages += 1;
        
        Ok(())
    }

    /// Map a range of memory
    ///
    /// # Arguments
    /// * `vaddr` - Starting virtual address (must be page-aligned)
    /// * `size` - Size in bytes (will be rounded up to page size)
    /// * `prot` - Memory protection
    pub fn map_range(
        &mut self,
        vaddr: VirtAddr,
        size: usize,
        prot: MemoryProtection,
    ) -> Result<(), &'static str> {
        let num_pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        let mut frames = Vec::with_capacity(num_pages);
        
        for i in 0..num_pages {
            let page_vaddr = VirtAddr::new(vaddr.as_u64() + (i * PAGE_SIZE) as u64);
            
            // Allocate frame
            let frame = allocate_frame().ok_or("Failed to allocate frame")?;
            let paddr = frame.start_address();
            
            // Zero the frame
            unsafe {
                let ptr = phys_to_virt(paddr).as_mut_ptr::<u8>();
                core::ptr::write_bytes(ptr, 0, PAGE_SIZE);
            }
            
            // Map in page tables
            let flags = prot.to_page_flags();
            unsafe {
                page_table::map_page(self.pml4_phys, page_vaddr, paddr, flags, || {
                    allocate_frame().map(|f| {
                        self.table_frames.push(f);
                        f.start_address()
                    })
                })?;
            }
            
            frames.push(frame);
        }
        
        // Track the region
        let region = MemoryRegion {
            base: vaddr,
            size: num_pages * PAGE_SIZE,
            prot,
            backing: MemoryBacking::Physical { frames },
        };
        self.regions.insert(vaddr.as_u64(), region);
        self.mapped_pages += num_pages;
        
        Ok(())
    }

    /// Unmap a page at the given virtual address
    pub fn unmap_page(&mut self, vaddr: VirtAddr) -> Result<(), &'static str> {
        // Unmap from page tables
        let paddr = unsafe { page_table::unmap_page(self.pml4_phys, vaddr) };
        
        if let Some(paddr) = paddr {
            // Free the physical frame
            free_frame(PhysFrame::containing_address(paddr));
            
            // Remove from regions
            self.regions.remove(&vaddr.as_u64());
            self.mapped_pages -= 1;
            
            // Flush TLB for this page
            super::tlb::flush_page(vaddr);
            
            Ok(())
        } else {
            Err("Page not mapped")
        }
    }

    /// Translate a virtual address to a physical address
    pub fn translate(&self, vaddr: VirtAddr) -> Option<PhysAddr> {
        page_table::translate(self.pml4_phys, vaddr)
    }

    /// Check if a virtual address is mapped
    pub fn is_mapped(&self, vaddr: VirtAddr) -> bool {
        self.translate(vaddr).is_some()
    }

    /// Get the region containing a virtual address
    pub fn get_region(&self, vaddr: VirtAddr) -> Option<&MemoryRegion> {
        // Find the region with the largest base <= vaddr
        self.regions
            .range(..=vaddr.as_u64())
            .next_back()
            .map(|(_, region)| region)
            .filter(|region| {
                let end = region.base.as_u64() + region.size as u64;
                vaddr.as_u64() < end
            })
    }

    /// Change protection for a mapped range
    pub fn protect(
        &mut self,
        vaddr: VirtAddr,
        size: usize,
        new_prot: MemoryProtection,
    ) -> Result<(), &'static str> {
        let num_pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        
        for i in 0..num_pages {
            let page_vaddr = VirtAddr::new(vaddr.as_u64() + (i * PAGE_SIZE) as u64);
            
            // Get current mapping
            if let Some(paddr) = self.translate(page_vaddr) {
                // Re-map with new protection
                let flags = new_prot.to_page_flags();
                unsafe {
                    page_table::map_page(self.pml4_phys, page_vaddr, paddr, flags, || {
                        allocate_frame().map(|f| {
                            self.table_frames.push(f);
                            f.start_address()
                        })
                    })?;
                }
                
                // Flush TLB
                super::tlb::flush_page(page_vaddr);
            }
        }
        
        // Update region protection
        if let Some(region) = self.regions.get_mut(&vaddr.as_u64()) {
            region.prot = new_prot;
        }
        
        Ok(())
    }

    /// Iterate over all regions
    pub fn regions(&self) -> impl Iterator<Item = &MemoryRegion> {
        self.regions.values()
    }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        // Free all mapped physical frames
        for region in self.regions.values() {
            if let MemoryBacking::Physical { frames } = &region.backing {
                for frame in frames {
                    free_frame(*frame);
                }
            }
        }
        
        // Free page table frames
        for frame in &self.table_frames {
            free_frame(*frame);
        }
        
        // Free the PML4 frame
        free_frame(self.pml4_frame);
    }
}
