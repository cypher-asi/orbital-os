//! TLB (Translation Lookaside Buffer) Management
//!
//! The TLB caches virtual-to-physical address translations.
//! It must be flushed when page table mappings change.

use x86_64::VirtAddr;

/// Flush a single page from the TLB
///
/// Uses the INVLPG instruction to invalidate the TLB entry
/// for the page containing the given virtual address.
#[inline]
pub fn flush_page(vaddr: VirtAddr) {
    unsafe {
        core::arch::asm!(
            "invlpg [{}]",
            in(reg) vaddr.as_u64(),
            options(nostack, preserves_flags)
        );
    }
}

/// Flush a range of pages from the TLB
///
/// Flushes all pages in the range [start, start + size).
pub fn flush_range(start: VirtAddr, size: usize) {
    let num_pages = (size + super::PAGE_SIZE - 1) / super::PAGE_SIZE;
    
    for i in 0..num_pages {
        let vaddr = VirtAddr::new(start.as_u64() + (i * super::PAGE_SIZE) as u64);
        flush_page(vaddr);
    }
}

/// Flush the entire TLB by reloading CR3
///
/// This is more expensive than flushing individual pages but necessary
/// when many mappings change (e.g., address space switch).
#[inline]
pub fn flush_all() {
    unsafe {
        // Read CR3 and write it back to flush all non-global TLB entries
        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack));
        core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nomem, nostack));
    }
}

/// Switch to a new address space by loading CR3
///
/// This automatically flushes non-global TLB entries.
///
/// # Safety
/// The physical address must point to a valid PML4 table.
#[inline]
pub unsafe fn switch_address_space(pml4_phys: x86_64::PhysAddr) {
    core::arch::asm!(
        "mov cr3, {}",
        in(reg) pml4_phys.as_u64(),
        options(nostack)
    );
}

/// Get the current CR3 value (current address space)
#[inline]
pub fn current_cr3() -> x86_64::PhysAddr {
    let cr3: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack));
    }
    x86_64::PhysAddr::new(cr3 & 0x000F_FFFF_FFFF_F000)
}
