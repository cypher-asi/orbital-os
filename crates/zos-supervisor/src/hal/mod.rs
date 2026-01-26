//! WASM HAL implementation for browser environment
//!
//! This module provides the Hardware Abstraction Layer for running Zero OS
//! in a web browser using Web Workers for process isolation.
//!
//! # Resource Limits
//!
//! To prevent unbounded memory growth from pending async operations:
//! - `MAX_PENDING_STORAGE_REQUESTS`: Maximum concurrent storage operations (1000)
//! - `MAX_PENDING_NETWORK_REQUESTS`: Maximum concurrent network operations (100)
//!
//! When limits are reached, new operations fail with `HalError::ResourceExhausted`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use zos_hal::{HalError, NetworkRequestId, StorageRequestId, HAL};

use crate::util::log;
use crate::worker::{self, PendingSyscall, WasmProcessHandle, WorkerMessage, WorkerProcess};

mod network;
mod process;
mod storage;

/// Maximum number of pending storage requests to prevent unbounded growth.
/// This is generous but prevents DoS from runaway processes.
const MAX_PENDING_STORAGE_REQUESTS: usize = 1000;

/// Maximum number of pending network requests to prevent unbounded growth.
/// Network requests are heavier, so limit is lower than storage.
const MAX_PENDING_NETWORK_REQUESTS: usize = 100;

/// Maximum number of pending keystore requests.
/// Key operations are similar to storage, use the same limit.
const MAX_PENDING_KEYSTORE_REQUESTS: usize = 1000;

/// WASM HAL implementation
///
/// This HAL runs in the browser and uses Web Workers for process isolation.
/// Each process runs in its own Worker with separate linear memory.
pub struct WasmHal {
    /// Next process ID to assign
    next_pid: AtomicU64,
    /// Worker processes (using Arc<Mutex> for HAL trait Send+Sync requirements)
    pub(crate) processes: Arc<Mutex<HashMap<u64, WorkerProcess>>>,
    /// Incoming messages from Workers (syscalls, status updates)
    incoming_messages: Arc<Mutex<Vec<WorkerMessage>>>,
    /// Next storage request ID (monotonically increasing)
    next_storage_request_id: AtomicU32,
    /// Pending storage requests: request_id -> requesting PID
    pending_storage_requests: Arc<Mutex<HashMap<u32, u64>>>,
    /// Next network request ID (monotonically increasing)
    next_network_request_id: AtomicU32,
    /// Pending network requests: request_id -> requesting PID
    pending_network_requests: Arc<Mutex<HashMap<u32, u64>>>,
    /// Next keystore request ID (monotonically increasing)
    next_keystore_request_id: AtomicU32,
    /// Pending keystore requests: request_id -> requesting PID
    pending_keystore_requests: Arc<Mutex<HashMap<u32, u64>>>,
}

impl WasmHal {
    /// Create a new WASM HAL
    pub fn new() -> Self {
        Self {
            next_pid: AtomicU64::new(1),
            processes: Arc::new(Mutex::new(HashMap::new())),
            incoming_messages: Arc::new(Mutex::new(Vec::new())),
            next_storage_request_id: AtomicU32::new(1),
            pending_storage_requests: Arc::new(Mutex::new(HashMap::new())),
            next_network_request_id: AtomicU32::new(1),
            pending_network_requests: Arc::new(Mutex::new(HashMap::new())),
            next_keystore_request_id: AtomicU32::new(1),
            pending_keystore_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Generate a new unique storage request ID
    fn next_request_id(&self) -> StorageRequestId {
        self.next_storage_request_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Generate a new unique network request ID
    fn next_network_request_id(&self) -> u32 {
        self.next_network_request_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Record a pending storage request with bounded limit enforcement.
    ///
    /// Returns true if the request was recorded, false if the limit was reached.
    fn record_pending_request(&self, request_id: StorageRequestId, pid: u64) -> bool {
        if let Ok(mut pending) = self.pending_storage_requests.lock() {
            if pending.len() >= MAX_PENDING_STORAGE_REQUESTS {
                log(&format!(
                    "[wasm-hal] ERROR: Pending storage request limit reached ({}) - rejecting request_id={} from PID {}",
                    MAX_PENDING_STORAGE_REQUESTS, request_id, pid
                ));
                return false;
            }
            pending.insert(request_id, pid);
            true
        } else {
            false
        }
    }

    /// Record a pending network request with bounded limit enforcement.
    ///
    /// Returns true if the request was recorded, false if the limit was reached.
    fn record_pending_network_request(&self, request_id: u32, pid: u64) -> bool {
        if let Ok(mut pending) = self.pending_network_requests.lock() {
            if pending.len() >= MAX_PENDING_NETWORK_REQUESTS {
                log(&format!(
                    "[wasm-hal] ERROR: Pending network request limit reached ({}) - rejecting request_id={} from PID {}",
                    MAX_PENDING_NETWORK_REQUESTS, request_id, pid
                ));
                return false;
            }
            pending.insert(request_id, pid);
            true
        } else {
            false
        }
    }

    /// Generate a new unique keystore request ID
    fn next_keystore_request_id(&self) -> u32 {
        self.next_keystore_request_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Record a pending keystore request with bounded limit enforcement.
    ///
    /// Returns true if the request was recorded, false if the limit was reached.
    fn record_pending_keystore_request(&self, request_id: u32, pid: u64) -> bool {
        if let Ok(mut pending) = self.pending_keystore_requests.lock() {
            if pending.len() >= MAX_PENDING_KEYSTORE_REQUESTS {
                log(&format!(
                    "[wasm-hal] ERROR: Pending keystore request limit reached ({}) - rejecting request_id={} from PID {}",
                    MAX_PENDING_KEYSTORE_REQUESTS, request_id, pid
                ));
                return false;
            }
            pending.insert(request_id, pid);
            true
        } else {
            false
        }
    }

    /// Get a clone of the incoming messages queue Arc
    pub fn incoming_messages(&self) -> Arc<Mutex<Vec<WorkerMessage>>> {
        self.incoming_messages.clone()
    }

    /// Update memory size for a process (called when Worker reports memory)
    pub fn update_process_memory(&self, pid: u64, memory_size: usize) {
        if let Ok(mut processes) = self.processes.lock() {
            if let Some(proc) = processes.get_mut(&pid) {
                proc.memory_size = memory_size;
            }
        }
    }

    /// Get the worker memory context ID for a process
    /// Returns the browser-assigned performance.timeOrigin value, or 0 if not a real worker
    pub fn get_worker_id(&self, pid: u64) -> u64 {
        self.processes
            .lock()
            .ok()
            .and_then(|p| p.get(&pid).map(|proc| proc.worker_id))
            .unwrap_or(0)
    }

    /// Poll all workers for pending syscalls
    ///
    /// This checks each worker's SharedArrayBuffer mailbox for pending syscalls.
    /// Returns a list of syscalls that need to be processed.
    ///
    /// NOTE: Workers must have sent their real SharedArrayBuffer via postMessage
    /// before we can poll them. We detect this by checking worker_id != 0.
    /// Until then, we'd be checking a placeholder buffer that the worker isn't using.
    pub fn poll_syscalls(&self) -> Vec<PendingSyscall> {
        let mut result = Vec::new();

        if let Ok(processes) = self.processes.lock() {
            for (&pid, proc) in processes.iter() {
                if !proc.alive {
                    continue;
                }

                // Skip workers that haven't sent their real SharedArrayBuffer yet.
                // worker_id is 0 until the worker sends its memory via postMessage.
                // Without this check, we'd be polling a placeholder buffer that
                // the worker isn't actually using, causing syscalls to hang.
                if proc.worker_id == 0 {
                    continue;
                }

                // Read status atomically
                let status = js_sys::Atomics::load(&proc.mailbox_view, worker::MAILBOX_STATUS)
                    .unwrap_or(worker::STATUS_IDLE);

                if status == worker::STATUS_PENDING {
                    // Read syscall parameters
                    let syscall_num =
                        js_sys::Atomics::load(&proc.mailbox_view, worker::MAILBOX_SYSCALL_NUM)
                            .unwrap_or(0) as u32;
                    let arg0 = js_sys::Atomics::load(&proc.mailbox_view, worker::MAILBOX_ARG0)
                        .unwrap_or(0) as u32;
                    let arg1 = js_sys::Atomics::load(&proc.mailbox_view, worker::MAILBOX_ARG1)
                        .unwrap_or(0) as u32;
                    let arg2 = js_sys::Atomics::load(&proc.mailbox_view, worker::MAILBOX_ARG2)
                        .unwrap_or(0) as u32;

                    result.push(PendingSyscall {
                        pid,
                        syscall_num,
                        args: [arg0, arg1, arg2],
                    });
                }
            }
        }

        result
    }

    /// Complete a syscall and wake the waiting worker
    ///
    /// Writes the result to the worker's mailbox and notifies it.
    pub fn complete_syscall(&self, pid: u64, result: i32) {
        if let Ok(processes) = self.processes.lock() {
            if let Some(proc) = processes.get(&pid) {
                // Only write to workers that have sent their real buffer
                if proc.worker_id == 0 {
                    log(&format!(
                        "[wasm-hal] Warning: complete_syscall for PID {} but worker_id is 0",
                        pid
                    ));
                    return;
                }

                // Write result
                let _ = js_sys::Atomics::store(&proc.mailbox_view, worker::MAILBOX_RESULT, result);

                // Set status to READY
                let _ = js_sys::Atomics::store(
                    &proc.mailbox_view,
                    worker::MAILBOX_STATUS,
                    worker::STATUS_READY,
                );

                // Wake the waiting worker
                let _ = js_sys::Atomics::notify(&proc.mailbox_view, worker::MAILBOX_STATUS);
            }
        }
    }

    /// Read data from a worker's syscall mailbox
    pub fn read_syscall_data(&self, pid: u64) -> Vec<u8> {
        let mut data = Vec::new();

        if let Ok(processes) = self.processes.lock() {
            if let Some(proc) = processes.get(&pid) {
                // Only read from workers that have sent their real buffer
                if proc.worker_id == 0 {
                    return data;
                }

                // Read data length
                let data_len = js_sys::Atomics::load(&proc.mailbox_view, worker::MAILBOX_DATA_LEN)
                    .unwrap_or(0) as usize;

                if data_len > 0 && data_len <= 16356 {
                    // Create a Uint8Array view starting at byte offset 28
                    let data_view = js_sys::Uint8Array::new_with_byte_offset_and_length(
                        &proc.syscall_buffer,
                        28,
                        data_len as u32,
                    );
                    data = data_view.to_vec();
                }
            }
        }

        data
    }

    /// Write data to a worker's syscall result buffer
    pub fn write_syscall_data(&self, pid: u64, data: &[u8]) {
        if let Ok(processes) = self.processes.lock() {
            if let Some(proc) = processes.get(&pid) {
                // Only write to workers that have sent their real buffer
                if proc.worker_id == 0 {
                    if pid == 1 && data.len() > 0 {
                        log(&format!(
                            "[wasm-hal] ERROR: Cannot write {} bytes to Init (PID 1) - worker_id is 0! \
                             SharedArrayBuffer not registered yet.",
                            data.len()
                        ));
                    }
                    return;
                }

                let len = data.len().min(16356);

                // Create a Uint8Array view starting at byte offset 28
                let data_view = js_sys::Uint8Array::new_with_byte_offset_and_length(
                    &proc.syscall_buffer,
                    28,
                    len as u32,
                );
                data_view.copy_from(&data[..len]);

                // Store data length
                let _ = js_sys::Atomics::store(
                    &proc.mailbox_view,
                    worker::MAILBOX_DATA_LEN,
                    len as i32,
                );
            }
        }
    }
}

impl Default for WasmHal {
    fn default() -> Self {
        Self::new()
    }
}

impl HAL for WasmHal {
    type ProcessHandle = WasmProcessHandle;

    fn spawn_process(&self, name: &str, binary: &[u8]) -> Result<Self::ProcessHandle, HalError> {
        let pid = self.next_pid.fetch_add(1, Ordering::SeqCst);
        // Delegate to spawn_with_pid (defined in process module)
        self.spawn_with_pid(pid, name, binary)
    }

    fn kill_process(&self, handle: &Self::ProcessHandle) -> Result<(), HalError> {
        self.do_kill_process(handle)
    }

    fn send_to_process(&self, handle: &Self::ProcessHandle, msg: &[u8]) -> Result<(), HalError> {
        self.do_send_to_process(handle, msg)
    }

    fn is_process_alive(&self, handle: &Self::ProcessHandle) -> bool {
        self.do_is_process_alive(handle)
    }

    fn get_process_memory_size(&self, handle: &Self::ProcessHandle) -> Result<usize, HalError> {
        self.do_get_process_memory_size(handle)
    }

    fn allocate(&self, size: usize, _align: usize) -> Result<*mut u8, HalError> {
        // In WASM, use the standard allocator
        let layout =
            std::alloc::Layout::from_size_align(size, 8).map_err(|_| HalError::InvalidArgument)?;
        let ptr = unsafe { std::alloc::alloc(layout) };
        if ptr.is_null() {
            Err(HalError::OutOfMemory)
        } else {
            Ok(ptr)
        }
    }

    unsafe fn deallocate(&self, ptr: *mut u8, size: usize, _align: usize) {
        if !ptr.is_null() {
            if let Ok(layout) = std::alloc::Layout::from_size_align(size, 8) {
                std::alloc::dealloc(ptr, layout);
            }
        }
    }

    fn now_nanos(&self) -> u64 {
        // Use performance.now() from web_sys
        if let Some(window) = web_sys::window() {
            if let Some(performance) = window.performance() {
                let millis = performance.now();
                return (millis * 1_000_000.0) as u64;
            }
        }
        0
    }

    fn wallclock_ms(&self) -> u64 {
        // Use Date.now() from js_sys
        js_sys::Date::now() as u64
    }

    fn random_bytes(&self, buf: &mut [u8]) -> Result<(), HalError> {
        // Use Web Crypto API
        if let Some(window) = web_sys::window() {
            if let Ok(crypto) = window.crypto() {
                return crypto
                    .get_random_values_with_u8_array(buf)
                    .map(|_| ())
                    .map_err(|_| HalError::NotSupported);
            }
        }
        Err(HalError::NotSupported)
    }

    fn debug_write(&self, msg: &str) {
        log(msg);
    }

    fn poll_messages(&self) -> Vec<(Self::ProcessHandle, Vec<u8>)> {
        // Return raw messages - caller processes them
        if let Ok(mut incoming) = self.incoming_messages.lock() {
            incoming
                .drain(..)
                .map(|msg| {
                    let handle = WasmProcessHandle::new(msg.pid);
                    let data = worker::serialize_worker_message(&msg);
                    (handle, data)
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    // === Async Platform Storage ===

    fn storage_read_async(&self, pid: u64, key: &str) -> Result<StorageRequestId, HalError> {
        self.do_storage_read_async(pid, key)
    }

    fn storage_write_async(
        &self,
        pid: u64,
        key: &str,
        value: &[u8],
    ) -> Result<StorageRequestId, HalError> {
        self.do_storage_write_async(pid, key, value)
    }

    fn storage_delete_async(&self, pid: u64, key: &str) -> Result<StorageRequestId, HalError> {
        self.do_storage_delete_async(pid, key)
    }

    fn storage_list_async(&self, pid: u64, prefix: &str) -> Result<StorageRequestId, HalError> {
        self.do_storage_list_async(pid, prefix)
    }

    fn storage_exists_async(&self, pid: u64, key: &str) -> Result<StorageRequestId, HalError> {
        self.do_storage_exists_async(pid, key)
    }

    fn get_storage_request_pid(&self, request_id: StorageRequestId) -> Option<u64> {
        self.do_get_storage_request_pid(request_id)
    }

    fn take_storage_request_pid(&self, request_id: StorageRequestId) -> Option<u64> {
        self.do_take_storage_request_pid(request_id)
    }

    // === Async Keystore (KeyService Only) ===

    fn keystore_read_async(&self, pid: u64, key: &str) -> Result<StorageRequestId, HalError> {
        self.do_keystore_read_async(pid, key)
    }

    fn keystore_write_async(
        &self,
        pid: u64,
        key: &str,
        value: &[u8],
    ) -> Result<StorageRequestId, HalError> {
        self.do_keystore_write_async(pid, key, value)
    }

    fn keystore_delete_async(&self, pid: u64, key: &str) -> Result<StorageRequestId, HalError> {
        self.do_keystore_delete_async(pid, key)
    }

    fn keystore_list_async(&self, pid: u64, prefix: &str) -> Result<StorageRequestId, HalError> {
        self.do_keystore_list_async(pid, prefix)
    }

    fn keystore_exists_async(&self, pid: u64, key: &str) -> Result<StorageRequestId, HalError> {
        self.do_keystore_exists_async(pid, key)
    }

    fn get_keystore_request_pid(&self, request_id: StorageRequestId) -> Option<u64> {
        self.do_get_keystore_request_pid(request_id)
    }

    fn take_keystore_request_pid(&self, request_id: StorageRequestId) -> Option<u64> {
        self.do_take_keystore_request_pid(request_id)
    }

    // === Bootstrap Storage (Supervisor Only) ===

    fn bootstrap_storage_init(&self) -> Result<bool, HalError> {
        self.do_bootstrap_storage_init()
    }

    fn bootstrap_storage_get_inode(&self, path: &str) -> Result<Option<Vec<u8>>, HalError> {
        self.do_bootstrap_storage_get_inode(path)
    }

    fn bootstrap_storage_put_inode(&self, path: &str, inode_json: &[u8]) -> Result<(), HalError> {
        self.do_bootstrap_storage_put_inode(path, inode_json)
    }

    fn bootstrap_storage_inode_count(&self) -> Result<u64, HalError> {
        self.do_bootstrap_storage_inode_count()
    }

    fn bootstrap_storage_clear(&self) -> Result<(), HalError> {
        self.do_bootstrap_storage_clear()
    }

    // === Async Network Operations ===

    fn network_fetch_async(&self, pid: u64, request: &[u8]) -> Result<NetworkRequestId, HalError> {
        self.do_network_fetch_async(pid, request)
    }

    fn get_network_request_pid(&self, request_id: NetworkRequestId) -> Option<u64> {
        self.do_get_network_request_pid(request_id)
    }

    fn take_network_request_pid(&self, request_id: NetworkRequestId) -> Option<u64> {
        self.do_take_network_request_pid(request_id)
    }
}
