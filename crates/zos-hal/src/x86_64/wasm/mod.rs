//! WASM Runtime for x86_64 Zero OS
//!
//! This module provides a WASM interpreter (via wasmi) for running Zero OS
//! services and applications on the QEMU/bare metal x86_64 target.
//!
//! # Architecture
//!
//! On x86_64, each process is a WASM module instance running within wasmi.
//! The supervisor executes WASM processes cooperatively, switching between
//! them when they yield or make blocking syscalls.
//!
//! ## Host Functions
//!
//! WASM processes communicate with the kernel through host functions:
//!
//! - `zos_syscall(num, arg1, arg2, arg3) -> u32` - Make a syscall
//! - `zos_send_bytes(ptr, len)` - Send bytes to syscall buffer
//! - `zos_recv_bytes(ptr, max_len) -> u32` - Receive bytes from syscall result
//! - `zos_yield()` - Yield execution
//! - `zos_get_pid() -> u32` - Get this process's PID
//!
//! ## Process Lifecycle
//!
//! 1. `spawn_process()` creates a new WASM instance
//! 2. The process's `_start` function is called
//! 3. Process makes syscalls via host functions
//! 4. `kill_process()` terminates the instance

pub mod host;
pub mod process;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use wasmi::{Engine, Linker, Module, Store};

use super::serial;
use crate::{HalError, NumericProcessHandle};

pub use host::HostState;
pub use process::{WasmProcess, ProcessState};

/// Maximum syscall data buffer size (matches WASM HAL)
pub const MAX_SYSCALL_BUFFER: usize = 16384;

/// WASM runtime manager
///
/// Manages all WASM process instances and their execution state.
pub struct WasmRuntime {
    /// The wasmi engine (shared across all modules)
    engine: Engine,
    /// Linker with host functions
    linker: Linker<HostState>,
    /// Active processes: pid -> WasmProcess
    processes: Mutex<BTreeMap<u64, WasmProcess>>,
    /// Pending syscalls from processes: pid -> (syscall_num, args, data)
    pending_syscalls: Mutex<Vec<PendingSyscall>>,
    /// Pending IPC messages to deliver to processes: pid -> messages
    pending_messages: Mutex<BTreeMap<u64, Vec<Vec<u8>>>>,
}

/// A pending syscall from a WASM process
#[derive(Clone, Debug)]
pub struct PendingSyscall {
    pub pid: u64,
    pub syscall_num: u32,
    pub args: [u32; 3],
    pub data: Vec<u8>,
}

impl WasmRuntime {
    /// Create a new WASM runtime
    pub fn new() -> Self {
        let engine = Engine::default();
        let mut linker = Linker::new(&engine);
        
        // Register host functions
        host::register_host_functions(&mut linker);
        
        Self {
            engine,
            linker,
            processes: Mutex::new(BTreeMap::new()),
            pending_syscalls: Mutex::new(Vec::new()),
            pending_messages: Mutex::new(BTreeMap::new()),
        }
    }
    
    /// Spawn a new WASM process
    ///
    /// # Arguments
    /// * `pid` - Process ID assigned by the kernel
    /// * `name` - Human-readable process name
    /// * `binary` - WASM binary to execute
    ///
    /// # Returns
    /// Handle to the spawned process
    pub fn spawn(&self, pid: u64, name: &str, binary: &[u8]) -> Result<NumericProcessHandle, HalError> {
        serial::write_str(&alloc::format!(
            "[wasm-rt] Spawning process '{}' with PID {}\n",
            name, pid
        ));
        
        // Parse the WASM module
        let module = Module::new(&self.engine, binary).map_err(|e| {
            serial::write_str(&alloc::format!(
                "[wasm-rt] Failed to parse WASM module: {:?}\n", e
            ));
            HalError::ProcessSpawnFailed
        })?;
        
        // Create store with host state
        let host_state = HostState::new(pid);
        let mut store = Store::new(&self.engine, host_state);
        
        // Instantiate the module with host functions
        let instance = self.linker.instantiate(&mut store, &module).map_err(|e| {
            serial::write_str(&alloc::format!(
                "[wasm-rt] Failed to instantiate WASM module: {:?}\n", e
            ));
            HalError::ProcessSpawnFailed
        })?.start(&mut store).map_err(|e| {
            serial::write_str(&alloc::format!(
                "[wasm-rt] Failed to start WASM module: {:?}\n", e
            ));
            HalError::ProcessSpawnFailed
        })?;
        
        // Get the _start function
        let start_func = instance.get_typed_func::<(), ()>(&store, "_start").ok();
        
        // Create process entry
        let process = WasmProcess {
            pid,
            name: String::from(name),
            state: ProcessState::Ready,
            store,
            instance,
            start_func,
            memory_size: 65536, // Default, will be updated
        };
        
        // Store the process
        self.processes.lock().insert(pid, process);
        
        serial::write_str(&alloc::format!(
            "[wasm-rt] Process '{}' (PID {}) spawned successfully\n",
            name, pid
        ));
        
        Ok(NumericProcessHandle::new(pid))
    }
    
    /// Run a process until it yields or makes a syscall
    ///
    /// Returns true if the process is still running, false if it exited.
    pub fn run_process(&self, pid: u64) -> Result<bool, HalError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&pid).ok_or(HalError::ProcessNotFound)?;
        
        if process.state != ProcessState::Ready {
            return Ok(process.state != ProcessState::Terminated);
        }
        
        // Run the _start function if we haven't started yet
        if let Some(start_func) = process.start_func.take() {
            process.state = ProcessState::Running;
            
            let result = start_func.call(&mut process.store, ());
            
            match result {
                Ok(()) => {
                    // Process completed normally
                    process.state = ProcessState::Terminated;
                    serial::write_str(&alloc::format!(
                        "[wasm-rt] Process {} exited normally\n", pid
                    ));
                    Ok(false)
                }
                Err(e) => {
                    // Check if it's a trap we expect (yield, syscall)
                    let trap_str = alloc::format!("{:?}", e);
                    if trap_str.contains("yield") {
                        process.state = ProcessState::Ready;
                        Ok(true)
                    } else {
                        serial::write_str(&alloc::format!(
                            "[wasm-rt] Process {} trapped: {:?}\n", pid, e
                        ));
                        process.state = ProcessState::Terminated;
                        Ok(false)
                    }
                }
            }
        } else {
            // Process already started, nothing to do
            Ok(process.state != ProcessState::Terminated)
        }
    }
    
    /// Kill a process
    pub fn kill(&self, pid: u64) -> Result<(), HalError> {
        let mut processes = self.processes.lock();
        if let Some(mut process) = processes.remove(&pid) {
            process.state = ProcessState::Terminated;
            serial::write_str(&alloc::format!(
                "[wasm-rt] Process {} killed\n", pid
            ));
            Ok(())
        } else {
            Err(HalError::ProcessNotFound)
        }
    }
    
    /// Check if a process is alive
    pub fn is_alive(&self, pid: u64) -> bool {
        self.processes
            .lock()
            .get(&pid)
            .map(|p| p.state != ProcessState::Terminated)
            .unwrap_or(false)
    }
    
    /// Get memory size of a process
    pub fn memory_size(&self, pid: u64) -> Result<usize, HalError> {
        self.processes
            .lock()
            .get(&pid)
            .map(|p| p.memory_size)
            .ok_or(HalError::ProcessNotFound)
    }
    
    /// Queue a message for delivery to a process
    pub fn queue_message(&self, pid: u64, msg: Vec<u8>) {
        let mut messages = self.pending_messages.lock();
        messages.entry(pid).or_insert_with(Vec::new).push(msg);
    }
    
    /// Get pending syscalls from all processes
    pub fn take_pending_syscalls(&self) -> Vec<PendingSyscall> {
        core::mem::take(&mut *self.pending_syscalls.lock())
    }
    
    /// Run all ready processes and collect their syscalls
    ///
    /// This is the main scheduler entry point. It:
    /// 1. Runs each process that is in Ready state
    /// 2. Collects any syscalls they made
    /// 3. Returns the pending syscalls for the kernel to process
    pub fn run_all_processes(&self) -> Vec<PendingSyscall> {
        let mut syscalls = Vec::new();
        
        // Get list of PIDs to run
        let pids: Vec<u64> = {
            self.processes
                .lock()
                .iter()
                .filter(|(_, p)| p.state == ProcessState::Ready)
                .map(|(pid, _)| *pid)
                .collect()
        };
        
        // Run each ready process
        for pid in pids {
            if let Err(e) = self.run_process(pid) {
                serial::write_str(&alloc::format!(
                    "[wasm-rt] Failed to run process {}: {:?}\n", pid, e
                ));
                continue;
            }
            
            // Check if process made a syscall
            if let Some(process) = self.processes.lock().get_mut(&pid) {
                if let Some(pending) = process.store.data_mut().pending_syscall.take() {
                    syscalls.push(PendingSyscall {
                        pid,
                        syscall_num: pending.syscall_num,
                        args: pending.args,
                        data: pending.data,
                    });
                    // Mark process as blocked waiting for syscall result
                    process.state = ProcessState::Blocked;
                }
            }
        }
        
        syscalls
    }
    
    /// Complete a syscall for a process
    pub fn complete_syscall(&self, pid: u64, result: u32, data: &[u8]) {
        if let Some(process) = self.processes.lock().get_mut(&pid) {
            // Store result in host state for the process to retrieve
            process.store.data_mut().set_syscall_result(result, data);
            process.state = ProcessState::Ready;
        }
    }
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: WasmRuntime is designed to be used from a single-threaded kernel
// All mutable state is protected by Mutex
unsafe impl Send for WasmRuntime {}
unsafe impl Sync for WasmRuntime {}
