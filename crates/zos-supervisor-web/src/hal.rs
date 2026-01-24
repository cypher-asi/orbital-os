//! WASM HAL implementation for browser environment
//!
//! This module provides the Hardware Abstraction Layer for running Zero OS
//! in a web browser using Web Workers for process isolation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use zos_hal::{HalError, StorageRequestId, HAL};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, Worker};

use crate::worker::{self, PendingSyscall, WasmProcessHandle, WorkerMessage, WorkerProcess};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

// JS functions for async storage operations
// These are defined on window.ZosStorage and call IndexedDB
// The supervisor must call supervisor.init_storage() to set up the callbacks
#[wasm_bindgen]
extern "C" {
    /// Type alias for window object
    type Window;

    /// Get the global window object
    #[wasm_bindgen(js_name = "window", thread_local)]
    static WINDOW: Window;

    /// Start async storage read operation
    /// JS will call supervisor.notify_storage_read_complete when done
    #[wasm_bindgen(method, structural, js_namespace = ["ZosStorage"], js_name = "startRead")]
    fn zos_storage_start_read(this: &Window, request_id: u32, key: &str);

    /// Start async storage write operation
    #[wasm_bindgen(method, structural, js_namespace = ["ZosStorage"], js_name = "startWrite")]
    fn zos_storage_start_write(this: &Window, request_id: u32, key: &str, value: &[u8]);

    /// Start async storage delete operation
    #[wasm_bindgen(method, structural, js_namespace = ["ZosStorage"], js_name = "startDelete")]
    fn zos_storage_start_delete(this: &Window, request_id: u32, key: &str);

    /// Start async storage list operation
    #[wasm_bindgen(method, structural, js_namespace = ["ZosStorage"], js_name = "startList")]
    fn zos_storage_start_list(this: &Window, request_id: u32, prefix: &str);

    /// Start async storage exists check
    #[wasm_bindgen(method, structural, js_namespace = ["ZosStorage"], js_name = "startExists")]
    fn zos_storage_start_exists(this: &Window, request_id: u32, key: &str);
}

/// Helper functions to call ZosStorage methods
fn start_storage_read(request_id: u32, key: &str) {
    if let Some(window) = web_sys::window() {
        // Call window.ZosStorage.startRead(request_id, key)
        let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
        if let Some(storage) = zos_storage {
            if !storage.is_undefined() {
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startRead".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of2(&request_id.into(), &key.into()),
                );
                return;
            }
        }
    }
    log(&format!("[wasm-hal] ZosStorage.startRead not available for request_id={}", request_id));
}

fn start_storage_write(request_id: u32, key: &str, value: &[u8]) {
    if let Some(window) = web_sys::window() {
        let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
        if let Some(storage) = zos_storage {
            if !storage.is_undefined() {
                let value_array = js_sys::Uint8Array::from(value);
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startWrite".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of3(&request_id.into(), &key.into(), &value_array),
                );
                return;
            }
        }
    }
    log(&format!("[wasm-hal] ZosStorage.startWrite not available for request_id={}", request_id));
}

fn start_storage_delete(request_id: u32, key: &str) {
    if let Some(window) = web_sys::window() {
        let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
        if let Some(storage) = zos_storage {
            if !storage.is_undefined() {
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startDelete".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of2(&request_id.into(), &key.into()),
                );
                return;
            }
        }
    }
    log(&format!("[wasm-hal] ZosStorage.startDelete not available for request_id={}", request_id));
}

fn start_storage_list(request_id: u32, prefix: &str) {
    if let Some(window) = web_sys::window() {
        let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
        if let Some(storage) = zos_storage {
            if !storage.is_undefined() {
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startList".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of2(&request_id.into(), &prefix.into()),
                );
                return;
            }
        }
    }
    log(&format!("[wasm-hal] ZosStorage.startList not available for request_id={}", request_id));
}

fn start_storage_exists(request_id: u32, key: &str) {
    if let Some(window) = web_sys::window() {
        let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
        if let Some(storage) = zos_storage {
            if !storage.is_undefined() {
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startExists".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of2(&request_id.into(), &key.into()),
                );
                return;
            }
        }
    }
    log(&format!("[wasm-hal] ZosStorage.startExists not available for request_id={}", request_id));
}

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
        }
    }
    
    /// Generate a new unique storage request ID
    fn next_request_id(&self) -> StorageRequestId {
        self.next_storage_request_id.fetch_add(1, Ordering::SeqCst)
    }
    
    /// Record a pending storage request
    fn record_pending_request(&self, request_id: StorageRequestId, pid: u64) {
        if let Ok(mut pending) = self.pending_storage_requests.lock() {
            pending.insert(request_id, pid);
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

    /// Spawn a process with a specific PID (used when kernel assigns the PID)
    pub fn spawn_with_pid(
        &self,
        pid: u64,
        name: &str,
        binary: &[u8],
    ) -> Result<WasmProcessHandle, HalError> {
        let handle = WasmProcessHandle::new(pid);

        // Create the Web Worker
        let worker = Worker::new("/worker.js").map_err(|e| {
            log(&format!("[wasm-hal] Failed to create Worker: {:?}", e));
            HalError::ProcessSpawnFailed
        })?;

        // Set up message handler to receive shared memory from worker
        let processes = self.processes.clone();
        let onmessage_closure = Closure::wrap(Box::new(move |event: MessageEvent| {
            let data = event.data();

            // Check message type
            if let Ok(msg_type) = js_sys::Reflect::get(&data, &"type".into()) {
                if let Some(type_str) = msg_type.as_string() {
                    if type_str == "memory" {
                        // Worker is sending us its WASM memory buffer
                        // This should be a SharedArrayBuffer for atomics to work
                        if let Ok(buffer_val) = js_sys::Reflect::get(&data, &"buffer".into()) {
                            // Get PID from message
                            let pid = js_sys::Reflect::get(&data, &"pid".into())
                                .ok()
                                .and_then(|v| v.as_f64())
                                .map(|v| v as u64)
                                .unwrap_or(0);

                            // Get worker ID (browser-assigned memory context timestamp)
                            let worker_id = js_sys::Reflect::get(&data, &"workerId".into())
                                .ok()
                                .and_then(|v| v.as_f64())
                                .map(|v| v as u64)
                                .unwrap_or(0);

                            // Try SharedArrayBuffer first (required for atomics)
                            if let Ok(shared_buf) =
                                buffer_val.clone().dyn_into::<js_sys::SharedArrayBuffer>()
                            {
                                log(&format!(
                                    "[wasm-hal] SUCCESS: Received SharedArrayBuffer from worker:{} (PID {}), size: {} bytes",
                                    worker_id, pid, shared_buf.byte_length()
                                ));
                                let view = js_sys::Int32Array::new(&shared_buf);
                                if let Ok(mut procs) = processes.lock() {
                                    if let Some(proc) = procs.get_mut(&pid) {
                                        proc.syscall_buffer = shared_buf;
                                        proc.mailbox_view = view;
                                        proc.worker_id = worker_id;
                                    }
                                }
                            } else {
                                // Check if it's an ArrayBuffer (module exported non-shared memory)
                                let is_array_buffer = buffer_val
                                    .dyn_ref::<js_sys::ArrayBuffer>()
                                    .is_some();
                                if is_array_buffer {
                                    log(&format!(
                                        "[wasm-hal] ERROR: Worker:{} (PID {}) sent ArrayBuffer instead of SharedArrayBuffer. \
                                         WASM modules must import shared memory for atomics to work. \
                                         Rebuild with .cargo/config.toml having: rustflags = [\"-C\", \"link-args=--import-memory\"]",
                                        worker_id, pid
                                    ));
                                } else {
                                    log(&format!(
                                        "[wasm-hal] ERROR: Unknown buffer type from worker:{} (PID {})",
                                        worker_id, pid
                                    ));
                                }
                            }
                        }
                    } else if type_str == "error" {
                        if let Ok(err_val) = js_sys::Reflect::get(&data, &"error".into()) {
                            log(&format!("[wasm-hal] Worker error: {:?}", err_val));
                        }
                    }
                }
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        worker.set_onmessage(Some(onmessage_closure.as_ref().unchecked_ref()));

        // Set up error handler - use JsValue to avoid wasm-bindgen ErrorEvent.message() issue
        let onerror_closure = Closure::wrap(Box::new(move |event: JsValue| {
            // Safely try to get the message property
            let msg = js_sys::Reflect::get(&event, &"message".into())
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_else(|| "Unknown error".to_string());
            log(&format!("[wasm-hal] Worker {} error: {}", pid, msg));
        }) as Box<dyn FnMut(JsValue)>);

        worker.set_onerror(Some(onerror_closure.as_ref().unchecked_ref()));

        // Create a placeholder SharedArrayBuffer (will be replaced when worker sends real one)
        let syscall_buffer = js_sys::SharedArrayBuffer::new(4096);
        let mailbox_view = js_sys::Int32Array::new(&syscall_buffer);

        // Send init message with WASM binary and PID
        let init_msg = js_sys::Object::new();

        let binary_array = js_sys::Uint8Array::from(binary);
        js_sys::Reflect::set(&init_msg, &"binary".into(), &binary_array)
            .map_err(|_| HalError::ProcessSpawnFailed)?;
        js_sys::Reflect::set(&init_msg, &"pid".into(), &(pid as f64).into())
            .map_err(|_| HalError::ProcessSpawnFailed)?;

        worker
            .post_message(&init_msg)
            .map_err(|_| HalError::ProcessSpawnFailed)?;

        // Store the process (mailbox and worker_id will be updated when worker sends memory)
        let process = WorkerProcess {
            name: name.to_string(),
            worker,
            alive: true,
            memory_size: 65536, // Default, will be updated
            worker_id: 0,       // Will be set when worker sends its memory context ID
            syscall_buffer,
            mailbox_view,
            onerror_closure,
            _onmessage_closure: onmessage_closure,
        };

        if let Ok(mut procs) = self.processes.lock() {
            procs.insert(pid, process);
        }
        log(&format!(
            "[wasm-hal] Spawned Worker process '{}' with kernel PID {}",
            name, pid
        ));

        Ok(handle)
    }

    /// Send a message to a Worker
    pub(crate) fn post_to_worker(
        &self,
        pid: u64,
        msg_type: &str,
        data: Option<&[u8]>,
    ) -> Result<(), HalError> {
        let processes = self
            .processes
            .lock()
            .map_err(|_| HalError::ProcessNotFound)?;
        let proc = processes.get(&pid).ok_or(HalError::ProcessNotFound)?;

        if !proc.alive {
            return Err(HalError::ProcessNotFound);
        }

        // Create message object
        let msg = js_sys::Object::new();
        js_sys::Reflect::set(&msg, &"type".into(), &msg_type.into())
            .map_err(|_| HalError::InvalidMessage)?;
        js_sys::Reflect::set(&msg, &"pid".into(), &(pid as f64).into())
            .map_err(|_| HalError::InvalidMessage)?;

        if let Some(data) = data {
            let array = js_sys::Uint8Array::from(data);
            js_sys::Reflect::set(&msg, &"data".into(), &array)
                .map_err(|_| HalError::InvalidMessage)?;
        }

        proc.worker
            .post_message(&msg)
            .map_err(|_| HalError::InvalidMessage)?;

        Ok(())
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

                if data_len > 0 && data_len <= 4068 {
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
                    return;
                }

                let len = data.len().min(4068);

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
        // Delegate to spawn_with_pid
        self.spawn_with_pid(pid, name, binary)
    }

    fn kill_process(&self, handle: &Self::ProcessHandle) -> Result<(), HalError> {
        let mut processes = self
            .processes
            .lock()
            .map_err(|_| HalError::ProcessNotFound)?;
        
        // Remove the process entry entirely (instead of just marking alive=false)
        // This releases: SharedArrayBuffer, Int32Array view, closures
        if let Some(proc) = processes.remove(&handle.id) {
            if proc.alive {
                // Send terminate message then terminate Worker
                let term_msg = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&term_msg, &"type".into(), &"terminate".into());
                let _ = proc.worker.post_message(&term_msg);

                proc.worker.terminate();
                // proc is dropped here, releasing:
                // - syscall_buffer (SharedArrayBuffer)
                // - mailbox_view (Int32Array)
                // - onerror_closure, _onmessage_closure
                log(&format!(
                    "[wasm-hal] Killed and removed Worker process PID {}",
                    handle.id
                ));
                Ok(())
            } else {
                // Process was already dead but entry existed - still clean it up
                log(&format!(
                    "[wasm-hal] Removed already-dead process entry PID {}",
                    handle.id
                ));
                Ok(())
            }
        } else {
            Err(HalError::ProcessNotFound)
        }
    }

    fn send_to_process(&self, handle: &Self::ProcessHandle, msg: &[u8]) -> Result<(), HalError> {
        // Send IPC message to Worker
        self.post_to_worker(handle.id, "ipc", Some(msg))
    }

    fn is_process_alive(&self, handle: &Self::ProcessHandle) -> bool {
        self.processes
            .lock()
            .ok()
            .and_then(|p| p.get(&handle.id).map(|proc| proc.alive))
            .unwrap_or(false)
    }

    fn get_process_memory_size(&self, handle: &Self::ProcessHandle) -> Result<usize, HalError> {
        self.processes
            .lock()
            .map_err(|_| HalError::ProcessNotFound)?
            .get(&handle.id)
            .filter(|p| p.alive)
            .map(|p| p.memory_size)
            .ok_or(HalError::ProcessNotFound)
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

    fn deallocate(&self, ptr: *mut u8, size: usize, _align: usize) {
        if !ptr.is_null() {
            if let Ok(layout) = std::alloc::Layout::from_size_align(size, 8) {
                unsafe { std::alloc::dealloc(ptr, layout) };
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
        let request_id = self.next_request_id();
        self.record_pending_request(request_id, pid);
        
        log(&format!(
            "[wasm-hal] storage_read_async: request_id={}, pid={}, key={}",
            request_id, pid, key
        ));
        
        // Call JavaScript to start IndexedDB operation
        start_storage_read(request_id, key);
        
        Ok(request_id)
    }

    fn storage_write_async(&self, pid: u64, key: &str, value: &[u8]) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_request_id();
        self.record_pending_request(request_id, pid);
        
        log(&format!(
            "[wasm-hal] storage_write_async: request_id={}, pid={}, key={}, len={}",
            request_id, pid, key, value.len()
        ));
        
        // Call JavaScript to start IndexedDB operation
        start_storage_write(request_id, key, value);
        
        Ok(request_id)
    }

    fn storage_delete_async(&self, pid: u64, key: &str) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_request_id();
        self.record_pending_request(request_id, pid);
        
        log(&format!(
            "[wasm-hal] storage_delete_async: request_id={}, pid={}, key={}",
            request_id, pid, key
        ));
        
        // Call JavaScript to start IndexedDB operation
        start_storage_delete(request_id, key);
        
        Ok(request_id)
    }

    fn storage_list_async(&self, pid: u64, prefix: &str) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_request_id();
        self.record_pending_request(request_id, pid);
        
        log(&format!(
            "[wasm-hal] storage_list_async: request_id={}, pid={}, prefix={}",
            request_id, pid, prefix
        ));
        
        // Call JavaScript to start IndexedDB operation
        start_storage_list(request_id, prefix);
        
        Ok(request_id)
    }

    fn storage_exists_async(&self, pid: u64, key: &str) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_request_id();
        self.record_pending_request(request_id, pid);
        
        log(&format!(
            "[wasm-hal] storage_exists_async: request_id={}, pid={}, key={}",
            request_id, pid, key
        ));
        
        // Call JavaScript to start IndexedDB operation
        start_storage_exists(request_id, key);
        
        Ok(request_id)
    }

    fn get_storage_request_pid(&self, request_id: StorageRequestId) -> Option<u64> {
        self.pending_storage_requests
            .lock()
            .ok()
            .and_then(|pending| pending.get(&request_id).copied())
    }

    fn take_storage_request_pid(&self, request_id: StorageRequestId) -> Option<u64> {
        self.pending_storage_requests
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&request_id))
    }

    // === Bootstrap Storage (Supervisor Only) ===
    // These methods use ZosStorage's synchronous cache for reads.
    // For async operations (init, writes), use vfs module's async functions directly.

    fn bootstrap_storage_init(&self) -> Result<bool, HalError> {
        // Check if ZosStorage is available (init happens asynchronously via vfs::init)
        if let Some(window) = web_sys::window() {
            let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
            if let Some(storage) = zos_storage {
                if !storage.is_undefined() {
                    // Check if DB is initialized by looking for the db property
                    let db = js_sys::Reflect::get(&storage, &"db".into()).ok();
                    if let Some(db_val) = db {
                        return Ok(!db_val.is_null() && !db_val.is_undefined());
                    }
                }
            }
        }
        Ok(false)
    }

    fn bootstrap_storage_get_inode(&self, path: &str) -> Result<Option<Vec<u8>>, HalError> {
        // Use ZosStorage.getInodeSync from the in-memory cache
        if let Some(window) = web_sys::window() {
            let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
            if let Some(storage) = zos_storage {
                if !storage.is_undefined() {
                    // Call getInodeSync
                    let get_fn = js_sys::Reflect::get(&storage, &"getInodeSync".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
                    
                    if let Some(func) = get_fn {
                        let result = func.call1(&storage, &path.into()).ok();
                        if let Some(inode) = result {
                            if !inode.is_null() && !inode.is_undefined() {
                                // Serialize to JSON
                                let json = js_sys::JSON::stringify(&inode).ok();
                                if let Some(json_str) = json {
                                    let json_string: String = json_str.into();
                                    return Ok(Some(json_string.into_bytes()));
                                }
                            }
                        }
                        return Ok(None);
                    }
                }
            }
        }
        Err(HalError::NotSupported)
    }

    fn bootstrap_storage_put_inode(&self, path: &str, inode_json: &[u8]) -> Result<(), HalError> {
        // Parse JSON and call ZosStorage.putInode asynchronously via spawn_local
        // Note: This returns immediately; the actual write completes asynchronously.
        // For bootstrap where waiting is required, use vfs::putInode().await directly.
        if let Some(window) = web_sys::window() {
            let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
            if let Some(storage) = zos_storage {
                if !storage.is_undefined() {
                    // Parse the JSON
                    let json_str = String::from_utf8_lossy(inode_json);
                    let inode = js_sys::JSON::parse(&json_str).ok();
                    
                    if let Some(inode_obj) = inode {
                        // Call putInode (this is async but we fire-and-forget)
                        let put_fn = js_sys::Reflect::get(&storage, &"putInode".into())
                            .ok()
                            .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
                        
                        if let Some(func) = put_fn {
                            let _ = func.call2(&storage, &path.into(), &inode_obj);
                            return Ok(());
                        }
                    }
                }
            }
        }
        Err(HalError::NotSupported)
    }

    fn bootstrap_storage_inode_count(&self) -> Result<u64, HalError> {
        // Use ZosStorage.inodeCache.size from the in-memory cache
        if let Some(window) = web_sys::window() {
            let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
            if let Some(storage) = zos_storage {
                if !storage.is_undefined() {
                    // Get inodeCache.size
                    let cache = js_sys::Reflect::get(&storage, &"inodeCache".into()).ok();
                    if let Some(cache_obj) = cache {
                        let size = js_sys::Reflect::get(&cache_obj, &"size".into()).ok();
                        if let Some(size_val) = size {
                            if let Some(n) = size_val.as_f64() {
                                return Ok(n as u64);
                            }
                        }
                    }
                }
            }
        }
        Err(HalError::NotSupported)
    }

    fn bootstrap_storage_clear(&self) -> Result<(), HalError> {
        // Call ZosStorage.clear() asynchronously
        // Note: This returns immediately; the actual clear completes asynchronously.
        // For bootstrap where waiting is required, use vfs::clear().await directly.
        if let Some(window) = web_sys::window() {
            let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
            if let Some(storage) = zos_storage {
                if !storage.is_undefined() {
                    let clear_fn = js_sys::Reflect::get(&storage, &"clear".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
                    
                    if let Some(func) = clear_fn {
                        let _ = func.call0(&storage);
                        return Ok(());
                    }
                }
            }
        }
        Err(HalError::NotSupported)
    }
}
