//! Page Table structures and operations for x86_64
//!
//! Implements 4-level page tables with support for 4KB pages.

use x86_64::{PhysAddr, VirtAddr};
use super::phys_to_virt;

/// Page table entry flags
#[allow(non_snake_case)] // Intentionally named like a type for ergonomic flag access
pub mod PageFlags {
    /// Page is present in memory
    pub const PRESENT: u64 = 1 << 0;
    /// Page is writable
    pub const WRITABLE: u64 = 1 << 1;
    /// Page is accessible from user mode
    pub const USER: u64 = 1 << 2;
    /// Write-through caching
    pub const WRITE_THROUGH: u64 = 1 << 3;
    /// Disable caching
    pub const NO_CACHE: u64 = 1 << 4;
    /// Page has been accessed
    pub const ACCESSED: u64 = 1 << 5;
    /// Page has been written to (dirty)
    pub const DIRTY: u64 = 1 << 6;
    /// Huge page (2MB or 1GB)
    pub const HUGE_PAGE: u64 = 1 << 7;
    /// Global page (not flushed on CR3 reload)
    pub const GLOBAL: u64 = 1 << 8;
    /// Disable execution (requires NX bit in EFER)
    pub const NO_EXECUTE: u64 = 1 << 63;
    
    /// Mask for physical address in entry (bits 12-51)
    pub const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
}

/// A page table entry (8 bytes)
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    /// Create an empty (not present) entry
    pub const fn empty() -> Self {
        Self(0)
    }
    
    /// Create an entry pointing to a physical frame with given flags
    pub fn new(phys_addr: PhysAddr, flags: u64) -> Self {
        Self((phys_addr.as_u64() & PageFlags::ADDR_MASK) | flags)
    }
    
    /// Check if the entry is present
    pub fn is_present(&self) -> bool {
        self.0 & PageFlags::PRESENT != 0
    }
    
    /// Check if the entry is writable
    pub fn is_writable(&self) -> bool {
        self.0 & PageFlags::WRITABLE != 0
    }
    
    /// Check if the entry is user-accessible
    pub fn is_user(&self) -> bool {
        self.0 & PageFlags::USER != 0
    }
    
    /// Check if the entry is a huge page
    pub fn is_huge(&self) -> bool {
        self.0 & PageFlags::HUGE_PAGE != 0
    }
    
    /// Get the physical address this entry points to
    pub fn phys_addr(&self) -> PhysAddr {
        PhysAddr::new(self.0 & PageFlags::ADDR_MASK)
    }
    
    /// Get the raw flags
    pub fn flags(&self) -> u64 {
        self.0 & !PageFlags::ADDR_MASK
    }
    
    /// Set the entry
    pub fn set(&mut self, phys_addr: PhysAddr, flags: u64) {
        self.0 = (phys_addr.as_u64() & PageFlags::ADDR_MASK) | flags;
    }
    
    /// Clear the entry
    pub fn clear(&mut self) {
        self.0 = 0;
    }
    
    /// Get the raw value
    pub fn raw(&self) -> u64 {
        self.0
    }
}

impl core::fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_present() {
            write!(f, "PTE({:?}, flags=0x{:X})", self.phys_addr(), self.flags())
        } else {
            write!(f, "PTE(empty)")
        }
    }
}

/// A page table (512 entries × 8 bytes = 4KB)
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; 512],
}

impl PageTable {
    /// Create an empty page table
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry::empty(); 512],
        }
    }
    
    /// Get an entry by index
    pub fn entry(&self, index: usize) -> &PageTableEntry {
        &self.entries[index]
    }
    
    /// Get a mutable entry by index
    pub fn entry_mut(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.entries[index]
    }
    
    /// Iterate over entries
    pub fn iter(&self) -> impl Iterator<Item = &PageTableEntry> {
        self.entries.iter()
    }
    
    /// Iterate mutably over entries
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut PageTableEntry> {
        self.entries.iter_mut()
    }
    
    /// Zero out the page table
    pub fn clear(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.clear();
        }
    }
}

impl Default for PageTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the PML4 index from a virtual address (bits 47-39)
pub fn pml4_index(vaddr: VirtAddr) -> usize {
    ((vaddr.as_u64() >> 39) & 0x1FF) as usize
}

/// Extract the PDPT index from a virtual address (bits 38-30)
pub fn pdpt_index(vaddr: VirtAddr) -> usize {
    ((vaddr.as_u64() >> 30) & 0x1FF) as usize
}

/// Extract the PD index from a virtual address (bits 29-21)
pub fn pd_index(vaddr: VirtAddr) -> usize {
    ((vaddr.as_u64() >> 21) & 0x1FF) as usize
}

/// Extract the PT index from a virtual address (bits 20-12)
pub fn pt_index(vaddr: VirtAddr) -> usize {
    ((vaddr.as_u64() >> 12) & 0x1FF) as usize
}

/// Extract the page offset from a virtual address (bits 11-0)
pub fn page_offset(vaddr: VirtAddr) -> usize {
    (vaddr.as_u64() & 0xFFF) as usize
}

/// Walk the page tables to translate a virtual address
///
/// Returns the physical address if the page is mapped, None otherwise.
pub fn translate(pml4_phys: PhysAddr, vaddr: VirtAddr) -> Option<PhysAddr> {
    // Get PML4 entry
    let pml4 = unsafe { &*phys_to_virt(pml4_phys).as_ptr::<PageTable>() };
    let pml4e = pml4.entry(pml4_index(vaddr));
    if !pml4e.is_present() {
        return None;
    }
    
    // Get PDPT entry
    let pdpt = unsafe { &*phys_to_virt(pml4e.phys_addr()).as_ptr::<PageTable>() };
    let pdpte = pdpt.entry(pdpt_index(vaddr));
    if !pdpte.is_present() {
        return None;
    }
    if pdpte.is_huge() {
        // 1GB huge page
        let offset = vaddr.as_u64() & 0x3FFF_FFFF; // 30 bits
        return Some(PhysAddr::new(pdpte.phys_addr().as_u64() + offset));
    }
    
    // Get PD entry
    let pd = unsafe { &*phys_to_virt(pdpte.phys_addr()).as_ptr::<PageTable>() };
    let pde = pd.entry(pd_index(vaddr));
    if !pde.is_present() {
        return None;
    }
    if pde.is_huge() {
        // 2MB huge page
        let offset = vaddr.as_u64() & 0x1F_FFFF; // 21 bits
        return Some(PhysAddr::new(pde.phys_addr().as_u64() + offset));
    }
    
    // Get PT entry
    let pt = unsafe { &*phys_to_virt(pde.phys_addr()).as_ptr::<PageTable>() };
    let pte = pt.entry(pt_index(vaddr));
    if !pte.is_present() {
        return None;
    }
    
    // 4KB page
    let offset = page_offset(vaddr);
    Some(PhysAddr::new(pte.phys_addr().as_u64() + offset as u64))
}

/// Map a virtual address to a physical address in the page tables
///
/// Creates intermediate page tables as needed.
///
/// # Arguments
/// * `pml4_phys` - Physical address of the PML4 table
/// * `vaddr` - Virtual address to map
/// * `paddr` - Physical address to map to
/// * `flags` - Page flags (PRESENT is added automatically)
/// * `allocate_frame` - Function to allocate a new frame for intermediate tables
///
/// # Safety
/// The caller must ensure the physical memory offset is valid and that
/// allocate_frame returns valid physical frames.
pub unsafe fn map_page<F>(
    pml4_phys: PhysAddr,
    vaddr: VirtAddr,
    paddr: PhysAddr,
    flags: u64,
    mut allocate_frame: F,
) -> Result<(), &'static str>
where
    F: FnMut() -> Option<PhysAddr>,
{
    let flags = flags | PageFlags::PRESENT;
    
    // Get or create PML4 entry
    let pml4 = &mut *phys_to_virt(pml4_phys).as_mut_ptr::<PageTable>();
    let pml4e = pml4.entry_mut(pml4_index(vaddr));
    let pdpt_phys = if pml4e.is_present() {
        pml4e.phys_addr()
    } else {
        let frame = allocate_frame().ok_or("Failed to allocate PDPT")?;
        // Zero the new table
        let pdpt = &mut *phys_to_virt(frame).as_mut_ptr::<PageTable>();
        pdpt.clear();
        pml4e.set(frame, PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER);
        frame
    };
    
    // Get or create PDPT entry
    let pdpt = &mut *phys_to_virt(pdpt_phys).as_mut_ptr::<PageTable>();
    let pdpte = pdpt.entry_mut(pdpt_index(vaddr));
    let pd_phys = if pdpte.is_present() {
        pdpte.phys_addr()
    } else {
        let frame = allocate_frame().ok_or("Failed to allocate PD")?;
        let pd = &mut *phys_to_virt(frame).as_mut_ptr::<PageTable>();
        pd.clear();
        pdpte.set(frame, PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER);
        frame
    };
    
    // Get or create PD entry
    let pd = &mut *phys_to_virt(pd_phys).as_mut_ptr::<PageTable>();
    let pde = pd.entry_mut(pd_index(vaddr));
    let pt_phys = if pde.is_present() {
        pde.phys_addr()
    } else {
        let frame = allocate_frame().ok_or("Failed to allocate PT")?;
        let pt = &mut *phys_to_virt(frame).as_mut_ptr::<PageTable>();
        pt.clear();
        pde.set(frame, PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER);
        frame
    };
    
    // Set PT entry
    let pt = &mut *phys_to_virt(pt_phys).as_mut_ptr::<PageTable>();
    let pte = pt.entry_mut(pt_index(vaddr));
    pte.set(paddr, flags);
    
    Ok(())
}

/// Unmap a virtual address from the page tables
///
/// Returns the physical address that was mapped, or None if not mapped.
pub unsafe fn unmap_page(pml4_phys: PhysAddr, vaddr: VirtAddr) -> Option<PhysAddr> {
    // Walk to the PT entry
    let pml4 = &mut *phys_to_virt(pml4_phys).as_mut_ptr::<PageTable>();
    let pml4e = pml4.entry(pml4_index(vaddr));
    if !pml4e.is_present() {
        return None;
    }
    
    let pdpt = &mut *phys_to_virt(pml4e.phys_addr()).as_mut_ptr::<PageTable>();
    let pdpte = pdpt.entry(pdpt_index(vaddr));
    if !pdpte.is_present() || pdpte.is_huge() {
        return None; // Don't handle huge pages in unmap
    }
    
    let pd = &mut *phys_to_virt(pdpte.phys_addr()).as_mut_ptr::<PageTable>();
    let pde = pd.entry(pd_index(vaddr));
    if !pde.is_present() || pde.is_huge() {
        return None;
    }
    
    let pt = &mut *phys_to_virt(pde.phys_addr()).as_mut_ptr::<PageTable>();
    let pte = pt.entry_mut(pt_index(vaddr));
    if !pte.is_present() {
        return None;
    }
    
    let phys = pte.phys_addr();
    pte.clear();
    
    Some(phys)
}
