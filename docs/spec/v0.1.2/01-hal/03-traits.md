# HAL Trait Interface

> The `HAL` trait defines the platform abstraction boundary for the kernel.

## Overview

The HAL trait is implemented once per target platform. The kernel is generic over `HAL`, allowing the same kernel code to run on WASM, QEMU, and bare metal.

```rust
pub struct Kernel<H: HAL> {
    hal: H,
    // ... kernel state
}
```

## Trait Definition

```rust
//! Hardware Abstraction Layer trait for Zero OS
//!
//! This trait allows the kernel to run on different platforms by abstracting
//! hardware operations.

#![no_std]

extern crate alloc;
use alloc::vec::Vec;

/// Hardware Abstraction Layer trait.
///
/// Implementations provide platform-specific functionality for:
/// - Process management (spawn, kill, message passing)
/// - Memory allocation
/// - Time measurement
/// - Entropy (random numbers)
/// - Debug output
pub trait HAL: Send + Sync + 'static {
    /// Handle to a spawned process.
    ///
    /// - On WASM: Reference to a Web Worker
    /// - On QEMU/bare metal: Process ID or memory region reference
    type ProcessHandle: Clone + Send + Sync;

    // =========================================================================
    // Process Management
    // =========================================================================

    /// Spawn a new process from WASM binary.
    ///
    /// On WASM: Creates a new Web Worker and loads the binary.
    /// On native: Creates a new process with isolated memory.
    ///
    /// # Arguments
    /// - `name`: Human-readable process name for debugging
    /// - `binary`: WASM binary to execute
    ///
    /// # Returns
    /// - `Ok(ProcessHandle)`: Handle to the spawned process
    /// - `Err(HalError::ProcessSpawnFailed)`: Failed to create process
    fn spawn_process(&self, name: &str, binary: &[u8]) -> Result<Self::ProcessHandle, HalError>;

    /// Terminate a process.
    ///
    /// # Arguments
    /// - `handle`: Handle to the process to terminate
    ///
    /// # Returns
    /// - `Ok(())`: Process terminated successfully
    /// - `Err(HalError::ProcessNotFound)`: Process doesn't exist
    fn kill_process(&self, handle: &Self::ProcessHandle) -> Result<(), HalError>;

    /// Send a message to a process.
    ///
    /// # Arguments
    /// - `handle`: Handle to the target process
    /// - `msg`: Message bytes to send
    ///
    /// # Returns
    /// - `Ok(())`: Message sent successfully
    /// - `Err(HalError::ProcessNotFound)`: Process doesn't exist
    /// - `Err(HalError::InvalidMessage)`: Message too large or malformed
    fn send_to_process(&self, handle: &Self::ProcessHandle, msg: &[u8]) -> Result<(), HalError>;

    /// Check if a process is still running.
    fn is_process_alive(&self, handle: &Self::ProcessHandle) -> bool;

    /// Get the memory size of a process in bytes.
    ///
    /// On WASM: Returns the linear memory size (pages * 64KB).
    fn get_process_memory_size(&self, handle: &Self::ProcessHandle) -> Result<usize, HalError>;

    // =========================================================================
    // Memory
    // =========================================================================

    /// Allocate memory (within current context).
    ///
    /// Note: On WASM, each process has its own linear memory managed by the
    /// WASM runtime. This is primarily for supervisor-side allocations.
    ///
    /// # Arguments
    /// - `size`: Number of bytes to allocate
    /// - `align`: Alignment requirement
    ///
    /// # Returns
    /// - `Ok(ptr)`: Pointer to allocated memory
    /// - `Err(HalError::OutOfMemory)`: Allocation failed
    fn allocate(&self, size: usize, align: usize) -> Result<*mut u8, HalError>;

    /// Deallocate memory.
    ///
    /// # Safety
    /// The pointer must have been allocated by `allocate` with the same size
    /// and alignment.
    fn deallocate(&self, ptr: *mut u8, size: usize, align: usize);

    // =========================================================================
    // Time & Entropy
    // =========================================================================

    /// Get current time in nanoseconds (monotonic).
    ///
    /// On WASM: Uses `performance.now()` converted to nanoseconds.
    /// On native: Uses HPET, TSC, or similar high-resolution timer.
    fn now_nanos(&self) -> u64;

    /// Fill buffer with random bytes.
    ///
    /// On WASM: Uses `crypto.getRandomValues()`.
    /// On native: Uses RDRAND, VirtIO-rng, or similar.
    ///
    /// # Arguments
    /// - `buf`: Buffer to fill with random bytes
    ///
    /// # Returns
    /// - `Ok(())`: Buffer filled successfully
    /// - `Err(HalError::NotSupported)`: Entropy source not available
    fn random_bytes(&self, buf: &mut [u8]) -> Result<(), HalError>;

    // =========================================================================
    // Debug
    // =========================================================================

    /// Write a debug message to the platform's console/log.
    ///
    /// On WASM: Uses `console.log()`.
    /// On native: Uses serial port or VGA text mode.
    fn debug_write(&self, msg: &str);

    // =========================================================================
    // Message Reception
    // =========================================================================

    /// Poll for incoming messages from processes (non-blocking).
    ///
    /// Returns a list of (process_handle, message_bytes) pairs.
    fn poll_messages(&self) -> Vec<(Self::ProcessHandle, Vec<u8>)>;

    /// Register a callback for when messages arrive from processes.
    ///
    /// Optional - implementations can use polling instead.
    fn set_message_callback(&self, _callback: Option<fn(&Self::ProcessHandle, &[u8])>) {
        // Default: no-op, use polling
    }
}
```

## Error Types

```rust
/// HAL errors.
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
```

## Process Message Types

```rust
/// Process message types (used in IPC between HAL and processes).
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
```

## Implementation: WASM HAL

The WASM HAL is implemented in JavaScript with Rust bindings:

```rust
// Zero-hal-wasm/src/lib.rs (simplified)

pub struct WasmHal {
    // Internal state managed by JavaScript
}

impl HAL for WasmHal {
    type ProcessHandle = WorkerHandle;
    
    fn spawn_process(&self, name: &str, binary: &[u8]) -> Result<WorkerHandle, HalError> {
        // Calls JavaScript: new Worker(...)
    }
    
    fn now_nanos(&self) -> u64 {
        // Calls JavaScript: performance.now() * 1_000_000
    }
    
    // ... etc
}

/// Handle to a Web Worker.
#[derive(Clone)]
pub struct WorkerHandle {
    id: u32,  // Worker ID tracked by JavaScript
}
```

## Implementation: Mock HAL (Testing)

```rust
// crates/Zero-kernel/src/lib.rs (test module: mock_hal)

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Mock HAL for testing (integrated into Zero-kernel as a cfg(test) module).
pub struct MockHal {
    time: AtomicU64,
    processes: RefCell<BTreeMap<u64, MockProcess>>,
    // ... more fields for simulation
}

impl MockHal {
    pub fn new() -> Self {
        Self { /* ... */ }
    }
    
    pub fn with_time(time: u64) -> Self {
        Self { time, messages: VecDeque::new() }
    }
    
    pub fn advance_time(&mut self, nanos: u64) {
        self.time += nanos;
    }
}

impl HAL for MockHal {
    type ProcessHandle = NumericProcessHandle;
    
    fn spawn_process(&self, _name: &str, _binary: &[u8]) -> Result<NumericProcessHandle, HalError> {
        Ok(NumericProcessHandle::new(1))
    }
    
    fn now_nanos(&self) -> u64 {
        self.time
    }
    
    fn debug_write(&self, msg: &str) {
        // In tests, could collect these for assertions
        #[cfg(test)]
        eprintln!("[MockHal] {}", msg);
    }
    
    // ... etc
}
```

## Implementation: Native HAL (Future)

```rust
// Zero-hal-native/src/lib.rs (future Phase 2)

pub struct NativeHal {
    // Platform-specific state
}

impl HAL for NativeHal {
    type ProcessHandle = ProcessId;
    
    fn spawn_process(&self, name: &str, binary: &[u8]) -> Result<ProcessId, HalError> {
        // 1. Allocate page tables
        // 2. Map WASM binary (or native ELF)
        // 3. Set up stack
        // 4. Create TCB
        // 5. Add to scheduler
    }
    
    fn now_nanos(&self) -> u64 {
        // Read HPET or TSC
    }
    
    fn random_bytes(&self, buf: &mut [u8]) -> Result<(), HalError> {
        // RDRAND instruction
        for chunk in buf.chunks_mut(8) {
            let rand: u64 = rdrand();
            chunk.copy_from_slice(&rand.to_le_bytes()[..chunk.len()]);
        }
        Ok(())
    }
    
    // ... etc
}
```

## Usage in Kernel

```rust
use Zero_hal::HAL;

pub struct Kernel<H: HAL> {
    hal: H,
    processes: BTreeMap<ProcessId, Process>,
    cap_spaces: BTreeMap<ProcessId, CapabilitySpace>,
    // ...
}

impl<H: HAL> Kernel<H> {
    pub fn new(hal: H) -> Self {
        let boot_time = hal.now_nanos();
        Self {
            hal,
            processes: BTreeMap::new(),
            cap_spaces: BTreeMap::new(),
            boot_time,
            // ...
        }
    }
    
    pub fn handle_syscall(&mut self, pid: ProcessId, syscall: Syscall) -> SyscallResult {
        match syscall {
            Syscall::Debug { msg } => {
                self.hal.debug_write(&format!("[PID {}] {}", pid.0, msg));
                SyscallResult::Ok(0)
            }
            Syscall::GetTime => {
                SyscallResult::Ok(self.hal.now_nanos() - self.boot_time)
            }
            // ...
        }
    }
}
```

## Current Code Compatibility

The existing `Zero-hal` crate already defines this trait with the same interface. The spec documents the expected behavior that implementations must provide.

Key compatibility points:
- `ProcessHandle` associated type (currently used)
- `spawn_process`, `kill_process`, `send_to_process` (implemented)
- `now_nanos`, `random_bytes` (implemented)
- `debug_write` (implemented)
- `poll_messages` (implemented)
