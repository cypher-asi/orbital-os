//! Application Runtime
//!
//! Runs inside each WASM process, providing the event loop and syscall interface.

use crate::app::{AppContext, ControlFlow, Message, UserContext, ZeroApp};
use alloc::format;
use alloc::string::String;
use zos_process as syscall;

/// Runtime environment for a Zero application.
/// Runs inside the WASM process.
pub struct AppRuntime {
    /// This process's ID (obtained via SYS_GET_PID)
    pid: u32,

    /// Capability slot for UI communication endpoint
    ui_slot: Option<u32>,

    /// Capability slot for receiving input
    input_slot: Option<u32>,

    /// Last update timestamp (for throttling)
    last_update_ns: u64,

    /// Minimum interval between updates (in nanoseconds)
    update_interval_ns: u64,

    /// User context (who launched this app)
    user_context: UserContext,

    /// App ID from manifest
    app_id: String,
}

impl AppRuntime {
    /// Default update interval (~60 FPS)
    const DEFAULT_UPDATE_INTERVAL_NS: u64 = 16_666_667;

    /// Create a new runtime for an app.
    pub fn new() -> Self {
        // Get our PID via syscall
        let pid = syscall::get_pid();

        AppRuntime {
            pid,
            ui_slot: None,
            input_slot: None,
            last_update_ns: 0,
            update_interval_ns: Self::DEFAULT_UPDATE_INTERVAL_NS,
            user_context: UserContext::system(),
            app_id: String::new(),
        }
    }

    /// Create a runtime with user context.
    pub fn with_user(mut self, user_context: UserContext) -> Self {
        self.user_context = user_context;
        self
    }

    /// Set the app ID (usually from manifest).
    pub fn set_app_id(&mut self, app_id: &str) {
        self.app_id = String::from(app_id);
    }

    /// Set the user context.
    pub fn set_user_context(&mut self, user_context: UserContext) {
        self.user_context = user_context;
    }

    /// Run the main event loop.
    ///
    /// This function never returns normally - it either runs forever
    /// or exits via syscall::exit().
    pub fn run<A: ZeroApp>(&mut self, mut app: A) -> ! {
        // Build initial context
        let ctx = self.build_context();

        // Initialize the app
        if let Err(e) = app.init(&ctx) {
            syscall::debug(&format!("App init failed: {}", e));
            syscall::exit(1);
        }

        // Main event loop
        loop {
            // Build fresh context with current time
            let ctx = self.build_context();

            // Poll for incoming messages
            if let Some(slot) = self.input_slot {
                while let Some(msg) = syscall::receive(slot) {
                    let message = Message::new(msg.tag, msg.from_pid, msg.data);
                    if let Err(e) = app.on_message(&ctx, message) {
                        syscall::debug(&format!("Message handling error: {}", e));
                    }
                }
            }

            // Throttle updates
            if ctx.uptime_ns - self.last_update_ns >= self.update_interval_ns {
                self.last_update_ns = ctx.uptime_ns;

                // Run app update
                match app.update(&ctx) {
                    ControlFlow::Continue => {}
                    ControlFlow::Yield => {
                        syscall::yield_now();
                    }
                    ControlFlow::Exit(code) => {
                        app.shutdown(&ctx);
                        syscall::exit(code);
                    }
                }
            } else {
                // Not time for update yet, yield
                syscall::yield_now();
            }
        }
    }

    /// Build the current execution context.
    fn build_context(&self) -> AppContext {
        AppContext {
            pid: self.pid,
            uptime_ns: syscall::get_time(),
            wallclock_ms: self.get_wallclock(),
            ui_endpoint: self.ui_slot,
            input_endpoint: self.input_slot,
            user: self.user_context.clone(),
            app_id: self.app_id.clone(),
        }
    }

    /// Get wall-clock time in milliseconds since Unix epoch
    fn get_wallclock(&self) -> u64 {
        syscall::get_wallclock()
    }

    /// Set the UI endpoint slot.
    pub fn set_ui_endpoint(&mut self, slot: u32) {
        self.ui_slot = Some(slot);
    }

    /// Set the input endpoint slot.
    pub fn set_input_endpoint(&mut self, slot: u32) {
        self.input_slot = Some(slot);
    }

    /// Set the update interval in milliseconds.
    pub fn set_update_interval_ms(&mut self, ms: u64) {
        self.update_interval_ns = ms * 1_000_000;
    }

    /// Set the update interval in nanoseconds.
    pub fn set_update_interval_ns(&mut self, ns: u64) {
        self.update_interval_ns = ns;
    }

    /// Get the current process ID.
    pub fn pid(&self) -> u32 {
        self.pid
    }
}

impl Default for AppRuntime {
    fn default() -> Self {
        Self::new()
    }
}
