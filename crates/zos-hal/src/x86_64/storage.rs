//! Block Storage - Key-Value Storage on VirtIO Block Device
//!
//! Provides a simple key-value storage abstraction on top of the raw block device.
//! This is used by the HAL storage methods to persist data.
//!
//! # Disk Layout
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │ Sector 0: Superblock (magic, version, entry count, next free sector)        │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │ Sector 1-N: Key-Value entries                                               │
//! │   Each entry: | entry_header | key bytes | value bytes | padding |          │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Entry Format
//!
//! Each entry starts at a sector boundary:
//! - 4 bytes: magic (0x5A4F5345 = "ZOSE")
//! - 4 bytes: flags (1 = valid, 0 = deleted)
//! - 4 bytes: key length
//! - 4 bytes: value length
//! - N bytes: key (UTF-8)
//! - M bytes: value
//! - Padding to sector boundary

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use super::virtio::blk_pci as blk;
use super::virtio::blk::SECTOR_SIZE;
use super::virtio::{VirtioError, VirtioResult};

/// Storage magic number for superblock ("ZOS\0")
const SUPERBLOCK_MAGIC: u32 = 0x5A4F5300;

/// Storage magic number for entries ("ZOSE")
const ENTRY_MAGIC: u32 = 0x5A4F5345;

/// Storage version
const STORAGE_VERSION: u32 = 1;

/// Entry flag: valid
const FLAG_VALID: u32 = 1;

/// Entry flag: deleted
const FLAG_DELETED: u32 = 0;

/// Maximum key length (bytes)
const MAX_KEY_LEN: usize = 256;

/// Maximum value length (bytes)
const MAX_VALUE_LEN: usize = 64 * 1024; // 64 KB

/// Entry header size
const ENTRY_HEADER_SIZE: usize = 16;

/// Superblock structure (fits in one sector)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Superblock {
    /// Magic number
    magic: u32,
    /// Version
    version: u32,
    /// Number of valid entries
    entry_count: u32,
    /// Next free sector
    next_free_sector: u32,
    /// Reserved for future use
    reserved: [u32; 123],
    /// Checksum
    checksum: u32,
}

impl Superblock {
    fn new() -> Self {
        let mut sb = Self {
            magic: SUPERBLOCK_MAGIC,
            version: STORAGE_VERSION,
            entry_count: 0,
            next_free_sector: 1, // Sector 0 is superblock
            reserved: [0; 123],
            checksum: 0,
        };
        sb.update_checksum();
        sb
    }

    fn update_checksum(&mut self) {
        self.checksum = 0;
        let bytes = unsafe {
            core::slice::from_raw_parts(
                self as *const _ as *const u8,
                SECTOR_SIZE - 4, // Exclude checksum itself
            )
        };
        let mut sum: u32 = 0;
        for &b in bytes {
            sum = sum.wrapping_add(b as u32);
        }
        self.checksum = sum;
    }

    fn verify_checksum(&self) -> bool {
        let bytes = unsafe {
            core::slice::from_raw_parts(
                self as *const _ as *const u8,
                SECTOR_SIZE - 4,
            )
        };
        let mut sum: u32 = 0;
        for &b in bytes {
            sum = sum.wrapping_add(b as u32);
        }
        sum == self.checksum
    }

    fn is_valid(&self) -> bool {
        self.magic == SUPERBLOCK_MAGIC && self.version == STORAGE_VERSION && self.verify_checksum()
    }

    fn to_bytes(&self) -> [u8; SECTOR_SIZE] {
        let mut bytes = [0u8; SECTOR_SIZE];
        unsafe {
            core::ptr::copy_nonoverlapping(
                self as *const _ as *const u8,
                bytes.as_mut_ptr(),
                core::mem::size_of::<Self>(),
            );
        }
        bytes
    }

    fn from_bytes(bytes: &[u8; SECTOR_SIZE]) -> Self {
        let mut sb = Self::new();
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                &mut sb as *mut _ as *mut u8,
                core::mem::size_of::<Self>(),
            );
        }
        sb
    }
}

/// Entry header
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct EntryHeader {
    /// Magic number
    magic: u32,
    /// Flags (valid/deleted)
    flags: u32,
    /// Key length
    key_len: u32,
    /// Value length
    value_len: u32,
}

impl EntryHeader {
    fn new(key_len: u32, value_len: u32) -> Self {
        Self {
            magic: ENTRY_MAGIC,
            flags: FLAG_VALID,
            key_len,
            value_len,
        }
    }

    fn is_valid(&self) -> bool {
        self.magic == ENTRY_MAGIC && self.flags == FLAG_VALID
    }

    #[allow(dead_code)] // Available for future compaction/recovery operations
    fn is_deleted(&self) -> bool {
        self.magic == ENTRY_MAGIC && self.flags == FLAG_DELETED
    }

    /// Total size of this entry in bytes (including header)
    fn total_size(&self) -> usize {
        ENTRY_HEADER_SIZE + self.key_len as usize + self.value_len as usize
    }

    /// Number of sectors needed for this entry
    fn sectors_needed(&self) -> u32 {
        ((self.total_size() + SECTOR_SIZE - 1) / SECTOR_SIZE) as u32
    }
}

/// In-memory index entry
#[derive(Clone, Debug)]
struct IndexEntry {
    /// Starting sector
    sector: u32,
    /// Number of sectors
    num_sectors: u32,
    /// Value length (for quick access without disk read)
    #[allow(dead_code)]
    value_len: u32,
}

/// Block storage manager
pub struct BlockStorage {
    /// In-memory index: key -> sector location
    index: BTreeMap<String, IndexEntry>,
    /// Superblock (cached)
    superblock: Superblock,
    /// Whether storage is initialized
    initialized: bool,
}

impl BlockStorage {
    /// Create a new uninitialized storage manager
    pub const fn new() -> Self {
        Self {
            index: BTreeMap::new(),
            superblock: Superblock {
                magic: 0,
                version: 0,
                entry_count: 0,
                next_free_sector: 1,
                reserved: [0; 123],
                checksum: 0,
            },
            initialized: false,
        }
    }

    /// Initialize storage, loading index from disk
    pub fn init(&mut self) -> VirtioResult<bool> {
        if !blk::is_initialized() {
            return Err(VirtioError::DeviceNotFound);
        }

        // Read superblock
        let mut sector_buf = [0u8; SECTOR_SIZE];
        blk::read_sectors(0, &mut sector_buf)?;

        let sb = Superblock::from_bytes(&sector_buf);
        
        if sb.is_valid() {
            // Existing storage, load index
            crate::serial_println!("[storage] Found existing storage with {} entries", sb.entry_count);
            self.superblock = sb;
            self.load_index()?;
            self.initialized = true;
            Ok(false) // Not newly created
        } else {
            // Format new storage
            crate::serial_println!("[storage] Formatting new storage...");
            self.superblock = Superblock::new();
            self.write_superblock()?;
            self.index.clear();
            self.initialized = true;
            Ok(true) // Newly created
        }
    }

    /// Load index from disk by scanning entries
    fn load_index(&mut self) -> VirtioResult<()> {
        self.index.clear();
        
        let mut sector = 1u64; // Start after superblock
        let max_sector = self.superblock.next_free_sector as u64;
        
        let mut sector_buf = [0u8; SECTOR_SIZE];
        
        while sector < max_sector {
            // Read entry header
            blk::read_sectors(sector, &mut sector_buf)?;
            
            let header = unsafe {
                core::ptr::read_unaligned(sector_buf.as_ptr() as *const EntryHeader)
            };
            
            if header.magic != ENTRY_MAGIC {
                // End of entries or corruption
                break;
            }
            
            let entry_sectors = header.sectors_needed();
            
            if header.is_valid() {
                // Read key (may span multiple sectors)
                let key_start = ENTRY_HEADER_SIZE;
                let key_end = key_start + header.key_len as usize;

                let key_bytes: Vec<u8>;
                let key_slice = if key_end <= SECTOR_SIZE {
                    &sector_buf[key_start..key_end]
                } else {
                    let total_bytes = entry_sectors as usize * SECTOR_SIZE;
                    let mut entry_buf = alloc::vec![0u8; total_bytes];
                    blk::read_sectors(sector, &mut entry_buf)?;
                    key_bytes = entry_buf[key_start..key_end.min(total_bytes)].to_vec();
                    key_bytes.as_slice()
                };
                
                if let Ok(key) = core::str::from_utf8(key_slice) {
                    self.index.insert(
                        String::from(key),
                        IndexEntry {
                            sector: sector as u32,
                            num_sectors: entry_sectors,
                            value_len: header.value_len,
                        },
                    );
                }
            }
            
            sector += entry_sectors as u64;
        }
        
        crate::serial_println!("[storage] Loaded {} entries into index", self.index.len());
        Ok(())
    }

    /// Write superblock to disk
    fn write_superblock(&mut self) -> VirtioResult<()> {
        self.superblock.update_checksum();
        let bytes = self.superblock.to_bytes();
        blk::write_sectors(0, &bytes)?;
        blk::flush_device()
    }

    /// Check if a key exists
    pub fn exists(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    /// Read a value by key
    pub fn read(&self, key: &str) -> VirtioResult<Option<Vec<u8>>> {
        let entry = match self.index.get(key) {
            Some(e) => e,
            None => return Ok(None),
        };

        // Read all sectors for this entry
        let total_bytes = entry.num_sectors as usize * SECTOR_SIZE;
        let mut buffer = alloc::vec![0u8; total_bytes];
        blk::read_sectors(entry.sector as u64, &mut buffer)?;

        // Parse header
        let header = unsafe {
            core::ptr::read_unaligned(buffer.as_ptr() as *const EntryHeader)
        };

        if !header.is_valid() {
            return Ok(None);
        }

        // Extract value
        let value_start = ENTRY_HEADER_SIZE + header.key_len as usize;
        let value_end = value_start + header.value_len as usize;

        if value_end <= buffer.len() {
            Ok(Some(buffer[value_start..value_end].to_vec()))
        } else {
            Err(VirtioError::IoError)
        }
    }

    /// Write a key-value pair
    pub fn write(&mut self, key: &str, value: &[u8]) -> VirtioResult<()> {
        if key.len() > MAX_KEY_LEN || value.len() > MAX_VALUE_LEN {
            return Err(VirtioError::InvalidArgument);
        }

        // Delete existing entry if present (mark as deleted)
        if self.index.contains_key(key) {
            self.delete(key)?;
        }

        // Create entry
        let header = EntryHeader::new(key.len() as u32, value.len() as u32);
        let entry_sectors = header.sectors_needed();

        // Check if we have space
        let capacity = blk::capacity_bytes().ok_or(VirtioError::DeviceNotFound)?;
        let max_sectors = (capacity / SECTOR_SIZE as u64) as u32;
        
        if self.superblock.next_free_sector + entry_sectors > max_sectors {
            // Try compaction to reclaim deleted space
            self.compact()?;
            if self.superblock.next_free_sector + entry_sectors > max_sectors {
                return Err(VirtioError::OutOfMemory);
            }
        }

        // Build entry buffer
        let buffer_size = entry_sectors as usize * SECTOR_SIZE;
        let mut buffer = alloc::vec![0u8; buffer_size];

        // Write header
        unsafe {
            core::ptr::write_unaligned(buffer.as_mut_ptr() as *mut EntryHeader, header);
        }

        // Write key
        buffer[ENTRY_HEADER_SIZE..ENTRY_HEADER_SIZE + key.len()].copy_from_slice(key.as_bytes());

        // Write value
        let value_start = ENTRY_HEADER_SIZE + key.len();
        buffer[value_start..value_start + value.len()].copy_from_slice(value);

        // Write to disk
        let sector = self.superblock.next_free_sector;
        blk::write_sectors(sector as u64, &buffer)?;

        // Update index
        self.index.insert(
            String::from(key),
            IndexEntry {
                sector,
                num_sectors: entry_sectors,
                value_len: value.len() as u32,
            },
        );

        // Update superblock
        self.superblock.entry_count += 1;
        self.superblock.next_free_sector += entry_sectors;
        self.write_superblock()?;

        Ok(())
    }

    /// Delete a key
    pub fn delete(&mut self, key: &str) -> VirtioResult<bool> {
        let entry = match self.index.remove(key) {
            Some(e) => e,
            None => return Ok(false),
        };

        // Mark entry as deleted on disk
        let mut sector_buf = [0u8; SECTOR_SIZE];
        blk::read_sectors(entry.sector as u64, &mut sector_buf)?;

        // Update flags to deleted
        let header = unsafe {
            &mut *(sector_buf.as_mut_ptr() as *mut EntryHeader)
        };
        header.flags = FLAG_DELETED;

        blk::write_sectors(entry.sector as u64, &sector_buf)?;

        // Update superblock
        self.superblock.entry_count = self.superblock.entry_count.saturating_sub(1);
        self.write_superblock()?;

        Ok(true)
    }

    /// List all keys with a given prefix
    pub fn list(&self, prefix: &str) -> Vec<String> {
        self.index
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }

    /// Get entry count
    pub fn count(&self) -> usize {
        self.index.len()
    }

    /// Check if initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Clear all storage (for testing/reset)
    pub fn clear(&mut self) -> VirtioResult<()> {
        self.superblock = Superblock::new();
        self.index.clear();
        self.write_superblock()
    }

    /// Compact storage by rewriting valid entries contiguously
    fn compact(&mut self) -> VirtioResult<()> {
        if !self.initialized {
            return Err(VirtioError::InvalidArgument);
        }

        let capacity = blk::capacity_bytes().ok_or(VirtioError::DeviceNotFound)?;
        let max_sectors = (capacity / SECTOR_SIZE as u64) as u32;

        let mut next_sector = 1u32;
        let mut new_index = BTreeMap::new();

        for (key, entry) in self.index.iter() {
            let total_bytes = entry.num_sectors as usize * SECTOR_SIZE;
            let mut buffer = alloc::vec![0u8; total_bytes];
            blk::read_sectors(entry.sector as u64, &mut buffer)?;

            let header = unsafe {
                core::ptr::read_unaligned(buffer.as_ptr() as *const EntryHeader)
            };
            if !header.is_valid() {
                continue;
            }

            if next_sector + entry.num_sectors > max_sectors {
                return Err(VirtioError::OutOfMemory);
            }

            blk::write_sectors(next_sector as u64, &buffer)?;

            new_index.insert(
                key.clone(),
                IndexEntry {
                    sector: next_sector,
                    num_sectors: entry.num_sectors,
                    value_len: entry.value_len,
                },
            );

            next_sector += entry.num_sectors;
        }

        self.index = new_index;
        self.superblock.entry_count = self.index.len() as u32;
        self.superblock.next_free_sector = next_sector;
        self.write_superblock()
    }
}

/// Global storage instance
static STORAGE: Mutex<BlockStorage> = Mutex::new(BlockStorage::new());

/// Initialize the global storage
pub fn init() -> VirtioResult<bool> {
    STORAGE.lock().init()
}

/// Check if storage is initialized
pub fn is_initialized() -> bool {
    STORAGE.lock().is_initialized()
}

/// Check if a key exists
pub fn exists(key: &str) -> VirtioResult<bool> {
    Ok(STORAGE.lock().exists(key))
}

/// Read a value
pub fn read(key: &str) -> VirtioResult<Option<Vec<u8>>> {
    STORAGE.lock().read(key)
}

/// Write a value
pub fn write(key: &str, value: &[u8]) -> VirtioResult<()> {
    STORAGE.lock().write(key, value)
}

/// Delete a key
pub fn delete(key: &str) -> VirtioResult<bool> {
    STORAGE.lock().delete(key)
}

/// List keys with prefix
pub fn list(prefix: &str) -> Vec<String> {
    STORAGE.lock().list(prefix)
}

/// Get entry count
pub fn count() -> usize {
    STORAGE.lock().count()
}

/// Clear all storage
pub fn clear() -> VirtioResult<()> {
    STORAGE.lock().clear()
}
