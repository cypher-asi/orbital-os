//! Hardware Abstraction Layer trait for Orbital OS
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
    fn deallocate(&self, ptr: *mut u8, size: usize, align: usize);

    // === Time & Entropy ===

    /// Get current time in nanoseconds (monotonic)
    ///
    /// On WASM: Uses `performance.now()` converted to nanoseconds
    fn now_nanos(&self) -> u64;

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
}

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
