//! Zero OS Virtual Filesystem Layer
//!
//! The VFS layer provides a hierarchical filesystem abstraction for Zero OS:
//!
//! - **Types**: Inode, FilePermissions, DirEntry for filesystem metadata
//! - **Path**: Path validation, normalization, and resolution
//! - **Service**: VfsService trait for filesystem operations
//! - **Storage**: Content storage, encryption, and quota management
//! - **Bootstrap**: Filesystem initialization on first boot
//! - **IPC**: Inter-process communication protocol for VFS operations
//!
//! # Design Principles
//!
//! 1. **Hierarchical paths**: Unix-like `/path/to/file` semantics
//! 2. **User-centric**: Each user has an isolated home directory
//! 3. **Permission-aware**: File access controlled by ownership and permissions
//! 4. **Encryption-ready**: Support for encrypted file storage
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                            VFS Layer                                         │
//! │                                                                              │
//! │  ┌──────────────────────────────────────────────────────────────────────┐   │
//! │  │                         VFS Service                                    │   │
//! │  │  • Path resolution      • Permission checking                         │   │
//! │  │  • Inode management     • Directory operations                        │   │
//! │  │  • File read/write      • Metadata operations                         │   │
//! │  └────────────────────────────────┬──────────────────────────────────────┘   │
//! │                                   │                                          │
//! │                                   ▼                                          │
//! │  ┌──────────────────────────────────────────────────────────────────────┐   │
//! │  │                       Storage Backend                                  │   │
//! │  │  ┌─────────────────────────┐  ┌─────────────────────────┐            │   │
//! │  │  │   zos-userspace DB      │  │   Content Store         │            │   │
//! │  │  │  • Inodes (metadata)    │  │  • File content blobs   │            │   │
//! │  │  │  • Directory entries    │  │  • Encrypted content    │            │   │
//! │  │  └─────────────────────────┘  └─────────────────────────┘            │   │
//! │  └──────────────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```

#![no_std]
extern crate alloc;

pub mod bootstrap;
pub mod error;
#[cfg(target_arch = "wasm32")]
pub mod indexeddb;
pub mod ipc;
pub mod memory;
pub mod path;
pub mod service;
pub mod storage;
pub mod types;

// Re-export main types
pub use error::{StorageError, VfsError};
pub use memory::MemoryVfs;
pub use path::{normalize_path, parent_path, validate_path};
pub use service::VfsService;
pub use storage::{ContentRecord, StorageQuota, StorageUsage};
pub use types::{DirEntry, FilePermissions, Inode, InodeType};

// IPC message constants
pub use ipc::vfs_msg;
