//! VirtIO Block Device Driver (PCI Transport)
//!
//! Implements a driver for VirtIO block devices using the PCI transport.
//! This is the typical configuration for QEMU x86_64.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use crate::x86_64::pci::{self, PciDevice};
use super::pci::{PciTransport, init_device, finalize_device};
use super::queue::Virtqueue;
use super::{DeviceId, VirtioError, VirtioResult};
use super::blk::{BlockRequestHeader, BlockRequestType, BlockStatus, SECTOR_SIZE, blk_features};

/// Page size for legacy queue address
const PAGE_SIZE: u64 = 4096;

/// Default queue size
const DEFAULT_QUEUE_SIZE: u16 = 128;

/// Legacy virtqueue memory layout
///
/// For legacy PCI devices, the queue address is a single physical page frame number
/// containing all three parts: descriptors, available ring, used ring.
#[allow(dead_code)] // Fields stored for debugging and future deallocation
struct LegacyQueueLayout {
    /// Physical address of the queue memory
    phys_addr: u64,
    /// Size of the allocated memory
    size: usize,
}

impl LegacyQueueLayout {
    /// Calculate total size needed for a legacy virtqueue
    fn calculate_size(queue_size: u16) -> usize {
        let q = queue_size as usize;
        
        // Descriptor table: 16 bytes * queue_size
        let desc_size = 16 * q;
        
        // Available ring: 2 + 2 + 2*queue_size + 2 (padding to align used ring)
        let avail_size = 4 + 2 * q + 2;
        
        // Align to 4096 for used ring
        let used_offset = ((desc_size + avail_size) + 4095) & !4095;
        
        // Used ring: 2 + 2 + 8*queue_size + 2
        let used_size = 4 + 8 * q + 2;
        
        used_offset + used_size
    }
}

/// VirtIO Block Device (PCI)
pub struct VirtioBlkPci {
    /// PCI transport
    transport: PciTransport,
    /// Request queue
    queue: Virtqueue,
    /// Queue memory layout (stored for future deallocation)
    #[allow(dead_code)]
    queue_layout: LegacyQueueLayout,
    /// Device capacity in sectors
    capacity: u64,
    /// Block size (usually 512)
    block_size: u32,
    /// In-flight requests keyed by descriptor index
    in_flight: BTreeMap<u16, InFlightRequest>,
    /// Next request ID
    next_request_id: u64,
}

/// In-flight request tracking
///
/// Fields are stored to keep allocations alive during DMA and for debugging.
#[allow(dead_code)]
struct InFlightRequest {
    /// Request header
    header: Box<BlockRequestHeader>,
    /// Status byte
    status: Box<u8>,
    /// Is this a read operation?
    is_read: bool,
}

// SAFETY: VirtioBlkPci is designed for single-threaded access
unsafe impl Send for VirtioBlkPci {}

impl VirtioBlkPci {
    /// Initialize a VirtIO block device from a PCI device
    ///
    /// # Safety
    /// The PCI device must be a valid VirtIO block device.
    pub unsafe fn new(pci_device: PciDevice, queue_memory: u64) -> VirtioResult<Self> {
        let transport = PciTransport::new(pci_device)?;
        
        // Verify device type
        if transport.device_id() != DeviceId::Block {
            return Err(VirtioError::DeviceNotFound);
        }
        
        // Initialize device
        // For legacy devices, we only use 32-bit features
        let driver_features = blk_features::SIZE_MAX as u32 | blk_features::SEG_MAX as u32;
        init_device(&transport, driver_features)?;
        
        // Read device configuration
        let capacity = transport.read_config_u64(0); // offset 0: capacity
        let block_size = 512u32; // Legacy devices don't always have blk_size field
        
        crate::serial_println!("[virtio-blk-pci] Capacity: {} sectors ({} MB)",
            capacity, capacity * 512 / (1024 * 1024));
        
        // Select queue 0 (request queue)
        transport.select_queue(0);
        let max_size = transport.queue_size();
        
        if max_size == 0 {
            return Err(VirtioError::QueueNotAvailable);
        }
        
        let queue_size = max_size.min(DEFAULT_QUEUE_SIZE);
        crate::serial_println!("[virtio-blk-pci] Queue size: {} (max: {})", queue_size, max_size);
        
        // Create virtqueue in provided memory
        let queue = Virtqueue::new(0, queue_size, queue_memory)?;
        
        // For legacy PCI, we need to set the queue PFN (page frame number)
        let pfn = (queue_memory / PAGE_SIZE) as u32;
        transport.set_queue_address(pfn);
        
        crate::serial_println!("[virtio-blk-pci] Queue address PFN: {}", pfn);
        
        // Complete initialization
        finalize_device(&transport);
        
        crate::serial_println!("[virtio-blk-pci] Device initialized successfully");
        
        let queue_layout = LegacyQueueLayout {
            phys_addr: queue_memory,
            size: LegacyQueueLayout::calculate_size(queue_size),
        };
        
        Ok(Self {
            transport,
            queue,
            queue_layout,
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
    pub fn flush_async(&mut self) -> VirtioResult<u64> {
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
        // Allocate request header and status on heap
        let header = Box::new(BlockRequestHeader::new(request_type, sector));
        let status = Box::new(0xFFu8);
        
        let header_addr = header.as_ref() as *const _ as u64;
        let status_addr = status.as_ref() as *const _ as u64;
        
        // Build descriptor chain
        let mut buffers: Vec<(u64, u32, bool)> = Vec::new();
        
        // Header: device-readable
        buffers.push((header_addr, BlockRequestHeader::SIZE as u32, false));
        
        // Data buffer (if any)
        if data_len > 0 {
            buffers.push((data_addr, data_len, is_read));
        }
        
        // Status: device-writable
        buffers.push((status_addr, 1, true));
        
        // Add to queue
        let desc_idx = self.queue.add_buffer_chain(&buffers)?;
        
        // Track request
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        
        self.in_flight.insert(desc_idx, InFlightRequest {
            header,
            status,
            is_read,
        });
        
        // Notify device
        self.transport.notify_queue(0);
        
        Ok(request_id)
    }
    
    /// Poll for completed requests
    pub fn poll_completions(&mut self) -> Vec<(u16, BlockStatus)> {
        let mut completions = Vec::new();
        
        while let Some((desc_idx, _bytes)) = self.queue.pop_used() {
            if let Some(request) = self.in_flight.remove(&desc_idx) {
                let status = BlockStatus::from(*request.status);
                completions.push((desc_idx, status));
            }
        }
        
        completions
    }
    
    /// Blocking read
    pub fn read(&mut self, sector: u64, buffer: &mut [u8]) -> VirtioResult<()> {
        self.read_async(sector, buffer)?;
        self.wait_for_completion()
    }
    
    /// Blocking write
    pub fn write(&mut self, sector: u64, buffer: &[u8]) -> VirtioResult<()> {
        self.write_async(sector, buffer)?;
        self.wait_for_completion()
    }
    
    /// Blocking flush
    pub fn flush(&mut self) -> VirtioResult<()> {
        self.flush_async()?;
        self.wait_for_completion()
    }
    
    /// Wait for completion
    fn wait_for_completion(&mut self) -> VirtioResult<()> {
        let mut iterations = 0u64;
        loop {
            let completions = self.poll_completions();
            if !completions.is_empty() {
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

impl Drop for VirtioBlkPci {
    fn drop(&mut self) {
        self.transport.reset();
    }
}

/// Global VirtIO block device (PCI) instance
static VIRTIO_BLK_PCI: Mutex<Option<VirtioBlkPci>> = Mutex::new(None);

/// Initialize the global VirtIO block device from PCI
///
/// # Safety
/// Must be called after PCI and VMM initialization.
pub unsafe fn init_from_pci(queue_memory: u64) -> VirtioResult<()> {
    // Find VirtIO block device on PCI bus
    let pci_device = pci::find_virtio_block()
        .ok_or(VirtioError::DeviceNotFound)?;
    
    crate::serial_println!("[virtio-blk-pci] Found device at {:02x}:{:02x}.{}",
        pci_device.addr.bus, pci_device.addr.device, pci_device.addr.function);
    
    let device = VirtioBlkPci::new(pci_device, queue_memory)?;
    
    let mut guard = VIRTIO_BLK_PCI.lock();
    *guard = Some(device);
    
    Ok(())
}

/// Check if the VirtIO block device (PCI) is initialized
pub fn is_initialized() -> bool {
    VIRTIO_BLK_PCI.lock().is_some()
}

/// Read sectors from the global device
pub fn read_sectors(sector: u64, buffer: &mut [u8]) -> VirtioResult<()> {
    let mut guard = VIRTIO_BLK_PCI.lock();
    match guard.as_mut() {
        Some(device) => device.read(sector, buffer),
        None => Err(VirtioError::DeviceNotFound),
    }
}

/// Write sectors to the global device
pub fn write_sectors(sector: u64, buffer: &[u8]) -> VirtioResult<()> {
    let mut guard = VIRTIO_BLK_PCI.lock();
    match guard.as_mut() {
        Some(device) => device.write(sector, buffer),
        None => Err(VirtioError::DeviceNotFound),
    }
}

/// Flush the global device
pub fn flush_device() -> VirtioResult<()> {
    let mut guard = VIRTIO_BLK_PCI.lock();
    match guard.as_mut() {
        Some(device) => device.flush(),
        None => Err(VirtioError::DeviceNotFound),
    }
}

/// Get device capacity in bytes
pub fn capacity_bytes() -> Option<u64> {
    VIRTIO_BLK_PCI.lock().as_ref().map(|d| d.capacity_bytes())
}
