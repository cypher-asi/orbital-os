//! VirtIO Block Device Driver
//!
//! Implements a driver for VirtIO block devices (virtio-blk).
//!
//! # Protocol
//!
//! Block requests use a three-part descriptor chain:
//! 1. Request header (read-only): operation type, sector number
//! 2. Data buffer: sector data (read or write depending on op)
//! 3. Status byte (write-only): completion status
//!
//! # Device Configuration
//!
//! ```text
//! Offset | Size | Name
//! -------|------|------
//! 0x00   | 8    | capacity (number of 512-byte sectors)
//! 0x08   | 4    | size_max
//! 0x0c   | 4    | seg_max
//! 0x10   | 2    | cylinders (geometry)
//! 0x12   | 1    | heads
//! 0x13   | 1    | sectors
//! 0x14   | 4    | blk_size
//! ```

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use super::queue::Virtqueue;
use super::transport::{init_device, finalize_device, MmioTransport};
use super::{features, DeviceId, VirtioError, VirtioResult};

/// VirtIO block device feature bits
pub mod blk_features {
    /// Maximum size of any single segment is in size_max
    pub const SIZE_MAX: u64 = 1 << 1;
    /// Maximum number of segments in a request is in seg_max
    pub const SEG_MAX: u64 = 1 << 2;
    /// Disk-style geometry specified in geometry
    pub const GEOMETRY: u64 = 1 << 4;
    /// Block size of disk is in blk_size
    pub const BLK_SIZE: u64 = 1 << 6;
    /// Cache flush command support
    pub const FLUSH: u64 = 1 << 9;
    /// Device exports information on optimal I/O alignment
    pub const TOPOLOGY: u64 = 1 << 10;
    /// Device can toggle its cache between writeback and writethrough modes
    pub const CONFIG_WCE: u64 = 1 << 11;
    /// Device supports multiqueue
    pub const MQ: u64 = 1 << 12;
    /// Device can support discard command
    pub const DISCARD: u64 = 1 << 13;
    /// Device can support write zeroes command
    pub const WRITE_ZEROES: u64 = 1 << 14;
}

/// Block request types
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockRequestType {
    /// Read sectors
    In = 0,
    /// Write sectors
    Out = 1,
    /// Flush cache
    Flush = 4,
    /// Get device ID
    GetId = 8,
    /// Discard sectors
    Discard = 11,
    /// Write zeroes
    WriteZeroes = 13,
}

/// Block request status
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockStatus {
    /// Success
    Ok = 0,
    /// I/O error
    IoErr = 1,
    /// Unsupported operation
    Unsupported = 2,
}

impl From<u8> for BlockStatus {
    fn from(value: u8) -> Self {
        match value {
            0 => BlockStatus::Ok,
            1 => BlockStatus::IoErr,
            _ => BlockStatus::Unsupported,
        }
    }
}

/// Block request header (16 bytes)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BlockRequestHeader {
    /// Request type
    pub request_type: u32,
    /// Reserved
    pub reserved: u32,
    /// Sector number (for read/write)
    pub sector: u64,
}

impl BlockRequestHeader {
    pub const SIZE: usize = 16;

    pub fn new(request_type: BlockRequestType, sector: u64) -> Self {
        Self {
            request_type: request_type as u32,
            reserved: 0,
            sector,
        }
    }
}

/// Sector size in bytes
pub const SECTOR_SIZE: usize = 512;

/// Default queue size
const DEFAULT_QUEUE_SIZE: u16 = 128;

/// In-flight request tracking
///
/// Fields are stored to keep allocations alive during DMA and for debugging.
#[allow(dead_code)]
struct InFlightRequest {
    /// Request header (heap allocated for stable address)
    header: Box<BlockRequestHeader>,
    /// Status byte (heap allocated for stable address)
    status: Box<u8>,
    /// Data buffer (owned by caller, we just track the address)
    data_addr: u64,
    /// Data length
    data_len: u32,
    /// Is this a read operation?
    is_read: bool,
}

/// VirtIO Block Device
///
/// Provides block-level read/write access to a VirtIO block device.
pub struct VirtioBlk {
    /// MMIO transport
    transport: MmioTransport,
    /// Request queue
    queue: Virtqueue,
    /// Device capacity in sectors
    capacity: u64,
    /// Block size (usually 512)
    block_size: u32,
    /// In-flight requests keyed by descriptor index
    in_flight: BTreeMap<u16, InFlightRequest>,
    /// Next request ID
    next_request_id: u64,
}

// SAFETY: VirtioBlk is designed for single-threaded access or with external synchronization
unsafe impl Send for VirtioBlk {}

impl VirtioBlk {
    /// Initialize a new VirtIO block device
    ///
    /// # Arguments
    /// * `transport` - MMIO transport for the device
    /// * `queue_memory` - Physical address of memory for virtqueue
    ///
    /// # Safety
    /// The transport must point to a valid virtio-blk device.
    /// The queue_memory must be properly sized and aligned.
    pub unsafe fn new(transport: MmioTransport, queue_memory: u64) -> VirtioResult<Self> {
        // Verify device type
        let device_id = transport.probe()?;
        if device_id != DeviceId::Block {
            return Err(VirtioError::DeviceNotFound);
        }

        // Initialize device with our supported features
        let driver_features = features::VERSION_1 | blk_features::FLUSH | blk_features::BLK_SIZE;
        init_device(&transport, driver_features)?;

        // Read device configuration
        let capacity = transport.read_config_u64(0); // offset 0: capacity
        let block_size = if transport.device_features_all() & blk_features::BLK_SIZE != 0 {
            transport.read_config_u32(0x14) // offset 0x14: blk_size
        } else {
            512
        };

        crate::serial_println!("[virtio-blk] Capacity: {} sectors ({} MB)", 
            capacity, capacity * 512 / (1024 * 1024));
        crate::serial_println!("[virtio-blk] Block size: {} bytes", block_size);

        // Set up the request queue (queue 0)
        transport.select_queue(0);
        let max_size = transport.queue_max_size();
        if max_size == 0 {
            return Err(VirtioError::QueueNotAvailable);
        }

        let queue_size = max_size.min(DEFAULT_QUEUE_SIZE);
        transport.set_queue_size(queue_size);

        // Create virtqueue
        let queue = Virtqueue::new(0, queue_size, queue_memory)?;

        // Configure queue addresses
        let (desc_addr, avail_addr, used_addr) = queue.get_addresses();
        transport.set_queue_desc(desc_addr);
        transport.set_queue_driver(avail_addr);
        transport.set_queue_device(used_addr);

        // Enable the queue
        transport.set_queue_ready(true);

        // Complete initialization
        finalize_device(&transport);

        crate::serial_println!("[virtio-blk] Device initialized successfully");

        Ok(Self {
            transport,
            queue,
            capacity,
            block_size,
            in_flight: BTreeMap::new(),
            next_request_id: 0,
        })
    }

    /// Get device capacity in sectors
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Get block size in bytes
    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    /// Get capacity in bytes
    pub fn capacity_bytes(&self) -> u64 {
        self.capacity * SECTOR_SIZE as u64
    }

    /// Check if the device has pending completions
    pub fn has_pending(&self) -> bool {
        self.queue.has_pending()
    }

    /// Submit a read request (async)
    ///
    /// # Arguments
    /// * `sector` - Starting sector number
    /// * `buffer` - Buffer to read into (must be sector-aligned size)
    ///
    /// # Returns
    /// Request ID that can be used to track completion.
    pub fn read_async(&mut self, sector: u64, buffer: &mut [u8]) -> VirtioResult<u64> {
        if sector >= self.capacity {
            return Err(VirtioError::InvalidArgument);
        }
        if buffer.len() % SECTOR_SIZE != 0 || buffer.is_empty() {
            return Err(VirtioError::InvalidArgument);
        }

        self.submit_request(BlockRequestType::In, sector, buffer.as_mut_ptr() as u64, buffer.len() as u32, true)
    }

    /// Submit a write request (async)
    ///
    /// # Arguments
    /// * `sector` - Starting sector number
    /// * `buffer` - Buffer containing data to write (must be sector-aligned size)
    ///
    /// # Returns
    /// Request ID that can be used to track completion.
    pub fn write_async(&mut self, sector: u64, buffer: &[u8]) -> VirtioResult<u64> {
        if sector >= self.capacity {
            return Err(VirtioError::InvalidArgument);
        }
        if buffer.len() % SECTOR_SIZE != 0 || buffer.is_empty() {
            return Err(VirtioError::InvalidArgument);
        }

        self.submit_request(BlockRequestType::Out, sector, buffer.as_ptr() as u64, buffer.len() as u32, false)
    }

    /// Submit a flush request (async)
    ///
    /// Ensures all previous writes are persisted to disk.
    pub fn flush_async(&mut self) -> VirtioResult<u64> {
        // Flush doesn't use data buffer
        self.submit_request(BlockRequestType::Flush, 0, 0, 0, false)
    }

    /// Submit a block request
    fn submit_request(
        &mut self,
        request_type: BlockRequestType,
        sector: u64,
        data_addr: u64,
        data_len: u32,
        is_read: bool,
    ) -> VirtioResult<u64> {
        // Allocate request header and status on heap for stable addresses
        let header = Box::new(BlockRequestHeader::new(request_type, sector));
        let status = Box::new(0xFFu8); // Initialize to invalid status

        let header_addr = header.as_ref() as *const _ as u64;
        let status_addr = status.as_ref() as *const _ as u64;

        // Build descriptor chain:
        // 1. Header (device-readable)
        // 2. Data buffer (device-readable for write, device-writable for read)
        // 3. Status (device-writable)
        let mut buffers: Vec<(u64, u32, bool)> = Vec::new();
        
        // Header: always device-readable
        buffers.push((header_addr, BlockRequestHeader::SIZE as u32, false));
        
        // Data buffer (if any)
        if data_len > 0 {
            buffers.push((data_addr, data_len, is_read));
        }
        
        // Status: always device-writable
        buffers.push((status_addr, 1, true));

        // Add to queue
        let desc_idx = self.queue.add_buffer_chain(&buffers)?;

        // Track in-flight request
        let request_id = self.next_request_id;
        self.next_request_id += 1;

        self.in_flight.insert(desc_idx, InFlightRequest {
            header,
            status,
            data_addr,
            data_len,
            is_read,
        });

        // Notify device
        self.transport.notify_queue(0);

        Ok(request_id)
    }

    /// Poll for completed requests
    ///
    /// Returns a vector of (request_id, status) pairs.
    pub fn poll_completions(&mut self) -> Vec<(u16, BlockStatus)> {
        let mut completions = Vec::new();

        while let Some((desc_idx, _bytes_written)) = self.queue.pop_used() {
            if let Some(request) = self.in_flight.remove(&desc_idx) {
                let status = BlockStatus::from(*request.status);
                completions.push((desc_idx, status));
            }
        }

        completions
    }

    /// Blocking read
    ///
    /// Submits a read request and waits for completion.
    pub fn read(&mut self, sector: u64, buffer: &mut [u8]) -> VirtioResult<()> {
        self.read_async(sector, buffer)?;
        self.wait_for_completion()
    }

    /// Blocking write
    ///
    /// Submits a write request and waits for completion.
    pub fn write(&mut self, sector: u64, buffer: &[u8]) -> VirtioResult<()> {
        self.write_async(sector, buffer)?;
        self.wait_for_completion()
    }

    /// Blocking flush
    ///
    /// Submits a flush request and waits for completion.
    pub fn flush(&mut self) -> VirtioResult<()> {
        self.flush_async()?;
        self.wait_for_completion()
    }

    /// Wait for the next completion
    fn wait_for_completion(&mut self) -> VirtioResult<()> {
        // Simple busy-wait (in a real system, use interrupts)
        let mut iterations = 0u64;
        loop {
            let completions = self.poll_completions();
            if !completions.is_empty() {
                // Check status of first completion
                let (_, status) = completions[0];
                return match status {
                    BlockStatus::Ok => Ok(()),
                    BlockStatus::IoErr => Err(VirtioError::IoError),
                    BlockStatus::Unsupported => Err(VirtioError::InvalidArgument),
                };
            }

            iterations += 1;
            if iterations > 1_000_000_000 {
                return Err(VirtioError::Timeout);
            }

            core::hint::spin_loop();
        }
    }
}

impl Drop for VirtioBlk {
    fn drop(&mut self) {
        // Reset device on drop
        self.transport.reset();
    }
}

impl core::fmt::Debug for VirtioBlk {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VirtioBlk")
            .field("capacity", &self.capacity)
            .field("block_size", &self.block_size)
            .field("in_flight", &self.in_flight.len())
            .finish()
    }
}

/// Global VirtIO block device instance
static VIRTIO_BLK: Mutex<Option<VirtioBlk>> = Mutex::new(None);

/// Frame allocator integration for queue memory
static QUEUE_MEMORY_BASE: AtomicU64 = AtomicU64::new(0);

/// Initialize the global VirtIO block device
///
/// # Arguments
/// * `mmio_base` - MMIO base address of the device
/// * `queue_memory` - Physical address of memory for the virtqueue
///
/// # Safety
/// Must be called after VMM initialization with valid addresses.
pub unsafe fn init_global(mmio_base: u64, queue_memory: u64) -> VirtioResult<()> {
    let transport = MmioTransport::new(mmio_base);
    let device = VirtioBlk::new(transport, queue_memory)?;
    
    let mut guard = VIRTIO_BLK.lock();
    *guard = Some(device);
    
    QUEUE_MEMORY_BASE.store(queue_memory, Ordering::SeqCst);
    
    Ok(())
}

/// Get access to the global VirtIO block device
pub fn with_device<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut VirtioBlk) -> R,
{
    let mut guard = VIRTIO_BLK.lock();
    guard.as_mut().map(f)
}

/// Check if the VirtIO block device is initialized
pub fn is_initialized() -> bool {
    VIRTIO_BLK.lock().is_some()
}

/// Read sectors from the global device
pub fn read_sectors(sector: u64, buffer: &mut [u8]) -> VirtioResult<()> {
    let mut guard = VIRTIO_BLK.lock();
    match guard.as_mut() {
        Some(device) => device.read(sector, buffer),
        None => Err(VirtioError::DeviceNotFound),
    }
}

/// Write sectors to the global device
pub fn write_sectors(sector: u64, buffer: &[u8]) -> VirtioResult<()> {
    let mut guard = VIRTIO_BLK.lock();
    match guard.as_mut() {
        Some(device) => device.write(sector, buffer),
        None => Err(VirtioError::DeviceNotFound),
    }
}

/// Flush the global device
pub fn flush_device() -> VirtioResult<()> {
    let mut guard = VIRTIO_BLK.lock();
    match guard.as_mut() {
        Some(device) => device.flush(),
        None => Err(VirtioError::DeviceNotFound),
    }
}

/// Get device capacity in bytes
pub fn capacity_bytes() -> Option<u64> {
    VIRTIO_BLK.lock().as_ref().map(|d| d.capacity_bytes())
}
