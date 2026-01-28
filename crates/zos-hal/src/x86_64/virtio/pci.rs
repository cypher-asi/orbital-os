//! VirtIO PCI Transport
//!
//! Implements the VirtIO PCI transport layer for modern VirtIO devices.
//! This supports both legacy and modern VirtIO PCI devices.
//!
//! # VirtIO PCI Legacy (pre-1.0)
//!
//! Legacy devices use fixed I/O port or MMIO BAR layouts:
//! - BAR0: I/O port space with device registers
//!
//! | Offset | Size | Name |
//! |--------|------|------|
//! | 0x00   | 4    | Device features |
//! | 0x04   | 4    | Driver features |
//! | 0x08   | 4    | Queue address (PFN) |
//! | 0x0C   | 2    | Queue size |
//! | 0x0E   | 2    | Queue select |
//! | 0x10   | 2    | Queue notify |
//! | 0x12   | 1    | Device status |
//! | 0x13   | 1    | ISR status |
//! | 0x14+  | var  | Device config |

use core::ptr::{read_volatile, write_volatile};
use x86_64::instructions::port::Port;

use crate::x86_64::pci::PciDevice;
use super::{DeviceId, DeviceStatus, VirtioResult};

/// Legacy VirtIO PCI I/O registers
mod legacy_regs {
    pub const DEVICE_FEATURES: u16 = 0x00;
    pub const DRIVER_FEATURES: u16 = 0x04;
    pub const QUEUE_ADDRESS: u16 = 0x08;
    pub const QUEUE_SIZE: u16 = 0x0C;
    pub const QUEUE_SELECT: u16 = 0x0E;
    pub const QUEUE_NOTIFY: u16 = 0x10;
    pub const DEVICE_STATUS: u16 = 0x12;
    pub const ISR_STATUS: u16 = 0x13;
    pub const CONFIG: u16 = 0x14;
}

/// VirtIO PCI Transport (Legacy Mode)
///
/// Provides access to a VirtIO device via PCI legacy (transitional) interface.
#[derive(Clone, Copy)]
pub struct PciTransport {
    /// PCI device info
    device: PciDevice,
    /// I/O base address (from BAR0)
    io_base: u16,
    /// Whether this is an I/O port or memory-mapped device
    is_io_port: bool,
    /// MMIO base address (if memory-mapped)
    mmio_base: u64,
}

impl PciTransport {
    /// Create a new PCI transport for a VirtIO device
    ///
    /// # Safety
    /// The device must be a valid VirtIO PCI device.
    pub unsafe fn new(device: PciDevice) -> VirtioResult<Self> {
        // Enable the device
        device.enable();
        
        // Get BAR0 for device registers
        let bar0 = device.read_bar(0);
        let is_io_port = device.bar_is_io(0);
        
        let (io_base, mmio_base) = if is_io_port {
            ((bar0 & !0x3) as u16, 0u64)
        } else {
            (0u16, device.bar_address(0))
        };
        
        crate::serial_println!("[virtio-pci] BAR0: io_port={}, base=0x{:x}", 
            is_io_port, if is_io_port { io_base as u64 } else { mmio_base });
        
        Ok(Self {
            device,
            io_base,
            is_io_port,
            mmio_base,
        })
    }

    /// Get the VirtIO device ID from PCI device ID
    pub fn device_id(&self) -> DeviceId {
        // VirtIO legacy device IDs: 0x1000 + device_type
        let device_type = self.device.device_id.saturating_sub(0x1000);
        DeviceId::from(device_type as u32)
    }

    /// Read device status
    pub fn status(&self) -> DeviceStatus {
        let status = self.read_u8(legacy_regs::DEVICE_STATUS);
        DeviceStatus::from_bits(status)
    }

    /// Write device status
    pub fn set_status(&self, status: DeviceStatus) {
        self.write_u8(legacy_regs::DEVICE_STATUS, status.bits());
    }

    /// Reset the device
    pub fn reset(&self) {
        self.write_u8(legacy_regs::DEVICE_STATUS, 0);
        // Wait for reset
        while self.read_u8(legacy_regs::DEVICE_STATUS) != 0 {
            core::hint::spin_loop();
        }
    }

    /// Read device feature bits (32-bit)
    pub fn device_features(&self) -> u32 {
        self.read_u32(legacy_regs::DEVICE_FEATURES)
    }

    /// Write driver feature bits (32-bit)
    pub fn set_driver_features(&self, features: u32) {
        self.write_u32(legacy_regs::DRIVER_FEATURES, features);
    }

    /// Select a virtqueue
    pub fn select_queue(&self, queue_index: u16) {
        self.write_u16(legacy_regs::QUEUE_SELECT, queue_index);
    }

    /// Get the queue size for the selected queue
    pub fn queue_size(&self) -> u16 {
        self.read_u16(legacy_regs::QUEUE_SIZE)
    }

    /// Set the queue address (physical page frame number)
    ///
    /// The address is the physical address of the queue divided by 4096.
    pub fn set_queue_address(&self, pfn: u32) {
        self.write_u32(legacy_regs::QUEUE_ADDRESS, pfn);
    }

    /// Get the queue address
    pub fn queue_address(&self) -> u32 {
        self.read_u32(legacy_regs::QUEUE_ADDRESS)
    }

    /// Notify the device that there are buffers in the queue
    pub fn notify_queue(&self, queue_index: u16) {
        self.write_u16(legacy_regs::QUEUE_NOTIFY, queue_index);
    }

    /// Read ISR status (acknowledges interrupt)
    pub fn isr_status(&self) -> u8 {
        self.read_u8(legacy_regs::ISR_STATUS)
    }

    /// Read a config byte
    pub fn read_config_u8(&self, offset: u16) -> u8 {
        self.read_u8(legacy_regs::CONFIG + offset)
    }

    /// Read a config u32
    pub fn read_config_u32(&self, offset: u16) -> u32 {
        self.read_u32(legacy_regs::CONFIG + offset)
    }

    /// Read a config u64
    pub fn read_config_u64(&self, offset: u16) -> u64 {
        let low = self.read_config_u32(offset) as u64;
        let high = self.read_config_u32(offset + 4) as u64;
        low | (high << 32)
    }

    // Helper methods for register access

    fn read_u8(&self, offset: u16) -> u8 {
        if self.is_io_port {
            let mut port: Port<u8> = Port::new(self.io_base + offset);
            unsafe { port.read() }
        } else {
            unsafe { read_volatile((self.mmio_base + offset as u64) as *const u8) }
        }
    }

    fn write_u8(&self, offset: u16, value: u8) {
        if self.is_io_port {
            let mut port: Port<u8> = Port::new(self.io_base + offset);
            unsafe { port.write(value) }
        } else {
            unsafe { write_volatile((self.mmio_base + offset as u64) as *mut u8, value) }
        }
    }

    fn read_u16(&self, offset: u16) -> u16 {
        if self.is_io_port {
            let mut port: Port<u16> = Port::new(self.io_base + offset);
            unsafe { port.read() }
        } else {
            unsafe { read_volatile((self.mmio_base + offset as u64) as *const u16) }
        }
    }

    fn write_u16(&self, offset: u16, value: u16) {
        if self.is_io_port {
            let mut port: Port<u16> = Port::new(self.io_base + offset);
            unsafe { port.write(value) }
        } else {
            unsafe { write_volatile((self.mmio_base + offset as u64) as *mut u16, value) }
        }
    }

    fn read_u32(&self, offset: u16) -> u32 {
        if self.is_io_port {
            let mut port: Port<u32> = Port::new(self.io_base + offset);
            unsafe { port.read() }
        } else {
            unsafe { read_volatile((self.mmio_base + offset as u64) as *const u32) }
        }
    }

    fn write_u32(&self, offset: u16, value: u32) {
        if self.is_io_port {
            let mut port: Port<u32> = Port::new(self.io_base + offset);
            unsafe { port.write(value) }
        } else {
            unsafe { write_volatile((self.mmio_base + offset as u64) as *mut u32, value) }
        }
    }

    /// Get the underlying PCI device
    pub fn pci_device(&self) -> &PciDevice {
        &self.device
    }
}

impl core::fmt::Debug for PciTransport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PciTransport")
            .field("device_id", &self.device_id())
            .field("io_base", &format_args!("0x{:x}", self.io_base))
            .field("is_io_port", &self.is_io_port)
            .field("status", &self.status())
            .finish()
    }
}

/// Standard device initialization sequence for PCI transport
///
/// Performs the VirtIO device initialization:
/// 1. Reset device
/// 2. Set ACKNOWLEDGE status bit
/// 3. Set DRIVER status bit
/// 4. Read/negotiate features
/// 5. Set FEATURES_OK status bit (for modern) or proceed (for legacy)
pub fn init_device(transport: &PciTransport, driver_features: u32) -> VirtioResult<()> {
    // 1. Reset device
    transport.reset();

    // 2. Set ACKNOWLEDGE status bit
    transport.set_status(DeviceStatus::ACKNOWLEDGE);

    // 3. Set DRIVER status bit
    transport.set_status(DeviceStatus::ACKNOWLEDGE | DeviceStatus::DRIVER);

    // 4. Read device features and negotiate
    let device_features = transport.device_features();
    let negotiated = device_features & driver_features;
    
    crate::serial_println!("[virtio-pci] Device features: 0x{:08x}", device_features);
    crate::serial_println!("[virtio-pci] Negotiated features: 0x{:08x}", negotiated);
    
    transport.set_driver_features(negotiated);

    // For legacy devices, we skip FEATURES_OK
    Ok(())
}

/// Finalize device initialization by setting DRIVER_OK
pub fn finalize_device(transport: &PciTransport) {
    let status = transport.status();
    transport.set_status(status | DeviceStatus::DRIVER_OK);
}
