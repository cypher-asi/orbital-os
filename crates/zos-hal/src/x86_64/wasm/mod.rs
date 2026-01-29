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

/// Fuel per timeslice for WASM execution
/// This needs to be high enough for:
/// - String formatting and debug output (expensive in WASM)
/// - Processing syscall results  
/// - Loop iterations before the next syscall/yield
/// 
/// Note: Fuel exhaustion during WASM execution (not in a host function) returns
/// an unresumable Err. So we need enough fuel to reach the next syscall or yield.
const FUEL_PER_TIMESLICE: u64 = 100_000_000;

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
        // Configure engine with fuel for cooperative multitasking
        let mut config = wasmi::Config::default();
        config.consume_fuel(true);
        
        // Increase stack limits to handle large operations
        // StackLimits::new(initial_value_stack, maximum_value_stack, maximum_recursion_depth)
        // NOTE: Values appear to be in stack ENTRIES (not bytes). Each entry is ~8 bytes.
        // Default is ~1024 initial, ~8192 max, 1024 recursion depth
        // We use conservative increases to avoid OOM while preventing stack overflow
        config.set_stack_limits(
            wasmi::StackLimits::new(
                4096,    // ~32KB initial value stack (in entries)
                262144,  // ~2MB maximum value stack (in entries, 262144 * 8 = 2MB)
                8192,    // 8192 maximum recursion depth
            ).expect("Valid stack limits")
        );
        
        let engine = Engine::new(&config);
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
        
        // Add initial fuel to the store
        store.set_fuel(FUEL_PER_TIMESLICE).expect("Failed to set fuel");
        
        // Create process entry
        let process = WasmProcess {
            pid,
            name: String::from(name),
            state: ProcessState::Ready,
            store,
            instance,
            start_func,
            resumable: None,
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
    
    /// Run a process until it yields, exhausts fuel, or makes a syscall
    ///
    /// Returns (is_alive, has_pending_syscall)
    fn run_process_internal(&self, process: &mut WasmProcess) -> (bool, bool) {
        if process.state != ProcessState::Ready {
            return (process.state != ProcessState::Terminated, false);
        }
        
        process.state = ProcessState::Running;
        
        // Refuel the process with a small amount for syscall-heavy code
        let _ = process.store.set_fuel(FUEL_PER_TIMESLICE);
        
        // Check if we have a resumable invocation (continuing from previous yield)
        if let Some(resumable) = process.resumable.take() {
            // When resuming after a host function trap, we need to provide the return value
            // that the host function was supposed to return
            let return_value = if process.store.data().has_pending_result {
                // We have a syscall result to return
                let result = process.store.data().syscall_result;
                // Clear the pending result flag
                process.store.data_mut().has_pending_result = false;
                process.store.data_mut().syscall_result = 0;
                result
            } else {
                // Yield or other - return 0
                0
            };
            
            // Log fuel before resume to track resource usage
            let fuel_before = process.store.get_fuel().unwrap_or(0);
            serial::write_str(&alloc::format!(
                "[wasm-rt] Process {} resuming with result={}, fuel={}\n", 
                process.pid, return_value, fuel_before
            ));
            
            // Provide the return value as input when resuming
            let result = resumable.resume(&mut process.store, &[wasmi::Val::I32(return_value as i32)]);
            let (is_alive, has_pending) = self.handle_execution_result(process, result);
            
            // Check if process is waiting for a syscall
            if is_alive && process.store.data().waiting_for_syscall {
                process.state = ProcessState::Blocked;
                return (true, true);
            }
            
            return (is_alive, has_pending);
        }
        
        // Start fresh - get _start function
        if let Some(start_func) = process.start_func.take() {
            // Use call_resumable to get a ResumableCall that we can continue later
            let result = start_func.call_resumable(&mut process.store, ());
            let (is_alive, has_pending) = self.handle_execution_result(process, result);
            
            // Check if process is waiting for a syscall
            if is_alive && process.store.data().waiting_for_syscall {
                process.state = ProcessState::Blocked;
                return (true, true);
            }
            
            return (is_alive, has_pending);
        }
        
        // Try to get _start from the instance (shouldn't normally happen)
        if let Ok(start_func) = process.instance.get_typed_func::<(), ()>(&process.store, "_start") {
            serial::write_str(&alloc::format!(
                "[wasm-rt] WARNING: Process {} restarting from _start (resumable was None)\n", process.pid
            ));
            let result = start_func.call_resumable(&mut process.store, ());
            let (is_alive, has_pending) = self.handle_execution_result(process, result);
            
            // Check if process is waiting for a syscall
            if is_alive && process.store.data().waiting_for_syscall {
                process.state = ProcessState::Blocked;
                return (true, true);
            }
            
            return (is_alive, has_pending);
        }
        
        // No way to run this process
        (process.state != ProcessState::Terminated, false)
    }
    
    /// Run a process until it yields or makes a syscall (public API)
    ///
    /// Returns true if the process is still running, false if it exited.
    pub fn run_process(&self, pid: u64) -> Result<bool, HalError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&pid).ok_or(HalError::ProcessNotFound)?;
        let (is_alive, _has_pending) = self.run_process_internal(process);
        Ok(is_alive)
    }
    
    /// Handle the result of a WASM execution (call or resume)
    /// 
    /// Returns: (is_alive, has_pending_syscall)
    fn handle_execution_result(
        &self,
        process: &mut WasmProcess,
        result: Result<wasmi::TypedResumableCall<()>, wasmi::Error>,
    ) -> (bool, bool) {
        // Debug: Log what result we received
        let result_type = match &result {
            Ok(wasmi::TypedResumableCall::Finished(_)) => "Finished",
            Ok(wasmi::TypedResumableCall::Resumable(_)) => "Resumable",
            Err(_) => "Err",
        };
        serial::write_str(&alloc::format!(
            "[wasm-rt] Process {} handle_execution_result: {}\n", process.pid, result_type
        ));
        
        match result {
            Ok(wasmi::TypedResumableCall::Finished(_)) => {
                // Process completed normally
                process.state = ProcessState::Terminated;
                process.resumable = None;
                serial::write_str(&alloc::format!(
                    "[wasm-rt] Process {} exited normally\n", process.pid
                ));
                (false, false)
            }
            Ok(wasmi::TypedResumableCall::Resumable(invocation)) => {
                // Process ran out of fuel - can be resumed
                // Store the invocation for later resumption
                process.resumable = Some(invocation);
                
                // Check and clear yield flag
                let yielded = process.store.data().yielded;
                if yielded {
                    process.store.data_mut().yielded = false;
                }
                
                // Check if there's a pending syscall
                let has_pending = process.store.data().pending_syscall.is_some();
                
                // Debug: log resumable stored
                serial::write_str(&alloc::format!(
                    "[wasm-rt] Process {} Resumable stored, has_pending={}, yielded={}\n", 
                    process.pid, has_pending, yielded
                ));
                
                // Set state based on whether syscall is pending
                if has_pending {
                    process.state = ProcessState::Blocked;
                } else {
                    process.state = ProcessState::Ready;
                }
                
                (true, has_pending)
            }
            Err(e) => {
                let trap_str = alloc::format!("{:?}", e);
                if trap_str.contains("out of fuel") || trap_str.contains("OutOfFuel") {
                    // Fuel exhaustion returned as Error means wasmi couldn't create a Resumable.
                    // This is a critical issue - we cannot continue execution without a Resumable.
                    // We must terminate the process to avoid infinite restart loops.
                    let has_pending = process.store.data().pending_syscall.is_some();
                    let yielded = process.store.data().yielded;
                    
                    serial::write_str(&alloc::format!(
                        "[wasm-rt] Process {} fuel exhausted as Err (has_pending={}, yielded={})\n", 
                        process.pid, has_pending, yielded
                    ));
                    
                    // Clear any pending state
                    process.store.data_mut().pending_syscall = None;
                    process.store.data_mut().yielded = false;
                    
                    // Terminate - we cannot resume without a Resumable
                    process.state = ProcessState::Terminated;
                    (false, false)
                } else {
                    serial::write_str(&alloc::format!(
                        "[wasm-rt] Process {} trapped: {:?}\n", process.pid, e
                    ));
                    process.state = ProcessState::Terminated;
                    process.resumable = None;
                    (false, false)
                }
            }
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
            let mut processes = self.processes.lock();
            let process = match processes.get_mut(&pid) {
                Some(p) => p,
                None => continue,
            };
            
            let (_is_alive, has_pending) = self.run_process_internal(process);
            
            // Collect pending syscall if any
            if has_pending {
                if let Some(pending) = process.store.data_mut().pending_syscall.take() {
                    syscalls.push(PendingSyscall {
                        pid,
                        syscall_num: pending.syscall_num,
                        args: pending.args,
                        data: pending.data,
                    });
                    // State should already be Blocked from handle_execution_result
                }
            }
        }
        
        syscalls
    }
    
    /// Run all processes with synchronous syscall handling
    ///
    /// This variant processes syscalls immediately as they are made,
    /// allowing the process to continue with the result without waiting
    /// for the next scheduler tick.
    pub fn run_all_processes_with_handler<F>(&self, handler: &mut F)
    where
        F: FnMut(PendingSyscall) -> (u32, Vec<u8>),
    {
        // Get list of PIDs to run
        let pids: Vec<u64> = {
            self.processes
                .lock()
                .iter()
                .filter(|(_, p)| p.state == ProcessState::Ready)
                .map(|(pid, _)| *pid)
                .collect()
        };
        
        // Run each ready process with synchronous syscall handling
        for pid in pids {
            self.run_process_with_handler(pid, handler);
        }
    }
    
    /// Run a single process with synchronous syscall handling
    ///
    /// Runs the process in a loop, processing syscalls immediately as they
    /// are made, until the process yields naturally or terminates.
    fn run_process_with_handler<F>(&self, pid: u64, handler: &mut F)
    where
        F: FnMut(PendingSyscall) -> (u32, Vec<u8>),
    {
        const MAX_SYSCALLS_PER_PROCESS: usize = 100; // Safety limit
        let mut syscall_count = 0;
        
        loop {
            // Run one timeslice
            let (is_alive, has_pending) = {
                let mut processes = self.processes.lock();
                let process = match processes.get_mut(&pid) {
                    Some(p) => p,
                    None => return,
                };
                self.run_process_internal(process)
            };
            
            if !is_alive {
                return;
            }
            
            // If there's a pending syscall, process it immediately
            if has_pending {
                syscall_count += 1;
                if syscall_count > MAX_SYSCALLS_PER_PROCESS {
                    serial::write_str(&alloc::format!(
                        "[wasm-rt] Process {} exceeded syscall limit\n", pid
                    ));
                    return;
                }
                
                // Get the pending syscall
                let pending = {
                    let mut processes = self.processes.lock();
                    let process = match processes.get_mut(&pid) {
                        Some(p) => p,
                        None => return,
                    };
                    process.store.data_mut().pending_syscall.take()
                };
                
                if let Some(pending) = pending {
                    // Process the syscall synchronously
                    let (result, data) = handler(PendingSyscall {
                        pid,
                        syscall_num: pending.syscall_num,
                        args: pending.args,
                        data: pending.data,
                    });
                    
                    // Complete the syscall
                    self.complete_syscall(pid, result, &data);
                }
            } else {
                // No pending syscall - process yielded naturally
                return;
            }
        }
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
