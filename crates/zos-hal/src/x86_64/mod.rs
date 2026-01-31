//! x86_64 Hardware Abstraction Layer Implementation
//!
//! This module provides the x86_64 HAL implementation for running Zero OS
//! on QEMU and bare metal x86_64 hardware.
//!
//! # Components
//!
//! - **Serial**: COM1 serial port driver for debug output
//! - **GDT**: Global Descriptor Table with TSS for interrupt handling
//! - **Interrupts**: Interrupt Descriptor Table for exception handling
//! - **VMM**: Virtual Memory Manager with 4-level page tables
//! - **APIC**: Local APIC for timer and interrupt handling
//! - **VirtIO**: VirtIO device drivers (block, network, etc.)
//! - **WASM**: WASM runtime for executing service binaries

pub mod apic;
pub mod gdt;
pub mod interrupts;
pub mod pci;
pub mod random;
pub mod rtc;
#[macro_use]
pub mod serial;
pub mod storage;
pub mod virtio;
pub mod vmm;
pub mod wasm;

// =============================================================================
// Embedded WASM Binaries (for QEMU Native Runtime)
// =============================================================================
//
// These binaries are embedded at compile time using include_bytes!().
// This provides zero-copy access to service binaries without requiring
// a filesystem or network fetch.

/// Embedded WASM binaries for core system services.
///
/// These are included at compile time for QEMU native runtime.
/// IMPORTANT: These use the qemu/processes/ directory which contains binaries
/// built WITHOUT shared memory (wasmi doesn't support the threads proposal).
/// The web/processes/ binaries are built WITH shared memory for browser support.
mod embedded_binaries {
    /// Init process - service registry and IPC router
    pub static INIT: &[u8] = include_bytes!("../../../../qemu/processes/init.wasm");
    /// PermissionService - capability authority
    pub static PERMISSION: &[u8] = include_bytes!("../../../../qemu/processes/permission.wasm");
    /// VfsService - virtual filesystem
    pub static VFS: &[u8] = include_bytes!("../../../../qemu/processes/vfs.wasm");
    /// KeystoreService - secure key storage
    pub static KEYSTORE: &[u8] = include_bytes!("../../../../qemu/processes/keystore.wasm");
    /// IdentityService - user/session management
    pub static IDENTITY: &[u8] = include_bytes!("../../../../qemu/processes/identity.wasm");
    /// TimeService - time settings
    pub static TIME: &[u8] = include_bytes!("../../../../qemu/processes/time.wasm");
    /// Terminal - console application
    pub static TERMINAL: &[u8] = include_bytes!("../../../../qemu/processes/terminal.wasm");
    /// Settings - system settings application
    pub static SETTINGS: &[u8] = include_bytes!("../../../../qemu/processes/settings.wasm");
    /// Calculator - calculator application
    pub static CALCULATOR: &[u8] = include_bytes!("../../../../qemu/processes/calculator.wasm");
    /// Clock - clock application
    pub static CLOCK: &[u8] = include_bytes!("../../../../qemu/processes/clock.wasm");
}

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::{HalError, NumericProcessHandle, StorageRequestId, HAL};

// Re-export WASM runtime types
pub use wasm::{WasmRuntime, PendingSyscall};

/// Global WASM runtime shared by all X86_64Hal instances.
///
/// This ensures that processes spawned via ANY HAL instance are visible to
/// the scheduler running on ANY other HAL instance. Without this, creating
/// multiple X86_64Hal instances (e.g., one for System and one global) would
/// result in separate WasmRuntimes that don't share process state.
static GLOBAL_WASM_RUNTIME: spin::Once<WasmRuntime> = spin::Once::new();

/// Maximum pending storage requests
const MAX_PENDING_STORAGE_REQUESTS: usize = 1000;

/// Storage request state
#[derive(Clone, Debug)]
#[allow(dead_code)] // Fields are stored for future retrieval but read operations not yet implemented
enum StorageRequestState {
    /// Read completed with result
    ReadComplete(Option<Vec<u8>>),
    /// Write/delete completed
    WriteComplete(bool),
    /// List completed with keys
    ListComplete(Vec<String>),
    /// Exists check completed
    ExistsComplete(bool),
}

/// x86_64 Hardware Abstraction Layer implementation
///
/// Provides platform-specific functionality for x86_64 targets:
/// - Serial console for debug output
/// - Time via TSC/HPET (currently stubbed)
/// - Entropy via RDRAND (currently stubbed)
/// - VMM for memory management
/// - VirtIO block storage
/// - WASM runtime for executing service binaries
pub struct X86_64Hal {
    /// Monotonic time counter (nanoseconds since boot)
    time_nanos: AtomicU64,
    /// Next process ID
    next_pid: AtomicU64,
    /// Incoming messages from processes
    messages: Mutex<Vec<(NumericProcessHandle, Vec<u8>)>>,
    /// Next storage request ID
    next_storage_request_id: AtomicU32,
    /// Pending storage requests: request_id -> (pid, result)
    storage_requests: Mutex<BTreeMap<StorageRequestId, (u64, StorageRequestState)>>,
    /// Storage initialized flag
    storage_initialized: Mutex<bool>,
    /// Pending IPC messages for processes: pid -> Vec<message_bytes>
    pending_ipc: Mutex<BTreeMap<u64, Vec<Vec<u8>>>>,
}

impl X86_64Hal {
    /// Create a new x86_64 HAL instance
    pub const fn new() -> Self {
        Self {
            time_nanos: AtomicU64::new(0),
            next_pid: AtomicU64::new(1), // PID 0 reserved for kernel
            messages: Mutex::new(Vec::new()),
            next_storage_request_id: AtomicU32::new(1),
            storage_requests: Mutex::new(BTreeMap::new()),
            storage_initialized: Mutex::new(false),
            pending_ipc: Mutex::new(BTreeMap::new()),
        }
    }
    
    /// Get or initialize the global WASM runtime
    ///
    /// The WASM runtime is shared globally across all X86_64Hal instances.
    /// This ensures that processes spawned via any HAL instance are visible
    /// to the scheduler running on any HAL instance.
    pub fn wasm_runtime(&self) -> &'static WasmRuntime {
        GLOBAL_WASM_RUNTIME.call_once(|| WasmRuntime::new())
    }
    
    /// Allocate a new storage request ID
    fn alloc_storage_request_id(&self) -> StorageRequestId {
        self.next_storage_request_id.fetch_add(1, Ordering::SeqCst)
    }
    
    /// Allocate a new process ID
    fn alloc_pid(&self) -> u64 {
        self.next_pid.fetch_add(1, Ordering::Relaxed)
    }
    
    /// Run the process scheduler
    ///
    /// Executes WASM processes cooperatively. Returns pending syscalls
    /// from processes for the supervisor to handle.
    /// 
    /// This runs processes in small timeslices, yielding when a syscall
    /// is made so the kernel can process it before the process continues.
    pub fn run_scheduler(&self) -> Vec<PendingSyscall> {
        self.wasm_runtime().run_all_processes()
    }
    
    /// Run the process scheduler with a syscall handler
    ///
    /// This variant processes syscalls synchronously as they are made,
    /// ensuring the process doesn't continue until the syscall is complete.
    /// Handler returns (i64, Vec<u8>) to support 64-bit packed return values.
    pub fn run_scheduler_with_handler<F>(&self, mut handler: F)
    where
        F: FnMut(PendingSyscall) -> (i64, Vec<u8>),
    {
        self.wasm_runtime().run_all_processes_with_handler(&mut handler)
    }
    
    /// Complete a syscall and resume the process (result is i64 for 64-bit return values)
    pub fn complete_syscall(&self, pid: u64, result: i64, data: &[u8]) {
        self.wasm_runtime().complete_syscall(pid, result, data);
    }

    /// Initialize all hardware subsystems
    ///
    /// This should be called early in kernel boot to set up:
    /// - Serial output
    /// - GDT with TSS
    /// - IDT with exception handlers
    /// - VMM (page tables, frame allocator)
    /// - APIC timer for preemptive scheduling
    ///
    /// # Safety
    /// Must be called only once during kernel initialization.
    /// Must be called after the bootloader has set up initial paging.
    pub unsafe fn init(&self, physical_memory_offset: u64, memory_regions: &[vmm::MemoryRegionDescriptor]) {
        // Initialize serial first for debug output
        serial::init();

        // Set up GDT with TSS for interrupt handling
        gdt::init();

        // Set up IDT for exception handling
        interrupts::init();

        // Initialize VMM with physical memory info
        vmm::init(physical_memory_offset, memory_regions);

        // Initialize APIC (timer will start after interrupts are enabled)
        apic::init();

        // Scan for VirtIO devices
        virtio::init();
    }

    /// Initialize with default settings (for simple boot)
    ///
    /// # Safety
    /// Must be called only once during kernel initialization.
    pub unsafe fn init_simple(&self) {
        serial::init();
        gdt::init();
        interrupts::init();
        // VMM and APIC not initialized - must call init() with memory regions for full features
    }

    /// Enable hardware interrupts
    ///
    /// This should be called after all interrupt handlers are set up.
    pub fn enable_interrupts(&self) {
        x86_64::instructions::interrupts::enable();
    }

    /// Disable hardware interrupts
    pub fn disable_interrupts(&self) {
        x86_64::instructions::interrupts::disable();
    }

    /// Start the APIC timer
    ///
    /// This should be called AFTER enabling interrupts. The timer
    /// will then fire every ~10ms and invoke the timer interrupt handler.
    pub fn start_timer(&self) {
        unsafe {
            apic::start_timer();
        }
    }

    /// Update the monotonic time counter
    ///
    /// Called by timer interrupt handler to advance time.
    pub fn update_time(&self, elapsed_nanos: u64) {
        self.time_nanos.fetch_add(elapsed_nanos, Ordering::Relaxed);
    }
}

impl Default for X86_64Hal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for X86_64Hal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("X86_64Hal")
            .field("time_nanos", &self.time_nanos.load(Ordering::Relaxed))
            .field("next_pid", &self.next_pid.load(Ordering::Relaxed))
            .finish()
    }
}

// SAFETY: X86_64Hal is designed to be shared across threads (in a preemptive kernel)
// All mutable state is protected by atomics or mutexes
unsafe impl Send for X86_64Hal {}
unsafe impl Sync for X86_64Hal {}

impl HAL for X86_64Hal {
    type ProcessHandle = NumericProcessHandle;

    // === Process Management (Stage 2.6 - WASM Runtime) ===

    fn spawn_process(&self, name: &str, binary: &[u8]) -> Result<Self::ProcessHandle, HalError> {
        let pid = self.alloc_pid();
        serial::write_str(&alloc::format!(
            "[x86_64-hal] spawn_process: name='{}', pid={}, binary_size={}\n",
            name, pid, binary.len()
        ));
        
        // Spawn the WASM process
        self.wasm_runtime().spawn(pid, name, binary)
    }

    fn spawn_process_with_pid(
        &self,
        pid: u64,
        name: &str,
        binary: &[u8],
    ) -> Result<Self::ProcessHandle, HalError> {
        serial::write_str(&alloc::format!(
            "[x86_64-hal] spawn_process_with_pid: name='{}', pid={}, binary_size={}\n",
            name, pid, binary.len()
        ));
        
        // Spawn the WASM process with the specified PID
        self.wasm_runtime().spawn(pid, name, binary)
    }

    fn kill_process(&self, handle: &Self::ProcessHandle) -> Result<(), HalError> {
        serial::write_str(&alloc::format!(
            "[x86_64-hal] kill_process: pid={}\n", handle.id()
        ));
        self.wasm_runtime().kill(handle.id())
    }

    fn send_to_process(&self, handle: &Self::ProcessHandle, msg: &[u8]) -> Result<(), HalError> {
        let pid = handle.id();
        
        // Queue the message for delivery
        self.pending_ipc
            .lock()
            .entry(pid)
            .or_insert_with(Vec::new)
            .push(msg.to_vec());
        
        // Also queue in the WASM runtime
        self.wasm_runtime().queue_message(pid, msg.to_vec());
        
        Ok(())
    }

    fn is_process_alive(&self, handle: &Self::ProcessHandle) -> bool {
        self.wasm_runtime().is_alive(handle.id())
    }

    fn get_process_memory_size(&self, handle: &Self::ProcessHandle) -> Result<usize, HalError> {
        self.wasm_runtime().memory_size(handle.id())
    }

    // === Memory ===

    fn allocate(&self, size: usize, align: usize) -> Result<*mut u8, HalError> {
        let layout = core::alloc::Layout::from_size_align(size, align)
            .map_err(|_| HalError::InvalidArgument)?;
        let ptr = unsafe { alloc::alloc::alloc(layout) };
        if ptr.is_null() {
            Err(HalError::OutOfMemory)
        } else {
            Ok(ptr)
        }
    }

    unsafe fn deallocate(&self, ptr: *mut u8, size: usize, align: usize) {
        if !ptr.is_null() {
            let layout = core::alloc::Layout::from_size_align(size, align).unwrap();
            alloc::alloc::dealloc(ptr, layout);
        }
    }

    // === Time & Entropy ===

    fn now_nanos(&self) -> u64 {
        // Use APIC timer if initialized, otherwise fallback to internal counter
        if apic::is_initialized() {
            apic::elapsed_nanos()
        } else {
            self.time_nanos.load(Ordering::Relaxed)
        }
    }

    fn wallclock_ms(&self) -> u64 {
        // Read wall-clock time from CMOS RTC
        rtc::unix_timestamp_ms()
    }

    fn random_bytes(&self, buf: &mut [u8]) -> Result<(), HalError> {
        // Check if RDRAND is supported
        if !is_rdrand_supported() {
            // Fallback: use a simple PRNG seeded from TSC
            let mut seed = read_tsc();
            for byte in buf.iter_mut() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                *byte = (seed >> 33) as u8;
            }
            return Ok(());
        }
        
        // Use RDRAND instruction
        for chunk in buf.chunks_mut(8) {
            let random = rdrand_u64().ok_or(HalError::NotSupported)?;
            let bytes = random.to_le_bytes();
            for (i, byte) in chunk.iter_mut().enumerate() {
                *byte = bytes[i];
            }
        }
        Ok(())
    }

    // === Debug ===

    fn debug_write(&self, msg: &str) {
        serial::write_str(msg);
    }

    // === Message Reception ===

    fn poll_messages(&self) -> Vec<(Self::ProcessHandle, Vec<u8>)> {
        // Get any queued messages from the internal message queue
        let mut messages = self.messages.lock();
        let result = core::mem::take(&mut *messages);
        
        // In x86_64, syscalls from WASM processes are handled via run_scheduler()
        // which returns PendingSyscall structs. The supervisor should call that
        // instead of poll_messages for syscall handling.
        result
    }
    
    /// Queue a message from a process (internal use)
    fn set_message_callback(&self, _callback: Option<crate::MessageCallback<Self::ProcessHandle>>) {
        // Callbacks not used on x86_64 - we use polling via run_scheduler()
    }

    // === Async Storage Operations ===
    // On x86_64, storage operations are synchronous internally but use the
    // async API pattern for consistency with other platforms.

    fn storage_read_async(&self, pid: u64, key: &str) -> Result<StorageRequestId, HalError> {
        let mut requests = self.storage_requests.lock();
        if requests.len() >= MAX_PENDING_STORAGE_REQUESTS {
            return Err(HalError::ResourceExhausted);
        }

        let request_id = self.alloc_storage_request_id();
        
        // Perform synchronous read
        let result = match storage::read(key) {
            Ok(data) => StorageRequestState::ReadComplete(data),
            Err(_) => StorageRequestState::ReadComplete(None),
        };
        
        requests.insert(request_id, (pid, result));
        Ok(request_id)
    }

    fn storage_write_async(&self, pid: u64, key: &str, value: &[u8]) -> Result<StorageRequestId, HalError> {
        let mut requests = self.storage_requests.lock();
        if requests.len() >= MAX_PENDING_STORAGE_REQUESTS {
            return Err(HalError::ResourceExhausted);
        }

        let request_id = self.alloc_storage_request_id();
        
        // Perform synchronous write
        let success = storage::write(key, value).is_ok();
        requests.insert(request_id, (pid, StorageRequestState::WriteComplete(success)));
        Ok(request_id)
    }

    fn storage_delete_async(&self, pid: u64, key: &str) -> Result<StorageRequestId, HalError> {
        let mut requests = self.storage_requests.lock();
        if requests.len() >= MAX_PENDING_STORAGE_REQUESTS {
            return Err(HalError::ResourceExhausted);
        }

        let request_id = self.alloc_storage_request_id();
        
        // Perform synchronous delete
        let success = storage::delete(key).unwrap_or(false);
        requests.insert(request_id, (pid, StorageRequestState::WriteComplete(success)));
        Ok(request_id)
    }

    fn storage_list_async(&self, pid: u64, prefix: &str) -> Result<StorageRequestId, HalError> {
        let mut requests = self.storage_requests.lock();
        if requests.len() >= MAX_PENDING_STORAGE_REQUESTS {
            return Err(HalError::ResourceExhausted);
        }

        let request_id = self.alloc_storage_request_id();
        
        // Perform synchronous list
        let keys = storage::list(prefix);
        requests.insert(request_id, (pid, StorageRequestState::ListComplete(keys)));
        Ok(request_id)
    }

    fn storage_exists_async(&self, pid: u64, key: &str) -> Result<StorageRequestId, HalError> {
        let mut requests = self.storage_requests.lock();
        if requests.len() >= MAX_PENDING_STORAGE_REQUESTS {
            return Err(HalError::ResourceExhausted);
        }

        let request_id = self.alloc_storage_request_id();
        
        // Perform synchronous exists check
        let exists = storage::exists(key).unwrap_or(false);
        requests.insert(request_id, (pid, StorageRequestState::ExistsComplete(exists)));
        Ok(request_id)
    }

    fn storage_batch_write_async(
        &self,
        pid: u64,
        items: &[(&str, &[u8])],
    ) -> Result<StorageRequestId, HalError> {
        let mut requests = self.storage_requests.lock();
        if requests.len() >= MAX_PENDING_STORAGE_REQUESTS {
            return Err(HalError::ResourceExhausted);
        }

        let request_id = self.alloc_storage_request_id();

        // Perform synchronous batch write (x86_64 doesn't have true batch, just iterate)
        let mut success = true;
        for (key, value) in items {
            if storage::write(key, value).is_err() {
                success = false;
                break;
            }
        }

        requests.insert(request_id, (pid, StorageRequestState::WriteComplete(success)));
        Ok(request_id)
    }

    fn get_storage_request_pid(&self, request_id: StorageRequestId) -> Option<u64> {
        self.storage_requests.lock().get(&request_id).map(|(pid, _)| *pid)
    }

    fn take_storage_request_pid(&self, request_id: StorageRequestId) -> Option<u64> {
        self.storage_requests.lock().remove(&request_id).map(|(pid, _)| pid)
    }

    // === Bootstrap Storage (Supervisor Only) ===
    // These methods are for supervisor initialization before processes exist.

    fn bootstrap_storage_init(&self) -> Result<bool, HalError> {
        // Check if storage device is available
        if !virtio::blk::is_initialized() {
            return Err(HalError::NotSupported);
        }
        
        // Initialize storage layer
        match storage::init() {
            Ok(is_new) => {
                *self.storage_initialized.lock() = true;
                Ok(is_new)
            }
            Err(_) => Err(HalError::StorageError),
        }
    }

    fn bootstrap_storage_get_inode(&self, path: &str) -> Result<Option<Vec<u8>>, HalError> {
        if !*self.storage_initialized.lock() {
            return Err(HalError::NotSupported);
        }
        
        // Use path as key directly (inode data stored as JSON bytes)
        match storage::read(path) {
            Ok(data) => Ok(data),
            Err(_) => Err(HalError::StorageError),
        }
    }

    fn bootstrap_storage_put_inode(&self, path: &str, inode_json: &[u8]) -> Result<(), HalError> {
        if !*self.storage_initialized.lock() {
            return Err(HalError::NotSupported);
        }
        
        storage::write(path, inode_json).map_err(|_| HalError::StorageError)
    }

    fn bootstrap_storage_inode_count(&self) -> Result<u64, HalError> {
        if !*self.storage_initialized.lock() {
            return Err(HalError::NotSupported);
        }
        
        Ok(storage::count() as u64)
    }

    fn bootstrap_storage_clear(&self) -> Result<(), HalError> {
        if !*self.storage_initialized.lock() {
            return Err(HalError::NotSupported);
        }
        
        storage::clear().map_err(|_| HalError::StorageError)
    }

    // === Binary Loading (QEMU Native Runtime) ===

    fn load_binary(&self, name: &str) -> Result<&'static [u8], HalError> {
        match name {
            "init" => Ok(embedded_binaries::INIT),
            "permission" => Ok(embedded_binaries::PERMISSION),
            "vfs" => Ok(embedded_binaries::VFS),
            "keystore" => Ok(embedded_binaries::KEYSTORE),
            "identity" => Ok(embedded_binaries::IDENTITY),
            "time" => Ok(embedded_binaries::TIME),
            "terminal" => Ok(embedded_binaries::TERMINAL),
            "settings" => Ok(embedded_binaries::SETTINGS),
            "calculator" => Ok(embedded_binaries::CALCULATOR),
            "clock" => Ok(embedded_binaries::CLOCK),
            _ => {
                serial::write_str(&alloc::format!(
                    "[x86_64-hal] load_binary: '{}' not found\n", name
                ));
                Err(HalError::NotFound)
            }
        }
    }
}

/// Check if RDRAND instruction is supported
fn is_rdrand_supported() -> bool {
    // Use CPUID to check for RDRAND support (ECX bit 30 when EAX=1)
    // Note: We need to save/restore rbx since LLVM uses it internally
    let ecx: u32;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 1",
            "cpuid",
            "pop rbx",
            out("ecx") ecx,
            out("eax") _,
            out("edx") _,
            options(preserves_flags),
        );
    }
    ecx & (1 << 30) != 0
}

/// Read the Time Stamp Counter (TSC)
fn read_tsc() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nostack, preserves_flags),
        );
    }
    ((high as u64) << 32) | (low as u64)
}

/// Read a random 64-bit value using RDRAND instruction
///
/// Returns None if RDRAND is not supported or fails.
fn rdrand_u64() -> Option<u64> {
    let mut value: u64;
    let success: u8;
    
    unsafe {
        core::arch::asm!(
            "rdrand {0}",
            "setc {1}",
            out(reg) value,
            out(reg_byte) success,
            options(nostack),
        );
    }
    
    if success != 0 {
        Some(value)
    } else {
        None
    }
}

/// Halt the CPU in an infinite loop
///
/// This is used after kernel initialization or on fatal errors.
pub fn halt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

/// Exit QEMU with a success code
///
/// This uses the QEMU debug exit device (isa-debug-exit).
/// The exit code will be `(code << 1) | 1`, so:
/// - `exit_qemu(0)` produces exit code 1 (odd = success in our convention)
/// - `exit_qemu(1)` produces exit code 3
///
/// To use this, run QEMU with: `-device isa-debug-exit,iobase=0xf4,iosize=0x04`
pub fn exit_qemu(code: u32) -> ! {
    unsafe {
        // Write to the debug exit port (0xf4)
        x86_64::instructions::port::Port::new(0xf4).write(code);
    }
    // If QEMU doesn't have the debug-exit device, fall back to halt
    halt_loop()
}
