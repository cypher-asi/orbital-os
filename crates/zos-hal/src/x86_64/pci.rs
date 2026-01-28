//! PCI Configuration Space Access
//!
//! Implements PCI configuration space access via I/O ports (mechanism #1).
//! This is the standard way to discover and configure PCI devices on x86.
//!
//! # PCI Configuration Address Format
//!
//! ```text
//! 31    24 23   16 15    11 10     8 7      2 1  0
//! ┌──────┬───────┬────────┬────────┬────────┬────┐
//! │Enable│Reserved│  Bus  │ Device │Function│Reg │00
//! └──────┴───────┴────────┴────────┴────────┴────┘
//! ```

use x86_64::instructions::port::{Port, PortWriteOnly, PortReadOnly};

/// PCI Configuration Address port (0xCF8)
const PCI_CONFIG_ADDRESS: u16 = 0xCF8;

/// PCI Configuration Data port (0xCFC)
const PCI_CONFIG_DATA: u16 = 0xCFC;

/// PCI device classes
pub mod class {
    pub const MASS_STORAGE: u8 = 0x01;
    pub const NETWORK: u8 = 0x02;
    pub const DISPLAY: u8 = 0x03;
    pub const BRIDGE: u8 = 0x06;
}

/// PCI Vendor IDs
pub mod vendor {
    pub const VIRTIO: u16 = 0x1AF4;
}

/// VirtIO PCI Device IDs (transitional devices)
/// Legacy IDs are 0x1000-0x103F
pub mod virtio_device {
    pub const NET: u16 = 0x1000;
    pub const BLOCK: u16 = 0x1001;
    pub const BALLOON: u16 = 0x1002;
    pub const CONSOLE: u16 = 0x1003;
    pub const SCSI: u16 = 0x1004;
    pub const RNG: u16 = 0x1005;
    pub const GPU: u16 = 0x1040;
}

/// A PCI device address (bus, device, function)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciAddress {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciAddress {
    pub const fn new(bus: u8, device: u8, function: u8) -> Self {
        Self { bus, device, function }
    }

    /// Build the configuration address for a register
    fn config_address(&self, register: u8) -> u32 {
        let bus = self.bus as u32;
        let device = self.device as u32;
        let function = self.function as u32;
        let register = (register as u32) & 0xFC; // Align to dword

        0x8000_0000 // Enable bit
            | (bus << 16)
            | (device << 11)
            | (function << 8)
            | register
    }
}

/// Read a 32-bit value from PCI configuration space
pub fn config_read_u32(addr: PciAddress, register: u8) -> u32 {
    let config_addr = addr.config_address(register);
    unsafe {
        let mut addr_port: PortWriteOnly<u32> = PortWriteOnly::new(PCI_CONFIG_ADDRESS);
        let mut data_port: PortReadOnly<u32> = PortReadOnly::new(PCI_CONFIG_DATA);
        
        addr_port.write(config_addr);
        data_port.read()
    }
}

/// Write a 32-bit value to PCI configuration space
pub fn config_write_u32(addr: PciAddress, register: u8, value: u32) {
    let config_addr = addr.config_address(register);
    unsafe {
        let mut addr_port: PortWriteOnly<u32> = PortWriteOnly::new(PCI_CONFIG_ADDRESS);
        let mut data_port: Port<u32> = Port::new(PCI_CONFIG_DATA);
        
        addr_port.write(config_addr);
        data_port.write(value);
    }
}

/// Read a 16-bit value from PCI configuration space
pub fn config_read_u16(addr: PciAddress, register: u8) -> u16 {
    let dword = config_read_u32(addr, register & 0xFC);
    let shift = ((register & 2) * 8) as u32;
    (dword >> shift) as u16
}

/// Read an 8-bit value from PCI configuration space
pub fn config_read_u8(addr: PciAddress, register: u8) -> u8 {
    let dword = config_read_u32(addr, register & 0xFC);
    let shift = ((register & 3) * 8) as u32;
    (dword >> shift) as u8
}

/// PCI configuration space header registers
pub mod regs {
    pub const VENDOR_ID: u8 = 0x00;
    pub const DEVICE_ID: u8 = 0x02;
    pub const COMMAND: u8 = 0x04;
    pub const STATUS: u8 = 0x06;
    pub const REVISION: u8 = 0x08;
    pub const PROG_IF: u8 = 0x09;
    pub const SUBCLASS: u8 = 0x0A;
    pub const CLASS: u8 = 0x0B;
    pub const HEADER_TYPE: u8 = 0x0E;
    pub const BAR0: u8 = 0x10;
    pub const BAR1: u8 = 0x14;
    pub const BAR2: u8 = 0x18;
    pub const BAR3: u8 = 0x1C;
    pub const BAR4: u8 = 0x20;
    pub const BAR5: u8 = 0x24;
    pub const SUBSYSTEM_VENDOR_ID: u8 = 0x2C;
    pub const SUBSYSTEM_ID: u8 = 0x2E;
    pub const CAP_PTR: u8 = 0x34;
    pub const INTERRUPT_LINE: u8 = 0x3C;
    pub const INTERRUPT_PIN: u8 = 0x3D;
}

/// PCI command register bits
pub mod command {
    pub const IO_SPACE: u16 = 1 << 0;
    pub const MEMORY_SPACE: u16 = 1 << 1;
    pub const BUS_MASTER: u16 = 1 << 2;
    pub const INTERRUPT_DISABLE: u16 = 1 << 10;
}

/// A discovered PCI device
#[derive(Clone, Copy, Debug)]
pub struct PciDevice {
    pub addr: PciAddress,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub header_type: u8,
}

impl PciDevice {
    /// Read device info from configuration space
    pub fn read(addr: PciAddress) -> Option<Self> {
        let vendor_id = config_read_u16(addr, regs::VENDOR_ID);
        
        // 0xFFFF means no device
        if vendor_id == 0xFFFF {
            return None;
        }

        let device_id = config_read_u16(addr, regs::DEVICE_ID);
        let class = config_read_u8(addr, regs::CLASS);
        let subclass = config_read_u8(addr, regs::SUBCLASS);
        let prog_if = config_read_u8(addr, regs::PROG_IF);
        let header_type = config_read_u8(addr, regs::HEADER_TYPE);

        Some(Self {
            addr,
            vendor_id,
            device_id,
            class,
            subclass,
            prog_if,
            header_type,
        })
    }

    /// Check if this is a VirtIO device
    pub fn is_virtio(&self) -> bool {
        self.vendor_id == vendor::VIRTIO
    }

    /// Get BAR (Base Address Register) value
    pub fn read_bar(&self, bar_index: u8) -> u32 {
        if bar_index > 5 {
            return 0;
        }
        config_read_u32(self.addr, regs::BAR0 + bar_index * 4)
    }

    /// Get BAR as memory address (masking off flags)
    pub fn bar_address(&self, bar_index: u8) -> u64 {
        let bar = self.read_bar(bar_index);
        
        // Check if memory-mapped (bit 0 = 0) or I/O (bit 0 = 1)
        if bar & 1 != 0 {
            // I/O BAR - mask lower 2 bits
            (bar & !0x3) as u64
        } else {
            // Memory BAR - check if 64-bit
            let bar_type = (bar >> 1) & 0x3;
            if bar_type == 2 && bar_index < 5 {
                // 64-bit BAR
                let bar_high = self.read_bar(bar_index + 1);
                ((bar_high as u64) << 32) | ((bar & !0xF) as u64)
            } else {
                // 32-bit BAR
                (bar & !0xF) as u64
            }
        }
    }

    /// Check if BAR is I/O space
    pub fn bar_is_io(&self, bar_index: u8) -> bool {
        self.read_bar(bar_index) & 1 != 0
    }

    /// Enable device by setting command register bits
    pub fn enable(&self) {
        let cmd = config_read_u16(self.addr, regs::COMMAND);
        let new_cmd = cmd | command::MEMORY_SPACE | command::BUS_MASTER;
        config_write_u32(self.addr, regs::COMMAND, new_cmd as u32);
    }

    /// Disable interrupts via command register
    pub fn disable_interrupts(&self) {
        let cmd = config_read_u16(self.addr, regs::COMMAND);
        let new_cmd = cmd | command::INTERRUPT_DISABLE;
        config_write_u32(self.addr, regs::COMMAND, new_cmd as u32);
    }
}

/// Enumerate all PCI devices
pub fn enumerate_devices() -> impl Iterator<Item = PciDevice> {
    PciEnumerator::new()
}

/// Iterator over PCI devices
struct PciEnumerator {
    bus: u8,
    device: u8,
    function: u8,
    done: bool,
}

impl PciEnumerator {
    fn new() -> Self {
        Self {
            bus: 0,
            device: 0,
            function: 0,
            done: false,
        }
    }

    fn advance(&mut self) {
        self.function += 1;
        if self.function >= 8 {
            self.function = 0;
            self.device += 1;
            if self.device >= 32 {
                self.device = 0;
                self.bus = self.bus.wrapping_add(1);
                if self.bus == 0 {
                    self.done = true;
                }
            }
        }
    }
}

impl Iterator for PciEnumerator {
    type Item = PciDevice;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.done {
            let addr = PciAddress::new(self.bus, self.device, self.function);
            
            // Try to read device
            if let Some(device) = PciDevice::read(addr) {
                // Check if multi-function device
                let is_multi_function = self.function == 0 && (device.header_type & 0x80) != 0;
                
                self.advance();
                
                // Skip other functions if not multi-function
                if !is_multi_function && self.function != 0 {
                    self.function = 0;
                    self.device += 1;
                    if self.device >= 32 {
                        self.device = 0;
                        self.bus = self.bus.wrapping_add(1);
                        if self.bus == 0 {
                            self.done = true;
                        }
                    }
                }
                
                return Some(device);
            } else {
                // No device, skip rest of functions for this device
                if self.function == 0 {
                    self.device += 1;
                    if self.device >= 32 {
                        self.device = 0;
                        self.bus = self.bus.wrapping_add(1);
                        if self.bus == 0 {
                            self.done = true;
                        }
                    }
                } else {
                    self.advance();
                }
            }
        }
        
        None
    }
}

/// Find VirtIO block devices
pub fn find_virtio_block() -> Option<PciDevice> {
    for device in enumerate_devices() {
        if device.is_virtio() && device.device_id == virtio_device::BLOCK {
            return Some(device);
        }
    }
    None
}

/// Initialize PCI subsystem and log discovered devices
pub fn init() {
    crate::serial_println!("[pci] Scanning PCI bus...");
    
    let mut count = 0;
    for device in enumerate_devices() {
        crate::serial_println!(
            "[pci] {:02x}:{:02x}.{} - {:04x}:{:04x} class={:02x}.{:02x}",
            device.addr.bus,
            device.addr.device,
            device.addr.function,
            device.vendor_id,
            device.device_id,
            device.class,
            device.subclass
        );
        count += 1;
    }
    
    crate::serial_println!("[pci] Found {} device(s)", count);
    
    // Look for VirtIO block device
    if let Some(blk) = find_virtio_block() {
        crate::serial_println!("[pci] Found VirtIO block device at {:02x}:{:02x}.{}",
            blk.addr.bus, blk.addr.device, blk.addr.function);
        crate::serial_println!("[pci]   BAR0: 0x{:x}", blk.bar_address(0));
        crate::serial_println!("[pci]   BAR1: 0x{:x}", blk.bar_address(1));
    }
}
