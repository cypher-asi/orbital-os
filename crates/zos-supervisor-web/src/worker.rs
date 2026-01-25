//! Worker process types and message handling

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, Worker};
use zos_hal::ProcessMessageType;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

/// Process handle for WASM - wraps a Web Worker
#[derive(Clone, Debug)]
pub struct WasmProcessHandle {
    /// Process ID assigned by the HAL
    pub id: u64,
}

impl WasmProcessHandle {
    pub(crate) fn new(id: u64) -> Self {
        Self { id }
    }
}

// Implement Send + Sync for WasmProcessHandle
// In WASM, there's only one thread, so this is safe
unsafe impl Send for WasmProcessHandle {}
unsafe impl Sync for WasmProcessHandle {}

/// Internal state for a real Web Worker process
pub(crate) struct WorkerProcess {
    /// Human-readable name
    #[allow(dead_code)]
    pub name: String,
    /// The actual Web Worker
    pub worker: Worker,
    /// Whether the process is still alive
    pub alive: bool,
    /// Memory size reported by the Worker
    pub memory_size: usize,
    /// Worker memory context ID (performance.timeOrigin from browser)
    /// Format: worker:<timestamp> e.g., "worker:1737158400123"
    pub worker_id: u64,
    /// SharedArrayBuffer for syscall mailbox (worker's WASM memory)
    pub syscall_buffer: js_sys::SharedArrayBuffer,
    /// Int32Array view for atomic operations on the mailbox
    pub mailbox_view: js_sys::Int32Array,
    /// Closures must be stored to prevent garbage collection
    #[allow(dead_code)]
    pub onerror_closure: Closure<dyn FnMut(JsValue)>,
    #[allow(dead_code)]
    pub _onmessage_closure: Closure<dyn FnMut(MessageEvent)>,
}

// Safety: In WASM, there is only one thread. These types contain JS references
// that are not Send/Sync in the general case, but are safe in single-threaded WASM.
unsafe impl Send for WorkerProcess {}
unsafe impl Sync for WorkerProcess {}

// Mailbox status values (must match worker.js)
pub const STATUS_IDLE: i32 = 0;
pub const STATUS_PENDING: i32 = 1;
pub const STATUS_READY: i32 = 2;

// Mailbox field offsets in i32 units (must match worker.js)
pub const MAILBOX_STATUS: u32 = 0;
pub const MAILBOX_SYSCALL_NUM: u32 = 1;
pub const MAILBOX_ARG0: u32 = 2;
pub const MAILBOX_ARG1: u32 = 3;
pub const MAILBOX_ARG2: u32 = 4;
pub const MAILBOX_RESULT: u32 = 5;
pub const MAILBOX_DATA_LEN: u32 = 6;
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

/// Parse worker message from MessageEvent
#[allow(dead_code)]
pub(crate) fn parse_worker_message(
    pid: u64,
    event: &MessageEvent,
) -> Result<WorkerMessage, JsValue> {
    let data = event.data();

    // Handle object messages
    if data.is_object() {
        // Safe: we just verified data.is_object() is true
        let obj = data
            .dyn_ref::<js_sys::Object>()
            .ok_or_else(|| JsValue::from_str("Expected object"))?;

        let msg_type = js_sys::Reflect::get(obj, &"type".into())?
            .as_string()
            .unwrap_or_default();

        match msg_type.as_str() {
            "ready" => parse_ready_message(pid, obj),
            "syscall" => parse_syscall_message(pid, obj),
            "error" => parse_error_message(pid, obj),
            "terminated" => Ok(create_terminated_message(pid)),
            "worker_loaded" => {
                log(&format!("[wasm-hal] Worker {} script loaded", pid));
                Ok(WorkerMessage {
                    pid,
                    msg_type: WorkerMessageType::Ready { memory_size: 0 },
                    data: Vec::new(),
                })
            }
            "memory_update" => parse_memory_update_message(pid, obj),
            "yield" => Ok(WorkerMessage {
                pid,
                msg_type: WorkerMessageType::Yield,
                data: Vec::new(),
            }),
            _ => {
                log(&format!(
                    "[wasm-hal] Unknown message type from worker {}: {}",
                    pid, msg_type
                ));
                Err(JsValue::from_str("Unknown message type"))
            }
        }
    } else {
        Err(JsValue::from_str("Invalid message format"))
    }
}

/// Parse ready message
fn parse_ready_message(pid: u64, obj: &js_sys::Object) -> Result<WorkerMessage, JsValue> {
    let memory_size = js_sys::Reflect::get(obj, &"memory_size".into())
        .ok()
        .and_then(|v| v.as_f64())
        .map(|v| v as usize)
        .unwrap_or(65536);

    log(&format!(
        "[wasm-hal] Worker {} ready, memory: {} bytes",
        pid, memory_size
    ));

    Ok(WorkerMessage {
        pid,
        msg_type: WorkerMessageType::Ready { memory_size },
        data: Vec::new(),
    })
}

/// Parse syscall message
fn parse_syscall_message(pid: u64, obj: &js_sys::Object) -> Result<WorkerMessage, JsValue> {
    let syscall_num = js_sys::Reflect::get(obj, &"syscall".into())
        .ok()
        .and_then(|v| v.as_f64())
        .map(|v| v as u32)
        .unwrap_or(0);

    let args_val = js_sys::Reflect::get(obj, &"args".into())?;
    let args_array = args_val
        .dyn_ref::<js_sys::Array>()
        .ok_or_else(|| JsValue::from_str("Expected args array"))?;
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

/// Parse error message
fn parse_error_message(pid: u64, obj: &js_sys::Object) -> Result<WorkerMessage, JsValue> {
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

/// Create terminated message
fn create_terminated_message(pid: u64) -> WorkerMessage {
    WorkerMessage {
        pid,
        msg_type: WorkerMessageType::Terminated,
        data: Vec::new(),
    }
}

/// Parse memory update message
fn parse_memory_update_message(pid: u64, obj: &js_sys::Object) -> Result<WorkerMessage, JsValue> {
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

/// Serialize a WorkerMessage to bytes for poll_messages
pub(crate) fn serialize_worker_message(msg: &WorkerMessage) -> Vec<u8> {
    let mut result = Vec::new();

    match &msg.msg_type {
        WorkerMessageType::Ready { memory_size } => {
            serialize_ready(&mut result, msg.pid, *memory_size);
        }
        WorkerMessageType::Syscall { syscall_num, args } => {
            serialize_syscall(&mut result, msg, *syscall_num, args);
        }
        WorkerMessageType::Error { message } => {
            serialize_error(&mut result, msg.pid, message);
        }
        WorkerMessageType::Terminated => {
            serialize_terminated(&mut result, msg.pid);
        }
        WorkerMessageType::MemoryUpdate { memory_size } => {
            serialize_ready(&mut result, msg.pid, *memory_size);
        }
        WorkerMessageType::Yield => {
            serialize_yield(&mut result, msg.pid);
        }
    }

    result
}

/// Serialize ready message
fn serialize_ready(result: &mut Vec<u8>, pid: u64, memory_size: usize) {
    result.push(ProcessMessageType::Ready as u8);
    result.extend_from_slice(&(pid as u32).to_le_bytes());
    result.extend_from_slice(&(memory_size as u32).to_le_bytes());
}

/// Serialize syscall message
fn serialize_syscall(result: &mut Vec<u8>, msg: &WorkerMessage, syscall_num: u32, args: &[u32; 3]) {
    result.push(ProcessMessageType::Syscall as u8);
    result.extend_from_slice(&(msg.pid as u32).to_le_bytes());
    result.extend_from_slice(&syscall_num.to_le_bytes());
    for arg in args {
        result.extend_from_slice(&arg.to_le_bytes());
    }
    result.extend_from_slice(&msg.data);
}

/// Serialize error message
fn serialize_error(result: &mut Vec<u8>, pid: u64, message: &str) {
    result.push(ProcessMessageType::Error as u8);
    result.extend_from_slice(&(pid as u32).to_le_bytes());
    result.extend_from_slice(message.as_bytes());
}

/// Serialize terminated message
fn serialize_terminated(result: &mut Vec<u8>, pid: u64) {
    result.push(ProcessMessageType::Terminate as u8);
    result.extend_from_slice(&(pid as u32).to_le_bytes());
}

/// Serialize yield message
fn serialize_yield(result: &mut Vec<u8>, pid: u64) {
    result.push(7u8); // Use 7 for yield
    result.extend_from_slice(&(pid as u32).to_le_bytes());
}
