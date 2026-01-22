# Boot Layer

> The boot layer handles system initialization before the kernel takes over.

## Overview

Boot behavior differs significantly between targets:

| Target      | Boot Mechanism              | Kernel Entry                |
|-------------|-----------------------------|-----------------------------|
| WASM        | Browser loads JavaScript    | HTML page load              |
| QEMU        | Multiboot2 / UEFI          | Bootloader loads ELF        |
| Bare Metal  | UEFI / Legacy BIOS         | Firmware loads bootloader   |

## WASM Boot (Phase 1)

On WASM, "boot" is handled by the browser:

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Browser Boot Sequence                         │
│                                                                     │
│  1. Browser loads index.html                                         │
│                                                                     │
│  2. HTML loads JavaScript files:                                     │
│     - worker.js (process runtime)                                   │
│     - supervisor.js (kernel host)                                   │
│                                                                     │
│  3. JavaScript fetches and instantiates WASM:                        │
│     - Zero_web.wasm (kernel)                                     │
│                                                                     │
│  4. Kernel initializes:                                              │
│     - Create HAL instance                                           │
│     - Initialize kernel state                                        │
│     - Register init process                                          │
│                                                                     │
│  5. Supervisor spawns init process:                                  │
│     - Load init.wasm                                                │
│     - Create Web Worker                                              │
│     - Start message loop                                             │
│                                                                     │
│  6. System running                                                   │
└─────────────────────────────────────────────────────────────────────┘
```

### HTML Entry Point

```html
<!DOCTYPE html>
<html>
<head>
    <title>Zero OS</title>
</head>
<body>
    <div id="terminal"></div>
    <script type="module">
        import { ZeroSupervisor } from './supervisor.js';
        
        const supervisor = new ZeroSupervisor();
        await supervisor.boot();
    </script>
</body>
</html>
```

### Supervisor Initialization

```javascript
class ZeroSupervisor {
    async boot() {
        // 1. Load kernel WASM
        const kernelWasm = await WebAssembly.instantiateStreaming(
            fetch('/Zero_web.wasm'),
            this.kernelImports()
        );
        this.kernel = kernelWasm.instance.exports;
        
        // 2. Initialize kernel
        this.kernel.init();
        
        // 3. Load and spawn init process
        const initBinary = await fetch('/init.wasm').then(r => r.arrayBuffer());
        await this.spawnProcess('init', initBinary);
        
        // 4. Start scheduler loop
        this.schedulerLoop();
    }
}
```

## QEMU Boot (Phase 2)

On QEMU, the kernel is loaded via Multiboot2 or UEFI.

### Multiboot2 Header

```rust
/// Multiboot2 header (must be in first 32KB).
#[repr(C, align(8))]
#[link_section = ".multiboot"]
static MULTIBOOT2_HEADER: [u32; 6] = [
    0xE85250D6,         // Magic
    0,                  // Architecture (i386/amd64)
    24,                 // Header length
    -(0xE85250D6_i32 + 0 + 24) as u32,  // Checksum
    0,                  // End tag type
    8,                  // End tag size
];
```

### Boot Sequence (Multiboot2)

```
┌─────────────────────────────────────────────────────────────────────┐
│                     QEMU Multiboot2 Boot Sequence                    │
│                                                                     │
│  1. QEMU loads kernel ELF via -kernel flag                          │
│     - Kernel must have Multiboot2 header                            │
│     - Entry point specified in ELF                                   │
│                                                                     │
│  2. CPU in 32-bit protected mode (Multiboot2 spec)                   │
│     - EAX = 0x36d76289 (Multiboot2 magic)                           │
│     - EBX = pointer to boot information                              │
│                                                                     │
│  3. Kernel entry (assembly stub):                                    │
│     - Set up temporary stack                                         │
│     - Enable long mode (64-bit)                                      │
│     - Set up page tables for higher-half kernel                      │
│     - Jump to Rust entry point                                       │
│                                                                     │
│  4. Rust kernel_main():                                              │
│     - Parse Multiboot2 info (memory map, modules)                    │
│     - Initialize GDT, IDT                                            │
│     - Initialize frame allocator                                     │
│     - Initialize APIC, HPET                                          │
│     - Mount VirtIO devices                                           │
│     - Start init process                                             │
│                                                                     │
│  5. System running                                                   │
└─────────────────────────────────────────────────────────────────────┘
```

### Entry Point (Assembly)

```asm
; boot.asm - 32-bit entry from Multiboot2

section .multiboot
align 8
multiboot_header:
    dd 0xE85250D6           ; magic
    dd 0                    ; architecture (i386)
    dd multiboot_header_end - multiboot_header
    dd -(0xE85250D6 + 0 + (multiboot_header_end - multiboot_header))
    
    ; end tag
    dw 0
    dw 0
    dd 8
multiboot_header_end:

section .text
bits 32
global _start
extern kernel_main

_start:
    ; Save Multiboot2 info pointer
    mov edi, ebx
    
    ; Set up stack
    mov esp, stack_top
    
    ; Enable PAE (required for long mode)
    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax
    
    ; Load PML4
    mov eax, pml4
    mov cr3, eax
    
    ; Enable long mode via EFER MSR
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr
    
    ; Enable paging
    mov eax, cr0
    or eax, 1 << 31
    mov cr0, eax
    
    ; Load 64-bit GDT and far jump to long mode
    lgdt [gdt64.pointer]
    jmp gdt64.code:long_mode_start

bits 64
long_mode_start:
    ; Clear segment registers
    mov ax, gdt64.data
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    
    ; Call Rust entry point
    call kernel_main
    
    ; Halt if we return
    hlt
```

### Rust Entry Point

```rust
/// Kernel entry point (called from assembly).
#[no_mangle]
pub extern "C" fn kernel_main(multiboot_info: usize) -> ! {
    // 1. Initialize serial console for early debug
    serial::init();
    kprintln!("Zero OS booting...");
    
    // 2. Parse Multiboot2 info
    let boot_info = unsafe { multiboot2::load(multiboot_info) };
    let memory_map = boot_info.memory_map_tag().expect("no memory map");
    
    // 3. Initialize frame allocator
    let mut frame_allocator = FrameAllocator::new(&memory_map);
    
    // 4. Set up kernel page tables
    let kernel_page_table = paging::init(&mut frame_allocator);
    
    // 5. Initialize GDT with TSS
    gdt::init();
    
    // 6. Initialize IDT
    idt::init();
    
    // 7. Initialize APIC
    apic::init();
    
    // 8. Initialize kernel heap
    heap::init(&mut frame_allocator);
    
    // 9. Detect and initialize VirtIO devices
    virtio::init();
    
    // 10. Create kernel instance
    let hal = NativeHal::new();
    let mut kernel = Kernel::new(hal);
    
    // 11. Load and start init process
    let init_binary = include_bytes!("../init.wasm");
    let init_pid = kernel.spawn_process("init", init_binary);
    
    // 12. Enter scheduler
    kprintln!("Starting scheduler...");
    kernel.run();
}
```

## Bare Metal Boot (Phase 7)

Bare metal uses UEFI for modern systems.

### UEFI Boot Sequence

```
┌─────────────────────────────────────────────────────────────────────┐
│                        UEFI Boot Sequence                            │
│                                                                     │
│  1. Firmware POST and initialization                                 │
│                                                                     │
│  2. UEFI loads bootloader from ESP:                                  │
│     /EFI/BOOT/BOOTX64.EFI                                           │
│                                                                     │
│  3. Bootloader:                                                      │
│     - Loads kernel from ESP                                          │
│     - Gets memory map from UEFI                                      │
│     - Sets up framebuffer (GOP)                                      │
│     - Exits UEFI boot services                                       │
│     - Jumps to kernel                                                │
│                                                                     │
│  4. Kernel entry (same as QEMU from here)                            │
│     - CPU already in 64-bit mode                                     │
│     - Must handle UEFI memory map                                    │
│                                                                     │
│  5. System running                                                   │
└─────────────────────────────────────────────────────────────────────┘
```

### UEFI Bootloader (Rust)

```rust
// bootloader/src/main.rs

#![no_std]
#![no_main]

use uefi::prelude::*;
use uefi::proto::media::file::{File, FileMode, FileAttribute};
use uefi::table::boot::MemoryType;

#[entry]
fn main(handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    // 1. Initialize UEFI services
    uefi_services::init(&mut system_table).unwrap();
    
    // 2. Get graphics output
    let gop = system_table.boot_services()
        .locate_protocol::<GraphicsOutput>()
        .unwrap();
    let mode_info = gop.current_mode_info();
    let framebuffer = gop.frame_buffer().as_mut_ptr();
    
    // 3. Load kernel from ESP
    let mut root = system_table.boot_services()
        .get_image_file_system(handle)
        .unwrap()
        .open_volume()
        .unwrap();
    
    let kernel_data = load_file(&mut root, "\\Zero\\kernel.elf");
    
    // 4. Get memory map and exit boot services
    let mmap_size = system_table.boot_services().memory_map_size();
    let (system_table, memory_map) = system_table
        .exit_boot_services(MemoryType::LOADER_DATA);
    
    // 5. Jump to kernel
    let kernel_entry: extern "C" fn(*const BootInfo) -> ! = 
        unsafe { core::mem::transmute(kernel_data.entry_point) };
    
    kernel_entry(&BootInfo {
        framebuffer,
        memory_map,
        // ...
    });
}
```

## Files

Since boot is platform-specific:

| Target      | Files Needed                      |
|-------------|-----------------------------------|
| WASM        | index.html, supervisor.js         |
| QEMU        | boot.asm, kernel_main (Rust)      |
| Bare Metal  | UEFI bootloader, kernel_main      |
