//! Supervisor - Pure Boundary Layer
//!
//! The Supervisor runs in the browser's main thread and acts as a pure
//! boundary layer between userspace WASM processes and the kernel.
//!
//! ## Responsibilities
//!
//! - Kernel lifecycle and boot sequence
//! - Process spawning and termination
//! - IPC message routing (NOT execution)
//! - Syscall dispatch to kernel
//!
//! ## Architecture
//!
//! The supervisor does NOT execute application logic. All terminal commands,
//! permission management, and app behavior runs in userspace WASM processes:
//!
//! - Init (PID 1): Bootstrap and service registry
//! - PermissionManager (PID 2): Capability authority
//! - Terminal (PID 3+): Command execution
//!
//! The supervisor only routes messages between these processes.

mod axiom_sync;
mod boot;
mod metrics;
mod spawn;

use std::collections::HashMap;

use orbital_hal::HAL;
use orbital_kernel::{Kernel, ProcessId};
use wasm_bindgen::prelude::*;

use crate::hal::WasmHal;
use crate::pingpong::PingPongTestState;
use crate::syscall;
use crate::worker::{WasmProcessHandle, WorkerMessage, WorkerMessageType};

// Note: Console I/O no longer uses IPC message tags.
// - Console output: Uses SYS_CONSOLE_WRITE syscall (drained by supervisor)
// - Console input: Uses kernel.deliver_console_input() privileged API

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

/// Supervisor - manages the kernel and processes
///
/// Note: Desktop functionality has been moved to the `orbital-desktop` crate.
/// Load `DesktopController` from `orbital_desktop.js` for desktop operations.
#[wasm_bindgen]
pub struct Supervisor {
    kernel: Kernel<WasmHal>,
    /// Per-process console callbacks (PID -> JS callback function)
    /// Each terminal instance registers its own callback to receive output
    console_callbacks: HashMap<u64, js_sys::Function>,
    /// Legacy single console callback (for backward compatibility)
    console_callback: Option<js_sys::Function>,
    spawn_callback: Option<js_sys::Function>,
    /// Buffered console output (for messages before callback is set)
    console_buffer: Vec<String>,
    /// State for automated ping-pong test
    pingpong_test: PingPongTestState,
    /// Pending spawn completion (name -> callback to call when spawn completes)
    #[allow(dead_code)]
    pending_spawn_name: Option<String>,
    /// Last CommitLog sequence number persisted to IndexedDB
    last_persisted_axiom_seq: u64,
    /// Whether Axiom IndexedDB storage has been initialized
    axiom_storage_ready: bool,
    /// Whether init process has been spawned
    init_spawned: bool,

    // ==========================================================================
    // Supervisor state - supervisor uses privileged kernel APIs (not IPC)
    // ==========================================================================
    /// Supervisor's process ID (PID 0) - supervisor is registered for auditing
    /// but does NOT use endpoints or capabilities
    supervisor_pid: ProcessId,
    /// Whether supervisor kernel process has been initialized
    supervisor_initialized: bool,
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
            console_callbacks: HashMap::new(),
            console_callback: None,
            spawn_callback: None,
            console_buffer: Vec::new(),
            pingpong_test: PingPongTestState::Idle,
            pending_spawn_name: None,
            last_persisted_axiom_seq: 0,
            axiom_storage_ready: false,
            init_spawned: false,
            // Supervisor state - initialized during boot()
            // Supervisor uses privileged kernel APIs (not IPC endpoints)
            supervisor_pid: ProcessId(0),
            supervisor_initialized: false,
        }
    }

    /// Set callback for console output (legacy - routes all output to single callback)
    /// 
    /// For process isolation, use `register_console_callback(pid, callback)` instead.
    #[wasm_bindgen]
    pub fn set_console_callback(&mut self, callback: js_sys::Function) {
        // Flush any buffered messages to the new callback
        let buffered_count = self.console_buffer.len();
        for text in self.console_buffer.drain(..) {
            let this = JsValue::null();
            let arg = JsValue::from_str(&text);
            let _ = callback.call1(&this, &arg);
        }
        self.console_callback = Some(callback);
        log(&format!("[supervisor] Console callback set (legacy), flushed {} buffered messages", buffered_count));
    }

    /// Register a console callback for a specific process
    /// 
    /// Each terminal window should register its own callback with its process PID.
    /// Console output from that process will be routed only to its registered callback.
    #[wasm_bindgen]
    pub fn register_console_callback(&mut self, pid: u64, callback: js_sys::Function) {
        log(&format!("[supervisor] Registered console callback for PID {}", pid));
        self.console_callbacks.insert(pid, callback);
    }

    /// Unregister the console callback for a specific process
    /// 
    /// Called when a terminal window is unmounted to clean up.
    #[wasm_bindgen]
    pub fn unregister_console_callback(&mut self, pid: u64) {
        if self.console_callbacks.remove(&pid).is_some() {
            log(&format!("[supervisor] Unregistered console callback for PID {}", pid));
        }
    }

    /// Set callback for spawning processes (JS will fetch WASM and call complete_spawn)
    #[wasm_bindgen]
    pub fn set_spawn_callback(&mut self, callback: js_sys::Function) {
        self.spawn_callback = Some(callback);
    }

    /// Write to console (calls JS callback, or buffers if not set yet)
    /// 
    /// This writes to the legacy global console callback (for system messages).
    /// For process-specific output, use write_console_to_process().
    pub(crate) fn write_console(&mut self, text: &str) {
        if let Some(ref callback) = self.console_callback {
            let this = JsValue::null();
            let arg = JsValue::from_str(text);
            let _ = callback.call1(&this, &arg);
        } else {
            // Buffer messages until callback is set
            self.console_buffer.push(text.to_string());
        }
    }

    /// Write console output to a specific process's callback
    /// 
    /// If no per-process callback is registered, falls back to the legacy global callback.
    fn write_console_to_process(&mut self, pid: u64, text: &str) {
        // Try per-process callback first
        if let Some(callback) = self.console_callbacks.get(&pid) {
            let this = JsValue::null();
            let arg = JsValue::from_str(text);
            let _ = callback.call1(&this, &arg);
            return;
        }

        // Fall back to legacy global callback
        if let Some(ref callback) = self.console_callback {
            let this = JsValue::null();
            let arg = JsValue::from_str(text);
            let _ = callback.call1(&this, &arg);
        } else {
            // Buffer if no callback available
            self.console_buffer.push(text.to_string());
        }
    }

    /// Request JS to spawn a process (fetch WASM binary)
    pub(crate) fn request_spawn(&mut self, proc_type: &str, name: &str) {
        if let Some(ref callback) = self.spawn_callback {
            let this = JsValue::null();
            let type_arg = JsValue::from_str(proc_type);
            let name_arg = JsValue::from_str(name);
            let _ = callback.call2(&this, &type_arg, &name_arg);
        } else {
            self.write_console("Error: Spawn callback not set\n");
        }
    }

    /// Send input to a specific terminal process via privileged kernel API
    ///
    /// This is the preferred method for process isolation - each terminal window
    /// sends input only to its associated process.
    #[wasm_bindgen]
    pub fn send_input_to_process(&mut self, pid: u64, input: &str) {
        let process_id = ProcessId(pid);
        
        // Verify process exists
        if self.kernel.get_process(process_id).is_none() {
            log(&format!("[supervisor] send_input_to_process: PID {} not found", pid));
            return;
        }

        // Terminal's input endpoint is at slot 1
        const TERMINAL_INPUT_SLOT: u32 = 1;
        
        match self.kernel.deliver_console_input(
            process_id,
            TERMINAL_INPUT_SLOT,
            input.as_bytes(),
        ) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Delivered {} bytes to PID {} via privileged API",
                    input.len(),
                    pid
                ));
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Console input delivery to PID {} failed: {:?}",
                    pid, e
                ));
            }
        }
    }

    /// Revoke/delete a capability from any process (supervisor privilege)
    ///
    /// This method allows the UI to directly revoke capabilities from any process
    /// without routing through terminal commands. The supervisor has privileged
    /// kernel access to delete capabilities from any process's CSpace.
    ///
    /// After revocation, a MSG_CAP_REVOKED notification is delivered to the
    /// affected process so it can react to the revocation (e.g., display a warning).
    #[wasm_bindgen]
    pub fn revoke_capability(&mut self, pid: u64, slot: u32) -> bool {
        // REVOKE_REASON_EXPLICIT = 1 (matches orbital_process::REVOKE_REASON_EXPLICIT)
        const REVOKE_REASON_EXPLICIT: u8 = 1;
        
        let process_id = ProcessId(pid);
        
        // Delete capability and get notification info
        match self.kernel.delete_capability_with_notification(process_id, slot, REVOKE_REASON_EXPLICIT) {
            Ok(notification) => {
                log(&format!(
                    "[supervisor] Revoked capability from PID {} slot {} (type: {}, object: {})",
                    pid, slot, notification.object_type, notification.object_id
                ));
                
                // Deliver notification to the process's input endpoint
                if notification.is_valid() {
                    if let Err(e) = self.kernel.deliver_revoke_notification(&notification) {
                        log(&format!(
                            "[supervisor] Failed to deliver revoke notification to PID {}: {:?}",
                            pid, e
                        ));
                        // Revocation still succeeded, just notification failed
                    }
                }
                
                true
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Revoke capability failed for PID {} slot {}: {:?}",
                    pid, slot, e
                ));
                false
            }
        }
    }

    /// Send input to the terminal via privileged kernel API (legacy)
    ///
    /// This method finds the first terminal process and sends input to it.
    /// For process isolation, use `send_input_to_process(pid, input)` instead.
    #[wasm_bindgen]
    pub fn send_input(&mut self, input: &str) {
        // Find the terminal process
        let terminal_pid = self.find_terminal_pid();
        
        if let Some(pid) = terminal_pid {
            self.send_input_to_process(pid.0, input);
        } else {
            // Terminal not yet spawned
            log("[supervisor] Terminal process not found");
            self.write_console("[supervisor] Terminal not ready - waiting for process spawn\n");
        }
    }
    
    /// Find the terminal process ID (returns first terminal found)
    fn find_terminal_pid(&self) -> Option<ProcessId> {
        for (pid, proc) in self.kernel.list_processes() {
            if proc.name == "terminal" {
                return Some(pid);
            }
        }
        None
    }

    /// Progress the ping-pong test state machine
    fn progress_pingpong_test(&mut self) {
        use crate::pingpong::{progress_pingpong_test, PingPongContext};

        let write_console = |s: &str| {
            if let Some(ref callback) = self.console_callback {
                let this = JsValue::null();
                let arg = JsValue::from_str(s);
                let _ = callback.call1(&this, &arg);
            }
        };
        let request_spawn = |t: &str, n: &str| {
            if let Some(ref callback) = self.spawn_callback {
                let this = JsValue::null();
                let type_arg = JsValue::from_str(t);
                let name_arg = JsValue::from_str(n);
                let _ = callback.call2(&this, &type_arg, &name_arg);
            }
        };

        let mut ctx = PingPongContext {
            kernel: &mut self.kernel,
            write_console: &write_console,
            request_spawn: &request_spawn,
        };

        self.pingpong_test = progress_pingpong_test(&self.pingpong_test, &mut ctx);
    }

    /// Check if the pingpong test completed
    fn check_pingpong_complete(&mut self, pid: u64) {
        if let Some(new_state) = crate::pingpong::check_pingpong_complete(&self.pingpong_test, pid)
        {
            log(&format!(
                "[pingpong] Test complete, results printed by PID {}",
                pid
            ));
            self.pingpong_test = new_state;
            self.progress_pingpong_test();
        }
    }

    /// Poll and process syscalls from Worker SharedArrayBuffer mailboxes
    #[wasm_bindgen]
    pub fn poll_syscalls(&mut self) -> usize {
        let pending = self.kernel.hal().poll_syscalls();
        let count = pending.len();

        // Collect syscall info to process (to avoid borrowing issues)
        let syscalls: Vec<_> = pending
            .into_iter()
            .map(|s| {
                let data = self.kernel.hal().read_syscall_data(s.pid);
                (s, data)
            })
            .collect();

        for (syscall_info, data) in syscalls {
            let pid = ProcessId(syscall_info.pid);

            // Process the syscall directly
            let result = self.process_syscall_internal(
                pid,
                syscall_info.syscall_num,
                syscall_info.args,
                &data,
            );

            // Write result and wake worker
            self.kernel.hal().complete_syscall(syscall_info.pid, result);
        }

        // Drain console output from kernel (from SYS_CONSOLE_WRITE syscalls)
        self.drain_console_output();

        // Progress the ping-pong test state machine if running
        self.progress_pingpong_test();

        count
    }
    
    /// Drain console output buffer from the kernel and forward to UI.
    ///
    /// This is called after processing syscalls to deliver console output
    /// from SYS_CONSOLE_WRITE syscalls to the browser UI.
    /// Output is routed to the process-specific callback if registered.
    fn drain_console_output(&mut self) {
        let outputs = self.kernel.drain_console_output();
        for (pid, data) in outputs {
            if let Ok(text) = std::str::from_utf8(&data) {
                log(&format!(
                    "[supervisor] Console output from PID {}: {} bytes",
                    pid.0,
                    text.len()
                ));
                // Route to process-specific callback (or fall back to global)
                self.write_console_to_process(pid.0, text);
            }
        }
    }

    /// Process a syscall internally through the Axiom gateway.
    ///
    /// This method routes all syscalls through `kernel.execute_raw_syscall()`
    /// which ensures proper audit logging via AxiomGateway.syscall().
    fn process_syscall_internal(
        &mut self,
        pid: ProcessId,
        syscall_num: u32,
        args: [u32; 3],
        data: &[u8],
    ) -> i32 {
        // Check if process exists in kernel
        if self.kernel.get_process(pid).is_none() {
            log(&format!(
                "[supervisor] Syscall from unknown process {}",
                pid.0
            ));
            return -1;
        }

        // Handle SYS_DEBUG specially - supervisor needs to process the message
        // before routing through the gateway
        if syscall_num == 0x01 {
            return self.handle_sys_debug(pid, data);
        }

        // Handle SYS_EXIT specially - need to kill worker after kernel operation
        if syscall_num == 0x11 {
            return self.handle_sys_exit(pid, args[0]);
        }

        // Route all other syscalls through the Axiom gateway
        let args4 = [args[0], args[1], args[2], 0];
        let (result, _rich_result, response_data) =
            self.kernel.execute_raw_syscall(pid, syscall_num, args4, data);

        // Always write response data (even if empty) to clear stale data from previous syscalls.
        // This prevents the process from reading leftover data from a prior syscall
        // (e.g., SYS_DEBUG text being misinterpreted as an IPC message).
        self.kernel.hal().write_syscall_data(pid.0, &response_data);

        result as i32
    }

    /// Handle SYS_DEBUG (0x01) syscall.
    ///
    /// Debug messages are processed by the supervisor for:
    /// - Spawn requests (INIT:SPAWN:)
    /// - Capability operations (INIT:GRANT:, INIT:REVOKE:)
    /// - Permission responses
    /// - Console output
    fn handle_sys_debug(&mut self, pid: ProcessId, data: &[u8]) -> i32 {
        let args4 = [0u32, 0, 0, 0];

        // Route through gateway for audit logging
        let (result, _, _) = self.kernel.execute_raw_syscall(pid, 0x01, args4, data);

        // Process the debug message for supervisor-level actions
        if let Ok(s) = std::str::from_utf8(data) {
            if let Some(service_name) = s.strip_prefix("INIT:SPAWN:") {
                log(&format!(
                    "[supervisor] Init requesting spawn of '{}'",
                    service_name
                ));
                self.request_spawn(service_name, service_name);
            } else if s.starts_with("INIT:GRANT:") {
                syscall::handle_init_grant(&mut self.kernel, s);
            } else if s.starts_with("INIT:REVOKE:") {
                syscall::handle_init_revoke(&mut self.kernel, s);
            } else if s.starts_with("INIT:PERM_RESPONSE:") {
                log(&format!("[supervisor] Permission response: {}", s));
            } else if s.starts_with("INIT:PERM_LIST:") {
                log(&format!("[supervisor] Permission list: {}", s));
            } else if let Some(init_msg) = s.strip_prefix("INIT:") {
                log(&format!("[init] {}", init_msg));
            } else {
                log(&format!("[process {}] {}", pid.0, s));
                self.write_console(&format!("[P{}] {}\n", pid.0, s));
                if s.contains("========================================") {
                    self.check_pingpong_complete(pid.0);
                }
            }
        }

        // Clear data buffer to prevent stale debug message text from being
        // misinterpreted as IPC message data by subsequent syscalls
        self.kernel.hal().write_syscall_data(pid.0, &[]);

        result as i32
    }

    /// Handle SYS_EXIT (0x11) syscall.
    ///
    /// Process exit requires both kernel state update (via gateway)
    /// and worker termination (via HAL).
    fn handle_sys_exit(&mut self, pid: ProcessId, exit_code: u32) -> i32 {
        log(&format!(
            "[supervisor] Process {} exiting with code {}",
            pid.0, exit_code
        ));

        // Route through gateway for kernel state + audit logging
        let args4 = [exit_code, 0, 0, 0];
        let (result, _, _) = self.kernel.execute_raw_syscall(pid, 0x11, args4, &[]);

        // Kill the worker process
        let handle = WasmProcessHandle::new(pid.0);
        let _ = self.kernel.hal().kill_process(&handle);

        result as i32
    }

    /// Process pending messages from Workers (legacy postMessage path)
    #[wasm_bindgen]
    pub fn process_worker_messages(&mut self) -> usize {
        const MAX_MESSAGES_PER_BATCH: usize = 5000;

        let incoming = self.kernel.hal().incoming_messages();
        let messages: Vec<WorkerMessage> = {
            if let Ok(mut queue) = incoming.lock() {
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
                    self.kernel
                        .hal()
                        .update_process_memory(msg.pid, memory_size);
                    log(&format!(
                        "[supervisor] Worker {} ready, memory: {} bytes",
                        msg.pid, memory_size
                    ));
                }
                WorkerMessageType::Syscall { syscall_num, args } => {
                    self.handle_worker_syscall(msg.pid, syscall_num, args, &msg.data);
                }
                WorkerMessageType::Error { ref message } => {
                    log(&format!(
                        "[supervisor] Worker {} error: {}",
                        msg.pid, message
                    ));
                }
                WorkerMessageType::Terminated => {
                    log(&format!("[supervisor] Worker {} terminated", msg.pid));
                    let pid = ProcessId(msg.pid);
                    if self.kernel.get_process(pid).is_some() {
                        self.kernel.kill_process(pid);
                    }
                }
                WorkerMessageType::MemoryUpdate { memory_size } => {
                    self.kernel
                        .hal()
                        .update_process_memory(msg.pid, memory_size);
                    let pid = ProcessId(msg.pid);
                    self.kernel.update_process_memory(pid, memory_size);
                }
                WorkerMessageType::Yield => {
                    // Worker yielded - nothing to do
                }
            }
        }

        count
    }

    /// Handle a syscall from a Worker process (legacy postMessage path)
    fn handle_worker_syscall(&mut self, pid: u64, syscall_num: u32, args: [u32; 3], data: &[u8]) {
        use orbital_kernel::{Syscall, SyscallResult};

        let process_id = ProcessId(pid);

        // Check if process exists in kernel
        if self.kernel.get_process(process_id).is_none() {
            log(&format!(
                "[supervisor] Syscall from unknown process {}",
                pid
            ));
            return;
        }

        // Parse syscall based on syscall_num (legacy numbering)
        let result = match syscall_num {
            0 => SyscallResult::Ok(0), // NOP
            1 => {
                // SYS_DEBUG
                if let Ok(s) = std::str::from_utf8(data) {
                    if let Some(service_name) = s.strip_prefix("INIT:SPAWN:") {
                        log(&format!(
                            "[supervisor] Init requesting spawn of '{}'",
                            service_name
                        ));
                        self.request_spawn(service_name, service_name);
                    } else if s.starts_with("INIT:GRANT:") {
                        syscall::handle_init_grant(&mut self.kernel, s);
                    } else if s.starts_with("INIT:REVOKE:") {
                        syscall::handle_init_revoke(&mut self.kernel, s);
                    } else if let Some(init_msg) = s.strip_prefix("INIT:") {
                        log(&format!("[init] {}", init_msg));
                    } else {
                        log(&format!("[process {}] {}", pid, s));
                    }
                }
                SyscallResult::Ok(0)
            }
            2 => self
                .kernel
                .handle_syscall(process_id, Syscall::CreateEndpoint),
            3 => {
                let slot = args[0];
                let tag = args[1];
                let syscall = Syscall::Send {
                    endpoint_slot: slot,
                    tag,
                    data: data.to_vec(),
                };
                self.kernel.handle_syscall(process_id, syscall)
            }
            4 => {
                let slot = args[0];
                let syscall = Syscall::Receive {
                    endpoint_slot: slot,
                };
                self.kernel.handle_syscall(process_id, syscall)
            }
            5 => self.kernel.handle_syscall(process_id, Syscall::ListCaps),
            6 => self
                .kernel
                .handle_syscall(process_id, Syscall::ListProcesses),
            7 => {
                let exit_code = args[0] as i32;
                log(&format!(
                    "[supervisor] Process {} exiting with code {}",
                    pid, exit_code
                ));
                self.kernel.kill_process(process_id);
                let handle = WasmProcessHandle::new(pid);
                let _ = self.kernel.hal().kill_process(&handle);
                SyscallResult::Ok(0)
            }
            8 => self.kernel.handle_syscall(process_id, Syscall::GetTime),
            9 => SyscallResult::Ok(0), // SYS_YIELD
            _ => {
                log(&format!(
                    "[supervisor] Unknown syscall {} from process {}",
                    syscall_num, pid
                ));
                SyscallResult::Err(orbital_kernel::KernelError::PermissionDenied)
            }
        };

        // Send result back to Worker
        syscall::send_syscall_result(self.kernel.hal(), pid, result);
    }

    /// Kill a process by PID
    #[wasm_bindgen]
    pub fn kill_process(&mut self, pid: u64) {
        let process_id = ProcessId(pid);
        log(&format!("[supervisor] Killing process {}", pid));
        
        // Clean up supervisor state for this process
        self.cleanup_process_state(pid);
        
        // Kill in kernel and HAL
        self.kernel.kill_process(process_id);
        let handle = WasmProcessHandle::new(pid);
        let _ = self.kernel.hal().kill_process(&handle);
    }

    /// Clean up supervisor state for a killed process
    fn cleanup_process_state(&mut self, pid: u64) {
        // Remove console callback for this process
        if self.console_callbacks.remove(&pid).is_some() {
            log(&format!("[supervisor] Cleaned up console callback for PID {}", pid));
        }
    }

    /// Kill all processes
    #[wasm_bindgen]
    pub fn kill_all_processes(&mut self) {
        log("[supervisor] Killing all processes");
        let pids: Vec<ProcessId> = self
            .kernel
            .list_processes()
            .into_iter()
            .map(|(pid, _)| pid)
            .collect();

        for pid in pids {
            self.kernel.kill_process(pid);
            let handle = WasmProcessHandle::new(pid.0);
            let _ = self.kernel.hal().kill_process(&handle);
        }
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}
