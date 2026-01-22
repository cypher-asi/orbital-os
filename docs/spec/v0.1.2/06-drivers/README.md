# User-Space Drivers

> Device drivers run in user-space for isolation and safety.

## Overview

Zero OS uses user-space drivers rather than kernel drivers:

1. **Isolation**: Driver bugs don't crash the kernel
2. **Security**: Drivers have only the capabilities they need
3. **Flexibility**: Drivers can be updated without reboot

## WASM Phase 1

On WASM, there are no hardware drivers. "Drivers" are JavaScript adapters:

- **Console Driver**: Adapts DOM/terminal UI to console protocol
- **Storage Driver**: Adapts IndexedDB to storage protocol
- **Network Driver**: Adapts Fetch API to network protocol

These are part of the supervisor, not separate processes.

## Native Drivers (Future)

On native targets, drivers are user-space processes with hardware capabilities:

```
┌─────────────────────────────────────────────────────────────────────┐
│                         User-Space Driver                            │
│                                                                     │
│  Capabilities:                                                       │
│  • I/O Port range (e.g., 0x1F0-0x1F7 for IDE)                       │
│  • IRQ (e.g., IRQ 14)                                               │
│  • Memory-mapped I/O region                                          │
│  • DMA buffer                                                        │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                      Driver Logic                              │ │
│  │                                                               │ │
│  │  1. Receive request via IPC                                   │ │
│  │  2. Program hardware registers                                 │ │
│  │  3. Wait for IRQ (via IPC notification)                       │ │
│  │  4. Read result, send response                                 │ │
│  └───────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

## Driver Types

| Driver Type      | Target Hardware           | Phase     |
|------------------|---------------------------|-----------|
| virtio-blk       | VirtIO block device       | Phase 2   |
| virtio-net       | VirtIO network            | Phase 2   |
| virtio-rng       | VirtIO entropy            | Phase 2   |
| ahci             | SATA controller           | Phase 7   |
| nvme             | NVMe SSD                  | Phase 7   |
| xhci             | USB 3.0                   | Phase 7   |
| e1000            | Intel NIC                 | Phase 7   |

## Driver Capabilities

Drivers receive specific hardware capabilities from init:

```rust
/// I/O port capability.
pub struct IoPortCap {
    pub base_port: u16,
    pub count: u16,
}

/// Memory-mapped I/O capability.
pub struct MmioCap {
    pub base_addr: u64,
    pub size: usize,
}

/// IRQ capability.
pub struct IrqCap {
    pub irq_num: u8,
    pub notification_endpoint: EndpointId,
}

/// DMA buffer capability.
pub struct DmaCap {
    pub phys_addr: u64,
    pub size: usize,
    pub virt_addr: usize,
}
```

## Example: VirtIO Block Driver

```rust
// virtio_blk.rs (future)

struct VirtioBlkDriver {
    // Capabilities from init
    mmio_cap: MmioCap,
    irq_cap: IrqCap,
    dma_cap: DmaCap,
    
    // VirtIO structures
    vring: VirtQueue,
}

impl VirtioBlkDriver {
    fn handle_read(&mut self, sector: u64, count: u32) -> Result<Vec<u8>, DriverError> {
        // 1. Build VirtIO descriptor chain
        let desc = self.build_read_descriptor(sector, count)?;
        
        // 2. Add to virtqueue
        self.vring.add_descriptor(desc)?;
        
        // 3. Notify device (write to MMIO)
        self.notify_device();
        
        // 4. Wait for IRQ
        let msg = receive_blocking(self.irq_cap.notification_endpoint);
        
        // 5. Process completion
        let result = self.vring.get_completed()?;
        
        Ok(result.data)
    }
}
```

## Documentation Structure (Future)

```
06-drivers/
├── README.md           (this file)
├── 01-virtio.md       (VirtIO common framework)
├── 02-block.md        (Block device protocol)
├── 03-network.md      (Network device protocol)
├── 04-input.md        (Keyboard/mouse)
└── 05-display.md      (Framebuffer/GPU)
```
