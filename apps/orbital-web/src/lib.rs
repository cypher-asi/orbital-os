//! Browser-based Supervisor for Orbital OS
//!
//! This crate runs in the browser's main thread and acts as the kernel
//! supervisor. It manages Web Workers (processes) and routes IPC messages.

pub mod desktop;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use orbital_hal::{HAL, HalError, ProcessMessageType};
use orbital_kernel::{Kernel, ProcessId, Syscall, SyscallResult};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, Worker};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);

    /// Get current date/time as ISO string
    #[wasm_bindgen(js_namespace = Date, js_name = now)]
    fn date_now() -> f64;
}

// =============================================================================
// Axiom Storage - IndexedDB persistence for WASM targets
// =============================================================================

#[wasm_bindgen]
extern "C" {
    /// AxiomStorage JavaScript object for IndexedDB persistence
    #[wasm_bindgen(js_namespace = AxiomStorage)]
    async fn init() -> JsValue;

    #[wasm_bindgen(js_namespace = AxiomStorage)]
    async fn persistEntry(entry: JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = AxiomStorage)]
    async fn persistEntries(entries: JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = AxiomStorage)]
    async fn loadAll() -> JsValue;

    #[wasm_bindgen(js_namespace = AxiomStorage)]
    async fn getCount() -> JsValue;

    #[wasm_bindgen(js_namespace = AxiomStorage)]
    async fn getLastSeq() -> JsValue;

    #[wasm_bindgen(js_namespace = AxiomStorage)]
    async fn clear() -> JsValue;
}

/// Serialize a Commit entry to a JavaScript object for IndexedDB storage
fn commit_to_js(commit: &orbital_kernel::Commit) -> JsValue {
    let obj = js_sys::Object::new();
    
    // seq: u64 (as number - safe up to 2^53)
    let _ = js_sys::Reflect::set(&obj, &"seq".into(), &JsValue::from_f64(commit.seq as f64));
    
    // timestamp: u64 (as number)
    let _ = js_sys::Reflect::set(&obj, &"timestamp".into(), &JsValue::from_f64(commit.timestamp as f64));
    
    // id: [u8; 32] (as hex string)
    let id_hex: String = commit.id.iter().map(|b| format!("{:02x}", b)).collect();
    let _ = js_sys::Reflect::set(&obj, &"id".into(), &JsValue::from_str(&id_hex));
    
    // prev_commit: [u8; 32] (as hex string)
    let prev_hex: String = commit.prev_commit.iter().map(|b| format!("{:02x}", b)).collect();
    let _ = js_sys::Reflect::set(&obj, &"prev_commit".into(), &JsValue::from_str(&prev_hex));
    
    // commit_type: CommitType (as type string)
    let commit_type = commit_type_to_string(&commit.commit_type);
    let _ = js_sys::Reflect::set(&obj, &"commit_type".into(), &JsValue::from_str(&commit_type));
    
    obj.into()
}

/// Convert CommitType to a human-readable string
fn commit_type_to_string(ct: &orbital_kernel::CommitType) -> String {
    match ct {
        orbital_kernel::CommitType::Genesis => "Genesis".to_string(),
        orbital_kernel::CommitType::ProcessCreated { pid, parent, name } => 
            format!("ProcessCreated(pid={}, parent={}, name={})", pid, parent, name),
        orbital_kernel::CommitType::ProcessExited { pid, code } => 
            format!("ProcessExited(pid={}, code={})", pid, code),
        orbital_kernel::CommitType::CapInserted { pid, slot, cap_id, .. } => 
            format!("CapInserted(pid={}, slot={}, cap={})", pid, slot, cap_id),
        orbital_kernel::CommitType::CapRemoved { pid, slot } => 
            format!("CapRemoved(pid={}, slot={})", pid, slot),
        orbital_kernel::CommitType::CapGranted { from_pid, to_pid, from_slot, to_slot, .. } => 
            format!("CapGranted(from={}.{} to={}.{})", from_pid, from_slot, to_pid, to_slot),
        orbital_kernel::CommitType::EndpointCreated { id, owner } => 
            format!("EndpointCreated(id={}, owner={})", id, owner),
        orbital_kernel::CommitType::EndpointDestroyed { id } => 
            format!("EndpointDestroyed(id={})", id),
    }
}

/// Get short commit type name for dashboard display
fn commit_type_short(ct: &orbital_kernel::CommitType) -> &'static str {
    match ct {
        orbital_kernel::CommitType::Genesis => "Genesis",
        orbital_kernel::CommitType::ProcessCreated { .. } => "ProcCreate",
        orbital_kernel::CommitType::ProcessExited { .. } => "ProcExit",
        orbital_kernel::CommitType::CapInserted { .. } => "CapInsert",
        orbital_kernel::CommitType::CapRemoved { .. } => "CapRemove",
        orbital_kernel::CommitType::CapGranted { .. } => "CapGrant",
        orbital_kernel::CommitType::EndpointCreated { .. } => "EpCreate",
        orbital_kernel::CommitType::EndpointDestroyed { .. } => "EpDestroy",
    }
}

/// Process handle for WASM - wraps a Web Worker
#[derive(Clone, Debug)]
pub struct WasmProcessHandle {
    /// Process ID assigned by the HAL
    pub id: u64,
}

impl WasmProcessHandle {
    fn new(id: u64) -> Self {
        Self { id }
    }
}

// Implement Send + Sync for WasmProcessHandle
// In WASM, there's only one thread, so this is safe
unsafe impl Send for WasmProcessHandle {}
unsafe impl Sync for WasmProcessHandle {}

/// Internal state for a real Web Worker process
struct WorkerProcess {
    /// Human-readable name
    #[allow(dead_code)]
    name: String,
    /// The actual Web Worker
    worker: Worker,
    /// Whether the process is still alive
    alive: bool,
    /// Memory size reported by the Worker
    memory_size: usize,
    /// Worker memory context ID (performance.timeOrigin from browser)
    /// Format: worker:<timestamp> e.g., "worker:1737158400123"
    worker_id: u64,
    /// SharedArrayBuffer for syscall mailbox (worker's WASM memory)
    syscall_buffer: js_sys::SharedArrayBuffer,
    /// Int32Array view for atomic operations on the mailbox
    mailbox_view: js_sys::Int32Array,
    /// Closures must be stored to prevent garbage collection
    #[allow(dead_code)]
    onerror_closure: Closure<dyn FnMut(JsValue)>,
    #[allow(dead_code)]
    _onmessage_closure: Closure<dyn FnMut(MessageEvent)>,
}

// Safety: In WASM, there is only one thread. These types contain JS references
// that are not Send/Sync in the general case, but are safe in single-threaded WASM.
unsafe impl Send for WorkerProcess {}
unsafe impl Sync for WorkerProcess {}

// Mailbox status values (must match orbital-wasm-rt)
const STATUS_IDLE: i32 = 0;
const STATUS_PENDING: i32 = 1;
const STATUS_READY: i32 = 2;

// Mailbox field offsets in i32 units (must match orbital-wasm-rt)
const MAILBOX_STATUS: u32 = 0;
const MAILBOX_SYSCALL_NUM: u32 = 1;
const MAILBOX_ARG0: u32 = 2;
const MAILBOX_ARG1: u32 = 3;
const MAILBOX_ARG2: u32 = 4;
const MAILBOX_RESULT: u32 = 5;
const MAILBOX_DATA_LEN: u32 = 6;
// MAILBOX_DATA starts at offset 7 (byte offset 28)

/// Pending syscall from a worker process
#[derive(Clone, Debug)]
pub struct PendingSyscall {
    pub pid: u64,
    pub syscall_num: u32,
    pub args: [u32; 3],
}

/// Incoming message from a Worker (syscall or status update)
#[derive(Clone, Debug)]
pub struct WorkerMessage {
    pub pid: u64,
    pub msg_type: WorkerMessageType,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum WorkerMessageType {
    Ready { memory_size: usize },
    MemoryUpdate { memory_size: usize },
    Syscall { syscall_num: u32, args: [u32; 3] },
    Error { message: String },
    Terminated,
    Yield,
}

/// WASM HAL implementation
/// 
/// This HAL runs in the browser and uses Web Workers for process isolation.
/// Each process runs in its own Worker with separate linear memory.
pub struct WasmHal {
    /// Next process ID to assign
    next_pid: AtomicU64,
    /// Worker processes (using Arc<Mutex> for HAL trait Send+Sync requirements)
    processes: Arc<Mutex<HashMap<u64, WorkerProcess>>>,
    /// Incoming messages from Workers (syscalls, status updates)
    incoming_messages: Arc<Mutex<Vec<WorkerMessage>>>,
}

impl WasmHal {
    /// Create a new WASM HAL
    pub fn new() -> Self {
        Self {
            next_pid: AtomicU64::new(1),
            processes: Arc::new(Mutex::new(HashMap::new())),
            incoming_messages: Arc::new(Mutex::new(Vec::new())),
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
    pub fn spawn_with_pid(&self, pid: u64, name: &str, binary: &[u8]) -> Result<WasmProcessHandle, HalError> {
        let handle = WasmProcessHandle::new(pid);

        // Create the Web Worker
        let worker = Worker::new("worker.js")
            .map_err(|e| {
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
                        // Worker is sending us its shared memory and worker ID
                        if let Ok(buffer_val) = js_sys::Reflect::get(&data, &"buffer".into()) {
                            if let Ok(buffer) = buffer_val.dyn_into::<js_sys::SharedArrayBuffer>() {
                                // Get PID from message
                                if let Ok(pid_val) = js_sys::Reflect::get(&data, &"pid".into()) {
                                    let pid = pid_val.as_f64().unwrap_or(0.0) as u64;
                                    
                                    // Get worker ID (browser-assigned memory context timestamp)
                                    let worker_id = js_sys::Reflect::get(&data, &"workerId".into())
                                        .ok()
                                        .and_then(|v| v.as_f64())
                                        .map(|v| v as u64)
                                        .unwrap_or(0);
                                    
                                    // Update the process with the shared memory and worker ID
                                    if let Ok(mut procs) = processes.lock() {
                                        if let Some(proc) = procs.get_mut(&pid) {
                                            proc.syscall_buffer = buffer.clone();
                                            proc.mailbox_view = js_sys::Int32Array::new(&buffer);
                                            proc.worker_id = worker_id;
                                            log(&format!("[wasm-hal] Received shared memory from worker:{} (PID {})", worker_id, pid));
                                        }
                                    }
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

        worker.post_message(&init_msg)
            .map_err(|_| HalError::ProcessSpawnFailed)?;

        // Store the process (mailbox and worker_id will be updated when worker sends memory)
        let process = WorkerProcess {
            name: name.to_string(),
            worker,
            alive: true,
            memory_size: 65536, // Default, will be updated
            worker_id: 0, // Will be set when worker sends its memory context ID
            syscall_buffer,
            mailbox_view,
            onerror_closure,
            _onmessage_closure: onmessage_closure,
        };

        self.processes.lock().unwrap().insert(pid, process);
        log(&format!("[wasm-hal] Spawned Worker process '{}' with kernel PID {}", name, pid));

        Ok(handle)
    }

    /// Send a message to a Worker
    fn post_to_worker(&self, pid: u64, msg_type: &str, data: Option<&[u8]>) -> Result<(), HalError> {
        let processes = self.processes.lock().map_err(|_| HalError::ProcessNotFound)?;
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
                let status = js_sys::Atomics::load(&proc.mailbox_view, MAILBOX_STATUS)
                    .unwrap_or(STATUS_IDLE);
                
                if status == STATUS_PENDING {
                    // Read syscall parameters
                    let syscall_num = js_sys::Atomics::load(&proc.mailbox_view, MAILBOX_SYSCALL_NUM)
                        .unwrap_or(0) as u32;
                    let arg0 = js_sys::Atomics::load(&proc.mailbox_view, MAILBOX_ARG0)
                        .unwrap_or(0) as u32;
                    let arg1 = js_sys::Atomics::load(&proc.mailbox_view, MAILBOX_ARG1)
                        .unwrap_or(0) as u32;
                    let arg2 = js_sys::Atomics::load(&proc.mailbox_view, MAILBOX_ARG2)
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
                    log(&format!("[wasm-hal] Warning: complete_syscall for PID {} but worker_id is 0", pid));
                    return;
                }
                
                // Write result
                let _ = js_sys::Atomics::store(&proc.mailbox_view, MAILBOX_RESULT, result);
                
                // Set status to READY
                let _ = js_sys::Atomics::store(&proc.mailbox_view, MAILBOX_STATUS, STATUS_READY);
                
                // Wake the waiting worker
                let _ = js_sys::Atomics::notify(&proc.mailbox_view, MAILBOX_STATUS);
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
                let data_len = js_sys::Atomics::load(&proc.mailbox_view, MAILBOX_DATA_LEN)
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
                let _ = js_sys::Atomics::store(&proc.mailbox_view, MAILBOX_DATA_LEN, len as i32);
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
        let mut processes = self.processes.lock().unwrap();
        if let Some(proc) = processes.get_mut(&handle.id) {
            if proc.alive {
                // Send terminate message then terminate Worker
                let term_msg = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&term_msg, &"type".into(), &"terminate".into());
                let _ = proc.worker.post_message(&term_msg);
                
                proc.worker.terminate();
                proc.alive = false;
                log(&format!("[wasm-hal] Killed Worker process PID {}", handle.id));
                Ok(())
            } else {
                Err(HalError::ProcessNotFound)
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
        let layout = std::alloc::Layout::from_size_align(size, 8)
            .map_err(|_| HalError::InvalidArgument)?;
        let ptr = unsafe { std::alloc::alloc(layout) };
        if ptr.is_null() {
            Err(HalError::OutOfMemory)
        } else {
            Ok(ptr)
        }
    }

    fn deallocate(&self, ptr: *mut u8, size: usize, _align: usize) {
        if !ptr.is_null() {
            let layout = std::alloc::Layout::from_size_align(size, 8).unwrap();
            unsafe { std::alloc::dealloc(ptr, layout) };
        }
    }

    fn now_nanos(&self) -> u64 {
        // Use performance.now() from web_sys
        let window = web_sys::window().expect("no window");
        let performance = window.performance().expect("no performance");
        let millis = performance.now();
        (millis * 1_000_000.0) as u64
    }

    fn random_bytes(&self, buf: &mut [u8]) -> Result<(), HalError> {
        // Use Web Crypto API
        let window = web_sys::window().expect("no window");
        let crypto = window.crypto().expect("no crypto");
        crypto
            .get_random_values_with_u8_array(buf)
            .map_err(|_| HalError::NotSupported)?;
        Ok(())
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
                    let data = serialize_worker_message(&msg);
                    (handle, data)
                })
                .collect()
        } else {
            Vec::new()
        }
    }
}

/// Parse a message received from a Worker
fn parse_worker_message(pid: u64, event: &MessageEvent) -> Result<WorkerMessage, JsValue> {
    let data = event.data();
    
    // Handle object messages
    if data.is_object() {
        let obj = data.dyn_ref::<js_sys::Object>().unwrap();
        
        let msg_type = js_sys::Reflect::get(obj, &"type".into())?
            .as_string()
            .unwrap_or_default();

        match msg_type.as_str() {
            "ready" => {
                let memory_size = js_sys::Reflect::get(obj, &"memory_size".into())
                    .ok()
                    .and_then(|v| v.as_f64())
                    .map(|v| v as usize)
                    .unwrap_or(65536);
                
                log(&format!("[wasm-hal] Worker {} ready, memory: {} bytes", pid, memory_size));
                
                Ok(WorkerMessage {
                    pid,
                    msg_type: WorkerMessageType::Ready { memory_size },
                    data: Vec::new(),
                })
            }
            "syscall" => {
                let syscall_num = js_sys::Reflect::get(obj, &"syscall".into())
                    .ok()
                    .and_then(|v| v.as_f64())
                    .map(|v| v as u32)
                    .unwrap_or(0);
                
                let args_val = js_sys::Reflect::get(obj, &"args".into())?;
                let args_array = args_val.dyn_ref::<js_sys::Array>().unwrap();
                let args = [
                    args_array.get(0).as_f64().unwrap_or(0.0) as u32,
                    args_array.get(1).as_f64().unwrap_or(0.0) as u32,
                    args_array.get(2).as_f64().unwrap_or(0.0) as u32,
                ];
                
                // Extract data bytes if present
                let data = js_sys::Reflect::get(obj, &"data".into())
                    .ok()
                    .and_then(|v| v.dyn_ref::<js_sys::Uint8Array>().map(|a| a.to_vec()))
                    .unwrap_or_default();

                Ok(WorkerMessage {
                    pid,
                    msg_type: WorkerMessageType::Syscall { syscall_num, args },
                    data,
                })
            }
            "error" => {
                let message = js_sys::Reflect::get(obj, &"error".into())
                    .ok()
                    .and_then(|v| v.as_string())
                    .unwrap_or_else(|| "Unknown error".to_string());
                
                Ok(WorkerMessage {
                    pid,
                    msg_type: WorkerMessageType::Error { message },
                    data: Vec::new(),
                })
            }
            "terminated" => {
                Ok(WorkerMessage {
                    pid,
                    msg_type: WorkerMessageType::Terminated,
                    data: Vec::new(),
                })
            }
            "worker_loaded" => {
                // Worker script loaded, waiting for init
                log(&format!("[wasm-hal] Worker {} script loaded", pid));
                Ok(WorkerMessage {
                    pid,
                    msg_type: WorkerMessageType::Ready { memory_size: 0 },
                    data: Vec::new(),
                })
            }
            "memory_update" => {
                // Memory size update from worker
                let memory_size = js_sys::Reflect::get(obj, &"memory_size".into())
                    .ok()
                    .and_then(|v| v.as_f64())
                    .map(|v| v as usize)
                    .unwrap_or(0);
                
                Ok(WorkerMessage {
                    pid,
                    msg_type: WorkerMessageType::MemoryUpdate { memory_size },
                    data: Vec::new(),
                })
            }
            "yield" => {
                // Worker yielded control - just acknowledge
                Ok(WorkerMessage {
                    pid,
                    msg_type: WorkerMessageType::Yield,
                    data: Vec::new(),
                })
            }
            _ => {
                log(&format!("[wasm-hal] Unknown message type from worker {}: {}", pid, msg_type));
                Err(JsValue::from_str("Unknown message type"))
            }
        }
    } else {
        Err(JsValue::from_str("Invalid message format"))
    }
}

/// Serialize a WorkerMessage to bytes for poll_messages
fn serialize_worker_message(msg: &WorkerMessage) -> Vec<u8> {
    // Simple binary protocol:
    // [0]: message type (0=Ready, 1=Syscall, 2=Error, 3=Terminated)
    // [1-4]: pid (u32 LE)
    // [5-8]: depends on type
    // [9+]: data
    
    let mut result = Vec::new();
    
    match &msg.msg_type {
        WorkerMessageType::Ready { memory_size } => {
            result.push(ProcessMessageType::Ready as u8);
            result.extend_from_slice(&(msg.pid as u32).to_le_bytes());
            result.extend_from_slice(&(*memory_size as u32).to_le_bytes());
        }
        WorkerMessageType::Syscall { syscall_num, args } => {
            result.push(ProcessMessageType::Syscall as u8);
            result.extend_from_slice(&(msg.pid as u32).to_le_bytes());
            result.extend_from_slice(&syscall_num.to_le_bytes());
            for arg in args {
                result.extend_from_slice(&arg.to_le_bytes());
            }
            result.extend_from_slice(&msg.data);
        }
        WorkerMessageType::Error { message } => {
            result.push(ProcessMessageType::Error as u8);
            result.extend_from_slice(&(msg.pid as u32).to_le_bytes());
            result.extend_from_slice(message.as_bytes());
        }
        WorkerMessageType::Terminated => {
            result.push(ProcessMessageType::Terminate as u8);
            result.extend_from_slice(&(msg.pid as u32).to_le_bytes());
        }
        WorkerMessageType::MemoryUpdate { memory_size } => {
            // Use Ready type for memory updates (same format)
            result.push(ProcessMessageType::Ready as u8);
            result.extend_from_slice(&(msg.pid as u32).to_le_bytes());
            result.extend_from_slice(&(*memory_size as u32).to_le_bytes());
        }
        WorkerMessageType::Yield => {
            // Yield is just a notification, minimal data
            result.push(7u8); // Use 7 for yield
            result.extend_from_slice(&(msg.pid as u32).to_le_bytes());
        }
    }
    
    result
}

// ============================================================================
// Ping-Pong Test State Machine
// ============================================================================

/// Command tags for pingpong test processes (must match orbital-test-procs)
const CMD_PING: u32 = 0x3001;
const CMD_PONG_MODE: u32 = 0x3002;
const CMD_EXIT: u32 = 0x1005;

/// State of the automated ping-pong test
#[derive(Clone, Debug)]
enum PingPongTestState {
    /// No test running
    Idle,
    /// Waiting for pinger process to spawn
    WaitingForPinger { iterations: u32 },
    /// Waiting for ponger process to spawn
    WaitingForPonger { iterations: u32, pinger_pid: u64 },
    /// Both processes spawned, setting up capabilities
    SettingUpCaps { iterations: u32, pinger_pid: u64, ponger_pid: u64 },
    /// Capabilities granted, sending commands to start test
    StartingTest { iterations: u32, pinger_pid: u64, ponger_pid: u64 },
    /// Test running, waiting for completion
    Running { iterations: u32, pinger_pid: u64, ponger_pid: u64, start_time: u64 },
    /// Test complete, cleaning up
    Cleanup { pinger_pid: u64, ponger_pid: u64 },
}

impl Default for PingPongTestState {
    fn default() -> Self {
        Self::Idle
    }
}

/// Supervisor - manages the kernel and processes
#[wasm_bindgen]
pub struct Supervisor {
    kernel: Kernel<WasmHal>,
    console_callback: Option<js_sys::Function>,
    spawn_callback: Option<js_sys::Function>,
    /// State for automated ping-pong test
    pingpong_test: PingPongTestState,
    /// Pending spawn completion (name -> callback to call when spawn completes)
    pending_spawn_name: Option<String>,
    /// Last CommitLog sequence number persisted to IndexedDB
    last_persisted_axiom_seq: u64,
    /// Whether Axiom IndexedDB storage has been initialized
    axiom_storage_ready: bool,
    /// Whether init process has been spawned
    init_spawned: bool,
    /// Desktop environment engine
    desktop: desktop::DesktopEngine,
}

#[wasm_bindgen]
impl Supervisor {
    /// Create a new supervisor
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        // Set up panic hook for better error messages
        #[cfg(feature = "console_error_panic_hook")]
        console_error_panic_hook::set_once();

        let hal = WasmHal::new();
        let kernel = Kernel::new(hal);

        Self {
            kernel,
            console_callback: None,
            spawn_callback: None,
            pingpong_test: PingPongTestState::Idle,
            pending_spawn_name: None,
            last_persisted_axiom_seq: 0,
            axiom_storage_ready: false,
            init_spawned: false,
            desktop: desktop::DesktopEngine::new(),
        }
    }
    
    // =========================================================================
    // Axiom IndexedDB Persistence (WASM-specific storage layer)
    // =========================================================================
    
    /// Initialize Axiom storage (IndexedDB) - call this before boot()
    /// Returns a Promise that resolves when storage is ready
    #[wasm_bindgen]
    pub async fn init_axiom_storage(&mut self) -> Result<JsValue, JsValue> {
        log("[axiom] Initializing IndexedDB storage...");
        
        let result = init().await;
        
        if result.is_truthy() {
            self.axiom_storage_ready = true;
            
            // Get the last persisted sequence number
            let last_seq = getLastSeq().await;
            if let Some(seq) = last_seq.as_f64() {
                self.last_persisted_axiom_seq = if seq < 0.0 { 0 } else { seq as u64 + 1 };
                log(&format!("[axiom] Storage ready, last_seq={}", self.last_persisted_axiom_seq));
            }
            
            // Get count of stored entries
            let count = getCount().await;
            if let Some(n) = count.as_f64() {
                log(&format!("[axiom] {} entries in IndexedDB", n as u64));
            }
            
            Ok(JsValue::from_bool(true))
        } else {
            log("[axiom] Failed to initialize IndexedDB");
            Ok(JsValue::from_bool(false))
        }
    }
    
    /// Sync new CommitLog entries to IndexedDB
    /// Call this periodically (e.g., after each command or in main loop)
    /// Returns the number of entries synced
    #[wasm_bindgen]
    pub async fn sync_axiom_log(&mut self) -> u32 {
        if !self.axiom_storage_ready {
            return 0;
        }
        
        let commitlog = self.kernel.commitlog();
        let current_seq = commitlog.current_seq() + 1; // next seq
        
        // Nothing new to persist
        if current_seq <= self.last_persisted_axiom_seq {
            return 0;
        }
        
        // Get commits to persist
        let commits_to_persist: Vec<_> = commitlog.commits()
            .iter()
            .filter(|c| c.seq >= self.last_persisted_axiom_seq)
            .collect();
        
        if commits_to_persist.is_empty() {
            return 0;
        }
        
        // Convert to JS array
        let js_entries = js_sys::Array::new();
        for commit in &commits_to_persist {
            js_entries.push(&commit_to_js(commit));
        }
        
        // Persist to IndexedDB
        let result = persistEntries(js_entries.into()).await;
        if let Some(count) = result.as_f64() {
            let persisted = count as u32;
            self.last_persisted_axiom_seq = current_seq;
            if persisted > 0 {
                log(&format!("[axiom] Persisted {} commits to IndexedDB (seq now {})", 
                    persisted, current_seq));
            }
            persisted
        } else {
            log("[axiom] Failed to persist commits");
            0
        }
    }
    
    /// Get Axiom statistics for dashboard (CommitLog + SysLog)
    #[wasm_bindgen]
    pub fn get_axiom_stats_json(&self) -> String {
        let commitlog = self.kernel.commitlog();
        let syslog = self.kernel.syslog();
        let commits_in_memory = commitlog.len();
        let commit_seq = commitlog.current_seq();
        let events_in_memory = syslog.len();
        let persisted = self.last_persisted_axiom_seq;
        let pending = if commit_seq + 1 > persisted { commit_seq + 1 - persisted } else { 0 };
        
        format!(
            r#"{{"commits":{},"events":{},"commit_seq":{},"persisted":{},"pending":{},"storage_ready":{}}}"#,
            commits_in_memory, events_in_memory, commit_seq, persisted, pending, self.axiom_storage_ready
        )
    }
    
    /// Get recent CommitLog entries as JSON for display
    #[wasm_bindgen]
    pub fn get_commitlog_json(&self, count: usize) -> String {
        let commitlog = self.kernel.commitlog();
        let commits = commitlog.get_recent(count);
        
        let mut json = String::from("[");
        for (i, commit) in commits.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            
            let commit_type = commit_type_short(&commit.commit_type);
            let details = commit_type_to_string(&commit.commit_type);
            
            json.push_str(&format!(
                r#"{{"seq":{},"timestamp":{},"type":"{}","details":"{}"}}"#,
                commit.seq, commit.timestamp, commit_type, details.replace('"', "'")
            ));
        }
        json.push(']');
        json
    }
    
    /// Get recent SysLog entries as JSON for display (syscall audit trail)
    #[wasm_bindgen]
    pub fn get_syslog_json(&self, count: usize) -> String {
        let syslog = self.kernel.syslog();
        let events = syslog.get_recent(count);
        
        let mut json = String::from("[");
        for (i, event) in events.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            
            let (event_type, details) = match &event.event_type {
                orbital_kernel::SysEventType::Request { syscall_num, args } => {
                    ("Request", format!("syscall={:#x} args=[{},{},{},{}]", 
                        syscall_num, args[0], args[1], args[2], args[3]))
                }
                orbital_kernel::SysEventType::Response { request_id, result } => {
                    ("Response", format!("req={} result={}", request_id, result))
                }
            };
            
            json.push_str(&format!(
                r#"{{"id":{},"sender":{},"timestamp":{},"type":"{}","details":"{}"}}"#,
                event.id, event.sender, event.timestamp, event_type, details
            ));
        }
        json.push(']');
        json
    }
    
    /// Clear all Axiom log entries from IndexedDB (for testing/reset)
    #[wasm_bindgen]
    pub async fn clear_axiom_storage(&mut self) -> bool {
        if !self.axiom_storage_ready {
            return false;
        }
        
        let result = clear().await;
        if result.is_undefined() || result.is_null() {
            // clear() returns undefined on success
            self.last_persisted_axiom_seq = 0;
            log("[axiom] Cleared IndexedDB storage");
            true
        } else {
            false
        }
    }

    /// Set callback for console output
    #[wasm_bindgen]
    pub fn set_console_callback(&mut self, callback: js_sys::Function) {
        self.console_callback = Some(callback);
    }

    /// Set callback for spawning processes (JS will fetch WASM and call complete_spawn)
    #[wasm_bindgen]
    pub fn set_spawn_callback(&mut self, callback: js_sys::Function) {
        self.spawn_callback = Some(callback);
    }

    /// Request JS to spawn a process (fetch WASM binary)
    fn request_spawn(&self, proc_type: &str, name: &str) {
        if let Some(ref callback) = self.spawn_callback {
            let this = JsValue::null();
            let type_arg = JsValue::from_str(proc_type);
            let name_arg = JsValue::from_str(name);
            let _ = callback.call2(&this, &type_arg, &name_arg);
        } else {
            self.write_console("Error: Spawn callback not set\n");
        }
    }

    /// Complete spawning a process with the WASM binary
    /// Called by JS after fetching the WASM file
    #[wasm_bindgen]
    pub fn complete_spawn(&mut self, name: &str, wasm_binary: &[u8]) -> u64 {
        log(&format!("[supervisor] complete_spawn called for '{}', {} bytes", name, wasm_binary.len()));
        
        // First register in kernel to get a PID
        let kernel_pid = self.kernel.register_process(name);
        log(&format!("[supervisor] Kernel assigned PID {} for '{}'", kernel_pid.0, name));
        
        // Create endpoints for the process based on its role
        if name == "init" {
            // Init gets: slot 0 = init endpoint (for service registration)
            //           slot 1 = console output
            if let Ok((eid, slot)) = self.kernel.create_endpoint(kernel_pid) {
                log(&format!("[supervisor] Created init endpoint {} at slot {} for init", eid.0, slot));
            }
            if let Ok((eid, slot)) = self.kernel.create_endpoint(kernel_pid) {
                log(&format!("[supervisor] Created console output endpoint {} at slot {} for init", eid.0, slot));
            }
        } else if name == "terminal" {
            // Terminal gets: slot 0 = console output, slot 1 = console input
            if let Ok((eid, slot)) = self.kernel.create_endpoint(kernel_pid) {
                log(&format!("[supervisor] Created console output endpoint {} at slot {} for terminal", eid.0, slot));
            }
            if let Ok((eid, slot)) = self.kernel.create_endpoint(kernel_pid) {
                log(&format!("[supervisor] Created console input endpoint {} at slot {} for terminal", eid.0, slot));
            }
        } else {
            // Other processes get a single endpoint
            if let Ok((eid, slot)) = self.kernel.create_endpoint(kernel_pid) {
                log(&format!("[supervisor] Created endpoint {} at slot {} for {}", eid.0, slot, name));
            }
        }
        
        // Spawn via HAL with the kernel PID (so Worker uses correct PID for syscalls)
        match self.kernel.hal().spawn_with_pid(kernel_pid.0, name, wasm_binary) {
            Ok(handle) => {
                log(&format!("[supervisor] Spawned Worker '{}' with PID {}", name, handle.id));
                
                // Track init spawn
                if name == "init" {
                    self.init_spawned = true;
                    log("[supervisor] Init process spawned (PID 1)");
                } else if self.init_spawned {
                    // Grant this process a capability to init's endpoint (slot 0 of PID 1)
                    // This allows the process to register with init's service registry
                    let init_pid = ProcessId(1);
                    match self.kernel.grant_capability(
                        init_pid, 
                        0,  // init's endpoint at slot 0
                        kernel_pid,
                        orbital_kernel::Permissions { read: false, write: true, grant: false }
                    ) {
                        Ok(slot) => {
                            log(&format!("[supervisor] Granted init endpoint cap to {} at slot {}", name, slot));
                        }
                        Err(e) => {
                            log(&format!("[supervisor] Failed to grant init cap to {}: {:?}", name, e));
                        }
                    }
                }
                
                // Check if this is part of an automated pingpong test
                let pid = kernel_pid.0;
                self.on_process_spawned(name, pid);
                
                pid
            }
            Err(e) => {
                self.write_console(&format!("Error spawning {}: {:?}\n", name, e));
                log(&format!("[supervisor] Failed to spawn {}: {:?}", name, e));
                // Clean up kernel registration
                self.kernel.kill_process(kernel_pid);
                0
            }
        }
    }
    
    /// Called when a process is successfully spawned - progress pingpong test state if needed
    fn on_process_spawned(&mut self, name: &str, pid: u64) {
        // Check if we're waiting for this spawn as part of the pingpong test
        match self.pingpong_test.clone() {
            PingPongTestState::WaitingForPinger { iterations } if name == "pp_pinger" => {
                self.write_console(&format!("  Pinger spawned as PID {}\n", pid));
                // Now spawn the ponger
                self.pingpong_test = PingPongTestState::WaitingForPonger { iterations, pinger_pid: pid };
                self.request_spawn("pingpong", "pp_ponger");
            }
            PingPongTestState::WaitingForPonger { iterations, pinger_pid } if name == "pp_ponger" => {
                self.write_console(&format!("  Ponger spawned as PID {}\n", pid));
                // Both spawned, set up capabilities
                self.pingpong_test = PingPongTestState::SettingUpCaps { 
                    iterations, 
                    pinger_pid, 
                    ponger_pid: pid 
                };
                // Progress immediately
                self.progress_pingpong_test();
            }
            _ => {
                // Normal spawn, just report
                self.write_console(&format!("Spawned Worker '{}' as PID {}\n", name, pid));
                
                // Auto-spawn terminal after init starts
                if name == "init" {
                    log("[supervisor] Init started, spawning terminal...");
                    self.request_spawn("terminal", "terminal");
                }
            }
        }
    }
    
    /// Progress the ping-pong test state machine
    fn progress_pingpong_test(&mut self) {
        match self.pingpong_test.clone() {
            PingPongTestState::SettingUpCaps { iterations, pinger_pid, ponger_pid } => {
                self.write_console("  Setting up IPC capabilities...\n");
                
                let pinger = ProcessId(pinger_pid);
                let ponger = ProcessId(ponger_pid);
                
                // Grant pinger's endpoint (slot 0) to ponger (so ponger can send pongs back)
                match self.kernel.grant_capability(pinger, 0, ponger, 
                    orbital_kernel::Permissions { read: false, write: true, grant: false }) {
                    Ok(slot) => {
                        log(&format!("[pingpong] Granted pinger endpoint to ponger at slot {}", slot));
                    }
                    Err(e) => {
                        self.write_console(&format!("  Error granting pinger->ponger cap: {:?}\n", e));
                    }
                }
                
                // Grant ponger's endpoint (slot 0) to pinger (so pinger can send pings)
                match self.kernel.grant_capability(ponger, 0, pinger,
                    orbital_kernel::Permissions { read: false, write: true, grant: false }) {
                    Ok(slot) => {
                        log(&format!("[pingpong] Granted ponger endpoint to pinger at slot {}", slot));
                    }
                    Err(e) => {
                        self.write_console(&format!("  Error granting ponger->pinger cap: {:?}\n", e));
                    }
                }
                
                // Move to starting test
                self.pingpong_test = PingPongTestState::StartingTest { iterations, pinger_pid, ponger_pid };
                self.progress_pingpong_test();
            }
            
            PingPongTestState::StartingTest { iterations, pinger_pid, ponger_pid } => {
                self.write_console(&format!("  Starting test with {} iterations...\n", iterations));
                
                let pinger = ProcessId(pinger_pid);
                let ponger = ProcessId(ponger_pid);
                
                // Send CMD_PONG_MODE to ponger (slot 0 is ponger's own endpoint)
                // We need to send from terminal (PID 2) to ponger
                // Actually, we send to the process's endpoint - we need to grant terminal access first
                // For simplicity, let's send directly using kernel's send_to_process
                
                // Put ponger in pong mode
                if let Err(e) = self.kernel.send_to_process(ProcessId(2), ponger, CMD_PONG_MODE, vec![]) {
                    self.write_console(&format!("  Error sending PONG_MODE: {:?}\n", e));
                }
                
                // Send ping command to pinger with iterations count
                let ping_data = iterations.to_le_bytes().to_vec();
                if let Err(e) = self.kernel.send_to_process(ProcessId(2), pinger, CMD_PING, ping_data) {
                    self.write_console(&format!("  Error sending PING cmd: {:?}\n", e));
                }
                
                // Move to running state
                let start_time = self.kernel.uptime_nanos();
                self.pingpong_test = PingPongTestState::Running { 
                    iterations, 
                    pinger_pid, 
                    ponger_pid,
                    start_time,
                };
                
                self.write_console("  Test running... (watch for results from processes)\n");
            }
            
            PingPongTestState::Running { iterations: _, pinger_pid, ponger_pid, start_time } => {
                // Check if enough time has passed (timeout after 30 seconds)
                let elapsed = self.kernel.uptime_nanos() - start_time;
                let elapsed_secs = elapsed / 1_000_000_000;
                
                if elapsed_secs >= 30 {
                    self.write_console("\n  Test timed out after 30 seconds.\n");
                    self.pingpong_test = PingPongTestState::Cleanup { pinger_pid, ponger_pid };
                    self.progress_pingpong_test();
                }
                // Otherwise, the test results will be printed by the processes via debug syscall
            }
            
            PingPongTestState::Cleanup { pinger_pid, ponger_pid } => {
                self.write_console("  Cleaning up test processes...\n");
                
                // Send exit commands and kill processes
                let pinger = ProcessId(pinger_pid);
                let ponger = ProcessId(ponger_pid);
                
                // Send exit commands
                let _ = self.kernel.send_to_process(ProcessId(2), pinger, CMD_EXIT, vec![]);
                let _ = self.kernel.send_to_process(ProcessId(2), ponger, CMD_EXIT, vec![]);
                
                // Kill via HAL
                let pinger_handle = WasmProcessHandle::new(pinger_pid);
                let ponger_handle = WasmProcessHandle::new(ponger_pid);
                let _ = self.kernel.hal().kill_process(&pinger_handle);
                let _ = self.kernel.hal().kill_process(&ponger_handle);
                
                // Kill in kernel
                self.kernel.kill_process(pinger);
                self.kernel.kill_process(ponger);
                
                self.write_console(&format!("  Killed processes {} and {}\n", pinger_pid, ponger_pid));
                self.write_console("Ping-pong test complete.\norbital> ");
                
                self.pingpong_test = PingPongTestState::Idle;
            }
            
            _ => {}
        }
    }
    
    /// Start an automated ping-pong test
    fn start_pingpong_test(&mut self, iterations: u32) {
        if !matches!(self.pingpong_test, PingPongTestState::Idle) {
            self.write_console("Error: A ping-pong test is already running.\n");
            return;
        }
        
        self.write_console(&format!("=== Ping-Pong Latency Test ({} iterations) ===\n", iterations));
        self.write_console("  Spawning pinger process...\n");
        
        // Start the state machine
        self.pingpong_test = PingPongTestState::WaitingForPinger { iterations };
        
        // Request spawn of first process
        self.request_spawn("pingpong", "pp_pinger");
    }
    
    /// Check if the pingpong test completed (called when a process prints results)
    fn check_pingpong_complete(&mut self, pid: u64) {
        if let PingPongTestState::Running { pinger_pid, ponger_pid, .. } = self.pingpong_test {
            // The pinger process prints the results, so when we see the results from it, cleanup
            if pid == pinger_pid {
                log(&format!("[pingpong] Test complete, results printed by PID {}", pid));
                self.pingpong_test = PingPongTestState::Cleanup { pinger_pid, ponger_pid };
                self.progress_pingpong_test();
            }
        }
    }

    /// Write to console (calls JS callback)
    fn write_console(&self, text: &str) {
        if let Some(ref callback) = self.console_callback {
            let this = JsValue::null();
            let arg = JsValue::from_str(text);
            let _ = callback.call1(&this, &arg);
        }
    }

    /// Boot the kernel
    #[wasm_bindgen]
    pub fn boot(&mut self) {
        log("[supervisor] Booting Orbital OS kernel...");

        self.write_console("Orbital OS Kernel Bootstrap\n");
        self.write_console("===========================\n\n");

        log("[supervisor] Boot complete - call spawn_init() to start init process");
    }
    
    /// Spawn the init process (PID 1)
    /// Call this after boot() and after setting the spawn callback
    #[wasm_bindgen]
    pub fn spawn_init(&mut self) {
        if self.init_spawned {
            log("[supervisor] Init already spawned");
            return;
        }
        
        log("[supervisor] Requesting init spawn...");
        self.write_console("Starting init process...\n");
        self.request_spawn("init", "init");
    }

    /// Send input to the terminal
    #[wasm_bindgen]
    pub fn send_input(&mut self, input: &str) {
        // For the vertical slice, we simulate the terminal locally
        // In the full implementation, this would send to the terminal Worker
        self.handle_terminal_input(input);
    }

    /// Check if terminal process is alive
    fn is_terminal_alive(&self) -> bool {
        let term_pid = ProcessId(2);
        self.kernel.get_process(term_pid).is_some()
    }

    /// Handle terminal input (simulated for vertical slice)
    fn handle_terminal_input(&mut self, input: &str) {
        // Check if terminal process is still alive
        if !self.is_terminal_alive() {
            self.write_console("[kernel] Terminal process not running. No shell available.\n");
            return;
        }

        let line = input.trim();
        if line.is_empty() {
            self.write_console("orbital> ");
            return;
        }

        // Parse and execute command
        let parts: Vec<&str> = line.split_whitespace().collect();
        let (cmd, args) = match parts.split_first() {
            Some((c, a)) => (*c, a),
            None => {
                self.write_console("orbital> ");
                return;
            }
        };

        match cmd {
            "help" => {
                self.write_console("=== Orbital Kernel Test Terminal ===\n\n");
                self.write_console("Process Management:\n");
                self.write_console("  ps                          - List processes\n");
                self.write_console("  spawn <type> [name]         - Spawn test process\n");
                self.write_console("      Types: memhog, sender, receiver, pingpong, idle\n");
                self.write_console("  kill <pid>                  - Kill process\n");
                self.write_console("  inspect <pid>               - Show detailed info\n");
                self.write_console("  workerinfo <pid> / wi       - Show worker details\n");
                self.write_console("\n");
                self.write_console("Memory:\n");
                self.write_console("  memstat                     - Memory per process\n");
                self.write_console("  alloc <pid> <bytes>         - Allocate in process\n");
                self.write_console("  free <pid> <bytes>          - Free in process\n");
                self.write_console("\n");
                self.write_console("IPC & Capabilities:\n");
                self.write_console("  endpoints / ep              - List endpoints\n");
                self.write_console("  queue <endpoint_id>         - Show queue contents\n");
                self.write_console("  ipcstat                     - IPC stats per process\n");
                self.write_console("  ipclog [count]              - Recent IPC traffic\n");
                self.write_console("  send <pid> <message>        - Send message\n");
                self.write_console("  caps [pid]                  - List capabilities\n");
                self.write_console("  grant <from> <to> <slot>    - Grant capability\n");
                self.write_console("\n");
                self.write_console("Testing:\n");
                self.write_console("  pingpong [n]                - Auto ping-pong test (default 100)\n");
                self.write_console("  burst <s> <r> <n> <sz>      - Send message burst\n");
                self.write_console("  ping <p1> <p2> [n]          - Manual worker ping-pong\n");
                self.write_console("  pingtest <p1> <p2> [n]      - Supervisor poll latency\n");
                self.write_console("\n");
                self.write_console("System:\n");
                self.write_console("  status / top                - System overview\n");
                self.write_console("  uptime                      - Show uptime\n");
                self.write_console("  datetime                    - Show date/time\n");
                self.write_console("  echo <text>                 - Echo text\n");
                self.write_console("  clear                       - Clear screen\n");
                self.write_console("  exit                        - Exit terminal\n");
            }
            "ps" => {
                self.write_console("PID  STATE    MEMORY     WORKER      NAME\n");
                self.write_console("---  -----    ------     ------      ----\n");
                for (pid, proc) in self.kernel.list_processes() {
                    let state = match proc.state {
                        orbital_kernel::ProcessState::Running => "Running",
                        orbital_kernel::ProcessState::Blocked => "Blocked",
                        orbital_kernel::ProcessState::Zombie => "Zombie",
                    };
                    let worker_id = self.kernel.hal().get_worker_id(pid.0);
                    let worker_str = if worker_id > 0 {
                        format!("W{:08x}", worker_id & 0xFFFFFFFF)
                    } else {
                        "virtual".to_string()
                    };
                    self.write_console(&format!(
                        "{:<4} {:<8} {:>6} KB  {:<10}  {}\n", 
                        pid.0, state, proc.metrics.memory_size / 1024, worker_str, proc.name
                    ));
                }
            }
            "spawn" => {
                if args.is_empty() {
                    self.write_console("Usage: spawn <type> [name]\n");
                    self.write_console("Types: memhog, sender, receiver, pingpong, idle, terminal, init\n");
                    self.write_console("Example: spawn memhog myhog\n");
                } else {
                    let proc_type = args[0];
                    let name = args.get(1).unwrap_or(&proc_type);
                    
                    // Validate process type
                    let valid_types = ["memhog", "sender", "receiver", "pingpong", "idle", "terminal", "init"];
                    if !valid_types.contains(&proc_type.to_lowercase().as_str()) {
                        self.write_console(&format!("Unknown process type: {}\n", proc_type));
                        self.write_console("Valid types: memhog, sender, receiver, pingpong, idle, terminal, init\n");
                    } else {
                        // Request JS to fetch WASM and spawn real Worker
                        let full_name = if name == &proc_type {
                            proc_type.to_string()
                        } else {
                            name.to_string()
                        };
                        self.write_console(&format!("Spawning {}...\n", full_name));
                        self.request_spawn(proc_type, &full_name);
                    }
                }
            }
            "kill" => {
                if args.is_empty() {
                    self.write_console("Usage: kill <pid>\n");
                } else if let Ok(pid_num) = args[0].parse::<u64>() {
                    let pid = ProcessId(pid_num);
                    if pid_num == 1 {
                        self.write_console("Error: Cannot kill init (PID 1)\n");
                    } else if pid_num == 2 {
                        self.write_console("Error: Use 'exit' to kill terminal\n");
                    } else if self.kernel.get_process(pid).is_some() {
                        // Kill Worker via HAL
                        let handle = WasmProcessHandle::new(pid_num);
                        if let Err(e) = self.kernel.hal().kill_process(&handle) {
                            log(&format!("[supervisor] HAL kill error for PID {}: {:?}", pid_num, e));
                        }
                        // Kill in kernel
                        self.kernel.kill_process(pid);
                        self.write_console(&format!("Killed process PID {}\n", pid_num));
                    } else {
                        self.write_console(&format!("Error: Process {} not found\n", pid_num));
                    }
                } else {
                    self.write_console("Error: Invalid PID\n");
                }
            }
            "inspect" => {
                if args.is_empty() {
                    self.write_console("Usage: inspect <pid>\n");
                } else if let Ok(pid_num) = args[0].parse::<u64>() {
                    let pid = ProcessId(pid_num);
                    if let Some(proc) = self.kernel.get_process(pid) {
                        let state = match proc.state {
                            orbital_kernel::ProcessState::Running => "Running",
                            orbital_kernel::ProcessState::Blocked => "Blocked",
                            orbital_kernel::ProcessState::Zombie => "Zombie",
                        };
                        let age_secs = (self.kernel.uptime_nanos() - proc.metrics.start_time_ns) / 1_000_000_000;
                        
                        self.write_console(&format!("Process {}\n", pid_num));
                        self.write_console(&format!("  Name:      {}\n", proc.name));
                        self.write_console(&format!("  State:     {}\n", state));
                        self.write_console(&format!("  Memory:    {} KB ({} bytes)\n", proc.metrics.memory_size / 1024, proc.metrics.memory_size));
                        self.write_console(&format!("  Age:       {}s\n", age_secs));
                        self.write_console(&format!("  Syscalls:  {}\n", proc.metrics.syscall_count));
                        self.write_console(&format!("  IPC Sent:  {} msgs, {} bytes\n", proc.metrics.ipc_sent, proc.metrics.ipc_bytes_sent));
                        self.write_console(&format!("  IPC Recv:  {} msgs, {} bytes\n", proc.metrics.ipc_received, proc.metrics.ipc_bytes_received));
                        
                        // Show capabilities
                        if let Some(cap_space) = self.kernel.get_cap_space(pid) {
                            self.write_console(&format!("  Caps:      {}\n", cap_space.len()));
                        }
                    } else {
                        self.write_console(&format!("Error: Process {} not found\n", pid_num));
                    }
                } else {
                    self.write_console("Error: Invalid PID\n");
                }
            }
            "memstat" => {
                let total = self.kernel.total_memory();
                self.write_console(&format!("Total System Memory: {} KB ({} bytes)\n\n", total / 1024, total));
                self.write_console("PID  MEMORY        PROCESS\n");
                self.write_console("---  ------        -------\n");
                for (pid, proc) in self.kernel.list_processes() {
                    let kb = proc.metrics.memory_size / 1024;
                    let pct = if total > 0 { proc.metrics.memory_size * 100 / total } else { 0 };
                    self.write_console(&format!(
                        "{:<4} {:>6} KB {:>3}%  {}\n",
                        pid.0, kb, pct, proc.name
                    ));
                }
            }
            "alloc" => {
                if args.len() < 2 {
                    self.write_console("Usage: alloc <pid> <bytes>\n");
                    self.write_console("Example: alloc 3 65536  (allocate 64KB to PID 3)\n");
                } else if let (Ok(pid_num), Ok(bytes)) = (args[0].parse::<u64>(), args[1].parse::<usize>()) {
                    let pid = ProcessId(pid_num);
                    match self.kernel.allocate_memory(pid, bytes) {
                        Ok(total) => {
                            self.write_console(&format!(
                                "Allocated {} bytes to PID {}. Total: {} KB\n",
                                bytes, pid_num, total / 1024
                            ));
                        }
                        Err(orbital_kernel::KernelError::ProcessNotFound) => {
                            self.write_console(&format!("Error: Process {} not found\n", pid_num));
                        }
                        Err(_) => self.write_console("Error: Allocation failed\n"),
                    }
                } else {
                    self.write_console("Error: Invalid arguments\n");
                }
            }
            "free" => {
                if args.len() < 2 {
                    self.write_console("Usage: free <pid> <bytes>\n");
                    self.write_console("Example: free 3 65536  (free 64KB from PID 3)\n");
                } else if let (Ok(pid_num), Ok(bytes)) = (args[0].parse::<u64>(), args[1].parse::<usize>()) {
                    let pid = ProcessId(pid_num);
                    match self.kernel.free_memory(pid, bytes) {
                        Ok(total) => {
                            self.write_console(&format!(
                                "Freed {} bytes from PID {}. Total: {} KB\n",
                                bytes, pid_num, total / 1024
                            ));
                        }
                        Err(orbital_kernel::KernelError::ProcessNotFound) => {
                            self.write_console(&format!("Error: Process {} not found\n", pid_num));
                        }
                        Err(_) => self.write_console("Error: Free failed\n"),
                    }
                } else {
                    self.write_console("Error: Invalid arguments\n");
                }
            }
            "endpoints" | "ep" => {
                let endpoints = self.kernel.list_endpoints();
                let total_msgs = self.kernel.total_pending_messages();
                self.write_console(&format!("IPC Endpoints ({} total, {} pending messages)\n\n", endpoints.len(), total_msgs));
                self.write_console("ID   OWNER  QUEUE  TOTAL    BYTES     OWNER_NAME\n");
                self.write_console("--   -----  -----  -----    -----     ----------\n");
                for ep in endpoints {
                    let owner_name = self.kernel.get_process(ep.owner)
                        .map(|p| p.name.as_str())
                        .unwrap_or("???");
                    // Get detailed metrics if available
                    if let Some(detail) = self.kernel.get_endpoint_detail(ep.id) {
                        self.write_console(&format!(
                            "{:<4} {:<6} {:>5}  {:>5}    {:>8}  {}\n",
                            ep.id.0, ep.owner.0, ep.queue_depth, 
                            detail.metrics.total_messages,
                            format_bytes(detail.metrics.total_bytes as usize),
                            owner_name
                        ));
                    } else {
                        self.write_console(&format!(
                            "{:<4} {:<6} {:>5}  -        -         {}\n",
                            ep.id.0, ep.owner.0, ep.queue_depth, owner_name
                        ));
                    }
                }
            }
            "queue" => {
                if args.is_empty() {
                    self.write_console("Usage: queue <endpoint_id>\n");
                } else if let Ok(eid) = args[0].parse::<u64>() {
                    let endpoint_id = orbital_kernel::EndpointId(eid);
                    if let Some(detail) = self.kernel.get_endpoint_detail(endpoint_id) {
                        let owner_name = self.kernel.get_process(detail.owner)
                            .map(|p| p.name.as_str())
                            .unwrap_or("???");
                        self.write_console(&format!("Endpoint {} (owner: PID {} - {})\n", eid, detail.owner.0, owner_name));
                        self.write_console(&format!("Queue depth: {}, Total msgs: {}, Total bytes: {}\n\n",
                            detail.queue_depth, detail.metrics.total_messages, detail.metrics.total_bytes));
                        
                        if detail.queued_messages.is_empty() {
                            self.write_console("(queue empty)\n");
                        } else {
                            self.write_console("FROM   TAG        SIZE\n");
                            self.write_console("----   ---        ----\n");
                            for msg in &detail.queued_messages {
                                self.write_console(&format!(
                                    "{:<5}  0x{:04X}     {} bytes\n",
                                    msg.from.0, msg.tag, msg.size
                                ));
                            }
                        }
                    } else {
                        self.write_console(&format!("Error: Endpoint {} not found\n", eid));
                    }
                } else {
                    self.write_console("Error: Invalid endpoint ID\n");
                }
            }
            "ipcstat" => {
                self.write_console("PID   NAME             SENT       RECV       BYTES_TX    BYTES_RX\n");
                self.write_console("---   ----             ----       ----       --------    --------\n");
                for (pid, proc) in self.kernel.list_processes() {
                    self.write_console(&format!(
                        "{:<5} {:<16} {:>8}   {:>8}   {:>8}    {:>8}\n",
                        pid.0,
                        truncate(&proc.name, 16),
                        proc.metrics.ipc_sent,
                        proc.metrics.ipc_received,
                        format_bytes(proc.metrics.ipc_bytes_sent as usize),
                        format_bytes(proc.metrics.ipc_bytes_received as usize),
                    ));
                }
            }
            "ipclog" => {
                let count = args.get(0).and_then(|s| s.parse().ok()).unwrap_or(20);
                let traffic = self.kernel.get_recent_ipc_traffic(count);
                if traffic.is_empty() {
                    self.write_console("No IPC traffic recorded yet.\n");
                } else {
                    self.write_console(&format!("Recent IPC Traffic (last {} messages):\n\n", traffic.len()));
                    self.write_console("TIME_MS    FROM   TO     EP    TAG      SIZE\n");
                    self.write_console("-------    ----   --     --    ---      ----\n");
                    for entry in traffic {
                        let time_ms = entry.timestamp / 1_000_000;
                        self.write_console(&format!(
                            "{:>7}    {:<5}  {:<5}  {:<4}  0x{:04X}   {} bytes\n",
                            time_ms, entry.from.0, entry.to.0, entry.endpoint.0, entry.tag, entry.size
                        ));
                    }
                }
            }
            "status" | "top" => {
                let metrics = self.kernel.get_system_metrics();
                let uptime_s = metrics.uptime_ns / 1_000_000_000;
                let uptime_ms = (metrics.uptime_ns % 1_000_000_000) / 1_000_000;
                
                self.write_console("=== Orbital Kernel Status ===\n\n");
                self.write_console(&format!("Uptime:            {}.{:03}s\n", uptime_s, uptime_ms));
                self.write_console(&format!("Processes:         {}\n", metrics.process_count));
                self.write_console(&format!("Endpoints:         {}\n", metrics.endpoint_count));
                self.write_console(&format!("Total Memory:      {}\n", format_bytes(metrics.total_memory)));
                self.write_console(&format!("Pending IPC:       {} messages\n", metrics.total_pending_messages));
                self.write_console(&format!("Total IPC:         {} messages\n", metrics.total_ipc_messages));
                
                self.write_console("\n--- Processes ---\n");
                self.write_console("PID  STATE    MEMORY     IPC_TX   IPC_RX   NAME\n");
                self.write_console("---  -----    ------     ------   ------   ----\n");
                for (pid, proc) in self.kernel.list_processes() {
                    let state = match proc.state {
                        orbital_kernel::ProcessState::Running => "Run",
                        orbital_kernel::ProcessState::Blocked => "Blk",
                        orbital_kernel::ProcessState::Zombie => "Zmb",
                    };
                    self.write_console(&format!(
                        "{:<4} {:<8} {:>6} KB  {:>6}   {:>6}   {}\n",
                        pid.0, state, 
                        proc.metrics.memory_size / 1024,
                        proc.metrics.ipc_sent,
                        proc.metrics.ipc_received,
                        proc.name
                    ));
                }
            }
            "pingpong" => {
                // Automated ping-pong latency test
                let iterations = args.get(0)
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(100);
                
                if iterations == 0 || iterations > 100000 {
                    self.write_console("Usage: pingpong [iterations]\n");
                    self.write_console("  iterations: 1-100000 (default: 100)\n");
                    self.write_console("Example: pingpong 1000\n");
                } else {
                    self.start_pingpong_test(iterations);
                    return; // Don't print prompt - state machine handles it
                }
            }
            "burst" => {
                if args.len() < 4 {
                    self.write_console("Usage: burst <sender_pid> <receiver_pid> <count> <msg_size>\n");
                    self.write_console("Example: burst 3 4 100 64  (send 100 messages of 64 bytes)\n");
                } else {
                    match (args[0].parse::<u64>(), args[1].parse::<u64>(), args[2].parse::<usize>(), args[3].parse::<usize>()) {
                        (Ok(sender_pid), Ok(receiver_pid), Ok(count), Ok(size)) => {
                            let sender = ProcessId(sender_pid);
                            let receiver = ProcessId(receiver_pid);
                            
                            // Verify both processes exist
                            if self.kernel.get_process(sender).is_none() {
                                self.write_console(&format!("Error: Sender PID {} not found\n", sender_pid));
                            } else if self.kernel.get_process(receiver).is_none() {
                                self.write_console(&format!("Error: Receiver PID {} not found\n", receiver_pid));
                            } else {
                                // Send burst of messages
                                let payload = vec![0x42u8; size];
                                let start_time = self.kernel.uptime_nanos();
                                let mut success_count = 0;
                                
                                for _ in 0..count {
                                    if self.kernel.send_to_process(sender, receiver, 0, payload.clone()).is_ok() {
                                        success_count += 1;
                                    }
                                }
                                
                                let elapsed = self.kernel.uptime_nanos() - start_time;
                                let elapsed_ms = elapsed as f64 / 1_000_000.0;
                                let msgs_per_sec = if elapsed > 0 {
                                    (success_count as u64 * 1_000_000_000) / elapsed
                                } else {
                                    0
                                };
                                
                                self.write_console(&format!(
                                    "Burst complete: {} messages of {} bytes each\n",
                                    success_count, size
                                ));
                                self.write_console(&format!(
                                    "Time: {:.3}ms, Rate: {} msgs/sec, Throughput: {}/sec\n",
                                    elapsed_ms, msgs_per_sec, format_bytes(msgs_per_sec as usize * size)
                                ));
                            }
                        }
                        _ => self.write_console("Error: Invalid arguments\n"),
                    }
                }
            }
            "ping" => {
                // Real worker-to-worker ping-pong test
                // Requires two spawned pingpong processes
                if args.len() < 2 {
                    self.write_console("Usage: ping <pinger_pid> <ponger_pid> [iterations]\n");
                    self.write_console("  Runs REAL worker-to-worker latency test via syscalls.\n");
                    self.write_console("  First spawn two pingpong processes:\n");
                    self.write_console("    spawn pingpong p1\n");
                    self.write_console("    spawn pingpong p2\n");
                    self.write_console("Example: ping 3 4 100\n");
                } else {
                    match (args[0].parse::<u64>(), args[1].parse::<u64>()) {
                        (Ok(pinger_pid), Ok(ponger_pid)) => {
                            let iterations = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);
                            let pinger = ProcessId(pinger_pid);
                            let ponger = ProcessId(ponger_pid);
                            
                            // Get worker IDs to verify they're real workers
                            let pinger_worker = self.kernel.hal().get_worker_id(pinger_pid);
                            let ponger_worker = self.kernel.hal().get_worker_id(ponger_pid);
                            
                            // Verify both processes exist and are real workers
                            if self.kernel.get_process(pinger).is_none() {
                                self.write_console(&format!("Error: PID {} not found\n", pinger_pid));
                            } else if self.kernel.get_process(ponger).is_none() {
                                self.write_console(&format!("Error: PID {} not found\n", ponger_pid));
                            } else if pinger_worker == 0 {
                                self.write_console(&format!("Error: PID {} is not a real worker\n", pinger_pid));
                            } else if ponger_worker == 0 {
                                self.write_console(&format!("Error: PID {} is not a real worker\n", ponger_pid));
                            } else {
                                self.write_console("=== Real Worker-to-Worker Ping Test ===\n\n");
                                self.write_console(&format!("Pinger: PID {} (worker:{})\n", pinger_pid, pinger_worker));
                                self.write_console(&format!("Ponger: PID {} (worker:{})\n", ponger_pid, ponger_worker));
                                self.write_console(&format!("Iterations: {}\n\n", iterations));
                                
                                // Step 1: Grant ponger's endpoint (slot 0) to pinger (becomes slot 1)
                                // This allows pinger to SEND pings to ponger
                                self.write_console("Setting up IPC capabilities...\n");
                                match self.kernel.grant_capability(ponger, 0, pinger, 
                                    orbital_kernel::Permissions { read: false, write: true, grant: false }) {
                                    Ok(slot) => {
                                        self.write_console(&format!("  Pinger can send to ponger (slot {})\n", slot));
                                    }
                                    Err(e) => {
                                        self.write_console(&format!("  Error granting ponger->pinger cap: {:?}\n", e));
                                    }
                                }
                                
                                // Step 2: Grant pinger's endpoint (slot 0) to ponger (becomes slot 1)
                                // This allows ponger to SEND pongs back to pinger
                                match self.kernel.grant_capability(pinger, 0, ponger,
                                    orbital_kernel::Permissions { read: false, write: true, grant: false }) {
                                    Ok(slot) => {
                                        self.write_console(&format!("  Ponger can send to pinger (slot {})\n", slot));
                                    }
                                    Err(e) => {
                                        self.write_console(&format!("  Error granting pinger->ponger cap: {:?}\n", e));
                                    }
                                }
                                
                                // Step 3: Send CMD_PONG_MODE to ponger
                                // This tells it to start responding to pings
                                const CMD_PONG_MODE: u32 = 0x3002;
                                const CMD_PING: u32 = 0x3001;
                                
                                self.write_console("\nSending commands to workers...\n");
                                
                                // Send CMD_PONG_MODE to ponger (no data needed)
                                match self.kernel.send_to_process(ProcessId(2), ponger, CMD_PONG_MODE, vec![]) {
                                    Ok(()) => self.write_console("  Sent PONG_MODE to ponger\n"),
                                    Err(e) => self.write_console(&format!("  Error sending to ponger: {:?}\n", e)),
                                }
                                
                                // Step 4: Send CMD_PING to pinger with iterations
                                // Data format: [iterations: u32 LE]
                                let ping_data = (iterations as u32).to_le_bytes().to_vec();
                                match self.kernel.send_to_process(ProcessId(2), pinger, CMD_PING, ping_data) {
                                    Ok(()) => self.write_console("  Sent PING command to pinger\n"),
                                    Err(e) => self.write_console(&format!("  Error sending to pinger: {:?}\n", e)),
                                }
                                
                                self.write_console("\nTest started! Results will appear in browser console (F12).\n");
                                self.write_console("Look for: [process N] ========== PING-PONG RESULTS ==========\n");
                                self.write_console("\nUse 'ipclog' to see message traffic between workers.\n");
                            }
                        }
                        _ => self.write_console("Error: Invalid PIDs\n"),
                    }
                }
            }
            "send" => {
                if args.len() < 2 {
                    self.write_console("Usage: send <pid> <message>\n");
                    self.write_console("Example: send 3 hello world\n");
                } else if let Ok(to_pid) = args[0].parse::<u64>() {
                    let msg = args[1..].join(" ");
                    let from_pid = ProcessId(2); // terminal
                    let to = ProcessId(to_pid);
                    
                    match self.kernel.send_to_process(from_pid, to, 0, msg.as_bytes().to_vec()) {
                        Ok(()) => {
                            self.write_console(&format!("Sent {} bytes to PID {}\n", msg.len(), to_pid));
                        }
                        Err(orbital_kernel::KernelError::EndpointNotFound) => {
                            self.write_console(&format!("Error: PID {} has no endpoint\n", to_pid));
                        }
                        Err(e) => {
                            self.write_console(&format!("Error: {:?}\n", e));
                        }
                    }
                } else {
                    self.write_console("Error: Invalid PID\n");
                }
            }
            "caps" => {
                // Show terminal's capabilities
                let term_pid = ProcessId(2);
                let result = self.kernel.handle_syscall(term_pid, Syscall::ListCaps);
                if let SyscallResult::CapList(caps) = result {
                    self.write_console("SLOT  TYPE      PERMS  OBJECT\n");
                    self.write_console("----  ----      -----  ------\n");
                    for (slot, cap) in caps {
                        let type_str = match cap.object_type {
                            orbital_kernel::ObjectType::Endpoint => "Endpoint",
                            orbital_kernel::ObjectType::Process => "Process",
                            orbital_kernel::ObjectType::Memory => "Memory",
                            orbital_kernel::ObjectType::Irq => "IRQ",
                            orbital_kernel::ObjectType::IoPort => "I/O Port",
                            orbital_kernel::ObjectType::Console => "Console",
                        };
                        let perms = format!(
                            "{}{}{}",
                            if cap.permissions.read { "R" } else { "-" },
                            if cap.permissions.write { "W" } else { "-" },
                            if cap.permissions.grant { "G" } else { "-" },
                        );
                        self.write_console(&format!(
                            "{:<5} {:<9} {}    {}\n",
                            slot, type_str, perms, cap.object_id
                        ));
                    }
                }
            }
            "echo" => {
                let text = args.join(" ");
                self.write_console(&text);
                self.write_console("\n");
            }
            "uptime" => {
                let nanos = self.kernel.uptime_nanos();
                let total_secs = nanos / 1_000_000_000;
                let ms = (nanos % 1_000_000_000) / 1_000_000;
                let hours = total_secs / 3600;
                let mins = (total_secs % 3600) / 60;
                let secs = total_secs % 60;
                if hours > 0 {
                    self.write_console(&format!("Uptime: {}h {}m {}.{:03}s\n", hours, mins, secs, ms));
                } else if mins > 0 {
                    self.write_console(&format!("Uptime: {}m {}.{:03}s\n", mins, secs, ms));
                } else {
                    self.write_console(&format!("Uptime: {}.{:03}s\n", secs, ms));
                }
            }
            "datetime" => {
                // Get current timestamp from JavaScript Date.now()
                let timestamp_ms = date_now() as i64;
                
                // Convert to components (simple UTC calculation)
                let secs = timestamp_ms / 1000;
                let ms = timestamp_ms % 1000;
                
                // Calculate date components from Unix timestamp
                // Days since epoch
                let days = secs / 86400;
                let time_secs = secs % 86400;
                
                let hours = time_secs / 3600;
                let mins = (time_secs % 3600) / 60;
                let seconds = time_secs % 60;
                
                // Calculate year, month, day from days since epoch (1970-01-01)
                let (year, month, day) = days_to_ymd(days);
                
                self.write_console(&format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} UTC\n",
                    year, month, day, hours, mins, seconds, ms
                ));
            }
            "clear" => {
                // Send escape sequence to clear
                self.write_console("\x1B[2J\x1B[H");
            }
            "grant" => {
                // Grant a capability from one process to another
                if args.len() < 3 {
                    self.write_console("Usage: grant <from_pid> <to_pid> <slot> [perms]\n");
                    self.write_console("  perms: rwg (read/write/grant), default: rw\n");
                    self.write_console("Example: grant 3 4 0 rw  (grant PID 3's slot 0 to PID 4)\n");
                } else {
                    match (args[0].parse::<u64>(), args[1].parse::<u64>(), args[2].parse::<u32>()) {
                        (Ok(from), Ok(to), Ok(slot)) => {
                            let perms_str = args.get(3).copied().unwrap_or("rw");
                            let perms = orbital_kernel::Permissions {
                                read: perms_str.contains('r'),
                                write: perms_str.contains('w'),
                                grant: perms_str.contains('g'),
                            };
                            
                            match self.kernel.grant_capability(ProcessId(from), slot, ProcessId(to), perms) {
                                Ok(new_slot) => {
                                    self.write_console(&format!(
                                        "Granted capability from PID {} slot {} to PID {} (new slot {})\n",
                                        from, slot, to, new_slot
                                    ));
                                }
                                Err(e) => {
                                    self.write_console(&format!("Error: {:?}\n", e));
                                }
                            }
                        }
                        _ => self.write_console("Error: Invalid arguments\n"),
                    }
                }
            }
            "pingtest" => {
                // Automated ping-pong latency test between two real workers
                if args.len() < 2 {
                    self.write_console("Usage: pingtest <pinger_pid> <ponger_pid> [iterations]\n");
                    self.write_console("  Runs latency test between two spawned pingpong processes.\n");
                    self.write_console("  First spawn two processes: spawn pingpong p1 && spawn pingpong p2\n");
                    self.write_console("Example: pingtest 3 4 100\n");
                } else {
                    match (args[0].parse::<u64>(), args[1].parse::<u64>()) {
                        (Ok(pinger_pid), Ok(ponger_pid)) => {
                            let iterations = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);
                            let pinger = ProcessId(pinger_pid);
                            let ponger = ProcessId(ponger_pid);
                            
                            // Get worker IDs for display
                            let pinger_worker = self.kernel.hal().get_worker_id(pinger_pid);
                            let ponger_worker = self.kernel.hal().get_worker_id(ponger_pid);
                            
                            // Verify both processes exist and are real workers
                            if self.kernel.get_process(pinger).is_none() {
                                self.write_console(&format!("Error: PID {} not found\n", pinger_pid));
                            } else if self.kernel.get_process(ponger).is_none() {
                                self.write_console(&format!("Error: PID {} not found\n", ponger_pid));
                            } else if pinger_worker == 0 {
                                self.write_console(&format!("Error: PID {} is not a real worker (virtual process)\n", pinger_pid));
                            } else if ponger_worker == 0 {
                                self.write_console(&format!("Error: PID {} is not a real worker (virtual process)\n", ponger_pid));
                            } else {
                                self.write_console("=== Ping-Pong Latency Test ===\n\n");
                                self.write_console(&format!("Pinger: PID {} (worker:{})\n", pinger_pid, pinger_worker));
                                self.write_console(&format!("Ponger: PID {} (worker:{})\n", ponger_pid, ponger_worker));
                                self.write_console(&format!("Iterations: {}\n\n", iterations));
                                
                                // Set up cross-process IPC:
                                // 1. Each process should already have an endpoint (slot 0)
                                // 2. Grant pinger's endpoint to ponger (so ponger can send pong back)
                                // 3. Grant ponger's endpoint to pinger (so pinger can send ping)
                                
                                // Grant pinger's slot 0 to ponger (for pong replies)
                                match self.kernel.grant_capability(pinger, 0, ponger, 
                                    orbital_kernel::Permissions { read: false, write: true, grant: false }) {
                                    Ok(slot) => {
                                        self.write_console(&format!("Granted pinger endpoint to ponger (slot {})\n", slot));
                                    }
                                    Err(e) => {
                                        self.write_console(&format!("Warning: Failed to grant pinger->ponger: {:?}\n", e));
                                    }
                                }
                                
                                // Grant ponger's slot 0 to pinger (for ping requests)
                                match self.kernel.grant_capability(ponger, 0, pinger,
                                    orbital_kernel::Permissions { read: false, write: true, grant: false }) {
                                    Ok(slot) => {
                                        self.write_console(&format!("Granted ponger endpoint to pinger (slot {})\n", slot));
                                    }
                                    Err(e) => {
                                        self.write_console(&format!("Warning: Failed to grant ponger->pinger: {:?}\n", e));
                                    }
                                }
                                
                                self.write_console("\nCapabilities set up. Processes can now communicate.\n");
                                self.write_console("Use 'caps <pid>' to see each process's capabilities.\n");
                                self.write_console("Use 'ipclog' to watch message traffic.\n");
                                
                                // Now run a quick supervisor-level latency measurement
                                self.write_console("\n--- Supervisor-level syscall latency test ---\n");
                                let start = self.kernel.uptime_nanos();
                                let mut latencies: Vec<u64> = Vec::with_capacity(iterations);
                                
                                for _ in 0..iterations {
                                    let t0 = self.kernel.uptime_nanos();
                                    // Simulate syscall round-trip by polling syscalls
                                    let _ = self.kernel.hal().poll_syscalls();
                                    let t1 = self.kernel.uptime_nanos();
                                    latencies.push(t1 - t0);
                                }
                                
                                let total_time = self.kernel.uptime_nanos() - start;
                                
                                if !latencies.is_empty() {
                                    latencies.sort();
                                    let count = latencies.len();
                                    let min = latencies[0];
                                    let max = latencies[count - 1];
                                    let sum: u64 = latencies.iter().sum();
                                    let avg = sum / count as u64;
                                    let median = latencies[count / 2];
                                    
                                    self.write_console(&format!("\nSyscall poll latency ({} iterations):\n", count));
                                    self.write_console(&format!("  Min:    {:>8} ns ({:.3} µs)\n", min, min as f64 / 1000.0));
                                    self.write_console(&format!("  Max:    {:>8} ns ({:.3} µs)\n", max, max as f64 / 1000.0));
                                    self.write_console(&format!("  Avg:    {:>8} ns ({:.3} µs)\n", avg, avg as f64 / 1000.0));
                                    self.write_console(&format!("  Median: {:>8} ns ({:.3} µs)\n", median, median as f64 / 1000.0));
                                    self.write_console(&format!("  Total:  {:>8} ns ({:.3} ms)\n", total_time, total_time as f64 / 1_000_000.0));
                                }
                            }
                        }
                        _ => self.write_console("Error: Invalid PIDs\n"),
                    }
                }
            }
            "workerinfo" | "wi" => {
                // Show detailed worker information for a process
                if args.is_empty() {
                    self.write_console("Usage: workerinfo <pid>\n");
                } else if let Ok(pid_num) = args[0].parse::<u64>() {
                    let pid = ProcessId(pid_num);
                    if let Some(proc) = self.kernel.get_process(pid) {
                        let worker_id = self.kernel.hal().get_worker_id(pid_num);
                        
                        self.write_console(&format!("Process {} ({})\n", pid_num, proc.name));
                        self.write_console(&format!("  State:     {:?}\n", proc.state));
                        self.write_console(&format!("  Memory:    {} bytes\n", proc.metrics.memory_size));
                        
                        if worker_id > 0 {
                            // Convert timestamp to readable date
                            let date_ms = worker_id as f64;
                            self.write_console(&format!("  Worker ID: worker:{}\n", worker_id));
                            self.write_console(&format!("  Context:   Created at Unix ms {}\n", date_ms));
                            self.write_console("  Type:      Real Web Worker (SharedArrayBuffer + Atomics)\n");
                        } else {
                            self.write_console("  Worker ID: (virtual)\n");
                            self.write_console("  Type:      Virtual process (no real worker)\n");
                        }
                        
                        self.write_console(&format!("  Syscalls:  {}\n", proc.metrics.syscall_count));
                        self.write_console(&format!("  IPC Sent:  {} msgs, {} bytes\n", 
                            proc.metrics.ipc_sent, proc.metrics.ipc_bytes_sent));
                        self.write_console(&format!("  IPC Recv:  {} msgs, {} bytes\n",
                            proc.metrics.ipc_received, proc.metrics.ipc_bytes_received));
                    } else {
                        self.write_console(&format!("Error: Process {} not found\n", pid_num));
                    }
                } else {
                    self.write_console("Error: Invalid PID\n");
                }
            }
            "exit" => {
                self.write_console("Goodbye!\n");
                // Kill the terminal process
                let term_pid = ProcessId(2);
                self.kernel.kill_process(term_pid);
                return; // Don't print prompt after exit
            }
            _ => {
                self.write_console(&format!("Unknown command: {}\n", cmd));
                self.write_console("Type 'help' for available commands.\n");
            }
        }

        self.write_console("orbital> ");
    }

    /// Get system uptime in milliseconds
    #[wasm_bindgen]
    pub fn get_uptime_ms(&self) -> f64 {
        self.kernel.uptime_nanos() as f64 / 1_000_000.0
    }

    /// Get process count
    #[wasm_bindgen]
    pub fn get_process_count(&self) -> usize {
        self.kernel.list_processes().len()
    }

    /// Get total memory usage in bytes
    #[wasm_bindgen]
    pub fn get_total_memory(&self) -> usize {
        self.kernel.total_memory()
    }

    /// Get endpoint count
    #[wasm_bindgen]
    pub fn get_endpoint_count(&self) -> usize {
        self.kernel.list_endpoints().len()
    }

    /// Get total pending IPC messages
    #[wasm_bindgen]
    pub fn get_pending_messages(&self) -> usize {
        self.kernel.total_pending_messages()
    }

    /// Get total IPC message count since boot
    #[wasm_bindgen]
    pub fn get_total_ipc_messages(&self) -> f64 {
        self.kernel.get_system_metrics().total_ipc_messages as f64
    }

    /// Get process list as JSON for dashboard
    #[wasm_bindgen]
    pub fn get_process_list_json(&self) -> String {
        let processes: Vec<_> = self.kernel.list_processes().iter().map(|(pid, proc)| {
            let state = match proc.state {
                orbital_kernel::ProcessState::Running => "Running",
                orbital_kernel::ProcessState::Blocked => "Blocked",
                orbital_kernel::ProcessState::Zombie => "Zombie",
            };
            // Get worker ID from HAL (browser-assigned memory context ID)
            let worker_id = self.kernel.hal().get_worker_id(pid.0);
            serde_json::json!({
                "pid": pid.0,
                "name": proc.name,
                "state": state,
                "memory": proc.metrics.memory_size,
                "ipc_sent": proc.metrics.ipc_sent,
                "ipc_received": proc.metrics.ipc_received,
                "syscalls": proc.metrics.syscall_count,
                "worker_id": worker_id
            })
        }).collect();
        serde_json::to_string(&processes).unwrap_or_else(|_| "[]".to_string())
    }

    /// Get endpoint list as JSON for dashboard
    #[wasm_bindgen]
    pub fn get_endpoint_list_json(&self) -> String {
        let endpoints: Vec<_> = self.kernel.list_endpoints().iter().map(|ep| {
            let detail = self.kernel.get_endpoint_detail(ep.id);
            serde_json::json!({
                "id": ep.id.0,
                "owner": ep.owner.0,
                "queue": ep.queue_depth,
                "total_msgs": detail.as_ref().map(|d| d.metrics.total_messages).unwrap_or(0),
                "total_bytes": detail.as_ref().map(|d| d.metrics.total_bytes).unwrap_or(0)
            })
        }).collect();
        serde_json::to_string(&endpoints).unwrap_or_else(|_| "[]".to_string())
    }

    /// Get recent IPC traffic as JSON for dashboard
    #[wasm_bindgen]
    pub fn get_ipc_traffic_json(&self, count: usize) -> String {
        let traffic: Vec<_> = self.kernel.get_recent_ipc_traffic(count).iter().map(|entry| {
            serde_json::json!({
                "from": entry.from.0,
                "to": entry.to.0,
                "endpoint": entry.endpoint.0,
                "tag": entry.tag,
                "size": entry.size,
                "timestamp": entry.timestamp
            })
        }).collect();
        serde_json::to_string(&traffic).unwrap_or_else(|_| "[]".to_string())
    }

    /// Get system metrics as JSON for dashboard
    #[wasm_bindgen]
    pub fn get_system_metrics_json(&self) -> String {
        let m = self.kernel.get_system_metrics();
        serde_json::to_string(&serde_json::json!({
            "process_count": m.process_count,
            "total_memory": m.total_memory,
            "endpoint_count": m.endpoint_count,
            "total_pending_messages": m.total_pending_messages,
            "total_ipc_messages": m.total_ipc_messages,
            "uptime_ns": m.uptime_ns
        })).unwrap_or_else(|_| "{}".to_string())
    }

    /// Poll and process syscalls from Worker SharedArrayBuffer mailboxes
    /// 
    /// This is the main syscall processing loop. Workers block on WASM atomics
    /// waiting for their syscall to be processed. This method:
    /// 1. Polls all worker mailboxes for pending syscalls
    /// 2. Processes each syscall through the kernel
    /// 3. Writes the result and wakes the worker via Atomics.notify()
    /// 
    /// Should be called frequently (e.g., every frame or via setInterval).
    /// Returns the number of syscalls processed.
    #[wasm_bindgen]
    pub fn poll_syscalls(&mut self) -> usize {
        let pending = self.kernel.hal().poll_syscalls();
        let count = pending.len();
        
        for syscall in pending {
            let pid = ProcessId(syscall.pid);
            
            // Read syscall data from mailbox if needed
            let data = self.kernel.hal().read_syscall_data(syscall.pid);
            
            // Process the syscall
            let result = self.process_syscall(pid, syscall.syscall_num, syscall.args, &data);
            
            // Write result and wake worker
            self.kernel.hal().complete_syscall(syscall.pid, result);
        }
        
        // Progress the ping-pong test state machine if running
        self.progress_pingpong_test();
        
        count
    }
    
    /// Process a single syscall and return the result code
    fn process_syscall(&mut self, pid: ProcessId, syscall_num: u32, args: [u32; 3], data: &[u8]) -> i32 {
        // Check if process exists in kernel
        if self.kernel.get_process(pid).is_none() {
            log(&format!("[supervisor] Syscall from unknown process {}", pid.0));
            return -1;
        }

        // Determine if this syscall should be logged (skip read-only/polling syscalls)
        // Only log syscalls that cause state changes
        let should_log = match syscall_num {
            0x00 => false, // NOP - no state change
            0x01 => true,  // SYS_DEBUG - has side effect (output)
            0x02 => false, // SYS_GET_TIME - read-only
            0x03 => false, // SYS_GET_PID - read-only
            0x04 => false, // SYS_LIST_CAPS - read-only
            0x05 => false, // SYS_LIST_PROCS - read-only
            0x11 => true,  // SYS_EXIT - state change
            0x12 => false, // SYS_YIELD - no state change
            0x35 => true,  // SYS_EP_CREATE - state change
            0x40 => true,  // SYS_SEND - state change
            0x41 => false, // SYS_RECEIVE - logged only if message received (handled below)
            _ => true,     // Unknown syscalls - log them
        };

        // Log syscall request to SysLog for state-changing syscalls
        let args4 = [args[0], args[1], args[2], 0];
        let request_id = if should_log {
            Some(self.kernel.log_syscall_request(pid, syscall_num, args4))
        } else {
            None
        };

        // Parse syscall based on syscall_num (canonical ABI from orbital-process)
        // Syscall number ranges:
        // 0x00-0x0F: Misc (debug, info, time)
        // 0x10-0x1F: Thread (exit, yield)
        // 0x30-0x3F: Capability (grant, revoke, etc.)
        // 0x40-0x4F: IPC (send, receive)
        // 0x50-0x5F: Process (list processes)
        let result = match syscall_num {
            0x00 => 0, // NOP
            
            // === Misc syscalls (0x01-0x05) ===
            0x01 => {
                // SYS_DEBUG - print the data as a string
                if let Ok(s) = std::str::from_utf8(data) {
                    log(&format!("[process {}] {}", pid.0, s));
                    // Also output to terminal UI
                    self.write_console(&format!("[P{}] {}\n", pid.0, s));
                    
                    // Check if this is the end of a pingpong test
                    if s.contains("========================================") {
                        self.check_pingpong_complete(pid.0);
                    }
                }
                0
            }
            0x02 => {
                // SYS_GET_TIME - get current time in nanoseconds
                // args[0] = 0 for low 32 bits, 1 for high 32 bits
                let nanos = self.kernel.uptime_nanos();
                if args[0] == 0 {
                    (nanos & 0xFFFFFFFF) as i32
                } else {
                    ((nanos >> 32) & 0xFFFFFFFF) as i32
                }
            }
            0x03 => {
                // SYS_GET_PID - get own process ID
                pid.0 as i32
            }
            0x04 => {
                // SYS_LIST_CAPS - list capabilities
                match self.kernel.handle_syscall(pid, Syscall::ListCaps) {
                    SyscallResult::CapList(caps) => {
                        let mut bytes = Vec::new();
                        bytes.extend_from_slice(&(caps.len() as u32).to_le_bytes());
                        for (slot, cap) in caps {
                            bytes.extend_from_slice(&slot.to_le_bytes());
                            bytes.push(cap.object_type as u8);
                            bytes.extend_from_slice(&cap.object_id.to_le_bytes());
                        }
                        self.kernel.hal().write_syscall_data(pid.0, &bytes);
                        0
                    }
                    _ => -1,
                }
            }
            0x05 => {
                // SYS_LIST_PROCS - list all processes
                match self.kernel.handle_syscall(pid, Syscall::ListProcesses) {
                    SyscallResult::ProcessList(procs) => {
                        let mut bytes = Vec::new();
                        bytes.extend_from_slice(&(procs.len() as u32).to_le_bytes());
                        for (proc_pid, name, _state) in procs {
                            bytes.extend_from_slice(&(proc_pid.0 as u32).to_le_bytes());
                            bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
                            bytes.extend_from_slice(name.as_bytes());
                        }
                        self.kernel.hal().write_syscall_data(pid.0, &bytes);
                        0
                    }
                    _ => -1,
                }
            }
            
            // === Thread syscalls (0x11-0x12) ===
            0x11 => {
                // SYS_EXIT - terminate the process
                let exit_code = args[0] as i32;
                log(&format!("[supervisor] Process {} exiting with code {}", pid.0, exit_code));
                self.kernel.kill_process(pid);
                let handle = WasmProcessHandle::new(pid.0);
                let _ = self.kernel.hal().kill_process(&handle);
                0
            }
            0x12 => {
                // SYS_YIELD - cooperative yield (no-op, worker already yielded by waiting)
                0
            }
            
            // === Capability syscalls (0x35) ===
            0x35 => {
                // SYS_EP_CREATE - create an IPC endpoint
                match self.kernel.handle_syscall(pid, Syscall::CreateEndpoint) {
                    SyscallResult::Ok(v) => v as i32,
                    SyscallResult::Err(_) => -1,
                    _ => -1,
                }
            }
            
            // === IPC syscalls (0x40-0x41) ===
            0x40 => {
                // SYS_SEND - send message via IPC
                let slot = args[0];
                let tag = args[1];
                let syscall = Syscall::Send {
                    endpoint_slot: slot,
                    tag,
                    data: data.to_vec(),
                };
                match self.kernel.handle_syscall(pid, syscall) {
                    SyscallResult::Ok(v) => v as i32,
                    SyscallResult::Err(_) => -1,
                    _ => -1,
                }
            }
            0x41 => {
                // SYS_RECEIVE - receive message from endpoint
                let slot = args[0];
                let syscall = Syscall::Receive { endpoint_slot: slot };
                match self.kernel.handle_syscall(pid, syscall) {
                    SyscallResult::Ok(_) => 0,
                    SyscallResult::Message(msg) => {
                        // Log successful receive (this is a state change)
                        let recv_req_id = self.kernel.log_syscall_request(pid, syscall_num, args4);
                        self.kernel.log_syscall_response(pid, recv_req_id, 1);
                        
                        // Write message to worker's mailbox
                        let mut msg_bytes = Vec::new();
                        msg_bytes.extend_from_slice(&(msg.from.0 as u32).to_le_bytes());
                        msg_bytes.extend_from_slice(&msg.tag.to_le_bytes());
                        msg_bytes.extend_from_slice(&msg.data);
                        self.kernel.hal().write_syscall_data(pid.0, &msg_bytes);
                        1 // Has message
                    }
                    SyscallResult::WouldBlock => 0,
                    SyscallResult::Err(_) => -1,
                    _ => 0,
                }
            }
            
            _ => {
                log(&format!("[supervisor] Unknown syscall 0x{:x} from process {}", syscall_num, pid.0));
                -1
            }
        };

        // Log syscall response to SysLog (only for state-changing syscalls)
        if let Some(req_id) = request_id {
            self.kernel.log_syscall_response(pid, req_id, result as i64);
        }

        result
    }

    /// Process pending messages from Workers (legacy postMessage path)
    /// 
    /// This should be called periodically (e.g., via setInterval) to:
    /// - Handle syscalls from Workers
    /// - Update memory sizes from Workers
    /// - Clean up terminated Workers
    /// 
    /// Processes up to MAX_MESSAGES_PER_BATCH to avoid blocking the event loop.
    /// Returns the number of messages processed.
    #[wasm_bindgen]
    pub fn process_worker_messages(&mut self) -> usize {
        const MAX_MESSAGES_PER_BATCH: usize = 5000; // Balance throughput with async task handling
        
        let incoming = self.kernel.hal().incoming_messages();
        let messages: Vec<WorkerMessage> = {
            if let Ok(mut queue) = incoming.lock() {
                // Only take up to MAX_MESSAGES_PER_BATCH
                let take_count = queue.len().min(MAX_MESSAGES_PER_BATCH);
                queue.drain(..take_count).collect()
            } else {
                return 0;
            }
        };

        let count = messages.len();
        
        for msg in messages {
            match msg.msg_type {
                WorkerMessageType::Ready { memory_size } => {
                    // Update memory size for the process
                    self.kernel.hal().update_process_memory(msg.pid, memory_size);
                    log(&format!("[supervisor] Worker {} ready, memory: {} bytes", msg.pid, memory_size));
                }
                WorkerMessageType::Syscall { syscall_num, args } => {
                    // Route syscall to kernel
                    self.handle_worker_syscall(msg.pid, syscall_num, args, &msg.data);
                }
                WorkerMessageType::Error { ref message } => {
                    log(&format!("[supervisor] Worker {} error: {}", msg.pid, message));
                    // Could kill the process here if desired
                }
                WorkerMessageType::Terminated => {
                    log(&format!("[supervisor] Worker {} terminated", msg.pid));
                    // The worker is already terminated, just clean up kernel state if needed
                    let pid = ProcessId(msg.pid);
                    if self.kernel.get_process(pid).is_some() {
                        self.kernel.kill_process(pid);
                    }
                }
                WorkerMessageType::MemoryUpdate { memory_size } => {
                    // Update memory size for the process
                    self.kernel.hal().update_process_memory(msg.pid, memory_size);
                    // Also update kernel's record
                    let pid = ProcessId(msg.pid);
                    if let Some(proc) = self.kernel.get_process_mut(pid) {
                        proc.metrics.memory_size = memory_size;
                    }
                }
                WorkerMessageType::Yield => {
                    // Worker yielded - nothing to do, just acknowledge
                    // This is used for cooperative scheduling
                }
            }
        }

        count
    }

    /// Handle a syscall from a Worker process
    fn handle_worker_syscall(&mut self, pid: u64, syscall_num: u32, args: [u32; 3], data: &[u8]) {
        let process_id = ProcessId(pid);
        
        // Check if process exists in kernel
        if self.kernel.get_process(process_id).is_none() {
            log(&format!("[supervisor] Syscall from unknown process {}", pid));
            return;
        }

        // Parse syscall based on syscall_num
        // Syscall numbers from orbital-process crate:
        // 1 = DEBUG, 2 = CREATE_ENDPOINT, 3 = SEND, 4 = RECEIVE, 5 = LIST_CAPS
        // 6 = LIST_PROCESSES, 7 = EXIT, 8 = GET_TIME, 9 = YIELD
        let result = match syscall_num {
            0 => {
                // NOP
                SyscallResult::Ok(0)
            }
            1 => {
                // SYS_DEBUG - print the data as a string
                if let Ok(s) = std::str::from_utf8(data) {
                    // Check for special INIT: commands
                    if s.starts_with("INIT:SPAWN:") {
                        // Init process requesting to spawn a service
                        let service_name = &s[11..]; // After "INIT:SPAWN:"
                        log(&format!("[supervisor] Init requesting spawn of '{}'", service_name));
                        self.request_spawn(service_name, service_name);
                    } else if s.starts_with("INIT:") {
                        // Other init commands
                        log(&format!("[init] {}", &s[5..]));
                    } else {
                        log(&format!("[process {}] {}", pid, s));
                    }
                }
                SyscallResult::Ok(0)
            }
            2 => {
                // SYS_CREATE_ENDPOINT - create an IPC endpoint
                let syscall = Syscall::CreateEndpoint;
                self.kernel.handle_syscall(process_id, syscall)
            }
            3 => {
                // SYS_SEND - send message via IPC
                // args[0] = target endpoint slot in our cap space
                // args[1] = tag
                // data = message payload
                let slot = args[0];
                let tag = args[1];
                
                // Use kernel's IPC send
                let syscall = Syscall::Send {
                    endpoint_slot: slot,
                    tag,
                    data: data.to_vec(),
                };
                self.kernel.handle_syscall(process_id, syscall)
            }
            4 => {
                // SYS_RECEIVE - receive message from endpoint
                // args[0] = endpoint slot
                let slot = args[0];
                let syscall = Syscall::Receive { endpoint_slot: slot };
                self.kernel.handle_syscall(process_id, syscall)
            }
            5 => {
                // SYS_LIST_CAPS - list capabilities
                let syscall = Syscall::ListCaps;
                self.kernel.handle_syscall(process_id, syscall)
            }
            6 => {
                // SYS_LIST_PROCESSES - list all processes
                let syscall = Syscall::ListProcesses;
                self.kernel.handle_syscall(process_id, syscall)
            }
            7 => {
                // SYS_EXIT - terminate the process
                let exit_code = args[0] as i32;
                log(&format!("[supervisor] Process {} exiting with code {}", pid, exit_code));
                self.kernel.kill_process(process_id);
                // Also terminate the Worker
                let handle = WasmProcessHandle::new(pid);
                let _ = self.kernel.hal().kill_process(&handle);
                SyscallResult::Ok(0)
            }
            8 => {
                // SYS_GET_TIME - get current time
                let syscall = Syscall::GetTime;
                self.kernel.handle_syscall(process_id, syscall)
            }
            9 => {
                // SYS_YIELD - cooperative yield (no-op for now, Worker yielded by returning)
                SyscallResult::Ok(0)
            }
            _ => {
                log(&format!("[supervisor] Unknown syscall {} from process {}", syscall_num, pid));
                SyscallResult::Err(orbital_kernel::KernelError::PermissionDenied)
            }
        };

        // Send result back to Worker
        self.send_syscall_result(pid, result);
    }

    /// Deliver pending IPC messages to Workers
    /// 
    /// This checks each process's endpoints for queued messages
    /// and delivers them to the corresponding Workers.
    /// 
    /// Returns the number of messages delivered
    #[wasm_bindgen]
    pub fn deliver_pending_messages(&mut self) -> usize {
        let mut delivered = 0;
        
        // Get list of processes with their PIDs
        let pids: Vec<ProcessId> = self.kernel.list_processes()
            .iter()
            .map(|(pid, _)| *pid)
            .collect();
        
        for pid in pids {
            // All processes are now real Workers, including init (PID 1) and terminal (PID 2)
            
            // Get the process's capability space to find endpoints
            if let Some(cap_space) = self.kernel.get_cap_space(pid) {
                for (slot, cap) in cap_space.list() {
                    if cap.object_type == orbital_kernel::ObjectType::Endpoint {
                        // Try to receive a message from this endpoint
                        if let Ok(Some(msg)) = self.kernel.ipc_receive(pid, slot) {
                            // Deliver to Worker
                            self.deliver_ipc_message(pid.0, &msg);
                            delivered += 1;
                        }
                    }
                }
            }
        }
        
        delivered
    }

    /// Deliver an IPC message to a Worker
    fn deliver_ipc_message(&self, pid: u64, msg: &orbital_kernel::Message) {
        if let Ok(processes) = self.kernel.hal().processes.lock() {
            if let Some(proc) = processes.get(&pid) {
                // Serialize message
                let mut bytes = Vec::new();
                bytes.extend_from_slice(&msg.from.0.to_le_bytes());
                bytes.extend_from_slice(&msg.tag.to_le_bytes());
                bytes.extend_from_slice(&msg.data);
                
                // Post to Worker
                let js_msg = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&js_msg, &"type".into(), &"ipc".into());
                let array = js_sys::Uint8Array::from(&bytes[..]);
                let _ = js_sys::Reflect::set(&js_msg, &"data".into(), &array);
                let _ = proc.worker.post_message(&js_msg);
            }
        }
    }

    /// Send syscall result back to a Worker
    fn send_syscall_result(&self, pid: u64, result: SyscallResult) {
        // Serialize the result
        let result_bytes = match result {
            SyscallResult::Ok(value) => {
                let mut bytes = vec![0u8]; // 0 = success
                bytes.extend_from_slice(&value.to_le_bytes());
                bytes
            }
            SyscallResult::Err(e) => {
                let mut bytes = vec![1u8]; // 1 = error
                let err_code = match e {
                    orbital_kernel::KernelError::ProcessNotFound => 1,
                    orbital_kernel::KernelError::EndpointNotFound => 2,
                    orbital_kernel::KernelError::InvalidCapability => 3,
                    orbital_kernel::KernelError::PermissionDenied => 4,
                    orbital_kernel::KernelError::WouldBlock => 5,
                    orbital_kernel::KernelError::Hal(_) => 6,
                };
                bytes.push(err_code);
                bytes
            }
            SyscallResult::WouldBlock => {
                vec![5u8] // 5 = would block
            }
            SyscallResult::Message(msg) => {
                let mut bytes = vec![2u8]; // 2 = message
                bytes.extend_from_slice(&msg.from.0.to_le_bytes());
                bytes.extend_from_slice(&msg.tag.to_le_bytes());
                bytes.extend_from_slice(&(msg.data.len() as u32).to_le_bytes());
                bytes.extend_from_slice(&msg.data);
                bytes
            }
            SyscallResult::CapList(caps) => {
                let mut bytes = vec![3u8]; // 3 = cap list
                bytes.extend_from_slice(&(caps.len() as u32).to_le_bytes());
                for (slot, cap) in caps {
                    bytes.extend_from_slice(&slot.to_le_bytes());
                    bytes.push(cap.object_type as u8);
                    bytes.extend_from_slice(&cap.object_id.to_le_bytes());
                }
                bytes
            }
            SyscallResult::ProcessList(procs) => {
                // Encode process list for syscall result
                let mut bytes = vec![4u8]; // 4 = process list
                bytes.extend_from_slice(&(procs.len() as u32).to_le_bytes());
                for (pid, name, _state) in procs {
                    bytes.extend_from_slice(&pid.0.to_le_bytes());
                    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
                    bytes.extend_from_slice(name.as_bytes());
                }
                bytes
            }
            SyscallResult::MessageWithCaps(msg, slots) => {
                // Message with transferred capability slots
                let mut bytes = vec![6u8]; // 6 = message with caps
                bytes.extend_from_slice(&msg.from.0.to_le_bytes());
                bytes.extend_from_slice(&msg.tag.to_le_bytes());
                bytes.extend_from_slice(&(msg.data.len() as u32).to_le_bytes());
                bytes.extend_from_slice(&msg.data);
                // Append capability slots
                bytes.extend_from_slice(&(slots.len() as u32).to_le_bytes());
                for slot in slots {
                    bytes.extend_from_slice(&slot.to_le_bytes());
                }
                bytes
            }
            SyscallResult::CapInfo(info) => {
                // Capability inspection result
                let mut bytes = vec![7u8]; // 7 = cap info
                bytes.push(info.object_type as u8);
                bytes.push(
                    (if info.permissions.read { 1 } else { 0 })
                    | (if info.permissions.write { 2 } else { 0 })
                    | (if info.permissions.grant { 4 } else { 0 })
                );
                bytes.extend_from_slice(&info.object_id.to_le_bytes());
                bytes.extend_from_slice(&info.id.to_le_bytes());
                bytes.extend_from_slice(&info.generation.to_le_bytes());
                bytes.extend_from_slice(&info.expires_at.to_le_bytes());
                bytes
            }
        };

        // Post message to Worker
        if let Ok(processes) = self.kernel.hal().processes.lock() {
            if let Some(proc) = processes.get(&pid) {
                let msg = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&msg, &"type".into(), &"syscall_result".into());
                let _ = js_sys::Reflect::set(&msg, &"result".into(), &JsValue::from(0));
                let array = js_sys::Uint8Array::from(&result_bytes[..]);
                let _ = js_sys::Reflect::set(&msg, &"data".into(), &array);
                let _ = proc.worker.post_message(&msg);
            }
        }
    }

    // =========================================================================
    // Desktop Environment API
    // =========================================================================

    /// Initialize the desktop environment with screen dimensions
    #[wasm_bindgen]
    pub fn init_desktop(&mut self, width: f32, height: f32) {
        log(&format!("[desktop] Initializing desktop {}x{}", width, height));
        self.desktop.init(width, height);

        // Create default Terminal window on boot (uses viewport-aware positioning)
        let window_id = self.desktop.launch_app("terminal");
        log(&format!("[desktop] Created Terminal window {}", window_id));
    }

    /// Resize the desktop viewport
    #[wasm_bindgen]
    pub fn resize_desktop(&mut self, width: f32, height: f32) {
        self.desktop.resize(width, height);
    }

    /// Pan the desktop viewport
    #[wasm_bindgen]
    pub fn desktop_pan(&mut self, dx: f32, dy: f32) {
        self.desktop.pan(dx, dy);
    }

    /// Zoom the desktop viewport at anchor point
    #[wasm_bindgen]
    pub fn desktop_zoom(&mut self, factor: f32, anchor_x: f32, anchor_y: f32) {
        self.desktop.zoom_at(factor, anchor_x, anchor_y);
    }

    /// Get window screen rects as JSON for React positioning
    /// Note: Takes &self (not &mut self) to avoid wasm-bindgen RefCell borrow
    /// conflicts when called concurrently with pointer event handlers.
    #[wasm_bindgen]
    pub fn get_window_screen_rects_json(&self) -> String {
        let rects = self.desktop.get_window_screen_rects();
        let json_rects: Vec<serde_json::Value> = rects
            .into_iter()
            .enumerate()
            .map(|(z_order, r)| {
                serde_json::json!({
                    "id": r.id,
                    "title": r.title,
                    "appId": r.app_id,
                    "state": match r.state {
                        desktop::WindowState::Normal => "normal",
                        desktop::WindowState::Minimized => "minimized",
                        desktop::WindowState::Maximized => "maximized",
                        desktop::WindowState::Fullscreen => "fullscreen",
                    },
                    "focused": r.focused,
                    "zOrder": z_order,
                    "screenRect": {
                        "x": r.screen_rect.x,
                        "y": r.screen_rect.y,
                        "width": r.screen_rect.width,
                        "height": r.screen_rect.height
                    }
                })
            })
            .collect();
        serde_json::to_string(&json_rects).unwrap_or_else(|_| "[]".to_string())
    }

    /// Create a new window
    #[wasm_bindgen]
    pub fn create_window(&mut self, title: &str, x: f32, y: f32, w: f32, h: f32, app_id: &str) -> u64 {
        let config = desktop::WindowConfig {
            title: title.to_string(),
            position: Some(desktop::Vec2::new(x, y)),
            size: desktop::Size::new(w, h),
            min_size: Some(desktop::Size::new(200.0, 150.0)),
            max_size: None,
            app_id: app_id.to_string(),
            process_id: None,
        };
        self.desktop.create_window(config)
    }

    /// Close a window
    #[wasm_bindgen]
    pub fn close_window(&mut self, id: u64) {
        self.desktop.close_window(id);
    }

    /// Focus a window
    #[wasm_bindgen]
    pub fn focus_window(&mut self, id: u64) {
        self.desktop.focus_window(id);
    }

    /// Move a window
    #[wasm_bindgen]
    pub fn move_window(&mut self, id: u64, x: f32, y: f32) {
        self.desktop.move_window(id, x, y);
    }

    /// Resize a window
    #[wasm_bindgen]
    pub fn resize_window(&mut self, id: u64, w: f32, h: f32) {
        self.desktop.resize_window(id, w, h);
    }

    /// Minimize a window
    #[wasm_bindgen]
    pub fn minimize_window(&mut self, id: u64) {
        self.desktop.minimize_window(id);
    }

    /// Maximize or restore a window
    #[wasm_bindgen]
    pub fn maximize_window(&mut self, id: u64) {
        self.desktop.maximize_window(id);
    }

    /// Restore a minimized window
    #[wasm_bindgen]
    pub fn restore_window(&mut self, id: u64) {
        self.desktop.restore_window(id);
    }

    /// Get the currently focused window ID
    #[wasm_bindgen]
    pub fn get_focused_window(&self) -> Option<u64> {
        self.desktop.windows.focused()
    }

    /// Get all windows in the active workspace as JSON
    #[wasm_bindgen]
    pub fn get_windows_json(&self) -> String {
        let active_workspace = self.desktop.workspaces.active_workspace();
        
        // Sort by ID (creation order) for stable taskbar ordering
        // Filter to only windows in the active workspace
        let mut windows_vec: Vec<_> = self.desktop.windows.all_windows()
            .filter(|w| active_workspace.contains_window(w.id))
            .collect();
        windows_vec.sort_by_key(|w| w.id);
        
        let focused_id = self.desktop.windows.focused();
        
        let windows: Vec<serde_json::Value> = windows_vec
            .iter()
            .map(|w| {
                serde_json::json!({
                    "id": w.id,
                    "title": w.title,
                    "appId": w.app_id,
                    "position": { "x": w.position.x, "y": w.position.y },
                    "size": { "width": w.size.width, "height": w.size.height },
                    "state": match w.state {
                        desktop::WindowState::Normal => "normal",
                        desktop::WindowState::Minimized => "minimized",
                        desktop::WindowState::Maximized => "maximized",
                        desktop::WindowState::Fullscreen => "fullscreen",
                    },
                    "zOrder": w.z_order,
                    "focused": focused_id == Some(w.id)
                })
            })
            .collect();
        serde_json::to_string(&windows).unwrap_or_else(|_| "[]".to_string())
    }

    /// Create a new workspace
    #[wasm_bindgen]
    pub fn create_workspace(&mut self, name: &str) -> u32 {
        self.desktop.create_workspace(name)
    }

    /// Switch to a workspace by index
    #[wasm_bindgen]
    pub fn switch_workspace(&mut self, index: u32) {
        self.desktop.switch_workspace(index as usize);
    }

    /// Get all workspaces as JSON
    #[wasm_bindgen]
    pub fn get_workspaces_json(&self) -> String {
        let active = self.desktop.workspaces.active_index();
        let workspaces: Vec<serde_json::Value> = self
            .desktop
            .workspaces
            .workspaces()
            .iter()
            .enumerate()
            .map(|(i, ws)| {
                serde_json::json!({
                    "id": ws.id,
                    "name": ws.name,
                    "active": i == active,
                    "windowCount": ws.windows.len()
                })
            })
            .collect();
        serde_json::to_string(&workspaces).unwrap_or_else(|_| "[]".to_string())
    }

    /// Get active workspace index
    #[wasm_bindgen]
    pub fn get_active_workspace(&self) -> u32 {
        self.desktop.workspaces.active_index() as u32
    }

    /// Handle pointer down event - returns JSON with result
    #[wasm_bindgen]
    pub fn desktop_pointer_down(&mut self, x: f32, y: f32, button: u8, ctrl: bool, shift: bool) -> String {
        let result = self.desktop.handle_pointer_down(x, y, button, ctrl, shift);
        serde_json::to_string(&result).unwrap_or_else(|_| r#"{"type":"unhandled"}"#.to_string())
    }

    /// Handle pointer move event - returns JSON with result
    #[wasm_bindgen]
    pub fn desktop_pointer_move(&mut self, x: f32, y: f32) -> String {
        let result = self.desktop.handle_pointer_move(x, y);
        serde_json::to_string(&result).unwrap_or_else(|_| r#"{"type":"unhandled"}"#.to_string())
    }

    /// Handle pointer up event - returns JSON with result
    #[wasm_bindgen]
    pub fn desktop_pointer_up(&mut self) -> String {
        let result = self.desktop.handle_pointer_up();
        serde_json::to_string(&result).unwrap_or_else(|_| r#"{"type":"unhandled"}"#.to_string())
    }

    /// Start a window resize drag operation directly
    /// Called by React resize handles to bypass hit testing
    #[wasm_bindgen]
    pub fn start_window_resize(&mut self, window_id: u64, direction: &str, x: f32, y: f32) {
        self.desktop.start_resize_drag(window_id, direction, x, y);
    }

    /// Handle wheel event - returns JSON with result
    #[wasm_bindgen]
    pub fn desktop_wheel(&mut self, dx: f32, dy: f32, x: f32, y: f32, ctrl: bool) -> String {
        let result = self.desktop.handle_wheel(dx, dy, x, y, ctrl);
        serde_json::to_string(&result).unwrap_or_else(|_| r#"{"type":"unhandled"}"#.to_string())
    }

    /// Launch an application (creates window)
    #[wasm_bindgen]
    pub fn launch_app(&mut self, app_id: &str) -> u64 {
        log(&format!("[desktop] Launching app: {}", app_id));
        self.desktop.launch_app(app_id)
    }

    /// Get viewport state as JSON
    #[wasm_bindgen]
    pub fn get_viewport_json(&self) -> String {
        let center = self.desktop.viewport.center();
        serde_json::to_string(&serde_json::json!({
            "center": { "x": center.x, "y": center.y },
            "zoom": self.desktop.viewport.zoom,
            "screenSize": {
                "width": self.desktop.viewport.screen_size.width,
                "height": self.desktop.viewport.screen_size.height
            }
        }))
        .unwrap_or_else(|_| "{}".to_string())
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert days since Unix epoch to (year, month, day)
fn days_to_ymd(days: i64) -> (i64, i64, i64) {
    // Algorithm from Howard Hinnant's date algorithms
    // http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // day of era
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year
    let mp = (5 * doy + 2) / 153; // month proxy
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as i64, d as i64)
}

/// Format bytes as human-readable string
fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

/// Truncate a string to a maximum length
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
