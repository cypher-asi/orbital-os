//! Advanced Programmable Interrupt Controller (APIC) support
//!
//! This module provides Local APIC (LAPIC) and I/O APIC support for
//! interrupt handling on x86_64 systems.
//!
//! # LAPIC Timer
//!
//! The LAPIC timer is used for preemptive scheduling. It's configured
//! to fire every 10ms (100Hz) by default.

use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::VirtAddr;

use crate::x86_64::vmm::phys_mem_offset;

/// LAPIC base physical address (standard x86_64 location)
const LAPIC_BASE_PHYS: u64 = 0xFEE0_0000;

/// IOAPIC base physical address (standard x86_64 location)
const IOAPIC_BASE_PHYS: u64 = 0xFEC0_0000;

/// Timer tick period in nanoseconds (10ms = 10,000,000 ns)
pub const TICK_NANOS: u64 = 10_000_000;

/// LAPIC register offsets
#[allow(dead_code)] // Constants defined for hardware completeness, some used later
mod lapic_reg {
    pub const ID: usize = 0x020;
    pub const VERSION: usize = 0x030;
    pub const TPR: usize = 0x080;        // Task Priority Register
    pub const EOI: usize = 0x0B0;        // End of Interrupt
    pub const SVR: usize = 0x0F0;        // Spurious Interrupt Vector Register
    pub const ICR_LOW: usize = 0x300;    // Interrupt Command Register (low)
    pub const ICR_HIGH: usize = 0x310;   // Interrupt Command Register (high)
    pub const TIMER_LVT: usize = 0x320;  // Timer Local Vector Table
    pub const TIMER_ICR: usize = 0x380;  // Timer Initial Count Register
    pub const TIMER_CCR: usize = 0x390;  // Timer Current Count Register
    pub const TIMER_DCR: usize = 0x3E0;  // Timer Divide Configuration Register
}

/// IOAPIC register offsets
#[allow(dead_code)] // Constants defined for hardware completeness, some used later
mod ioapic_reg {
    pub const IOREGSEL: usize = 0x00;
    pub const IOWIN: usize = 0x10;
    pub const ID: u8 = 0x00;
    pub const VER: u8 = 0x01;
    pub const REDTBL_BASE: u8 = 0x10;
}

/// Timer interrupt vector number (must match InterruptIndex::Timer)
pub const TIMER_VECTOR: u8 = 32;

/// Spurious interrupt vector number
const SPURIOUS_VECTOR: u8 = 255;

/// Timer LVT flags
mod timer_lvt {
    pub const PERIODIC: u32 = 0x20000;   // Periodic mode (bit 17)
    pub const MASKED: u32 = 0x10000;     // Masked (bit 16)
}

/// Global timer tick counter
pub static TICK_COUNT: AtomicU64 = AtomicU64::new(0);

/// Whether APIC has been initialized
static APIC_INITIALIZED: AtomicU64 = AtomicU64::new(0);

/// Get the virtual address of the LAPIC
fn lapic_virt_addr() -> VirtAddr {
    VirtAddr::new(LAPIC_BASE_PHYS + phys_mem_offset())
}

/// Get the virtual address of the IOAPIC
#[allow(dead_code)]
fn ioapic_virt_addr() -> VirtAddr {
    VirtAddr::new(IOAPIC_BASE_PHYS + phys_mem_offset())
}

/// Read a LAPIC register
unsafe fn read_lapic(offset: usize) -> u32 {
    let addr = lapic_virt_addr() + offset as u64;
    core::ptr::read_volatile(addr.as_ptr::<u32>())
}

/// Write a LAPIC register
unsafe fn write_lapic(offset: usize, value: u32) {
    let addr = lapic_virt_addr() + offset as u64;
    core::ptr::write_volatile(addr.as_mut_ptr::<u32>(), value);
}

/// Read an IOAPIC register
#[allow(dead_code)]
unsafe fn read_ioapic(reg: u8) -> u32 {
    let base = ioapic_virt_addr();
    // Write register selector
    core::ptr::write_volatile((base.as_u64() + ioapic_reg::IOREGSEL as u64) as *mut u32, reg as u32);
    // Read value
    core::ptr::read_volatile((base.as_u64() + ioapic_reg::IOWIN as u64) as *const u32)
}

/// Write an IOAPIC register
#[allow(dead_code)]
unsafe fn write_ioapic(reg: u8, value: u32) {
    let base = ioapic_virt_addr();
    // Write register selector
    core::ptr::write_volatile((base.as_u64() + ioapic_reg::IOREGSEL as u64) as *mut u32, reg as u32);
    // Write value
    core::ptr::write_volatile((base.as_u64() + ioapic_reg::IOWIN as u64) as *mut u32, value);
}

/// Disable the legacy 8259 PIC
///
/// When using the APIC, the legacy PIC must be disabled to prevent
/// it from generating spurious interrupts.
unsafe fn disable_pic() {
    use x86_64::instructions::port::Port;
    
    // ICW1: Initialize PICs
    let mut pic1_cmd: Port<u8> = Port::new(0x20);
    let mut pic1_data: Port<u8> = Port::new(0x21);
    let mut pic2_cmd: Port<u8> = Port::new(0xA0);
    let mut pic2_data: Port<u8> = Port::new(0xA1);
    
    // Remap PICs to vectors 0x20-0x2F (above CPU exceptions)
    // This prevents PIC interrupts from triggering exception handlers
    pic1_cmd.write(0x11); // ICW1: Initialize + ICW4 needed
    pic2_cmd.write(0x11);
    
    pic1_data.write(0x20); // ICW2: PIC1 vector offset (32)
    pic2_data.write(0x28); // ICW2: PIC2 vector offset (40)
    
    pic1_data.write(0x04); // ICW3: PIC1 has slave at IRQ2
    pic2_data.write(0x02); // ICW3: PIC2 cascade identity
    
    pic1_data.write(0x01); // ICW4: 8086 mode
    pic2_data.write(0x01);
    
    // Mask all interrupts on both PICs
    pic1_data.write(0xFF);
    pic2_data.write(0xFF);
}

/// Initialize the Local APIC
///
/// This function:
/// 1. Disables the legacy 8259 PIC
/// 2. Enables the LAPIC via the Spurious Interrupt Vector Register
/// 3. Configures the LAPIC timer but does NOT start it
///
/// Call `start_timer()` after enabling interrupts to begin receiving
/// timer interrupts.
///
/// # Safety
/// Must be called only once during kernel initialization.
/// Physical memory offset must be set (VMM initialized).
pub unsafe fn init() {
    if APIC_INITIALIZED.load(Ordering::Acquire) != 0 {
        return;
    }

    // First, disable the legacy PIC to prevent spurious interrupts
    disable_pic();

    // Read current SVR
    let svr = read_lapic(lapic_reg::SVR);
    
    // Enable LAPIC (bit 8) and set spurious vector
    write_lapic(lapic_reg::SVR, svr | 0x100 | (SPURIOUS_VECTOR as u32));

    // Set task priority to 0 (accept all interrupts)
    write_lapic(lapic_reg::TPR, 0);

    // Configure timer divide configuration
    // Value 0x03 = divide by 16
    write_lapic(lapic_reg::TIMER_DCR, 0x03);

    // Configure timer LVT but MASK it initially:
    // - Vector = TIMER_VECTOR (32)
    // - Mode = Periodic
    // - MASKED (bit 16) - timer won't fire until unmasked
    write_lapic(lapic_reg::TIMER_LVT, (TIMER_VECTOR as u32) | timer_lvt::PERIODIC | timer_lvt::MASKED);

    // Set initial count to 0 (timer not running)
    write_lapic(lapic_reg::TIMER_ICR, 0);

    APIC_INITIALIZED.store(1, Ordering::Release);
}

/// Start the LAPIC timer
///
/// This unmasks the timer and starts it counting. Should be called
/// after interrupts are enabled.
///
/// # Safety
/// Interrupts should be enabled before calling this, or the timer
/// will accumulate pending interrupts.
pub unsafe fn start_timer() {
    // Calculate timer initial count for ~10ms at ~100MHz bus
    // This is approximate - real hardware would need calibration
    // For QEMU, the default CPU frequency works well enough
    // 
    // Formula: count = (bus_freq / divisor) * desired_period_seconds
    // With unknown bus freq, we use a reasonable default that works in QEMU
    let timer_count: u32 = 10_000_000 / 16; // Approx 10ms with divide-by-16

    // Unmask the timer
    write_lapic(lapic_reg::TIMER_LVT, (TIMER_VECTOR as u32) | timer_lvt::PERIODIC);
    
    // Set initial count to start the timer
    write_lapic(lapic_reg::TIMER_ICR, timer_count);
}

/// Send End-Of-Interrupt signal to LAPIC
///
/// This must be called at the end of every interrupt handler
/// to acknowledge the interrupt and allow further interrupts.
#[inline]
pub fn eoi() {
    unsafe {
        write_lapic(lapic_reg::EOI, 0);
    }
}

/// Get the current tick count since APIC initialization
pub fn tick_count() -> u64 {
    TICK_COUNT.load(Ordering::Relaxed)
}

/// Get elapsed nanoseconds since APIC initialization
pub fn elapsed_nanos() -> u64 {
    tick_count() * TICK_NANOS
}

/// Handle timer tick (called from interrupt handler)
///
/// This function:
/// 1. Increments the global tick counter
/// 2. Returns the new tick count
pub fn handle_timer_tick() -> u64 {
    let new_count = TICK_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    new_count
}

/// Disable the LAPIC timer (mask it)
#[allow(dead_code)]
pub fn disable_timer() {
    unsafe {
        let lvt = read_lapic(lapic_reg::TIMER_LVT);
        write_lapic(lapic_reg::TIMER_LVT, lvt | timer_lvt::MASKED);
    }
}

/// Enable the LAPIC timer (unmask it)
#[allow(dead_code)]
pub fn enable_timer() {
    unsafe {
        let lvt = read_lapic(lapic_reg::TIMER_LVT);
        write_lapic(lapic_reg::TIMER_LVT, lvt & !timer_lvt::MASKED);
    }
}

/// Get LAPIC ID
pub fn lapic_id() -> u32 {
    unsafe { read_lapic(lapic_reg::ID) >> 24 }
}

/// Get LAPIC version
pub fn lapic_version() -> u32 {
    unsafe { read_lapic(lapic_reg::VERSION) & 0xFF }
}

/// Check if APIC is initialized
pub fn is_initialized() -> bool {
    APIC_INITIALIZED.load(Ordering::Acquire) != 0
}

/// Configure IOAPIC redirection entry for an IRQ
#[allow(dead_code)]
pub unsafe fn ioapic_configure(irq: u8, vector: u8, dest_cpu: u8) {
    let reg = ioapic_reg::REDTBL_BASE + irq * 2;
    // Low dword: vector, active high, edge triggered, fixed delivery, unmasked
    let low = vector as u32;
    // High dword: destination CPU in bits 24-31
    let high = (dest_cpu as u32) << 24;

    write_ioapic(reg, low);
    write_ioapic(reg + 1, high);
}

/// Mask an IRQ in IOAPIC
#[allow(dead_code)]
pub unsafe fn ioapic_mask(irq: u8) {
    let reg = ioapic_reg::REDTBL_BASE + irq * 2;
    let low = read_ioapic(reg);
    write_ioapic(reg, low | (1 << 16)); // Set mask bit
}

/// Unmask an IRQ in IOAPIC
#[allow(dead_code)]
pub unsafe fn ioapic_unmask(irq: u8) {
    let reg = ioapic_reg::REDTBL_BASE + irq * 2;
    let low = read_ioapic(reg);
    write_ioapic(reg, low & !(1 << 16)); // Clear mask bit
}
