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
//! - PermissionService (PID 2): Capability authority
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
//! - PermissionService's endpoint (slot in `ps_endpoint_slot`)
//! - Terminal input endpoints (slots in `terminal_endpoint_slots`)
//!
//! All supervisor operations use capability-checked `ipc_send()`:
//!
//! 1. Console input → Direct IPC to terminal OR routed via Init
//! 2. Capability revocation → Routed to PermissionService
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
mod console;
mod debug_dispatch;
mod ipc;
mod metrics;
mod network;
mod spawn;
mod storage;
mod syscall_dispatch;
mod worker_events;

use std::collections::HashMap;

use wasm_bindgen::prelude::*;
use zos_hal::HAL;
use zos_kernel::{ProcessId, System};

use crate::constants::SERVICE_INPUT_SLOT;
use crate::hal::WasmHal;
use crate::pingpong::PingPongTestState;
use crate::util::log;
use crate::worker::WasmProcessHandle;

use spawn::SpawnTracker;

// Note: Console I/O uses capability-checked IPC.
// - Console output: Uses SYS_CONSOLE_WRITE syscall (supervisor delivers to UI)
// - Console input: Uses capability-checked ipc_send to terminal's input endpoint

/// Supervisor - manages the system and processes
///
/// The Supervisor is a thin boundary layer between userspace WASM processes
/// and the System (which combines Axiom verification + KernelCore execution).
///
/// All syscalls flow through `system.process_syscall()` to ensure proper
/// audit logging via SysLog and state recording via CommitLog.
///
/// Note: Desktop functionality has been moved to the `zos-desktop` crate.
/// Load `DesktopController` from `zos_desktop.js` for desktop operations.
#[wasm_bindgen]
pub struct Supervisor {
    /// System combines Axiom (verification layer) and KernelCore (execution layer)
    system: System<WasmHal>,
    /// Per-process console callbacks (PID -> JS callback function)
    /// Each terminal instance registers its own callback to receive output
    console_callbacks: HashMap<u64, js_sys::Function>,
    spawn_callback: Option<js_sys::Function>,
    /// Buffered console output (for messages before callback is set)
    console_buffer: Vec<String>,
    /// State for automated ping-pong test
    pingpong_test: PingPongTestState,
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
    /// Capability slot for PermissionService's endpoint (granted during PS spawn)  
    ps_endpoint_slot: Option<u32>,
    /// Map of terminal PID to capability slot for that terminal's input endpoint
    terminal_endpoint_slots: HashMap<u64, u32>,

    // ==========================================================================
    // Spawn tracking for async spawn operations
    // ==========================================================================
    /// Tracks pending spawn operations for timeout detection and state correlation.
    /// Used during transitional direct-spawn and required for future Init-driven spawn.
    spawn_tracker: SpawnTracker,
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
        let system = System::new(hal);

        Self {
            system,
            console_callbacks: HashMap::new(),
            spawn_callback: None,
            console_buffer: Vec::new(),
            pingpong_test: PingPongTestState::Idle,
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
            ps_endpoint_slot: None,
            terminal_endpoint_slots: HashMap::new(),
            // Spawn tracking for async operations
            spawn_tracker: SpawnTracker::new(),
        }
    }

    /// Set callback for spawning processes (JS will fetch WASM and call complete_spawn)
    #[wasm_bindgen]
    pub fn set_spawn_callback(&mut self, callback: js_sys::Function) {
        self.spawn_callback = Some(callback);
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
        if self.system.get_process(process_id).is_none() {
            log(&format!(
                "[supervisor] send_input_to_process: PID {} not found",
                pid
            ));
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
        match self.system.ipc_send(
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

    /// Revoke/delete a capability from any process via PermissionService
    ///
    /// This method allows the UI to revoke capabilities from any process
    /// by routing the request through the PermissionService (PID 2).
    /// The supervisor sends an IPC message to PM, which performs the
    /// capability deletion and notifies the affected process.
    ///
    /// Returns true if the revocation request was sent successfully.
    #[wasm_bindgen]
    pub fn revoke_capability(&mut self, pid: u64, slot: u32) -> bool {
        // Use canonical constants from zos-ipc (the single source of truth)
        use zos_ipc::revoke_reason::EXPLICIT as REVOKE_REASON_EXPLICIT;
        use zos_ipc::supervisor::MSG_SUPERVISOR_REVOKE_CAP;

        let ps_slot = match self.ps_endpoint_slot {
            Some(s) => s,
            None => {
                log(&format!(
                    "[supervisor] Cannot revoke capability PID {} slot {}: PS not initialized",
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

        match self
            .system
            .ipc_send(supervisor_pid, ps_slot, MSG_SUPERVISOR_REVOKE_CAP, payload)
        {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Sent revoke request to PS for PID {} slot {}",
                    pid, slot
                ));
                true
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to send revoke request to PS: {:?}",
                    e
                ));
                false
            }
        }
    }

    /// Progress the ping-pong test state machine
    fn progress_pingpong_test(&mut self) {
        use crate::pingpong::{progress_pingpong_test, PingPongContext};

        // Pingpong test messages go to browser console log
        let write_console = |s: &str| {
            log(&format!("[pingpong] {}", s.trim_end()));
        };
        let mut ctx = PingPongContext {
            system: &mut self.system,
            write_console: &write_console,
            init_endpoint_slot: self.init_endpoint_slot,
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
        let pending = self.system.hal().poll_syscalls();
        let count = pending.len();

        // Collect syscall info to process (to avoid borrowing issues)
        let syscalls: Vec<_> = pending
            .into_iter()
            .map(|s| {
                let data = self.system.hal().read_syscall_data(s.pid);
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
            self.system.hal().complete_syscall(syscall_info.pid, result);
        }

        // Progress the ping-pong test state machine if running
        self.progress_pingpong_test();

        // Check for spawn timeouts periodically
        self.check_spawn_timeouts();

        count
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
        if pid == 1 {
            // Direct kill for Init only (Init cannot kill itself)
            self.kill_process_direct(process_id);
            return;
        }

        self.kill_process_via_init(process_id);
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
                return;
            }
        };

        use zos_ipc::supervisor::MSG_SUPERVISOR_KILL_PROCESS;

        // Build message for Init: [target_pid: u32]
        let payload = (target_pid.0 as u32).to_le_bytes().to_vec();
        let supervisor_pid = ProcessId(0);

        match self.system.ipc_send(
            supervisor_pid,
            init_slot,
            MSG_SUPERVISOR_KILL_PROCESS,
            payload,
        ) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Sent kill request for PID {} to Init (awaiting INIT:KILL_OK)",
                    target_pid.0
                ));
                // Init will invoke SYS_KILL and notify supervisor via INIT:KILL_OK/FAIL
                // HAL worker cleanup happens in handle_init_kill_ok() after confirmation
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to route kill via Init: {:?}",
                    e
                ));
            }
        }
    }

    /// Directly kill a process via kernel call (bootstrap-only exception).
    ///
    /// # Invariant Exception
    ///
    /// This method violates Invariant 13 & 16 (supervisor must not have direct
    /// kernel access) but is permitted ONLY for:
    ///
    /// 1. **Init (PID 1) termination**: Init cannot kill itself via IPC
    /// 2. **Bootstrap failures**: Before Init is fully spawned
    ///
    /// All other process kills MUST route through `kill_process_via_init()`.
    ///
    /// # Rationale
    ///
    /// - Init is the IPC routing hub, so it cannot route messages to itself
    /// - During bootstrap, Init may not be ready to handle IPC
    /// - This is an architectural necessity, not a design flaw
    fn kill_process_direct(&mut self, process_id: ProcessId) {
        log(&format!(
            "[supervisor] DIRECT KILL (bootstrap exception) PID {}",
            process_id.0
        ));
        self.system.kill_process(process_id);
        let handle = WasmProcessHandle::new(process_id.0);
        let _ = self.system.hal().kill_process(&handle);
        self.cleanup_process_state(process_id.0);
    }

    /// Clean up supervisor state for a killed process
    fn cleanup_process_state(&mut self, pid: u64) {
        // Remove console callback for this process
        if self.console_callbacks.remove(&pid).is_some() {
            log(&format!(
                "[supervisor] Cleaned up console callback for PID {}",
                pid
            ));
        }

        // Remove terminal endpoint capability slot
        if self.terminal_endpoint_slots.remove(&pid).is_some() {
            log(&format!(
                "[supervisor] Cleaned up terminal endpoint slot for PID {}",
                pid
            ));
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
            .system
            .list_processes()
            .into_iter()
            .map(|(pid, _)| pid)
            .filter(|pid| pid.0 != 0) // Don't kill supervisor
            .collect();

        // Kill non-Init processes first (routed through Init)
        for pid in &pids {
            if pid.0 != 1 {
                self.cleanup_process_state(pid.0);
                self.kill_process_via_init(*pid);
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
    // Wasm-bindgen wrappers for storage callbacks
    // ==========================================================================

    /// Initialize the async storage system.
    ///
    /// This must be called by React after the supervisor is created to set up
    /// the ZosStorage callbacks. ZosStorage needs a reference to the supervisor
    /// to call notify_storage_* methods.
    #[wasm_bindgen]
    pub fn init_storage(&self) {
        // ZosStorage.init is called from React with the supervisor reference
        // This method exists for documentation purposes - actual init happens in JS
        log("[supervisor] Storage system ready (init_storage called)");
    }

    /// Called by JavaScript when storage read completes successfully.
    #[wasm_bindgen]
    pub fn notify_storage_read_complete(&mut self, request_id: u32, data: &[u8]) {
        self.notify_storage_read_complete_internal(request_id, data)
    }

    /// Called by JavaScript when storage read returns not found.
    #[wasm_bindgen]
    pub fn notify_storage_not_found(&mut self, request_id: u32) {
        self.notify_storage_not_found_internal(request_id)
    }

    /// Called by JavaScript when storage write/delete completes successfully.
    #[wasm_bindgen]
    pub fn notify_storage_write_complete(&mut self, request_id: u32) {
        self.notify_storage_write_complete_internal(request_id)
    }

    /// Called by JavaScript when storage list completes.
    #[wasm_bindgen]
    pub fn notify_storage_list_complete(&mut self, request_id: u32, keys_json: &str) {
        self.notify_storage_list_complete_internal(request_id, keys_json)
    }

    /// Called by JavaScript when storage exists check completes.
    #[wasm_bindgen]
    pub fn notify_storage_exists_complete(&mut self, request_id: u32, exists: bool) {
        self.notify_storage_exists_complete_internal(request_id, exists)
    }

    /// Called by JavaScript when storage operation fails.
    #[wasm_bindgen]
    pub fn notify_storage_error(&mut self, request_id: u32, error: &str) {
        self.notify_storage_error_internal(request_id, error)
    }

    // ==========================================================================
    // Wasm-bindgen wrappers for keystore callbacks (ZosKeystore)
    // ==========================================================================

    /// Called by JavaScript ZosKeystore when key read completes successfully.
    #[wasm_bindgen]
    pub fn notify_keystore_read_complete(&mut self, request_id: u32, data: &[u8]) {
        self.notify_keystore_read_complete_internal(request_id, data)
    }

    /// Called by JavaScript ZosKeystore when key read returns not found.
    #[wasm_bindgen]
    pub fn notify_keystore_not_found(&mut self, request_id: u32) {
        self.notify_keystore_not_found_internal(request_id)
    }

    /// Called by JavaScript ZosKeystore when key write/delete completes successfully.
    #[wasm_bindgen]
    pub fn notify_keystore_write_complete(&mut self, request_id: u32) {
        self.notify_keystore_write_complete_internal(request_id)
    }

    /// Called by JavaScript ZosKeystore when key list completes.
    #[wasm_bindgen]
    pub fn notify_keystore_list_complete(&mut self, request_id: u32, keys_json: &str) {
        self.notify_keystore_list_complete_internal(request_id, keys_json)
    }

    /// Called by JavaScript ZosKeystore when key exists check completes.
    #[wasm_bindgen]
    pub fn notify_keystore_exists_complete(&mut self, request_id: u32, exists: bool) {
        self.notify_keystore_exists_complete_internal(request_id, exists)
    }

    /// Called by JavaScript ZosKeystore when key operation fails.
    #[wasm_bindgen]
    pub fn notify_keystore_error(&mut self, request_id: u32, error: &str) {
        self.notify_keystore_error_internal(request_id, error)
    }

    // ==========================================================================
    // Wasm-bindgen wrappers for network callbacks
    // ==========================================================================

    /// Called by JavaScript ZosNetwork when a network fetch completes.
    #[wasm_bindgen(js_name = "onNetworkResult")]
    pub fn on_network_result(&mut self, request_id: u32, pid: u64, result: JsValue) {
        self.on_network_result_internal(request_id, pid, result)
    }

    // ==========================================================================
    // Wasm-bindgen wrappers for IPC methods
    // ==========================================================================

    /// Register a callback for IPC responses from services.
    #[wasm_bindgen]
    pub fn set_ipc_response_callback(&mut self, callback: js_sys::Function) {
        self.ipc_response_callback = Some(callback);
        log("[supervisor] IPC response callback registered");
    }

    /// Send an IPC message to a named service via capability-checked IPC.
    #[wasm_bindgen]
    pub fn send_service_ipc(&mut self, service_name: &str, tag: u32, data: &str) -> String {
        let pid = match self.find_service_pid(service_name) {
            Some(p) => p,
            None => return format!("error:service_not_found:{}", service_name),
        };

        // Request ID = response tag hex (convention: response tag = request tag + 1)
        let response_tag = tag + 1;
        let request_id = format!("{:08x}", response_tag);

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

        match self.system.ipc_send(
            supervisor_pid,
            init_slot,
            MSG_SUPERVISOR_IPC_DELIVERY,
            payload,
        ) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Routed {} bytes to {} service PID {} via Init",
                    data.len(),
                    service_name,
                    pid.0
                ));
                request_id
            }
            Err(e) => format!("error:delivery_failed:{:?}", e),
        }
    }

    // ==========================================================================
    // Worker event handling
    // ==========================================================================

    /// Process pending worker events (non-syscall postMessage events).
    ///
    /// This handles worker lifecycle events like Ready, MemoryUpdate, Error,
    /// Terminated, and Yield. Syscalls use the SharedArrayBuffer polling path
    /// (poll_syscalls) for better performance.
    #[wasm_bindgen]
    pub fn process_worker_messages(&mut self) -> usize {
        self.process_worker_messages_internal()
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}
