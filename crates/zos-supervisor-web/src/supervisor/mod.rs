//! Supervisor - Pure Boundary Layer (Invariant-Compliant)
//!
//! The Supervisor runs in the browser's main thread and acts as a pure
//! boundary layer between userspace WASM processes and the kernel.
//!
//! ## Responsibilities
//!
//! - Kernel lifecycle and boot sequence
//! - Process spawning and termination
//! - IPC message routing (NOT execution)
//! - Syscall dispatch to kernel via Axiom gateway
//!
//! ## Architecture
//!
//! The supervisor does NOT execute application logic. All terminal commands,
//! permission management, and app behavior runs in userspace WASM processes:
//!
//! - Init (PID 1): Bootstrap, service registry, and supervisor request handling
//! - PermissionManager (PID 2): Capability authority
//! - Terminal (PID 3+): Command execution
//!
//! The supervisor routes messages through capability-checked IPC.
//!
//! ## Invariant 16 Compliance: Supervisor Privilege Model
//!
//! Per `docs/invariants/invariants.md`, the supervisor cannot bypass Axiom.
//! The supervisor is registered as PID 0 and holds capabilities to:
//!
//! - Init's endpoint (slot in `init_endpoint_slot`)
//! - PermissionManager's endpoint (slot in `pm_endpoint_slot`)
//! - Terminal input endpoints (slots in `terminal_endpoint_slots`)
//!
//! All supervisor operations use capability-checked `ipc_send()`:
//!
//! 1. Console input → Direct IPC to terminal OR routed via Init
//! 2. Capability revocation → Routed to PermissionManager
//! 3. IPC delivery → Routed via Init
//!
//! This ensures:
//!
//! 1. All supervisor actions are capability-checked (Invariant 16)
//! 2. All operations flow through Axiom logging (Invariant 9)
//! 3. Supervisor identity (PID 0) is auditable via SysLog (Invariant 10)
//!
//! The supervisor CANNOT:
//! - Inject commits directly into CommitLog (commits come from kernel operations)
//! - Forge sender identity in syscalls (identity from trusted execution context)
//! - Bypass capability checks (uses standard ipc_send)

mod axiom_sync;
mod boot;
mod metrics;
mod spawn;

use std::collections::HashMap;

use zos_hal::HAL;
use zos_kernel::{Kernel, ProcessId};
use wasm_bindgen::prelude::*;

use crate::hal::WasmHal;
use crate::pingpong::PingPongTestState;
use crate::syscall;
use crate::worker::{WasmProcessHandle, WorkerMessage, WorkerMessageType};

// Note: Console I/O uses capability-checked IPC.
// - Console output: Uses SYS_CONSOLE_WRITE syscall (supervisor delivers to UI)
// - Console input: Uses capability-checked ipc_send to terminal's input endpoint

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

/// Decode a hex string to bytes
fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, &'static str> {
    if hex.len() % 2 != 0 {
        return Err("Invalid hex length");
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| "Invalid hex character"))
        .collect()
}

/// Supervisor - manages the kernel and processes
///
/// Note: Desktop functionality has been moved to the `zos-desktop` crate.
/// Load `DesktopController` from `zos_desktop.js` for desktop operations.
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
    // Supervisor state - supervisor uses capability-checked IPC
    // ==========================================================================
    /// Supervisor's process ID (PID 0) - supervisor is a kernel process
    /// that holds capabilities to Init, PM, and terminal endpoints
    supervisor_pid: ProcessId,
    /// Whether supervisor kernel process has been initialized
    supervisor_initialized: bool,
    
    // ==========================================================================
    // Generic IPC response callback - event-based, no storage needed
    // ==========================================================================
    /// Callback invoked when service IPC responses arrive (event-based)
    /// The supervisor immediately invokes this callback when a SERVICE:RESPONSE
    /// debug message is received, rather than storing responses for polling.
    ipc_response_callback: Option<js_sys::Function>,

    // ==========================================================================
    // Supervisor capability slots for IPC-based communication
    // ==========================================================================
    // The supervisor (PID 0) holds capabilities to Init and PM endpoints,
    // enabling capability-checked IPC instead of privileged kernel APIs.
    /// Capability slot for Init's endpoint (granted during Init spawn)
    init_endpoint_slot: Option<u32>,
    /// Capability slot for PermissionManager's endpoint (granted during PM spawn)  
    pm_endpoint_slot: Option<u32>,
    /// Map of terminal PID to capability slot for that terminal's input endpoint
    terminal_endpoint_slots: HashMap<u64, u32>,
}

// =============================================================================
// Storage Constants (matching zos-process::storage_result)
// =============================================================================

/// Storage result types for MSG_STORAGE_RESULT IPC
mod storage_const {
    // Storage result type constants from zos-ipc
    pub const STORAGE_READ_OK: u8 = zos_ipc::storage::result::READ_OK;
    pub const STORAGE_WRITE_OK: u8 = zos_ipc::storage::result::WRITE_OK;
    pub const STORAGE_NOT_FOUND: u8 = zos_ipc::storage::result::NOT_FOUND;
    pub const STORAGE_ERROR: u8 = zos_ipc::storage::result::ERROR;
    pub const STORAGE_LIST_OK: u8 = zos_ipc::storage::result::LIST_OK;
    pub const STORAGE_EXISTS_OK: u8 = zos_ipc::storage::result::EXISTS_OK;
    
    /// MSG_STORAGE_RESULT tag from zos-ipc (the single source of truth)
    pub const MSG_STORAGE_RESULT: u32 = zos_ipc::storage::MSG_STORAGE_RESULT;
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
            // Supervisor state - initialized during boot(), capabilities granted during spawn
            supervisor_pid: ProcessId(0),
            supervisor_initialized: false,
            // Generic IPC response callback (event-based)
            ipc_response_callback: None,
            // Supervisor capability slots for IPC-based communication
            init_endpoint_slot: None,
            pm_endpoint_slot: None,
            terminal_endpoint_slots: HashMap::new(),
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

    /// Send input to a specific terminal process via capability-checked IPC
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

        // Get supervisor's capability slot for this terminal's input endpoint
        let supervisor_slot = match self.terminal_endpoint_slots.get(&pid) {
            Some(&slot) => slot,
            None => {
                log(&format!(
                    "[supervisor] No capability for terminal PID {} - using fallback via Init",
                    pid
                ));
                // Fallback: route through Init
                self.route_console_input_via_init(pid, input);
                return;
            }
        };
        
        // Send console input via capability-checked IPC
        let supervisor_pid = ProcessId(0);
        match self.kernel.ipc_send(
            supervisor_pid,
            supervisor_slot,
            zos_kernel::MSG_CONSOLE_INPUT,
            input.as_bytes().to_vec(),
        ) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Delivered {} bytes to PID {} via IPC (slot {})",
                    input.len(),
                    pid,
                    supervisor_slot
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
    
    /// Route console input through Init (fallback when supervisor lacks capability)
    fn route_console_input_via_init(&mut self, target_pid: u64, input: &str) {
        let init_slot = match self.init_endpoint_slot {
            Some(slot) => slot,
            None => {
                log("[supervisor] Cannot route console input: no Init capability");
                return;
            }
        };
        
        // Build message for Init: [target_pid: u32, endpoint_slot: u32, data_len: u16, data: [u8]]
        let mut payload = Vec::with_capacity(10 + input.len());
        payload.extend_from_slice(&(target_pid as u32).to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes()); // Terminal input slot
        payload.extend_from_slice(&(input.len() as u16).to_le_bytes());
        payload.extend_from_slice(input.as_bytes());
        
        let supervisor_pid = ProcessId(0);
        use zos_ipc::supervisor::MSG_SUPERVISOR_CONSOLE_INPUT;
        
        match self.kernel.ipc_send(supervisor_pid, init_slot, MSG_SUPERVISOR_CONSOLE_INPUT, payload) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Routed {} bytes to PID {} via Init",
                    input.len(),
                    target_pid
                ));
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to route console input via Init: {:?}",
                    e
                ));
            }
        }
    }

    /// Revoke/delete a capability from any process via PermissionManager
    ///
    /// This method allows the UI to revoke capabilities from any process
    /// by routing the request through the PermissionManager (PID 2).
    /// The supervisor sends an IPC message to PM, which performs the
    /// capability deletion and notifies the affected process.
    ///
    /// Returns true if the revocation request was sent successfully.
    #[wasm_bindgen]
    pub fn revoke_capability(&mut self, pid: u64, slot: u32) -> bool {
        // Use canonical constants from zos-ipc (the single source of truth)
        use zos_ipc::revoke_reason::EXPLICIT as REVOKE_REASON_EXPLICIT;
        use zos_ipc::supervisor::MSG_SUPERVISOR_REVOKE_CAP;
        
        let pm_slot = match self.pm_endpoint_slot {
            Some(s) => s,
            None => {
                log(&format!(
                    "[supervisor] Cannot revoke capability PID {} slot {}: PM not initialized",
                    pid, slot
                ));
                return false;
            }
        };
        
        // Build message for PM: [target_pid: u32, cap_slot: u32, reason: u8]
        let mut payload = Vec::with_capacity(9);
        payload.extend_from_slice(&(pid as u32).to_le_bytes());
        payload.extend_from_slice(&slot.to_le_bytes());
        payload.push(REVOKE_REASON_EXPLICIT);
        
        let supervisor_pid = ProcessId(0);
        
        match self.kernel.ipc_send(supervisor_pid, pm_slot, MSG_SUPERVISOR_REVOKE_CAP, payload) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Sent revoke request to PM for PID {} slot {}",
                    pid, slot
                ));
                true
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to send revoke request to PM: {:?}",
                    e
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

        // Progress the ping-pong test state machine if running
        self.progress_pingpong_test();

        count
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

        // Handle SYS_CONSOLE_WRITE (0x07) specially - supervisor delivers to UI directly
        if syscall_num == 0x07 {
            return self.handle_sys_console_write(pid, data);
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
    /// Handle SYS_DEBUG (0x01) syscall.
    ///
    /// Debug messages are used for inter-process communication with the supervisor:
    /// - Spawn requests (INIT:SPAWN:)
    /// - Capability operations (INIT:GRANT:, INIT:REVOKE:)
    /// - Permission responses
    /// - Service IPC responses
    /// - Console output
    fn handle_sys_debug(&mut self, pid: ProcessId, data: &[u8]) -> i32 {
        let args4 = [0u32, 0, 0, 0];

        // Route through gateway for audit logging
        let (result, _, _) = self.kernel.execute_raw_syscall(pid, 0x01, args4, data);

        // Process the debug message for supervisor-level actions
        if let Ok(s) = std::str::from_utf8(data) {
            self.dispatch_debug_message(pid, s);
        }

        // Clear data buffer to prevent stale debug message text from being
        // misinterpreted as IPC message data by subsequent syscalls
        self.kernel.hal().write_syscall_data(pid.0, &[]);

        result as i32
    }

    /// Dispatch debug message to appropriate handler based on prefix.
    fn dispatch_debug_message(&mut self, pid: ProcessId, msg: &str) {
        // Try each handler in order of specificity
        if let Some(service_name) = msg.strip_prefix("INIT:SPAWN:") {
            self.handle_debug_spawn(service_name);
        } else if msg.starts_with("INIT:GRANT:") {
            syscall::handle_init_grant(&mut self.kernel, msg);
        } else if msg.starts_with("INIT:REVOKE:") {
            syscall::handle_init_revoke(&mut self.kernel, msg);
        } else if msg.starts_with("INIT:PERM_RESPONSE:") {
            log(&format!("[supervisor] Permission response: {}", msg));
        } else if msg.starts_with("INIT:PERM_LIST:") {
            log(&format!("[supervisor] Permission list: {}", msg));
        } else if let Some(init_msg) = msg.strip_prefix("INIT:") {
            log(&format!("[init] {}", init_msg));
        } else if let Some(rest) = msg.strip_prefix("SERVICE:RESPONSE:") {
            self.handle_debug_service_response(rest);
        } else if let Some(rest) = msg.strip_prefix("VFS:RESPONSE:") {
            self.handle_debug_vfs_response(rest);
        // Init-driven spawn protocol responses
        } else if let Some(rest) = msg.strip_prefix("SPAWN:RESPONSE:") {
            self.handle_init_spawn_response(rest);
        } else if let Some(rest) = msg.strip_prefix("ENDPOINT:RESPONSE:") {
            self.handle_init_endpoint_response(rest);
        } else if let Some(rest) = msg.strip_prefix("CAP:RESPONSE:") {
            self.handle_init_cap_response(rest);
        } else {
            self.handle_debug_console_output(pid, msg);
        }
    }

    /// Handle SPAWN:RESPONSE from Init (Init-driven spawn protocol).
    ///
    /// This is called when Init responds to MSG_SUPERVISOR_SPAWN_PROCESS.
    /// Format: hex-encoded [success: u8, pid: u32]
    fn handle_init_spawn_response(&mut self, hex_data: &str) {
        let bytes = match hex_to_bytes(hex_data) {
            Ok(b) => b,
            Err(_) => {
                log("[supervisor] SPAWN:RESPONSE invalid hex");
                return;
            }
        };

        if bytes.len() < 5 {
            log("[supervisor] SPAWN:RESPONSE too short");
            return;
        }

        let success = bytes[0];
        let pid = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);

        if success == 1 {
            log(&format!(
                "[supervisor] Init-driven spawn: process registered with PID {}",
                pid
            ));
            // TODO: Continue spawn flow with pending spawn tracking
        } else {
            log("[supervisor] Init-driven spawn: registration failed");
        }
    }

    /// Handle ENDPOINT:RESPONSE from Init (Init-driven spawn protocol).
    ///
    /// This is called when Init responds to MSG_SUPERVISOR_CREATE_ENDPOINT.
    /// Format: hex-encoded [success: u8, endpoint_id: u64, slot: u32]
    fn handle_init_endpoint_response(&mut self, hex_data: &str) {
        let bytes = match hex_to_bytes(hex_data) {
            Ok(b) => b,
            Err(_) => {
                log("[supervisor] ENDPOINT:RESPONSE invalid hex");
                return;
            }
        };

        if bytes.len() < 13 {
            log("[supervisor] ENDPOINT:RESPONSE too short");
            return;
        }

        let success = bytes[0];
        let endpoint_id = u64::from_le_bytes([
            bytes[1], bytes[2], bytes[3], bytes[4],
            bytes[5], bytes[6], bytes[7], bytes[8],
        ]);
        let slot = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);

        if success == 1 {
            log(&format!(
                "[supervisor] Init-driven spawn: endpoint {} created at slot {}",
                endpoint_id, slot
            ));
            // TODO: Continue spawn flow with pending spawn tracking
        } else {
            log("[supervisor] Init-driven spawn: endpoint creation failed");
        }
    }

    /// Handle CAP:RESPONSE from Init (Init-driven spawn protocol).
    ///
    /// This is called when Init responds to MSG_SUPERVISOR_GRANT_CAP.
    /// Format: hex-encoded [success: u8, new_slot: u32]
    fn handle_init_cap_response(&mut self, hex_data: &str) {
        let bytes = match hex_to_bytes(hex_data) {
            Ok(b) => b,
            Err(_) => {
                log("[supervisor] CAP:RESPONSE invalid hex");
                return;
            }
        };

        if bytes.len() < 5 {
            log("[supervisor] CAP:RESPONSE too short");
            return;
        }

        let success = bytes[0];
        let new_slot = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);

        if success == 1 {
            log(&format!(
                "[supervisor] Init-driven spawn: capability granted at slot {}",
                new_slot
            ));
            // TODO: Continue spawn flow with pending spawn tracking
        } else {
            log("[supervisor] Init-driven spawn: capability grant failed");
        }
    }

    /// Handle INIT:SPAWN: debug message.
    fn handle_debug_spawn(&mut self, service_name: &str) {
        log(&format!(
            "[supervisor] Init requesting spawn of '{}'",
            service_name
        ));
        self.request_spawn(service_name, service_name);
    }

    /// Handle SERVICE:RESPONSE: debug message.
    ///
    /// Format: {to_pid}:{tag_hex}:{hex_data}
    /// Example: "0:00007055:7b22..."
    fn handle_debug_service_response(&self, rest: &str) {
        let parts: Vec<&str> = rest.splitn(3, ':').collect();
        if parts.len() != 3 {
            log(&format!("[supervisor] Malformed SERVICE:RESPONSE: {}", rest));
            return;
        }

        let request_id = parts[1]; // tag hex is the request_id
        let hex_data = parts[2];

        // Invoke JS callback if registered
        let Some(ref callback) = self.ipc_response_callback else {
            log(&format!(
                "[supervisor] SERVICE:RESPONSE received but no callback registered (request_id={})",
                request_id
            ));
            return;
        };

        let bytes = match hex_to_bytes(hex_data) {
            Ok(b) => b,
            Err(_) => {
                log(&format!(
                    "[supervisor] SERVICE:RESPONSE invalid hex for request_id={}",
                    request_id
                ));
                return;
            }
        };

        let json = match String::from_utf8(bytes) {
            Ok(j) => j,
            Err(_) => {
                log(&format!(
                    "[supervisor] SERVICE:RESPONSE invalid UTF-8 for request_id={}",
                    request_id
                ));
                return;
            }
        };

        let this = JsValue::null();
        let id_arg = JsValue::from_str(request_id);
        let data_arg = JsValue::from_str(&json);
        let _ = callback.call2(&this, &id_arg, &data_arg);
        log(&format!(
            "[supervisor] Invoked IPC callback for request_id={}",
            request_id
        ));
    }

    /// Handle VFS:RESPONSE: debug message.
    ///
    /// Format: {to_pid}:{tag_hex}:{hex_data}
    /// Example: "4:00008013:7b22..."
    /// Routes VFS responses back to the requesting process via Init.
    fn handle_debug_vfs_response(&mut self, rest: &str) {
        let parts: Vec<&str> = rest.splitn(3, ':').collect();
        if parts.len() != 3 {
            log(&format!("[supervisor] Malformed VFS:RESPONSE: {}", rest));
            return;
        }

        let to_pid = match parts[0].parse::<u32>() {
            Ok(p) => p,
            Err(_) => {
                log(&format!("[supervisor] VFS:RESPONSE invalid PID: {}", parts[0]));
                return;
            }
        };

        let tag = match u32::from_str_radix(parts[1], 16) {
            Ok(t) => t,
            Err(_) => {
                log(&format!("[supervisor] VFS:RESPONSE invalid tag: {}", parts[1]));
                return;
            }
        };

        let data = match hex_to_bytes(parts[2]) {
            Ok(d) => d,
            Err(_) => {
                log("[supervisor] VFS:RESPONSE invalid hex data");
                return;
            }
        };

        // Route response through Init for capability-checked delivery
        // Use slot 4 (VFS_RESPONSE_SLOT) instead of slot 1 to avoid race conditions
        // where the VFS client's blocking receive could consume other IPC messages.
        const VFS_RESPONSE_SLOT: u32 = 4;
        self.route_ipc_via_init(to_pid as u64, VFS_RESPONSE_SLOT, tag, &data);
    }
    
    /// Route an IPC message through Init for capability-checked delivery
    fn route_ipc_via_init(&mut self, target_pid: u64, endpoint_slot: u32, tag: u32, data: &[u8]) {
        let init_slot = match self.init_endpoint_slot {
            Some(slot) => slot,
            None => {
                log("[supervisor] Cannot route IPC: no Init capability");
                return;
            }
        };
        
        use zos_ipc::supervisor::MSG_SUPERVISOR_IPC_DELIVERY;
        
        // Build message for Init: [target_pid: u32, endpoint_slot: u32, tag: u32, data_len: u16, data: [u8]]
        let mut payload = Vec::with_capacity(14 + data.len());
        payload.extend_from_slice(&(target_pid as u32).to_le_bytes());
        payload.extend_from_slice(&endpoint_slot.to_le_bytes());
        payload.extend_from_slice(&tag.to_le_bytes());
        payload.extend_from_slice(&(data.len() as u16).to_le_bytes());
        payload.extend_from_slice(data);
        
        let supervisor_pid = ProcessId(0);
        
        match self.kernel.ipc_send(supervisor_pid, init_slot, MSG_SUPERVISOR_IPC_DELIVERY, payload) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Routed IPC to PID {} endpoint {} tag 0x{:x} via Init",
                    target_pid, endpoint_slot, tag
                ));
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to route IPC via Init: {:?}",
                    e
                ));
            }
        }
    }

    /// Handle default debug message (console output).
    fn handle_debug_console_output(&mut self, pid: ProcessId, msg: &str) {
        log(&format!("[process {}] {}", pid.0, msg));
        self.write_console(&format!("[P{}] {}\n", pid.0, msg));
        if msg.contains("========================================") {
            self.check_pingpong_complete(pid.0);
        }
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

    /// Handle SYS_CONSOLE_WRITE (0x07) syscall.
    ///
    /// Console output is delivered directly to the UI by the supervisor.
    /// Per kernel invariant, no buffering in the kernel - supervisor handles
    /// the output directly from the syscall data.
    fn handle_sys_console_write(&mut self, pid: ProcessId, data: &[u8]) -> i32 {
        let args4 = [0u32, 0, 0, 0];

        // Route through gateway for audit logging
        let (result, _, _) = self.kernel.execute_raw_syscall(pid, 0x07, args4, data);

        // Deliver console output directly to UI
        if let Ok(text) = std::str::from_utf8(data) {
            log(&format!(
                "[supervisor] Console output from PID {}: {} bytes",
                pid.0,
                text.len()
            ));
            // Route to process-specific callback (or fall back to global)
            self.write_console_to_process(pid.0, text);
        }

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
                        // Route through Init for proper auditing
                        self.cleanup_process_state(msg.pid);
                        if msg.pid != 1 && self.init_endpoint_slot.is_some() {
                            self.kill_process_via_init(pid);
                        } else {
                            self.kill_process_direct(pid);
                        }
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
        use zos_kernel::{Syscall, SyscallResult};

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
                // Route through Init for proper auditing
                self.cleanup_process_state(pid);
                if pid != 1 && self.init_endpoint_slot.is_some() {
                    self.kill_process_via_init(process_id);
                } else {
                    self.kill_process_direct(process_id);
                }
                SyscallResult::Ok(0)
            }
            8 => self.kernel.handle_syscall(process_id, Syscall::GetTime),
            9 => SyscallResult::Ok(0), // SYS_YIELD
            _ => {
                log(&format!(
                    "[supervisor] Unknown syscall {} from process {}",
                    syscall_num, pid
                ));
                SyscallResult::Err(zos_kernel::KernelError::PermissionDenied)
            }
        };

        // Send result back to Worker
        syscall::send_syscall_result(self.kernel.hal(), pid, result);
    }

    /// Kill a process by PID.
    ///
    /// # Architecture: Kill Routing
    ///
    /// For non-Init processes, this routes the kill request through Init
    /// via MSG_SUPERVISOR_KILL_PROCESS. This ensures:
    /// - Kill operations flow through Init for proper auditing
    /// - Init can perform cleanup before process termination
    ///
    /// For Init itself (PID 1), direct kernel calls are used since Init
    /// cannot kill itself via IPC.
    #[wasm_bindgen]
    pub fn kill_process(&mut self, pid: u64) {
        let process_id = ProcessId(pid);
        log(&format!("[supervisor] Killing process {}", pid));
        
        // Clean up supervisor state for this process
        self.cleanup_process_state(pid);
        
        // Route through Init for non-Init processes
        if pid != 1 && self.init_endpoint_slot.is_some() {
            self.kill_process_via_init(process_id);
        } else {
            // Direct kill for Init or if Init isn't available (bootstrap)
            self.kill_process_direct(process_id);
        }
    }
    
    /// Route a kill request through Init via MSG_SUPERVISOR_KILL_PROCESS.
    ///
    /// Init receives the request and invokes SYS_KILL syscall, which is
    /// properly logged via SysLog.
    fn kill_process_via_init(&mut self, target_pid: ProcessId) {
        let init_slot = match self.init_endpoint_slot {
            Some(slot) => slot,
            None => {
                log("[supervisor] Cannot route kill via Init: no Init capability");
                self.kill_process_direct(target_pid);
                return;
            }
        };
        
        use zos_ipc::supervisor::MSG_SUPERVISOR_KILL_PROCESS;
        
        // Build message for Init: [target_pid: u32]
        let payload = (target_pid.0 as u32).to_le_bytes().to_vec();
        let supervisor_pid = ProcessId(0);
        
        match self.kernel.ipc_send(supervisor_pid, init_slot, MSG_SUPERVISOR_KILL_PROCESS, payload) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Sent kill request for PID {} to Init",
                    target_pid.0
                ));
                // Init will invoke SYS_KILL and notify supervisor via INIT:KILL_OK/FAIL
                // We still need to kill the HAL worker
                let handle = WasmProcessHandle::new(target_pid.0);
                let _ = self.kernel.hal().kill_process(&handle);
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to route kill via Init: {:?}, falling back to direct",
                    e
                ));
                self.kill_process_direct(target_pid);
            }
        }
    }
    
    /// Directly kill a process via kernel call (fallback/bootstrap).
    ///
    /// Used when Init is not available (bootstrap) or for killing Init itself.
    fn kill_process_direct(&mut self, process_id: ProcessId) {
        log(&format!(
            "[supervisor] Direct kill of PID {} (Init unavailable or bootstrap)",
            process_id.0
        ));
        self.kernel.kill_process(process_id);
        let handle = WasmProcessHandle::new(process_id.0);
        let _ = self.kernel.hal().kill_process(&handle);
    }

    /// Clean up supervisor state for a killed process
    fn cleanup_process_state(&mut self, pid: u64) {
        // Remove console callback for this process
        if self.console_callbacks.remove(&pid).is_some() {
            log(&format!("[supervisor] Cleaned up console callback for PID {}", pid));
        }
        
        // Remove terminal endpoint capability slot
        if self.terminal_endpoint_slots.remove(&pid).is_some() {
            log(&format!("[supervisor] Cleaned up terminal endpoint slot for PID {}", pid));
        }
    }

    /// Kill all processes.
    ///
    /// Routes kill requests through Init for proper auditing.
    /// Init itself is killed last via direct kernel call.
    #[wasm_bindgen]
    pub fn kill_all_processes(&mut self) {
        log("[supervisor] Killing all processes");
        let pids: Vec<ProcessId> = self
            .kernel
            .list_processes()
            .into_iter()
            .map(|(pid, _)| pid)
            .filter(|pid| pid.0 != 0) // Don't kill supervisor
            .collect();

        // Kill non-Init processes first (routed through Init)
        for pid in &pids {
            if pid.0 != 1 {
                self.cleanup_process_state(pid.0);
                if self.init_endpoint_slot.is_some() {
                    self.kill_process_via_init(*pid);
                } else {
                    self.kill_process_direct(*pid);
                }
            }
        }
        
        // Kill Init last (direct call since Init can't kill itself)
        for pid in &pids {
            if pid.0 == 1 {
                self.cleanup_process_state(pid.0);
                self.kill_process_direct(*pid);
            }
        }
    }

    // ==========================================================================
    // Generic Service IPC API - Thin boundary layer for service communication
    // ==========================================================================

    /// Register a callback for IPC responses from services.
    ///
    /// This callback is invoked immediately when a SERVICE:RESPONSE debug message
    /// is received from a service process. The callback receives:
    /// - request_id: The response tag as hex string (e.g., "00007055")
    /// - data: The JSON response data as a string
    ///
    /// This is event-based, not polling-based. The supervisor does NOT store
    /// responses - it invokes the callback immediately when they arrive.
    #[wasm_bindgen]
    pub fn set_ipc_response_callback(&mut self, callback: js_sys::Function) {
        self.ipc_response_callback = Some(callback);
        log("[supervisor] IPC response callback registered");
    }

    /// Send an IPC message to a named service via capability-checked IPC.
    ///
    /// This method routes the message through Init for capability-checked delivery.
    /// Init then forwards the message to the service.
    ///
    /// The request_id is the expected response tag as hex (response_tag = request_tag + 1).
    /// When the service responds, the IPC response callback is invoked with this request_id.
    ///
    /// # Arguments
    /// - service_name: Service name without "_service" suffix (e.g., "identity", "vfs")
    /// - tag: Request message tag (e.g., 0x7054 for MSG_GENERATE_NEURAL_KEY)
    /// - data: JSON request data as a string
    ///
    /// # Returns
    /// - On success: request_id string (e.g., "00007055")
    /// - On error: "error:..." string
    #[wasm_bindgen]
    pub fn send_service_ipc(&mut self, service_name: &str, tag: u32, data: &str) -> String {
        let pid = match self.find_service_pid(service_name) {
            Some(p) => p,
            None => return format!("error:service_not_found:{}", service_name),
        };

        // Request ID = response tag hex (convention: response tag = request tag + 1)
        let response_tag = tag + 1;
        let request_id = format!("{:08x}", response_tag);

        // Service's input endpoint is at slot 1
        const SERVICE_INPUT_SLOT: u32 = 1;

        log(&format!(
            "[supervisor] Sending service IPC: service={} tag=0x{:x} to PID {} slot {} (expecting response tag=0x{:x})",
            service_name, tag, pid.0, SERVICE_INPUT_SLOT, response_tag
        ));

        // Route through Init for capability-checked delivery
        let init_slot = match self.init_endpoint_slot {
            Some(slot) => slot,
            None => {
                log("[supervisor] Cannot send service IPC: no Init capability");
                return "error:no_init_capability".to_string();
            }
        };
        
        use zos_ipc::supervisor::MSG_SUPERVISOR_IPC_DELIVERY;
        
        // Build message for Init: [target_pid: u32, endpoint_slot: u32, tag: u32, data_len: u16, data: [u8]]
        let data_bytes = data.as_bytes();
        let mut payload = Vec::with_capacity(14 + data_bytes.len());
        payload.extend_from_slice(&(pid.0 as u32).to_le_bytes());
        payload.extend_from_slice(&SERVICE_INPUT_SLOT.to_le_bytes());
        payload.extend_from_slice(&tag.to_le_bytes());
        payload.extend_from_slice(&(data_bytes.len() as u16).to_le_bytes());
        payload.extend_from_slice(data_bytes);
        
        let supervisor_pid = ProcessId(0);
        
        match self.kernel.ipc_send(supervisor_pid, init_slot, MSG_SUPERVISOR_IPC_DELIVERY, payload) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Routed {} bytes to {} service PID {} via Init",
                    data.len(), service_name, pid.0
                ));
                request_id
            }
            Err(e) => format!("error:delivery_failed:{:?}", e),
        }
    }

    /// Find a service process by name.
    ///
    /// Looks up "{service_name}_service" in the process list.
    fn find_service_pid(&self, service_name: &str) -> Option<ProcessId> {
        let expected_name = format!("{}_service", service_name);
        for (pid, proc) in self.kernel.list_processes() {
            if proc.name == expected_name {
                return Some(pid);
            }
        }
        None
    }

    // ==========================================================================
    // Storage Initialization
    // ==========================================================================

    /// Initialize the async storage system.
    ///
    /// This must be called by React after the supervisor is created to set up
    /// the ZosStorage callbacks. ZosStorage needs a reference to the supervisor
    /// to call notify_storage_* methods.
    ///
    /// # Arguments
    /// * `supervisor` - Reference to this supervisor (passed as JsValue)
    ///
    /// # Example (React)
    /// ```js
    /// const supervisor = new Supervisor();
    /// window.ZosStorage.init(supervisor);
    /// ```
    #[wasm_bindgen]
    pub fn init_storage(&self) {
        // ZosStorage.init is called from React with the supervisor reference
        // This method exists for documentation purposes - actual init happens in JS
        log("[supervisor] Storage system ready (init_storage called)");
    }

    // ==========================================================================
    // Storage Result Callbacks (called by JavaScript when IndexedDB completes)
    // ==========================================================================

    /// Called by JavaScript when storage read completes successfully.
    ///
    /// # Arguments
    /// * `request_id` - The request ID returned from storage_read_async
    /// * `data` - The read data as bytes
    #[wasm_bindgen]
    pub fn notify_storage_read_complete(&mut self, request_id: u32, data: &[u8]) {
        log(&format!(
            "[supervisor] notify_storage_read_complete: request_id={}, len={}",
            request_id, data.len()
        ));
        
        // Look up which PID requested this
        let pid = match self.kernel.hal().take_storage_request_pid(request_id) {
            Some(p) => p,
            None => {
                log(&format!(
                    "[supervisor] Unknown storage request_id: {}",
                    request_id
                ));
                return;
            }
        };
        
        // Build MSG_STORAGE_RESULT payload
        // Format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
        let mut payload = Vec::with_capacity(9 + data.len());
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_READ_OK);
        payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
        payload.extend_from_slice(data);
        
        // Deliver to requesting process via Init
        self.deliver_storage_result(pid, &payload);
    }

    /// Called by JavaScript when storage read returns not found.
    #[wasm_bindgen]
    pub fn notify_storage_not_found(&mut self, request_id: u32) {
        log(&format!(
            "[supervisor] notify_storage_not_found: request_id={}",
            request_id
        ));
        
        let pid = match self.kernel.hal().take_storage_request_pid(request_id) {
            Some(p) => p,
            None => return,
        };
        
        // Build MSG_STORAGE_RESULT payload for NOT_FOUND
        let mut payload = Vec::with_capacity(9);
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_NOT_FOUND);
        payload.extend_from_slice(&0u32.to_le_bytes());
        
        self.deliver_storage_result(pid, &payload);
    }

    /// Called by JavaScript when storage write/delete completes successfully.
    #[wasm_bindgen]
    pub fn notify_storage_write_complete(&mut self, request_id: u32) {
        log(&format!(
            "[supervisor] notify_storage_write_complete: request_id={}",
            request_id
        ));
        
        let pid = match self.kernel.hal().take_storage_request_pid(request_id) {
            Some(p) => p,
            None => return,
        };
        
        // Build MSG_STORAGE_RESULT payload for WRITE_OK
        let mut payload = Vec::with_capacity(9);
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_WRITE_OK);
        payload.extend_from_slice(&0u32.to_le_bytes());
        
        self.deliver_storage_result(pid, &payload);
    }

    /// Called by JavaScript when storage list completes.
    ///
    /// # Arguments
    /// * `request_id` - The request ID
    /// * `keys_json` - JSON array of key strings
    #[wasm_bindgen]
    pub fn notify_storage_list_complete(&mut self, request_id: u32, keys_json: &str) {
        log(&format!(
            "[supervisor] notify_storage_list_complete: request_id={}, len={}",
            request_id, keys_json.len()
        ));
        
        let pid = match self.kernel.hal().take_storage_request_pid(request_id) {
            Some(p) => p,
            None => return,
        };
        
        let data = keys_json.as_bytes();
        let mut payload = Vec::with_capacity(9 + data.len());
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_LIST_OK);
        payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
        payload.extend_from_slice(data);
        
        self.deliver_storage_result(pid, &payload);
    }

    /// Called by JavaScript when storage exists check completes.
    ///
    /// # Arguments
    /// * `request_id` - The request ID
    /// * `exists` - Whether the key exists
    #[wasm_bindgen]
    pub fn notify_storage_exists_complete(&mut self, request_id: u32, exists: bool) {
        log(&format!(
            "[supervisor] notify_storage_exists_complete: request_id={}, exists={}",
            request_id, exists
        ));
        
        let pid = match self.kernel.hal().take_storage_request_pid(request_id) {
            Some(p) => p,
            None => return,
        };
        
        let mut payload = Vec::with_capacity(10);
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_EXISTS_OK);
        payload.extend_from_slice(&1u32.to_le_bytes()); // data_len = 1
        payload.push(if exists { 1 } else { 0 });
        
        self.deliver_storage_result(pid, &payload);
    }

    /// Called by JavaScript when storage operation fails.
    #[wasm_bindgen]
    pub fn notify_storage_error(&mut self, request_id: u32, error: &str) {
        log(&format!(
            "[supervisor] notify_storage_error: request_id={}, error={}",
            request_id, error
        ));
        
        let pid = match self.kernel.hal().take_storage_request_pid(request_id) {
            Some(p) => p,
            None => return,
        };
        
        let error_bytes = error.as_bytes();
        let mut payload = Vec::with_capacity(9 + error_bytes.len());
        payload.extend_from_slice(&request_id.to_le_bytes());
        payload.push(storage_const::STORAGE_ERROR);
        payload.extend_from_slice(&(error_bytes.len() as u32).to_le_bytes());
        payload.extend_from_slice(error_bytes);
        
        self.deliver_storage_result(pid, &payload);
    }

    /// Deliver a storage result to a process via IPC through Init.
    fn deliver_storage_result(&mut self, pid: u64, payload: &[u8]) {
        // Route through Init for capability-checked delivery
        // Use slot 1 (INPUT_ENDPOINT_SLOT) for storage results to services.
        // Services like IdentityService and VfsService use storage syscalls and
        // receive all IPC on slot 1 via the app_main! framework.
        // Note: VFS_RESPONSE_SLOT (4) is for VFS *client* responses, not storage syscalls.
        const INPUT_ENDPOINT_SLOT: u32 = 1;
        
        self.route_ipc_via_init(pid, INPUT_ENDPOINT_SLOT, storage_const::MSG_STORAGE_RESULT, payload);
    }

}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}
