//! VirtIO Driver Framework for Zero OS
//!
//! This module implements the VirtIO 1.1 specification for QEMU virtual devices.
//! VirtIO provides a standardized interface for paravirtualized devices.
//!
//! # Components
//!
//! - **transport**: MMIO transport for device discovery and access
//! - **queue**: Virtqueue implementation (split virtqueue)
//! - **blk**: VirtIO block device driver
//!
//! # References
//!
//! - VirtIO Spec 1.1: <https://docs.oasis-open.org/virtio/virtio/v1.1/virtio-v1.1.html>

pub mod queue;
pub mod transport;
pub mod pci;
pub mod blk;
pub mod blk_pci;

use core::fmt;

/// VirtIO device IDs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum DeviceId {
    /// Network card
    Network = 1,
    /// Block device
    Block = 2,
    /// Console
    Console = 3,
    /// Entropy source
    Entropy = 4,
    /// Memory ballooning
    Balloon = 5,
    /// SCSI host
    Scsi = 8,
    /// GPU
    Gpu = 16,
    /// Unknown device
    Unknown = 0xFFFF,
}

impl From<u32> for DeviceId {
    fn from(value: u32) -> Self {
        match value {
            1 => DeviceId::Network,
            2 => DeviceId::Block,
            3 => DeviceId::Console,
            4 => DeviceId::Entropy,
            5 => DeviceId::Balloon,
            8 => DeviceId::Scsi,
            16 => DeviceId::Gpu,
            _ => DeviceId::Unknown,
        }
    }
}

/// VirtIO device status bits
#[derive(Clone, Copy, Debug)]
pub struct DeviceStatus(u8);

impl DeviceStatus {
    /// Reset device
    pub const RESET: Self = Self(0);
    /// Guest OS has found the device
    pub const ACKNOWLEDGE: Self = Self(1);
    /// Guest OS knows how to drive the device
    pub const DRIVER: Self = Self(2);
    /// Driver is ready to drive the device
    pub const DRIVER_OK: Self = Self(4);
    /// Feature negotiation complete
    pub const FEATURES_OK: Self = Self(8);
    /// Device needs reset (device sets this)
    pub const DEVICE_NEEDS_RESET: Self = Self(64);
    /// Fatal error (driver sets this)
    pub const FAILED: Self = Self(128);

    pub fn bits(&self) -> u8 {
        self.0
    }

    pub fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl core::ops::BitOr for DeviceStatus {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for DeviceStatus {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// VirtIO driver error type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtioError {
    /// Device not found
    DeviceNotFound,
    /// Invalid magic value in MMIO header
    InvalidMagic,
    /// Unsupported VirtIO version
    UnsupportedVersion,
    /// Queue not available
    QueueNotAvailable,
    /// Queue already in use
    QueueAlreadyUsed,
    /// No memory for queue buffers
    OutOfMemory,
    /// Device configuration failed
    ConfigFailed,
    /// Feature negotiation failed
    FeatureNegotiationFailed,
    /// I/O error during operation
    IoError,
    /// Request timeout
    Timeout,
    /// Buffer too small
    BufferTooSmall,
    /// Invalid argument
    InvalidArgument,
}

impl fmt::Display for VirtioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VirtioError::DeviceNotFound => write!(f, "VirtIO device not found"),
            VirtioError::InvalidMagic => write!(f, "Invalid VirtIO magic value"),
            VirtioError::UnsupportedVersion => write!(f, "Unsupported VirtIO version"),
            VirtioError::QueueNotAvailable => write!(f, "Virtqueue not available"),
            VirtioError::QueueAlreadyUsed => write!(f, "Virtqueue already in use"),
            VirtioError::OutOfMemory => write!(f, "Out of memory for virtqueue"),
            VirtioError::ConfigFailed => write!(f, "Device configuration failed"),
            VirtioError::FeatureNegotiationFailed => write!(f, "Feature negotiation failed"),
            VirtioError::IoError => write!(f, "I/O error"),
            VirtioError::Timeout => write!(f, "Request timeout"),
            VirtioError::BufferTooSmall => write!(f, "Buffer too small"),
            VirtioError::InvalidArgument => write!(f, "Invalid argument"),
        }
    }
}

/// Result type for VirtIO operations
pub type VirtioResult<T> = Result<T, VirtioError>;

/// Common VirtIO feature bits (bits 0-23 device specific, 24-37 reserved, 38+ transport)
pub mod features {
    /// Ring has indirect descriptor support
    pub const RING_INDIRECT_DESC: u64 = 1 << 28;
    /// Ring has event index support
    pub const RING_EVENT_IDX: u64 = 1 << 29;
    /// VirtIO version 1.0 compliance
    pub const VERSION_1: u64 = 1 << 32;
    /// Access platform (IOMMU) support
    pub const ACCESS_PLATFORM: u64 = 1 << 33;
    /// Ring can be packed
    pub const RING_PACKED: u64 = 1 << 34;
    /// In-order buffer usage
    pub const IN_ORDER: u64 = 1 << 35;
    /// Configuration change notification
    pub const ORDER_PLATFORM: u64 = 1 << 36;
    /// Single root I/O virtualization
    pub const SR_IOV: u64 = 1 << 37;
    /// Notification without data
    pub const NOTIFICATION_DATA: u64 = 1 << 38;
}

/// Initialize VirtIO subsystem
///
/// Scans for VirtIO devices via PCI bus.
/// For x86_64 QEMU, VirtIO devices are typically PCI devices.
pub fn init() {
    crate::serial_println!("[virtio] Initializing VirtIO subsystem...");
    
    // Initialize PCI subsystem and scan for devices
    crate::x86_64::pci::init();
    
    // Check for VirtIO block device
    if let Some(blk_device) = crate::x86_64::pci::find_virtio_block() {
        crate::serial_println!("[virtio] Found VirtIO block device");
        crate::serial_println!("[virtio]   PCI {:02x}:{:02x}.{} - {:04x}:{:04x}",
            blk_device.addr.bus, blk_device.addr.device, blk_device.addr.function,
            blk_device.vendor_id, blk_device.device_id);
    } else {
        crate::serial_println!("[virtio] No VirtIO block device found");
    }
    
    crate::serial_println!("[virtio] VirtIO initialization complete");
}

/// Initialize VirtIO block device with provided queue memory
///
/// # Safety
/// Must be called after VMM initialization with valid queue memory.
pub unsafe fn init_block_device(queue_memory: u64) -> VirtioResult<()> {
    blk_pci::init_from_pci(queue_memory)
}
