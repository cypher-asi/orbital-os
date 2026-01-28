//! VirtIO MMIO Transport
//!
//! Implements the VirtIO Memory-Mapped I/O (MMIO) transport layer.
//! This is the simplest transport, used by QEMU's virt machine.
//!
//! # MMIO Register Layout (VirtIO 1.0+)
//!
//! | Offset | Size | Name | Description |
//! |--------|------|------|-------------|
//! | 0x000  | 4    | MagicValue | "virt" = 0x74726976 |
//! | 0x004  | 4    | Version | Device version (2 for virtio 1.0+) |
//! | 0x008  | 4    | DeviceID | VirtIO device ID |
//! | 0x00c  | 4    | VendorID | VirtIO vendor ID |
//! | 0x010  | 4    | DeviceFeatures | Device feature bits (selected by sel) |
//! | 0x014  | 4    | DeviceFeaturesSel | Device feature selection |
//! | 0x020  | 4    | DriverFeatures | Driver feature bits (selected by sel) |
//! | 0x024  | 4    | DriverFeaturesSel | Driver feature selection |
//! | 0x030  | 4    | QueueSel | Virtual queue index |
//! | 0x034  | 4    | QueueNumMax | Max size of selected queue |
//! | 0x038  | 4    | QueueNum | Queue size |
//! | 0x044  | 4    | QueueReady | Queue ready bit |
//! | 0x050  | 4    | QueueNotify | Queue notification |
//! | 0x060  | 4    | InterruptStatus | Interrupt status |
//! | 0x064  | 4    | InterruptACK | Interrupt acknowledge |
//! | 0x070  | 4    | Status | Device status |
//! | 0x080  | 4    | QueueDescLow | Descriptor table address (low) |
//! | 0x084  | 4    | QueueDescHigh | Descriptor table address (high) |
//! | 0x090  | 4    | QueueDriverLow | Available ring address (low) |
//! | 0x094  | 4    | QueueDriverHigh | Available ring address (high) |
//! | 0x0a0  | 4    | QueueDeviceLow | Used ring address (low) |
//! | 0x0a4  | 4    | QueueDeviceHigh | Used ring address (high) |
//! | 0x100+ | var  | Config | Device-specific configuration |

use core::ptr::{read_volatile, write_volatile};
use super::{DeviceId, DeviceStatus, VirtioError, VirtioResult};

/// VirtIO MMIO magic value ("virt" in little-endian)
const VIRTIO_MAGIC: u32 = 0x74726976;

/// VirtIO MMIO version for virtio 1.0+
const VIRTIO_VERSION: u32 = 2;

/// Legacy VirtIO MMIO version
const VIRTIO_VERSION_LEGACY: u32 = 1;

/// MMIO register offsets
mod regs {
    pub const MAGIC_VALUE: usize = 0x000;
    pub const VERSION: usize = 0x004;
    pub const DEVICE_ID: usize = 0x008;
    pub const VENDOR_ID: usize = 0x00c;
    pub const DEVICE_FEATURES: usize = 0x010;
    pub const DEVICE_FEATURES_SEL: usize = 0x014;
    pub const DRIVER_FEATURES: usize = 0x020;
    pub const DRIVER_FEATURES_SEL: usize = 0x024;
    pub const QUEUE_SEL: usize = 0x030;
    pub const QUEUE_NUM_MAX: usize = 0x034;
    pub const QUEUE_NUM: usize = 0x038;
    pub const QUEUE_READY: usize = 0x044;
    pub const QUEUE_NOTIFY: usize = 0x050;
    pub const INTERRUPT_STATUS: usize = 0x060;
    pub const INTERRUPT_ACK: usize = 0x064;
    pub const STATUS: usize = 0x070;
    pub const QUEUE_DESC_LOW: usize = 0x080;
    pub const QUEUE_DESC_HIGH: usize = 0x084;
    pub const QUEUE_DRIVER_LOW: usize = 0x090;
    pub const QUEUE_DRIVER_HIGH: usize = 0x094;
    pub const QUEUE_DEVICE_LOW: usize = 0x0a0;
    pub const QUEUE_DEVICE_HIGH: usize = 0x0a4;
    pub const CONFIG: usize = 0x100;
}

/// VirtIO MMIO Transport
///
/// Provides low-level access to a VirtIO device via MMIO.
#[derive(Clone, Copy)]
pub struct MmioTransport {
    /// Base address of the MMIO region
    base: u64,
}

impl MmioTransport {
    /// Create a new MMIO transport at the given base address
    ///
    /// # Safety
    /// The base address must point to a valid VirtIO MMIO device.
    pub const unsafe fn new(base: u64) -> Self {
        Self { base }
    }

    /// Probe the device and verify it's a valid VirtIO device
    pub fn probe(&self) -> VirtioResult<DeviceId> {
        let magic = self.read_reg(regs::MAGIC_VALUE);
        if magic != VIRTIO_MAGIC {
            return Err(VirtioError::InvalidMagic);
        }

        let version = self.read_reg(regs::VERSION);
        if version != VIRTIO_VERSION && version != VIRTIO_VERSION_LEGACY {
            return Err(VirtioError::UnsupportedVersion);
        }

        let device_id = self.read_reg(regs::DEVICE_ID);
        if device_id == 0 {
            return Err(VirtioError::DeviceNotFound);
        }

        Ok(DeviceId::from(device_id))
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        DeviceId::from(self.read_reg(regs::DEVICE_ID))
    }

    /// Get the vendor ID
    pub fn vendor_id(&self) -> u32 {
        self.read_reg(regs::VENDOR_ID)
    }

    /// Get the device version
    pub fn version(&self) -> u32 {
        self.read_reg(regs::VERSION)
    }

    /// Read device status
    pub fn status(&self) -> DeviceStatus {
        DeviceStatus::from_bits(self.read_reg(regs::STATUS) as u8)
    }

    /// Write device status
    pub fn set_status(&self, status: DeviceStatus) {
        self.write_reg(regs::STATUS, status.bits() as u32);
    }

    /// Reset the device
    pub fn reset(&self) {
        self.write_reg(regs::STATUS, 0);
        // Wait for reset to complete
        while self.read_reg(regs::STATUS) != 0 {
            core::hint::spin_loop();
        }
    }

    /// Read device feature bits
    ///
    /// Features are split into two 32-bit halves selected by `sel`.
    pub fn device_features(&self, sel: u32) -> u32 {
        self.write_reg(regs::DEVICE_FEATURES_SEL, sel);
        self.read_reg(regs::DEVICE_FEATURES)
    }

    /// Read all 64 device feature bits
    pub fn device_features_all(&self) -> u64 {
        let low = self.device_features(0) as u64;
        let high = self.device_features(1) as u64;
        low | (high << 32)
    }

    /// Write driver feature bits
    ///
    /// Features are split into two 32-bit halves selected by `sel`.
    pub fn set_driver_features(&self, sel: u32, features: u32) {
        self.write_reg(regs::DRIVER_FEATURES_SEL, sel);
        self.write_reg(regs::DRIVER_FEATURES, features);
    }

    /// Write all 64 driver feature bits
    pub fn set_driver_features_all(&self, features: u64) {
        self.set_driver_features(0, features as u32);
        self.set_driver_features(1, (features >> 32) as u32);
    }

    /// Select a virtqueue
    pub fn select_queue(&self, queue_index: u16) {
        self.write_reg(regs::QUEUE_SEL, queue_index as u32);
    }

    /// Get the maximum queue size for the selected queue
    pub fn queue_max_size(&self) -> u16 {
        self.read_reg(regs::QUEUE_NUM_MAX) as u16
    }

    /// Set the queue size for the selected queue
    pub fn set_queue_size(&self, size: u16) {
        self.write_reg(regs::QUEUE_NUM, size as u32);
    }

    /// Check if the selected queue is ready
    pub fn queue_ready(&self) -> bool {
        self.read_reg(regs::QUEUE_READY) != 0
    }

    /// Set the selected queue as ready
    pub fn set_queue_ready(&self, ready: bool) {
        self.write_reg(regs::QUEUE_READY, if ready { 1 } else { 0 });
    }

    /// Set the descriptor table address for the selected queue
    pub fn set_queue_desc(&self, addr: u64) {
        self.write_reg(regs::QUEUE_DESC_LOW, addr as u32);
        self.write_reg(regs::QUEUE_DESC_HIGH, (addr >> 32) as u32);
    }

    /// Set the available ring address for the selected queue
    pub fn set_queue_driver(&self, addr: u64) {
        self.write_reg(regs::QUEUE_DRIVER_LOW, addr as u32);
        self.write_reg(regs::QUEUE_DRIVER_HIGH, (addr >> 32) as u32);
    }

    /// Set the used ring address for the selected queue
    pub fn set_queue_device(&self, addr: u64) {
        self.write_reg(regs::QUEUE_DEVICE_LOW, addr as u32);
        self.write_reg(regs::QUEUE_DEVICE_HIGH, (addr >> 32) as u32);
    }

    /// Notify the device that there are new buffers in the queue
    pub fn notify_queue(&self, queue_index: u16) {
        self.write_reg(regs::QUEUE_NOTIFY, queue_index as u32);
    }

    /// Read interrupt status
    pub fn interrupt_status(&self) -> u32 {
        self.read_reg(regs::INTERRUPT_STATUS)
    }

    /// Acknowledge interrupts
    pub fn interrupt_ack(&self, status: u32) {
        self.write_reg(regs::INTERRUPT_ACK, status);
    }

    /// Read a device-specific configuration byte
    pub fn read_config_u8(&self, offset: usize) -> u8 {
        unsafe {
            read_volatile((self.base + (regs::CONFIG + offset) as u64) as *const u8)
        }
    }

    /// Read a device-specific configuration u32
    pub fn read_config_u32(&self, offset: usize) -> u32 {
        unsafe {
            read_volatile((self.base + (regs::CONFIG + offset) as u64) as *const u32)
        }
    }

    /// Read a device-specific configuration u64
    pub fn read_config_u64(&self, offset: usize) -> u64 {
        // Read as two u32s to avoid alignment issues
        let low = self.read_config_u32(offset) as u64;
        let high = self.read_config_u32(offset + 4) as u64;
        low | (high << 32)
    }

    /// Write a device-specific configuration byte
    pub fn write_config_u8(&self, offset: usize, value: u8) {
        unsafe {
            write_volatile((self.base + (regs::CONFIG + offset) as u64) as *mut u8, value);
        }
    }

    /// Write a device-specific configuration u32
    pub fn write_config_u32(&self, offset: usize, value: u32) {
        unsafe {
            write_volatile((self.base + (regs::CONFIG + offset) as u64) as *mut u32, value);
        }
    }

    /// Read a 32-bit register
    fn read_reg(&self, offset: usize) -> u32 {
        unsafe {
            read_volatile((self.base + offset as u64) as *const u32)
        }
    }

    /// Write a 32-bit register
    fn write_reg(&self, offset: usize, value: u32) {
        unsafe {
            write_volatile((self.base + offset as u64) as *mut u32, value);
        }
    }

    /// Get the base address
    pub fn base(&self) -> u64 {
        self.base
    }
}

impl core::fmt::Debug for MmioTransport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MmioTransport")
            .field("base", &format_args!("0x{:x}", self.base))
            .field("device_id", &self.device_id())
            .field("version", &self.version())
            .field("status", &self.status())
            .finish()
    }
}

/// Probe for a VirtIO MMIO device at the given address
///
/// Returns the device ID if a valid device is found, None otherwise.
pub fn probe_mmio(addr: u64) -> Option<DeviceId> {
    let transport = unsafe { MmioTransport::new(addr) };
    transport.probe().ok()
}

/// Standard device initialization sequence
///
/// Performs the VirtIO device initialization as per spec section 3.1:
/// 1. Reset device
/// 2. Set ACKNOWLEDGE status bit
/// 3. Set DRIVER status bit
/// 4. Read/negotiate features
/// 5. Set FEATURES_OK status bit
/// 6. Re-read status to confirm FEATURES_OK
/// 7. Perform device-specific setup
/// 8. Set DRIVER_OK status bit
pub fn init_device(transport: &MmioTransport, driver_features: u64) -> VirtioResult<()> {
    // 1. Reset device
    transport.reset();

    // 2. Set ACKNOWLEDGE status bit
    transport.set_status(DeviceStatus::ACKNOWLEDGE);

    // 3. Set DRIVER status bit
    transport.set_status(DeviceStatus::ACKNOWLEDGE | DeviceStatus::DRIVER);

    // 4. Read device features and negotiate
    let device_features = transport.device_features_all();
    let negotiated = device_features & driver_features;
    transport.set_driver_features_all(negotiated);

    // 5. Set FEATURES_OK status bit
    transport.set_status(
        DeviceStatus::ACKNOWLEDGE | DeviceStatus::DRIVER | DeviceStatus::FEATURES_OK
    );

    // 6. Re-read status to ensure FEATURES_OK is still set
    let status = transport.status();
    if !status.contains(DeviceStatus::FEATURES_OK) {
        transport.set_status(DeviceStatus::FAILED);
        return Err(VirtioError::FeatureNegotiationFailed);
    }

    // Device-specific setup happens after this, then DRIVER_OK is set
    Ok(())
}

/// Finalize device initialization by setting DRIVER_OK
pub fn finalize_device(transport: &MmioTransport) {
    let status = transport.status();
    transport.set_status(status | DeviceStatus::DRIVER_OK);
}
