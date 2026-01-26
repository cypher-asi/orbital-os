//! Hardware Abstraction Layer trait for Zero OS
//!
//! This crate defines the HAL trait that allows the kernel to run
//! on different platforms (WASM, QEMU, bare metal) by abstracting hardware
//! operations.
//!
//! # Platform Implementations
//!
//! - **WASM**: Web Workers for processes, `performance.now()` for time, `crypto.getRandomValues()` for entropy
//! - **QEMU**: Virtual hardware abstraction (Phase 2)
//! - **Bare Metal**: Direct hardware access (Phase 7)

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

/// Callback type for process message notifications
pub type MessageCallback<P> = fn(&P, &[u8]);

/// Hardware Abstraction Layer trait
///
/// Implementations provide platform-specific functionality for:
/// - Process management (spawn, kill, message passing)
/// - Memory allocation
/// - Time measurement
/// - Entropy (random numbers)
/// - Debug output
///
/// # Associated Types
///
/// - `ProcessHandle`: Platform-specific handle to a spawned process
///   - On WASM: Reference to a Web Worker
///   - On QEMU/bare metal: Process ID or memory region reference
pub trait HAL: Send + Sync + 'static {
    /// Handle to a spawned process (Web Worker on WASM, process descriptor on native)
    type ProcessHandle: Clone + Send + Sync;

    // === Process Management ===

    /// Spawn a new process from WASM binary
    ///
    /// On WASM: Creates a new Web Worker and loads the binary
    /// On native: Creates a new process with isolated memory
    ///
    /// # Arguments
    /// * `name` - Human-readable process name for debugging
    /// * `binary` - WASM binary to execute
    ///
    /// # Returns
    /// * `Ok(ProcessHandle)` - Handle to the spawned process
    /// * `Err(HalError::ProcessSpawnFailed)` - Failed to create process
    fn spawn_process(&self, name: &str, binary: &[u8]) -> Result<Self::ProcessHandle, HalError>;

    /// Terminate a process
    ///
    /// # Arguments
    /// * `handle` - Handle to the process to terminate
    ///
    /// # Returns
    /// * `Ok(())` - Process terminated successfully
    /// * `Err(HalError::ProcessNotFound)` - Process doesn't exist
    fn kill_process(&self, handle: &Self::ProcessHandle) -> Result<(), HalError>;

    /// Send a message to a process
    ///
    /// # Arguments
    /// * `handle` - Handle to the target process
    /// * `msg` - Message bytes to send
    ///
    /// # Returns
    /// * `Ok(())` - Message sent successfully
    /// * `Err(HalError::ProcessNotFound)` - Process doesn't exist
    /// * `Err(HalError::InvalidMessage)` - Message too large or malformed
    fn send_to_process(&self, handle: &Self::ProcessHandle, msg: &[u8]) -> Result<(), HalError>;

    /// Check if a process is still running
    fn is_process_alive(&self, handle: &Self::ProcessHandle) -> bool;

    /// Get the memory size of a process in bytes
    ///
    /// On WASM: Returns the linear memory size (pages * 64KB)
    fn get_process_memory_size(&self, handle: &Self::ProcessHandle) -> Result<usize, HalError>;

    // === Memory ===

    /// Allocate memory (within current context)
    ///
    /// Note: On WASM, each process has its own linear memory managed by the
    /// WASM runtime. This is primarily for supervisor-side allocations.
    ///
    /// # Arguments
    /// * `size` - Number of bytes to allocate
    /// * `align` - Alignment requirement
    ///
    /// # Returns
    /// * `Ok(ptr)` - Pointer to allocated memory
    /// * `Err(HalError::OutOfMemory)` - Allocation failed
    fn allocate(&self, size: usize, align: usize) -> Result<*mut u8, HalError>;

    /// Deallocate memory
    ///
    /// # Safety
    /// The pointer must have been allocated by `allocate` with the same size and alignment
    unsafe fn deallocate(&self, ptr: *mut u8, size: usize, align: usize);

    // === Time & Entropy ===

    /// Get current time in nanoseconds (monotonic)
    ///
    /// On WASM: Uses `performance.now()` converted to nanoseconds
    fn now_nanos(&self) -> u64;

    /// Get wall-clock time in milliseconds since Unix epoch
    ///
    /// This is real time-of-day, not monotonic (can jump due to NTP sync).
    /// On WASM: Uses `Date.now()`
    fn wallclock_ms(&self) -> u64;

    /// Fill buffer with random bytes
    ///
    /// On WASM: Uses `crypto.getRandomValues()`
    ///
    /// # Arguments
    /// * `buf` - Buffer to fill with random bytes
    ///
    /// # Returns
    /// * `Ok(())` - Buffer filled successfully
    /// * `Err(HalError::NotSupported)` - Entropy source not available
    fn random_bytes(&self, buf: &mut [u8]) -> Result<(), HalError>;

    // === Debug ===

    /// Write a debug message to the platform's console/log
    ///
    /// On WASM: Uses `console.log()`
    fn debug_write(&self, msg: &str);

    // === Message Reception (for supervisor) ===

    /// Poll for incoming messages from processes (non-blocking)
    ///
    /// Returns a list of (process_handle, message_bytes) pairs
    fn poll_messages(&self) -> Vec<(Self::ProcessHandle, Vec<u8>)>;

    /// Register a callback for when messages arrive from processes
    ///
    /// This is optional - implementations can use polling instead
    fn set_message_callback(&self, _callback: Option<MessageCallback<Self::ProcessHandle>>) {
        // Default: no-op, use polling
    }

    // === Async Platform Storage ===
    // These methods start async storage operations and return immediately with a request_id.
    // Results are delivered via push callbacks (see notify_storage_* methods).

    /// Start async read from platform storage (returns immediately)
    ///
    /// The result will be delivered via notify_storage_read_complete callback.
    ///
    /// # Arguments
    /// * `pid` - Process ID requesting the operation
    /// * `key` - Storage key to read
    ///
    /// # Returns
    /// * `Ok(request_id)` - Unique request ID to match with result
    /// * `Err(HalError)` - Failed to start operation
    fn storage_read_async(&self, _pid: u64, _key: &str) -> Result<StorageRequestId, HalError> {
        Err(HalError::NotSupported)
    }

    /// Start async write to platform storage (returns immediately)
    ///
    /// The result will be delivered via notify_storage_write_complete callback.
    ///
    /// # Arguments
    /// * `pid` - Process ID requesting the operation
    /// * `key` - Storage key to write
    /// * `value` - Data to store
    ///
    /// # Returns
    /// * `Ok(request_id)` - Unique request ID to match with result
    /// * `Err(HalError)` - Failed to start operation
    fn storage_write_async(
        &self,
        _pid: u64,
        _key: &str,
        _value: &[u8],
    ) -> Result<StorageRequestId, HalError> {
        Err(HalError::NotSupported)
    }

    /// Start async delete from platform storage (returns immediately)
    ///
    /// The result will be delivered via notify_storage_write_complete callback.
    ///
    /// # Arguments
    /// * `pid` - Process ID requesting the operation
    /// * `key` - Storage key to delete
    ///
    /// # Returns
    /// * `Ok(request_id)` - Unique request ID to match with result
    /// * `Err(HalError)` - Failed to start operation
    fn storage_delete_async(&self, _pid: u64, _key: &str) -> Result<StorageRequestId, HalError> {
        Err(HalError::NotSupported)
    }

    /// Start async list keys with prefix (returns immediately)
    ///
    /// The result will be delivered via notify_storage_list_complete callback.
    ///
    /// # Arguments
    /// * `pid` - Process ID requesting the operation
    /// * `prefix` - Key prefix to match
    ///
    /// # Returns
    /// * `Ok(request_id)` - Unique request ID to match with result
    /// * `Err(HalError)` - Failed to start operation
    fn storage_list_async(&self, _pid: u64, _prefix: &str) -> Result<StorageRequestId, HalError> {
        Err(HalError::NotSupported)
    }

    /// Start async exists check (returns immediately)
    ///
    /// The result will be delivered via notify_storage_exists_complete callback.
    ///
    /// # Arguments
    /// * `pid` - Process ID requesting the operation
    /// * `key` - Storage key to check
    ///
    /// # Returns
    /// * `Ok(request_id)` - Unique request ID to match with result
    /// * `Err(HalError)` - Failed to start operation
    fn storage_exists_async(&self, _pid: u64, _key: &str) -> Result<StorageRequestId, HalError> {
        Err(HalError::NotSupported)
    }

    /// Get the PID associated with a pending storage request
    ///
    /// # Arguments
    /// * `request_id` - The request ID to look up
    ///
    /// # Returns
    /// * `Some(pid)` - The PID that initiated this request
    /// * `None` - Request ID not found
    fn get_storage_request_pid(&self, _request_id: StorageRequestId) -> Option<u64> {
        None
    }

    /// Remove and return the PID for a completed storage request
    ///
    /// # Arguments
    /// * `request_id` - The request ID to remove
    ///
    /// # Returns
    /// * `Some(pid)` - The PID that initiated this request (now removed)
    /// * `None` - Request ID not found
    fn take_storage_request_pid(&self, _request_id: StorageRequestId) -> Option<u64> {
        None
    }

    // === Async Keystore (KeyService Only) ===
    // These methods provide access to the dedicated keystore (zos-keystore IndexedDB).
    // Only KeyService should use these syscalls - other processes use KeyService IPC.

    /// Start async read from keystore (returns immediately)
    ///
    /// The result will be delivered via notify_keystore_read_complete callback.
    ///
    /// # Arguments
    /// * `pid` - Process ID requesting the operation (must be KeyService)
    /// * `key` - Keystore path to read (format: /keys/{user_id}/...)
    ///
    /// # Returns
    /// * `Ok(request_id)` - Unique request ID to match with result
    /// * `Err(HalError)` - Failed to start operation
    fn keystore_read_async(&self, _pid: u64, _key: &str) -> Result<StorageRequestId, HalError> {
        Err(HalError::NotSupported)
    }

    /// Start async write to keystore (returns immediately)
    ///
    /// The result will be delivered via notify_keystore_write_complete callback.
    ///
    /// # Arguments
    /// * `pid` - Process ID requesting the operation (must be KeyService)
    /// * `key` - Keystore path to write (format: /keys/{user_id}/...)
    /// * `value` - Key data to store
    ///
    /// # Returns
    /// * `Ok(request_id)` - Unique request ID to match with result
    /// * `Err(HalError)` - Failed to start operation
    fn keystore_write_async(
        &self,
        _pid: u64,
        _key: &str,
        _value: &[u8],
    ) -> Result<StorageRequestId, HalError> {
        Err(HalError::NotSupported)
    }

    /// Start async delete from keystore (returns immediately)
    ///
    /// The result will be delivered via notify_keystore_write_complete callback.
    ///
    /// # Arguments
    /// * `pid` - Process ID requesting the operation (must be KeyService)
    /// * `key` - Keystore path to delete
    ///
    /// # Returns
    /// * `Ok(request_id)` - Unique request ID to match with result
    /// * `Err(HalError)` - Failed to start operation
    fn keystore_delete_async(&self, _pid: u64, _key: &str) -> Result<StorageRequestId, HalError> {
        Err(HalError::NotSupported)
    }

    /// Start async list keys with prefix (returns immediately)
    ///
    /// The result will be delivered via notify_keystore_list_complete callback.
    ///
    /// # Arguments
    /// * `pid` - Process ID requesting the operation (must be KeyService)
    /// * `prefix` - Key path prefix to match
    ///
    /// # Returns
    /// * `Ok(request_id)` - Unique request ID to match with result
    /// * `Err(HalError)` - Failed to start operation
    fn keystore_list_async(&self, _pid: u64, _prefix: &str) -> Result<StorageRequestId, HalError> {
        Err(HalError::NotSupported)
    }

    /// Start async exists check on keystore (returns immediately)
    ///
    /// The result will be delivered via notify_keystore_exists_complete callback.
    ///
    /// # Arguments
    /// * `pid` - Process ID requesting the operation (must be KeyService)
    /// * `key` - Keystore path to check
    ///
    /// # Returns
    /// * `Ok(request_id)` - Unique request ID to match with result
    /// * `Err(HalError)` - Failed to start operation
    fn keystore_exists_async(&self, _pid: u64, _key: &str) -> Result<StorageRequestId, HalError> {
        Err(HalError::NotSupported)
    }

    /// Get the PID associated with a pending keystore request
    ///
    /// # Arguments
    /// * `request_id` - The request ID to look up
    ///
    /// # Returns
    /// * `Some(pid)` - The PID that initiated this request
    /// * `None` - Request ID not found
    fn get_keystore_request_pid(&self, _request_id: StorageRequestId) -> Option<u64> {
        None
    }

    /// Remove and return the PID for a completed keystore request
    ///
    /// # Arguments
    /// * `request_id` - The request ID to remove
    ///
    /// # Returns
    /// * `Some(pid)` - The PID that initiated this request (now removed)
    /// * `None` - Request ID not found
    fn take_keystore_request_pid(&self, _request_id: StorageRequestId) -> Option<u64> {
        None
    }

    // === Async Network Operations ===
    // These methods start async network (HTTP) operations and return immediately with a request_id.
    // Results are delivered via push callbacks (see onNetworkResult in JS HAL).

    /// Start async HTTP fetch operation (returns immediately)
    ///
    /// The result will be delivered via MSG_NET_RESULT IPC callback.
    ///
    /// # Arguments
    /// * `pid` - Process ID requesting the operation
    /// * `request` - Serialized HttpRequest (JSON bytes)
    ///
    /// # Returns
    /// * `Ok(request_id)` - Unique request ID to match with result
    /// * `Err(HalError)` - Failed to start operation
    fn network_fetch_async(
        &self,
        _pid: u64,
        _request: &[u8],
    ) -> Result<NetworkRequestId, HalError> {
        Err(HalError::NotSupported)
    }

    /// Get the PID associated with a pending network request
    ///
    /// # Arguments
    /// * `request_id` - The request ID to look up
    ///
    /// # Returns
    /// * `Some(pid)` - The PID that initiated this request
    /// * `None` - Request ID not found
    fn get_network_request_pid(&self, _request_id: NetworkRequestId) -> Option<u64> {
        None
    }

    /// Remove and return the PID for a completed network request
    ///
    /// # Arguments
    /// * `request_id` - The request ID to remove
    ///
    /// # Returns
    /// * `Some(pid)` - The PID that initiated this request (now removed)
    /// * `None` - Request ID not found
    fn take_network_request_pid(&self, _request_id: NetworkRequestId) -> Option<u64> {
        None
    }

    // === Bootstrap Storage (Supervisor Only) ===
    // These methods are used ONLY during supervisor initialization before processes exist.
    // They provide direct storage access for bootstrap operations like creating the root
    // filesystem structure. After Init starts, all storage access should go through
    // processes using syscalls routed via HAL.

    /// Initialize bootstrap storage (supervisor boot only).
    ///
    /// This initializes the underlying storage backend (e.g., IndexedDB) for
    /// use during supervisor bootstrap. Must be called before other bootstrap
    /// storage operations.
    ///
    /// # Returns
    /// * `Ok(true)` - Storage initialized successfully
    /// * `Err(HalError)` - Initialization failed
    fn bootstrap_storage_init(&self) -> Result<bool, HalError> {
        Err(HalError::NotSupported)
    }

    /// Read an inode from bootstrap storage (supervisor boot only).
    ///
    /// # Arguments
    /// * `path` - The filesystem path to read
    ///
    /// # Returns
    /// * `Ok(Some(data))` - Inode data as JSON bytes
    /// * `Ok(None)` - Path not found
    /// * `Err(HalError)` - Read failed
    fn bootstrap_storage_get_inode(&self, _path: &str) -> Result<Option<Vec<u8>>, HalError> {
        Err(HalError::NotSupported)
    }

    /// Write an inode to bootstrap storage (supervisor boot only).
    ///
    /// # Arguments
    /// * `path` - The filesystem path
    /// * `inode_json` - The inode data as JSON bytes
    ///
    /// # Returns
    /// * `Ok(())` - Write successful
    /// * `Err(HalError)` - Write failed
    fn bootstrap_storage_put_inode(&self, _path: &str, _inode_json: &[u8]) -> Result<(), HalError> {
        Err(HalError::NotSupported)
    }

    /// Get the count of inodes in storage (supervisor boot only).
    ///
    /// # Returns
    /// * `Ok(count)` - Number of inodes
    /// * `Err(HalError)` - Count failed
    fn bootstrap_storage_inode_count(&self) -> Result<u64, HalError> {
        Err(HalError::NotSupported)
    }

    /// Clear all storage data (supervisor only, for testing/reset).
    ///
    /// # Returns
    /// * `Ok(())` - Clear successful
    /// * `Err(HalError)` - Clear failed
    fn bootstrap_storage_clear(&self) -> Result<(), HalError> {
        Err(HalError::NotSupported)
    }
}

/// HAL errors
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HalError {
    /// Not enough memory available
    OutOfMemory,
    /// Failed to spawn a process
    ProcessSpawnFailed,
    /// Process not found or already terminated
    ProcessNotFound,
    /// Invalid message format or too large
    InvalidMessage,
    /// Operation not supported on this platform
    NotSupported,
    /// I/O error
    IoError,
    /// Invalid argument
    InvalidArgument,
    /// Storage operation failed
    StorageError,
    /// Resource limit reached (e.g., too many pending requests)
    ResourceExhausted,
}

/// Request ID for tracking async storage operations
pub type StorageRequestId = u32;

/// Request ID for tracking async network operations
pub type NetworkRequestId = u32;

/// Process message types (used in IPC between HAL and processes)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProcessMessageType {
    /// Initialize process with PID and capabilities
    Init = 0,
    /// Syscall from process to supervisor
    Syscall = 1,
    /// Syscall result from supervisor to process
    SyscallResult = 2,
    /// IPC message delivery
    IpcDeliver = 3,
    /// Process termination request
    Terminate = 4,
    /// Process ready notification
    Ready = 5,
    /// Error notification
    Error = 6,
}

impl ProcessMessageType {
    /// Convert from u8
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Init),
            1 => Some(Self::Syscall),
            2 => Some(Self::SyscallResult),
            3 => Some(Self::IpcDeliver),
            4 => Some(Self::Terminate),
            5 => Some(Self::Ready),
            6 => Some(Self::Error),
            _ => None,
        }
    }
}

/// A simple process handle for platforms that use numeric IDs
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NumericProcessHandle(pub u64);

impl NumericProcessHandle {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn id(&self) -> u64 {
        self.0
    }
}

/// A minimal test HAL for unit testing
///
/// This HAL implementation provides stub implementations for all HAL methods,
/// suitable for unit tests that don't need full platform functionality.
#[derive(Default)]
pub struct TestHal {
    time: core::sync::atomic::AtomicU64,
}

impl TestHal {
    pub fn new() -> Self {
        Self {
            time: core::sync::atomic::AtomicU64::new(0),
        }
    }
}

unsafe impl Send for TestHal {}
unsafe impl Sync for TestHal {}

impl HAL for TestHal {
    type ProcessHandle = NumericProcessHandle;

    fn spawn_process(&self, _name: &str, _binary: &[u8]) -> Result<Self::ProcessHandle, HalError> {
        Ok(NumericProcessHandle::new(1))
    }

    fn kill_process(&self, _handle: &Self::ProcessHandle) -> Result<(), HalError> {
        Ok(())
    }

    fn send_to_process(&self, _handle: &Self::ProcessHandle, _msg: &[u8]) -> Result<(), HalError> {
        Ok(())
    }

    fn is_process_alive(&self, _handle: &Self::ProcessHandle) -> bool {
        true
    }

    fn get_process_memory_size(&self, _handle: &Self::ProcessHandle) -> Result<usize, HalError> {
        Ok(65536)
    }

    fn allocate(&self, size: usize, _align: usize) -> Result<*mut u8, HalError> {
        let layout =
            core::alloc::Layout::from_size_align(size, 8).map_err(|_| HalError::InvalidArgument)?;
        let ptr = unsafe { alloc::alloc::alloc(layout) };
        if ptr.is_null() {
            Err(HalError::OutOfMemory)
        } else {
            Ok(ptr)
        }
    }

    unsafe fn deallocate(&self, ptr: *mut u8, size: usize, _align: usize) {
        if !ptr.is_null() {
            // SAFETY: Layout is guaranteed valid since we use the same size and alignment
            // that was used in allocate(). align=8 is always valid.
            let layout = core::alloc::Layout::from_size_align(size, 8).unwrap();
            alloc::alloc::dealloc(ptr, layout);
        }
    }

    fn now_nanos(&self) -> u64 {
        self.time.load(core::sync::atomic::Ordering::SeqCst)
    }

    fn wallclock_ms(&self) -> u64 {
        1737504000000
    }

    fn random_bytes(&self, buf: &mut [u8]) -> Result<(), HalError> {
        for byte in buf.iter_mut() {
            *byte = 0x42;
        }
        Ok(())
    }

    fn debug_write(&self, _msg: &str) {
        // No-op for tests
    }

    fn poll_messages(&self) -> Vec<(Self::ProcessHandle, Vec<u8>)> {
        Vec::new()
    }
}
