# Interrupt Handling

> Interrupts provide asynchronous event notification. On WASM, there are no hardware interrupts.

## Overview

The interrupt subsystem manages:

1. **IRQ Routing**: Directing hardware interrupts to handlers
2. **IRQ Capabilities**: User-space drivers receive IRQs via capabilities
3. **Exception Handling**: CPU exceptions (page fault, divide by zero, etc.)

## WASM: No Interrupts

On WASM Phase 1, there are no hardware interrupts:

- Timer: Use `setTimeout` / `requestAnimationFrame` in JavaScript
- I/O: Use Promises and async/await
- Events: Use message passing to processes

```javascript
// WASM supervisor: simulate timer events
setInterval(() => {
    for (const [pid, worker] of workers) {
        worker.postMessage({ type: 'tick', timestamp: performance.now() });
    }
}, 10);  // 10ms tick
```

### WASM IRQ Syscalls

On WASM, these syscalls return `ENOSYS`:

- `SYS_IRQ_REGISTER` (0x50)
- `SYS_IRQ_ACK` (0x51)
- `SYS_IRQ_MASK` (0x52)
- `SYS_IRQ_UNMASK` (0x53)

## Native: Hardware Interrupts

On native x86_64 targets, full interrupt handling is implemented.

### Interrupt Descriptor Table

```rust
/// IDT entry (16 bytes on x86_64).
#[repr(C, packed)]
pub struct IdtEntry {
    /// Handler offset bits 0-15
    offset_low: u16,
    /// Code segment selector
    selector: u16,
    /// IST index (0 = no IST)
    ist: u8,
    /// Type and attributes
    type_attr: u8,
    /// Handler offset bits 16-31
    offset_mid: u16,
    /// Handler offset bits 32-63
    offset_high: u32,
    /// Reserved (must be 0)
    reserved: u32,
}

/// Type attributes for IDT entry.
pub mod IdtType {
    pub const INTERRUPT_GATE: u8 = 0x8E;  // Present, DPL=0, Interrupt Gate
    pub const TRAP_GATE: u8 = 0x8F;       // Present, DPL=0, Trap Gate
    pub const USER_INTERRUPT: u8 = 0xEE;  // Present, DPL=3, Interrupt Gate
}

/// IDT (256 entries).
#[repr(C, align(16))]
pub struct Idt {
    entries: [IdtEntry; 256],
}
```

### Interrupt Vectors

| Vector    | Name                    | Type       | Description                    |
|-----------|-------------------------|------------|--------------------------------|
| 0         | #DE - Divide Error      | Exception  | Division by zero               |
| 1         | #DB - Debug             | Exception  | Debug exception                |
| 2         | NMI                     | Interrupt  | Non-maskable interrupt         |
| 3         | #BP - Breakpoint        | Trap       | INT3 instruction               |
| 4         | #OF - Overflow          | Trap       | INTO instruction               |
| 5         | #BR - Bound Range       | Exception  | BOUND instruction              |
| 6         | #UD - Invalid Opcode    | Exception  | Undefined instruction          |
| 7         | #NM - Device N/A        | Exception  | FPU not available              |
| 8         | #DF - Double Fault      | Abort      | Exception during exception     |
| 10        | #TS - Invalid TSS       | Exception  | Task switch error              |
| 11        | #NP - Segment N/P       | Exception  | Segment not present            |
| 12        | #SS - Stack Segment     | Exception  | Stack segment fault            |
| 13        | #GP - General Prot.     | Exception  | General protection fault       |
| 14        | #PF - Page Fault        | Exception  | Page fault                     |
| 16        | #MF - x87 FPU           | Exception  | FPU exception                  |
| 17        | #AC - Alignment Check   | Exception  | Alignment error                |
| 18        | #MC - Machine Check     | Abort      | Hardware error                 |
| 19        | #XM - SIMD              | Exception  | SIMD exception                 |
| 32-255    | IRQs / User-defined     | Interrupt  | Hardware interrupts, IPIs      |

### Exception Handling

```rust
/// Interrupt stack frame (pushed by CPU).
#[repr(C)]
pub struct InterruptStackFrame {
    pub instruction_pointer: u64,
    pub code_segment: u64,
    pub cpu_flags: u64,
    pub stack_pointer: u64,
    pub stack_segment: u64,
}

/// Page fault error code.
pub mod PageFaultError {
    pub const PRESENT: u64 = 1 << 0;       // Page was present
    pub const WRITE: u64 = 1 << 1;         // Write access
    pub const USER: u64 = 1 << 2;          // User mode
    pub const RESERVED: u64 = 1 << 3;      // Reserved bit set in PTE
    pub const INSTRUCTION: u64 = 1 << 4;   // Instruction fetch
}

/// Page fault handler.
extern "x86-interrupt" fn page_fault_handler(
    frame: InterruptStackFrame,
    error_code: u64,
) {
    let fault_addr: u64;
    unsafe { core::arch::asm!("mov {}, cr2", out(reg) fault_addr) };
    
    // Try to handle (e.g., demand paging, COW)
    if !handle_page_fault(fault_addr, error_code) {
        // Unrecoverable - kill process or panic
        panic!("Page fault at {:#x}, error: {:#x}", fault_addr, error_code);
    }
}

/// General protection fault handler.
extern "x86-interrupt" fn gpf_handler(
    frame: InterruptStackFrame,
    error_code: u64,
) {
    panic!("GPF at {:#x}, error: {:#x}", frame.instruction_pointer, error_code);
}
```

### Hardware IRQ Handling

```rust
/// IRQ handler state.
pub struct IrqHandler {
    /// IRQ number (0-223 for IOAPIC)
    pub irq: u8,
    /// Process that registered the handler
    pub owner: ProcessId,
    /// Endpoint to notify
    pub endpoint: EndpointId,
    /// Whether IRQ is currently masked
    pub masked: bool,
    /// Count of interrupts received
    pub count: u64,
}

/// IRQ subsystem.
pub struct IrqSubsystem {
    /// Registered handlers (IRQ number -> handler)
    handlers: BTreeMap<u8, IrqHandler>,
    /// Pending IRQs (not yet acknowledged)
    pending: u64,
}

impl IrqSubsystem {
    /// Register a handler for an IRQ.
    ///
    /// # Pre-conditions
    /// - Caller has IRQ capability with Write permission
    /// - IRQ not already registered
    ///
    /// # Post-conditions
    /// - Handler installed
    /// - IRQ unmasked in IOAPIC
    pub fn register(
        &mut self,
        kernel: &mut Kernel,
        pid: ProcessId,
        irq: u8,
        endpoint_slot: CapSlot,
    ) -> Result<(), KernelError> {
        // Verify endpoint capability
        let cspace = kernel.cap_spaces.get(&pid)
            .ok_or(KernelError::ProcessNotFound)?;
        let cap = axiom_check(
            cspace,
            endpoint_slot,
            Permissions::write_only(),
            Some(ObjectType::Endpoint),
        ).map_err(|_| KernelError::InvalidCapability)?;
        
        let endpoint = EndpointId(cap.object_id);
        
        // Check not already registered
        if self.handlers.contains_key(&irq) {
            return Err(KernelError::ResourceBusy);
        }
        
        // Register handler
        self.handlers.insert(irq, IrqHandler {
            irq,
            owner: pid,
            endpoint,
            masked: false,
            count: 0,
        });
        
        // Unmask in IOAPIC
        ioapic_unmask(irq);
        
        Ok(())
    }
    
    /// Handle an IRQ (called from interrupt handler).
    pub fn handle_irq(&mut self, kernel: &mut Kernel, irq: u8) {
        if let Some(handler) = self.handlers.get_mut(&irq) {
            handler.count += 1;
            self.pending |= 1 << irq;
            
            // Send notification to endpoint
            let msg = Message {
                from: ProcessId(0),  // Kernel
                tag: 0x0100 | (irq as u32),  // IRQ_NOTIFY | irq_num
                data: irq.to_le_bytes().to_vec(),
                transferred_caps: vec![],
            };
            
            if let Some(ep) = kernel.endpoints.get_mut(&handler.endpoint) {
                ep.queue.push_back(msg);
            }
            
            // Mask until acknowledged
            handler.masked = true;
            ioapic_mask(irq);
        }
        
        // Send EOI to LAPIC
        lapic_eoi();
    }
    
    /// Acknowledge an IRQ (unmask it).
    pub fn acknowledge(&mut self, pid: ProcessId, irq: u8) -> Result<(), KernelError> {
        let handler = self.handlers.get_mut(&irq)
            .ok_or(KernelError::InvalidArgument)?;
        
        if handler.owner != pid {
            return Err(KernelError::PermissionDenied);
        }
        
        self.pending &= !(1 << irq);
        handler.masked = false;
        ioapic_unmask(irq);
        
        Ok(())
    }
}
```

### APIC Configuration

```rust
/// Local APIC base address (mapped from MSR).
const LAPIC_BASE: usize = 0xFEE0_0000;

/// LAPIC registers.
mod lapic {
    pub const ID: usize = 0x020;
    pub const VERSION: usize = 0x030;
    pub const TPR: usize = 0x080;      // Task Priority
    pub const EOI: usize = 0x0B0;      // End of Interrupt
    pub const SVR: usize = 0x0F0;      // Spurious Vector
    pub const ICR_LOW: usize = 0x300;  // Interrupt Command (low)
    pub const ICR_HIGH: usize = 0x310; // Interrupt Command (high)
    pub const TIMER_LVT: usize = 0x320;
    pub const TIMER_ICR: usize = 0x380; // Initial Count
    pub const TIMER_CCR: usize = 0x390; // Current Count
    pub const TIMER_DCR: usize = 0x3E0; // Divide Config
}

/// Initialize Local APIC.
pub fn lapic_init() {
    unsafe {
        // Enable LAPIC via SVR
        let svr = read_lapic(lapic::SVR);
        write_lapic(lapic::SVR, svr | 0x100);  // Set enable bit
        
        // Configure timer for periodic interrupts
        write_lapic(lapic::TIMER_DCR, 0x03);   // Divide by 16
        write_lapic(lapic::TIMER_LVT, 32 | 0x20000);  // Vector 32, periodic
        write_lapic(lapic::TIMER_ICR, 10_000_000);    // Initial count
    }
}

/// Send End-Of-Interrupt to LAPIC.
pub fn lapic_eoi() {
    unsafe { write_lapic(lapic::EOI, 0); }
}

fn read_lapic(reg: usize) -> u32 {
    unsafe { core::ptr::read_volatile((LAPIC_BASE + reg) as *const u32) }
}

fn write_lapic(reg: usize, value: u32) {
    unsafe { core::ptr::write_volatile((LAPIC_BASE + reg) as *mut u32, value); }
}
```

### IOAPIC Configuration

```rust
/// I/O APIC base address.
const IOAPIC_BASE: usize = 0xFEC0_0000;

/// IOAPIC registers.
mod ioapic {
    pub const IOREGSEL: usize = 0x00;
    pub const IOWIN: usize = 0x10;
    pub const ID: u8 = 0x00;
    pub const VER: u8 = 0x01;
    pub const REDTBL_BASE: u8 = 0x10;
}

/// Configure IOAPIC redirection entry.
pub fn ioapic_configure(irq: u8, vector: u8, dest_cpu: u8) {
    let reg = ioapic::REDTBL_BASE + irq * 2;
    let low = vector as u32;  // Active high, edge, fixed, physical, unmasked
    let high = (dest_cpu as u32) << 24;
    
    write_ioapic(reg, low);
    write_ioapic(reg + 1, high);
}

/// Mask an IRQ in IOAPIC.
pub fn ioapic_mask(irq: u8) {
    let reg = ioapic::REDTBL_BASE + irq * 2;
    let low = read_ioapic(reg);
    write_ioapic(reg, low | (1 << 16));  // Set mask bit
}

/// Unmask an IRQ in IOAPIC.
pub fn ioapic_unmask(irq: u8) {
    let reg = ioapic::REDTBL_BASE + irq * 2;
    let low = read_ioapic(reg);
    write_ioapic(reg, low & !(1 << 16));  // Clear mask bit
}

fn read_ioapic(reg: u8) -> u32 {
    unsafe {
        core::ptr::write_volatile((IOAPIC_BASE + ioapic::IOREGSEL) as *mut u32, reg as u32);
        core::ptr::read_volatile((IOAPIC_BASE + ioapic::IOWIN) as *const u32)
    }
}

fn write_ioapic(reg: u8, value: u32) {
    unsafe {
        core::ptr::write_volatile((IOAPIC_BASE + ioapic::IOREGSEL) as *mut u32, reg as u32);
        core::ptr::write_volatile((IOAPIC_BASE + ioapic::IOWIN) as *mut u32, value);
    }
}
```

## Timer Interrupt

The timer interrupt drives preemptive scheduling:

```rust
/// Timer interrupt handler (vector 32).
extern "x86-interrupt" fn timer_handler(frame: InterruptStackFrame) {
    // Send EOI first (allows nested interrupts)
    lapic_eoi();
    
    // Update system time
    UPTIME_NANOS.fetch_add(TICK_NANOS, Ordering::Relaxed);
    
    // Trigger scheduler tick
    unsafe {
        if let Some(kernel) = KERNEL.as_mut() {
            if let Some(next) = kernel.scheduler.tick(current_cpu()) {
                // Context switch needed
                switch_to(next);
            }
        }
    }
}
```

## LOC Budget

Target: ~200 LOC for interrupt subsystem.

| Component           | Estimated LOC | WASM Phase 1 |
|---------------------|---------------|--------------|
| IDT setup           | ~50           | N/A          |
| Exception handlers  | ~50           | N/A          |
| IRQ subsystem       | ~80           | N/A          |
| APIC/IOAPIC config  | ~50           | N/A          |
| **Total**           | **~230**      | 0            |

Note: The native interrupt code exceeds the target slightly but is necessary for hardware support.
