//! Global Descriptor Table (GDT) setup for x86_64
//!
//! The GDT defines segment descriptors for kernel and user mode code/data,
//! as well as the Task State Segment (TSS) for interrupt stack switching.

use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

/// Size of the interrupt stack in bytes (16KB)
pub const INTERRUPT_STACK_SIZE: usize = 4096 * 4;

/// Stack index for double fault handler (uses IST entry 0)
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

/// Stack index for page fault handler (uses IST entry 1)
pub const PAGE_FAULT_IST_INDEX: u16 = 1;

/// The Task State Segment
static mut TSS: TaskStateSegment = TaskStateSegment::new();

/// Interrupt stack storage for double fault
static mut DOUBLE_FAULT_STACK: [u8; INTERRUPT_STACK_SIZE] = [0; INTERRUPT_STACK_SIZE];

/// Interrupt stack storage for page fault
static mut PAGE_FAULT_STACK: [u8; INTERRUPT_STACK_SIZE] = [0; INTERRUPT_STACK_SIZE];

/// The Global Descriptor Table
static mut GDT: Option<(GlobalDescriptorTable, Selectors)> = None;

/// Segment selectors for kernel code and data
pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

/// Initialize the GDT with TSS for interrupt handling
///
/// This sets up:
/// - Kernel code segment (ring 0)
/// - Kernel data segment (ring 0)
/// - Task State Segment with interrupt stacks
///
/// # Safety
/// Must be called only once during kernel initialization.
pub unsafe fn init() {
    // Set up the TSS with interrupt stacks
    // Use raw pointers to avoid creating references to mutable statics
    let double_fault_stack_ptr = &raw const DOUBLE_FAULT_STACK;
    let double_fault_stack_end = 
        VirtAddr::from_ptr((*double_fault_stack_ptr).as_ptr()) + INTERRUPT_STACK_SIZE as u64;
    
    let tss_ptr = &raw mut TSS;
    (*tss_ptr).interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = double_fault_stack_end;

    let page_fault_stack_ptr = &raw const PAGE_FAULT_STACK;
    let page_fault_stack_end = 
        VirtAddr::from_ptr((*page_fault_stack_ptr).as_ptr()) + PAGE_FAULT_STACK_SIZE as u64;
    (*tss_ptr).interrupt_stack_table[PAGE_FAULT_IST_INDEX as usize] = page_fault_stack_end;

    // Build the GDT
    let mut gdt = GlobalDescriptorTable::new();
    let code_selector = gdt.append(Descriptor::kernel_code_segment());
    let data_selector = gdt.append(Descriptor::kernel_data_segment());
    let tss_selector = gdt.append(Descriptor::tss_segment(&*tss_ptr));

    let gdt_ptr = &raw mut GDT;
    *gdt_ptr = Some((
        gdt,
        Selectors {
            code_selector,
            data_selector,
            tss_selector,
        },
    ));

    // Load the GDT
    let (gdt, selectors) = (*gdt_ptr).as_ref().unwrap();
    gdt.load();

    // Reload segment registers
    CS::set_reg(selectors.code_selector);
    DS::set_reg(selectors.data_selector);
    ES::set_reg(selectors.data_selector);
    FS::set_reg(selectors.data_selector);
    GS::set_reg(selectors.data_selector);
    SS::set_reg(selectors.data_selector);

    // Load the TSS
    load_tss(selectors.tss_selector);
}

/// Stack size constant for page fault (same as interrupt stack)
const PAGE_FAULT_STACK_SIZE: usize = INTERRUPT_STACK_SIZE;

/// Get the kernel code selector
pub fn kernel_code_selector() -> SegmentSelector {
    unsafe { 
        let gdt_ptr = &raw const GDT;
        (*gdt_ptr).as_ref().unwrap().1.code_selector 
    }
}

/// Get the kernel data selector
pub fn kernel_data_selector() -> SegmentSelector {
    unsafe { 
        let gdt_ptr = &raw const GDT;
        (*gdt_ptr).as_ref().unwrap().1.data_selector 
    }
}
