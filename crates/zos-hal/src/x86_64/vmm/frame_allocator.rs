//! Physical Frame Allocator
//!
//! Manages physical memory frames using a bitmap allocator.
//! This is simpler than a buddy system but sufficient for our needs.

use alloc::vec::Vec;
use x86_64::structures::paging::PhysFrame;
use x86_64::PhysAddr;

use super::PAGE_SIZE;

/// Maximum number of frames to track (1GB of physical memory)
const MAX_FRAMES: usize = 1024 * 1024 / 4; // 256K frames = 1GB

/// Physical frame allocator using a bitmap
pub struct FrameAllocator {
    /// Bitmap of free frames (1 = free, 0 = used)
    bitmap: Vec<u64>,
    /// Base physical address of managed memory (lowest address)
    base_addr: u64,
    /// Total number of frames
    total_frames: usize,
    /// Number of free frames
    free_frames: usize,
    /// Next frame to check (optimization for sequential allocation)
    next_frame: usize,
}

impl FrameAllocator {
    /// Create a new empty frame allocator
    pub fn new() -> Self {
        Self {
            bitmap: Vec::new(),
            base_addr: 0,
            total_frames: 0,
            free_frames: 0,
            next_frame: 0,
        }
    }

    /// Add a usable memory region to the allocator
    ///
    /// # Arguments
    /// * `start` - Physical start address (will be page-aligned)
    /// * `size` - Region size in bytes
    pub fn add_region(&mut self, start: u64, size: u64) {
        // Skip very low memory (< 1MB) to avoid BIOS/bootloader areas
        let safe_start = if start < 0x100000 { 0x100000 } else { start };
        let size = if start < 0x100000 {
            size.saturating_sub(0x100000 - start)
        } else {
            size
        };
        
        if size == 0 {
            return;
        }

        // Align start up to page boundary
        let aligned_start = (safe_start + PAGE_SIZE as u64 - 1) & !(PAGE_SIZE as u64 - 1);
        
        // Calculate number of frames
        let aligned_size = size.saturating_sub(aligned_start - safe_start);
        let num_frames = (aligned_size / PAGE_SIZE as u64) as usize;
        
        if num_frames == 0 {
            return;
        }

        // If this is the first region, initialize with a large enough bitmap
        if self.bitmap.is_empty() {
            self.base_addr = aligned_start;
            // Allocate bitmap for up to MAX_FRAMES
            let bitmap_size = (MAX_FRAMES + 63) / 64;
            self.bitmap = alloc::vec![0u64; bitmap_size];
        }

        // If this region is below our base_addr, we can't handle it
        // (would need to re-base the bitmap)
        if aligned_start < self.base_addr {
            return;
        }

        // Mark frames as free
        let start_frame = ((aligned_start - self.base_addr) / PAGE_SIZE as u64) as usize;
        let end_frame = (start_frame + num_frames).min(MAX_FRAMES);

        if start_frame >= MAX_FRAMES {
            return; // Region is too far from base
        }

        for frame_idx in start_frame..end_frame {
            let word_idx = frame_idx / 64;
            let bit_idx = frame_idx % 64;
            
            if word_idx < self.bitmap.len() {
                // Only count if not already marked free
                if self.bitmap[word_idx] & (1 << bit_idx) == 0 {
                    self.bitmap[word_idx] |= 1 << bit_idx;
                    self.total_frames += 1;
                    self.free_frames += 1;
                }
            }
        }
    }

    /// Allocate a single physical frame
    pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
        if self.free_frames == 0 {
            return None;
        }

        // Start searching from next_frame for better performance
        let start_word = self.next_frame / 64;
        
        // Search from start_word to end
        for word_idx in start_word..self.bitmap.len() {
            if self.bitmap[word_idx] != 0 {
                // Found a word with free frames
                let bit_idx = self.bitmap[word_idx].trailing_zeros() as usize;
                let frame_idx = word_idx * 64 + bit_idx;
                
                // Mark as used
                self.bitmap[word_idx] &= !(1 << bit_idx);
                self.free_frames -= 1;
                self.next_frame = frame_idx + 1;
                
                let phys_addr = self.base_addr + (frame_idx as u64 * PAGE_SIZE as u64);
                return Some(PhysFrame::containing_address(PhysAddr::new(phys_addr)));
            }
        }

        // Wrap around and search from beginning
        for word_idx in 0..start_word {
            if self.bitmap[word_idx] != 0 {
                let bit_idx = self.bitmap[word_idx].trailing_zeros() as usize;
                let frame_idx = word_idx * 64 + bit_idx;
                
                self.bitmap[word_idx] &= !(1 << bit_idx);
                self.free_frames -= 1;
                self.next_frame = frame_idx + 1;
                
                let phys_addr = self.base_addr + (frame_idx as u64 * PAGE_SIZE as u64);
                return Some(PhysFrame::containing_address(PhysAddr::new(phys_addr)));
            }
        }

        None
    }

    /// Free a physical frame
    pub fn free_frame(&mut self, frame: PhysFrame) {
        let phys_addr = frame.start_address().as_u64();
        
        if phys_addr < self.base_addr {
            return; // Frame not in our managed range
        }
        
        let frame_idx = ((phys_addr - self.base_addr) / PAGE_SIZE as u64) as usize;
        let word_idx = frame_idx / 64;
        let bit_idx = frame_idx % 64;
        
        if word_idx < self.bitmap.len() {
            // Only free if currently used
            if self.bitmap[word_idx] & (1 << bit_idx) == 0 {
                self.bitmap[word_idx] |= 1 << bit_idx;
                self.free_frames += 1;
            }
        }
    }

    /// Allocate multiple contiguous frames
    ///
    /// Note: This is a simple implementation that scans for contiguous free frames.
    /// For better performance with large allocations, consider a buddy allocator.
    pub fn allocate_frames(&mut self, count: usize) -> Option<PhysFrame> {
        if count == 0 || count > self.free_frames {
            return None;
        }

        if count == 1 {
            return self.allocate_frame();
        }

        // Simple scan for contiguous frames
        let total_frames = self.bitmap.len() * 64;
        let mut start_frame = None;
        let mut consecutive = 0;

        for frame_idx in 0..total_frames {
            let word_idx = frame_idx / 64;
            let bit_idx = frame_idx % 64;

            if word_idx >= self.bitmap.len() {
                break;
            }

            if self.bitmap[word_idx] & (1 << bit_idx) != 0 {
                // Frame is free
                if start_frame.is_none() {
                    start_frame = Some(frame_idx);
                }
                consecutive += 1;

                if consecutive == count {
                    // Found enough contiguous frames, allocate them
                    let start = start_frame.unwrap();
                    for i in 0..count {
                        let idx = start + i;
                        let w = idx / 64;
                        let b = idx % 64;
                        self.bitmap[w] &= !(1 << b);
                    }
                    self.free_frames -= count;

                    let phys_addr = self.base_addr + (start as u64 * PAGE_SIZE as u64);
                    return Some(PhysFrame::containing_address(PhysAddr::new(phys_addr)));
                }
            } else {
                // Frame is used, reset search
                start_frame = None;
                consecutive = 0;
            }
        }

        None
    }

    /// Free multiple contiguous frames
    pub fn free_contiguous(&mut self, frame: PhysFrame, count: usize) {
        let phys_addr = frame.start_address().as_u64();
        
        if phys_addr < self.base_addr {
            return;
        }
        
        let start_frame = ((phys_addr - self.base_addr) / PAGE_SIZE as u64) as usize;
        
        for i in 0..count {
            let frame_idx = start_frame + i;
            let word_idx = frame_idx / 64;
            let bit_idx = frame_idx % 64;
            
            if word_idx < self.bitmap.len() && self.bitmap[word_idx] & (1 << bit_idx) == 0 {
                self.bitmap[word_idx] |= 1 << bit_idx;
                self.free_frames += 1;
            }
        }
    }

    /// Get the number of free frames
    pub fn free_frames(&self) -> usize {
        self.free_frames
    }

    /// Get the total number of frames
    pub fn total_frames(&self) -> usize {
        self.total_frames
    }

    /// Get the base address of managed memory
    pub fn base_addr(&self) -> u64 {
        self.base_addr
    }
}

impl Default for FrameAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocate_single() {
        let mut allocator = FrameAllocator::new();
        allocator.add_region(0x100000, 0x100000); // 1MB region

        let frame = allocator.allocate_frame();
        assert!(frame.is_some());
    }

    #[test]
    fn test_allocate_and_free() {
        let mut allocator = FrameAllocator::new();
        allocator.add_region(0x100000, 0x10000); // 64KB region

        let initial_free = allocator.free_frames();
        
        let frame = allocator.allocate_frame().unwrap();
        assert_eq!(allocator.free_frames(), initial_free - 1);
        
        allocator.free_frame(frame);
        assert_eq!(allocator.free_frames(), initial_free);
    }
}
