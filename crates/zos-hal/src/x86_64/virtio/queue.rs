//! VirtIO Split Virtqueue Implementation
//!
//! Implements the split virtqueue as defined in VirtIO spec section 2.6.
//!
//! A split virtqueue consists of three parts:
//! - **Descriptor Table**: Array of buffer descriptors
//! - **Available Ring**: Ring buffer of descriptor indices (driver → device)
//! - **Used Ring**: Ring buffer of completed descriptors (device → driver)
//!
//! # Memory Layout
//!
//! ```text
//! ┌───────────────────────────────────────┐
//! │         Descriptor Table              │  16 bytes * queue_size
//! │  (array of VirtqDesc)                 │
//! ├───────────────────────────────────────┤
//! │         Available Ring                │  6 + 2 * queue_size bytes
//! │  flags, idx, ring[], used_event       │
//! ├───────────────────────────────────────┤
//! │         Used Ring                     │  6 + 8 * queue_size bytes
//! │  flags, idx, ring[], avail_event      │
//! └───────────────────────────────────────┘
//! ```

use core::sync::atomic::{fence, Ordering};
use super::{VirtioError, VirtioResult};

/// Virtqueue descriptor flags
pub mod desc_flags {
    /// Buffer continues via next field
    pub const NEXT: u16 = 1;
    /// Buffer is device write-only (otherwise read-only)
    pub const WRITE: u16 = 2;
    /// Buffer contains indirect descriptor table
    pub const INDIRECT: u16 = 4;
}

/// Virtqueue descriptor (16 bytes)
///
/// Each descriptor describes a buffer in guest memory.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VirtqDesc {
    /// Physical address of the buffer
    pub addr: u64,
    /// Length of the buffer in bytes
    pub len: u32,
    /// Descriptor flags
    pub flags: u16,
    /// Next descriptor index (if NEXT flag is set)
    pub next: u16,
}

impl VirtqDesc {
    pub const SIZE: usize = 16;
}

/// Available ring header
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VirtqAvailHeader {
    /// Flags (currently unused, set to 0)
    pub flags: u16,
    /// Next available descriptor index
    pub idx: u16,
}

/// Used ring element
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VirtqUsedElem {
    /// Index of start of used descriptor chain
    pub id: u32,
    /// Total length written to descriptor chain
    pub len: u32,
}

impl VirtqUsedElem {
    pub const SIZE: usize = 8;
}

/// Used ring header
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VirtqUsedHeader {
    /// Flags
    pub flags: u16,
    /// Next used index
    pub idx: u16,
}

/// Maximum queue size we support
pub const MAX_QUEUE_SIZE: u16 = 256;

/// Split Virtqueue
///
/// Manages a single virtqueue for communication with a VirtIO device.
pub struct Virtqueue {
    /// Queue index
    index: u16,
    /// Queue size (number of descriptors)
    size: u16,
    /// Number of free descriptors
    num_free: u16,
    /// Index of first free descriptor
    free_head: u16,
    /// Last seen used index (for polling)
    last_used_idx: u16,

    // Pointers to queue memory
    /// Descriptor table
    desc_table: *mut VirtqDesc,
    /// Available ring
    avail_ring: *mut u8,
    /// Used ring
    used_ring: *mut u8,
}

// SAFETY: Virtqueue can be sent between threads (single-writer access)
unsafe impl Send for Virtqueue {}

impl Virtqueue {
    /// Calculate total memory needed for a virtqueue of given size
    pub fn memory_size(queue_size: u16) -> usize {
        let desc_size = VirtqDesc::SIZE * queue_size as usize;
        let avail_size = 6 + 2 * queue_size as usize; // flags + idx + ring + used_event
        let used_size = 6 + VirtqUsedElem::SIZE * queue_size as usize; // flags + idx + ring + avail_event
        
        // Align to 4096 for safety
        let desc_aligned = (desc_size + 4095) & !4095;
        let avail_aligned = (avail_size + 4095) & !4095;
        let used_aligned = (used_size + 4095) & !4095;
        
        desc_aligned + avail_aligned + used_aligned
    }

    /// Calculate addresses for queue components given a base address
    pub fn addresses(base: u64, queue_size: u16) -> (u64, u64, u64) {
        let desc_size = VirtqDesc::SIZE * queue_size as usize;
        let avail_size = 6 + 2 * queue_size as usize;
        
        // Align to page boundaries
        let desc_aligned = (desc_size + 4095) & !4095;
        let avail_aligned = (avail_size + 4095) & !4095;
        
        let desc_addr = base;
        let avail_addr = base + desc_aligned as u64;
        let used_addr = avail_addr + avail_aligned as u64;
        
        (desc_addr, avail_addr, used_addr)
    }

    /// Create a new virtqueue
    ///
    /// # Arguments
    /// * `index` - Queue index
    /// * `size` - Queue size (must be power of 2)
    /// * `memory` - Physical address of queue memory (must be aligned)
    ///
    /// # Safety
    /// The memory region must be valid and properly sized.
    pub unsafe fn new(index: u16, size: u16, memory: u64) -> VirtioResult<Self> {
        if size == 0 || size > MAX_QUEUE_SIZE || !size.is_power_of_two() {
            return Err(VirtioError::InvalidArgument);
        }

        let (desc_addr, avail_addr, used_addr) = Self::addresses(memory, size);

        let desc_table = desc_addr as *mut VirtqDesc;
        let avail_ring = avail_addr as *mut u8;
        let used_ring = used_addr as *mut u8;

        // Zero out the memory
        core::ptr::write_bytes(desc_table, 0, size as usize);
        core::ptr::write_bytes(avail_ring, 0, 6 + 2 * size as usize);
        core::ptr::write_bytes(used_ring, 0, 6 + VirtqUsedElem::SIZE * size as usize);

        // Initialize free list: each descriptor points to the next
        for i in 0..size {
            let desc = desc_table.add(i as usize);
            (*desc).next = if i < size - 1 { i + 1 } else { 0 };
        }

        Ok(Self {
            index,
            size,
            num_free: size,
            free_head: 0,
            last_used_idx: 0,
            desc_table,
            avail_ring,
            used_ring,
        })
    }

    /// Get the queue index
    pub fn index(&self) -> u16 {
        self.index
    }

    /// Get the queue size
    pub fn size(&self) -> u16 {
        self.size
    }

    /// Get physical addresses for queue setup
    pub fn get_addresses(&self) -> (u64, u64, u64) {
        (
            self.desc_table as u64,
            self.avail_ring as u64,
            self.used_ring as u64,
        )
    }

    /// Check if there are any pending used buffers
    pub fn has_pending(&self) -> bool {
        fence(Ordering::SeqCst);
        let used_idx = self.read_used_idx();
        used_idx != self.last_used_idx
    }

    /// Get number of free descriptors
    pub fn num_free(&self) -> u16 {
        self.num_free
    }

    /// Add a single buffer to the queue (for simple requests)
    ///
    /// Returns the descriptor index used.
    pub fn add_buffer(
        &mut self,
        buffer: u64,
        len: u32,
        write_only: bool,
    ) -> VirtioResult<u16> {
        if self.num_free == 0 {
            return Err(VirtioError::QueueNotAvailable);
        }

        let desc_idx = self.free_head;
        
        // Get next free descriptor
        let desc = unsafe { &mut *self.desc_table.add(desc_idx as usize) };
        self.free_head = desc.next;
        self.num_free -= 1;

        // Fill descriptor
        desc.addr = buffer;
        desc.len = len;
        desc.flags = if write_only { desc_flags::WRITE } else { 0 };
        desc.next = 0;

        // Add to available ring
        let avail_idx = self.read_avail_idx();
        self.write_avail_ring(avail_idx, desc_idx);
        
        fence(Ordering::SeqCst);
        
        self.write_avail_idx(avail_idx.wrapping_add(1));

        Ok(desc_idx)
    }

    /// Add a buffer chain to the queue
    ///
    /// # Arguments
    /// * `buffers` - List of (address, length, write_only) tuples
    ///
    /// Returns the head descriptor index.
    pub fn add_buffer_chain(
        &mut self,
        buffers: &[(u64, u32, bool)],
    ) -> VirtioResult<u16> {
        if buffers.is_empty() {
            return Err(VirtioError::InvalidArgument);
        }

        if self.num_free < buffers.len() as u16 {
            return Err(VirtioError::QueueNotAvailable);
        }

        let head_idx = self.free_head;
        let mut prev_idx: Option<u16> = None;

        for (i, &(addr, len, write_only)) in buffers.iter().enumerate() {
            let desc_idx = self.free_head;
            let desc = unsafe { &mut *self.desc_table.add(desc_idx as usize) };
            
            self.free_head = desc.next;
            self.num_free -= 1;

            desc.addr = addr;
            desc.len = len;
            desc.flags = if write_only { desc_flags::WRITE } else { 0 };

            // Link previous descriptor if any
            if let Some(prev) = prev_idx {
                let prev_desc = unsafe { &mut *self.desc_table.add(prev as usize) };
                prev_desc.flags |= desc_flags::NEXT;
                prev_desc.next = desc_idx;
            }

            // Mark as end of chain unless more buffers follow
            if i < buffers.len() - 1 {
                desc.flags |= desc_flags::NEXT;
            } else {
                desc.next = 0;
            }

            prev_idx = Some(desc_idx);
        }

        // Add head to available ring
        let avail_idx = self.read_avail_idx();
        self.write_avail_ring(avail_idx, head_idx);
        
        fence(Ordering::SeqCst);
        
        self.write_avail_idx(avail_idx.wrapping_add(1));

        Ok(head_idx)
    }

    /// Pop a used buffer from the queue
    ///
    /// Returns (descriptor_index, bytes_written) or None if no buffers available.
    pub fn pop_used(&mut self) -> Option<(u16, u32)> {
        fence(Ordering::SeqCst);
        
        let used_idx = self.read_used_idx();
        if used_idx == self.last_used_idx {
            return None;
        }

        // Read the used element
        let elem_idx = self.last_used_idx % self.size;
        let elem = self.read_used_elem(elem_idx);
        
        self.last_used_idx = self.last_used_idx.wrapping_add(1);

        // Return descriptors to free list
        let head = elem.id as u16;
        self.free_descriptor_chain(head);

        Some((head, elem.len))
    }

    /// Return a descriptor chain to the free list
    fn free_descriptor_chain(&mut self, head: u16) {
        let mut idx = head;
        loop {
            let desc = unsafe { &*self.desc_table.add(idx as usize) };
            let next = desc.next;
            let has_next = desc.flags & desc_flags::NEXT != 0;

            // Add to free list
            let free_desc = unsafe { &mut *self.desc_table.add(idx as usize) };
            free_desc.next = self.free_head;
            self.free_head = idx;
            self.num_free += 1;

            if !has_next {
                break;
            }
            idx = next;
        }
    }

    // Helper methods for ring access

    fn read_avail_idx(&self) -> u16 {
        unsafe {
            let header = self.avail_ring as *const VirtqAvailHeader;
            core::ptr::read_volatile(&(*header).idx)
        }
    }

    fn write_avail_idx(&mut self, idx: u16) {
        unsafe {
            let header = self.avail_ring as *mut VirtqAvailHeader;
            core::ptr::write_volatile(&mut (*header).idx, idx);
        }
    }

    fn write_avail_ring(&mut self, idx: u16, desc_idx: u16) {
        unsafe {
            let ring_offset = core::mem::size_of::<VirtqAvailHeader>();
            let entry = (self.avail_ring.add(ring_offset) as *mut u16)
                .add((idx % self.size) as usize);
            core::ptr::write_volatile(entry, desc_idx);
        }
    }

    fn read_used_idx(&self) -> u16 {
        unsafe {
            let header = self.used_ring as *const VirtqUsedHeader;
            core::ptr::read_volatile(&(*header).idx)
        }
    }

    fn read_used_elem(&self, idx: u16) -> VirtqUsedElem {
        unsafe {
            let ring_offset = core::mem::size_of::<VirtqUsedHeader>();
            let entry = (self.used_ring.add(ring_offset) as *const VirtqUsedElem)
                .add((idx % self.size) as usize);
            core::ptr::read_volatile(entry)
        }
    }
}

impl core::fmt::Debug for Virtqueue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Virtqueue")
            .field("index", &self.index)
            .field("size", &self.size)
            .field("num_free", &self.num_free)
            .field("free_head", &self.free_head)
            .field("last_used_idx", &self.last_used_idx)
            .finish()
    }
}
