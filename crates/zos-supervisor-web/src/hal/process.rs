//! Process management for WASM HAL
//!
//! This module handles Web Worker lifecycle, message handling, and process operations.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, Worker};
use zos_hal::HalError;

use super::WasmHal;
use crate::worker::{WasmProcessHandle, WorkerProcess};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

/// Worker creation result with message and error handlers
type WorkerCreationResult = Result<
    (
        Worker,
        Closure<dyn FnMut(MessageEvent)>,
        Closure<dyn FnMut(JsValue)>,
    ),
    HalError,
>;

// === Worker Message Handlers ===

pub(crate) fn handle_worker_message(
    processes: &Arc<Mutex<HashMap<u64, WorkerProcess>>>,
    event: MessageEvent,
) {
    let data = event.data();
    let msg_type = js_sys::Reflect::get(&data, &"type".into());
    let type_str = msg_type.ok().and_then(|value| value.as_string());

    match type_str.as_deref() {
        Some("memory") => handle_worker_memory(processes, &data),
        Some("error") => handle_worker_error_message(&data),
        _ => {}
    }
}

pub(crate) fn handle_worker_memory(
    processes: &Arc<Mutex<HashMap<u64, WorkerProcess>>>,
    data: &JsValue,
) {
    let buffer_val = js_sys::Reflect::get(data, &"buffer".into());
    if let Ok(buffer_val) = buffer_val {
        let pid = js_sys::Reflect::get(data, &"pid".into())
            .ok()
            .and_then(|v| v.as_f64())
            .map(|v| v as u64)
            .unwrap_or(0);

        let worker_id = js_sys::Reflect::get(data, &"workerId".into())
            .ok()
            .and_then(|v| v.as_f64())
            .map(|v| v as u64)
            .unwrap_or(0);

        if let Ok(shared_buf) = buffer_val.clone().dyn_into::<js_sys::SharedArrayBuffer>() {
            let is_update = if let Ok(procs) = processes.lock() {
                procs.get(&pid).map(|p| p.worker_id != 0).unwrap_or(false)
            } else {
                false
            };
            
            if is_update {
                log(&format!(
                    "[wasm-hal] MEMORY UPDATE: Received NEW SharedArrayBuffer from worker:{} (PID {}) after memory growth, size: {} bytes",
                    worker_id, pid, shared_buf.byte_length()
                ));
            } else {
                log(&format!(
                    "[wasm-hal] SUCCESS: Received SharedArrayBuffer from worker:{} (PID {}), size: {} bytes",
                    worker_id, pid, shared_buf.byte_length()
                ));
            }
            
            let view = js_sys::Int32Array::new(&shared_buf);
            if let Ok(mut procs) = processes.lock() {
                if let Some(proc) = procs.get_mut(&pid) {
                    proc.syscall_buffer = shared_buf;
                    proc.mailbox_view = view;
                    proc.worker_id = worker_id;
                }
            }
        } else {
            let is_array_buffer = buffer_val.dyn_ref::<js_sys::ArrayBuffer>().is_some();
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
}

pub(crate) fn handle_worker_error_message(data: &JsValue) {
    if let Ok(err_val) = js_sys::Reflect::get(data, &"error".into()) {
        log(&format!("[wasm-hal] Worker error: {:?}", err_val));
    }
}

pub(crate) fn handle_worker_error(pid: u64, event: JsValue) {
    let msg = js_sys::Reflect::get(&event, &"message".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| "Unknown error".to_string());
    log(&format!("[wasm-hal] Worker {} error: {}", pid, msg));
}

// === Worker Creation Helpers ===

pub(crate) fn create_placeholder_buffers() -> (js_sys::SharedArrayBuffer, js_sys::Int32Array) {
    let syscall_buffer = js_sys::SharedArrayBuffer::new(16384);
    let mailbox_view = js_sys::Int32Array::new(&syscall_buffer);
    (syscall_buffer, mailbox_view)
}

pub(crate) fn create_worker_with_handlers(
    pid: u64,
    processes: Arc<Mutex<HashMap<u64, WorkerProcess>>>,
) -> WorkerCreationResult {
    let worker = Worker::new("/worker.js").map_err(|e| {
        log(&format!("[wasm-hal] Failed to create Worker: {:?}", e));
        HalError::ProcessSpawnFailed
    })?;

    let onmessage_closure = Closure::wrap(Box::new(move |event: MessageEvent| {
        handle_worker_message(&processes, event);
    }) as Box<dyn FnMut(MessageEvent)>);

    let onerror_closure = Closure::wrap(Box::new(move |event: JsValue| {
        handle_worker_error(pid, event);
    }) as Box<dyn FnMut(JsValue)>);

    worker.set_onmessage(Some(onmessage_closure.as_ref().unchecked_ref()));
    worker.set_onerror(Some(onerror_closure.as_ref().unchecked_ref()));

    Ok((worker, onmessage_closure, onerror_closure))
}

pub(crate) fn build_worker_process(
    name: &str,
    worker: Worker,
    syscall_buffer: js_sys::SharedArrayBuffer,
    mailbox_view: js_sys::Int32Array,
    onerror_closure: Closure<dyn FnMut(JsValue)>,
    onmessage_closure: Closure<dyn FnMut(MessageEvent)>,
) -> WorkerProcess {
    WorkerProcess {
        name: name.to_string(),
        worker,
        alive: true,
        memory_size: 65536, // Default, will be updated
        worker_id: 0,       // Will be set when worker sends its memory context ID
        syscall_buffer,
        mailbox_view,
        onerror_closure,
        _onmessage_closure: onmessage_closure,
    }
}

pub(crate) fn send_worker_init_message(
    worker: &Worker,
    pid: u64,
    binary: &[u8],
) -> Result<(), HalError> {
    let init_msg = js_sys::Object::new();
    let binary_array = js_sys::Uint8Array::from(binary);
    js_sys::Reflect::set(&init_msg, &"binary".into(), &binary_array)
        .map_err(|_| HalError::ProcessSpawnFailed)?;
    js_sys::Reflect::set(&init_msg, &"pid".into(), &(pid as f64).into())
        .map_err(|_| HalError::ProcessSpawnFailed)?;
    worker
        .post_message(&init_msg)
        .map_err(|_| HalError::ProcessSpawnFailed)?;
    Ok(())
}

// === WasmHal Process Management Methods ===

impl WasmHal {
    /// Spawn a process with a specific PID (used when kernel assigns the PID)
    pub fn spawn_with_pid(
        &self,
        pid: u64,
        name: &str,
        binary: &[u8],
    ) -> Result<WasmProcessHandle, HalError> {
        let handle = WasmProcessHandle::new(pid);

        let (worker, onmessage_closure, onerror_closure) =
            create_worker_with_handlers(pid, self.processes.clone())?;

        // Create a placeholder SharedArrayBuffer (will be replaced when worker sends real one)
        // Size: 16KB to support large IPC responses (e.g., PQ hybrid keys ~6KB)
        let (syscall_buffer, mailbox_view) = create_placeholder_buffers();

        // Send init message with WASM binary and PID
        send_worker_init_message(&worker, pid, binary)?;

        // Store the process (mailbox and worker_id will be updated when worker sends memory)
        let process = build_worker_process(
            name,
            worker,
            syscall_buffer,
            mailbox_view,
            onerror_closure,
            onmessage_closure,
        );

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

    /// Kill a process and clean up its resources
    pub fn do_kill_process(&self, handle: &WasmProcessHandle) -> Result<(), HalError> {
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

    /// Send an IPC message to a process
    pub fn do_send_to_process(
        &self,
        handle: &WasmProcessHandle,
        msg: &[u8],
    ) -> Result<(), HalError> {
        // Send IPC message to Worker
        self.post_to_worker(handle.id, "ipc", Some(msg))
    }

    /// Check if a process is alive
    pub fn do_is_process_alive(&self, handle: &WasmProcessHandle) -> bool {
        self.processes
            .lock()
            .ok()
            .and_then(|p| p.get(&handle.id).map(|proc| proc.alive))
            .unwrap_or(false)
    }

    /// Get the memory size of a process
    pub fn do_get_process_memory_size(
        &self,
        handle: &WasmProcessHandle,
    ) -> Result<usize, HalError> {
        self.processes
            .lock()
            .map_err(|_| HalError::ProcessNotFound)?
            .get(&handle.id)
            .filter(|p| p.alive)
            .map(|p| p.memory_size)
            .ok_or(HalError::ProcessNotFound)
    }
}
