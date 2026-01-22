//! Terminal Application
//!
//! Command-line interface for Orbital OS. Demonstrates:
//! - Console output via SYS_CONSOLE_WRITE syscall
//! - Console input via kernel-delivered messages
//! - Direct syscalls (ps, caps, time)
//!
//! This is a canonical OrbitalApp implementation - all command execution
//! happens in userspace, not in the supervisor.
//!
//! ## Console I/O Architecture
//!
//! - **Output**: Terminal calls `console_write()` which uses SYS_CONSOLE_WRITE.
//!   The kernel buffers the output and the supervisor drains it to the UI.
//! - **Input**: Supervisor delivers keyboard input via privileged kernel API
//!   to the terminal's input endpoint (slot 1).

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use orbital_apps::app_protocol::{tags, TerminalInput, InputAction, MSG_CONSOLE_INPUT};
use orbital_apps::manifest::TERMINAL_MANIFEST;
use orbital_apps::syscall;
use orbital_apps::{app_main, AppContext, AppError, AppManifest, ControlFlow, Message, OrbitalApp};
use orbital_process::{MSG_CAP_REVOKED, ObjectType, error};

// TODO: Used for future capability requests to PermissionManager
// const PERMISSION_MANAGER_PID: u32 = 2;
// const MSG_REQUEST_CAPABILITY: u32 = 0x2010;

/// Terminal application state
#[derive(Default)]
pub struct TerminalApp {
    /// Output buffer (pending text to send to UI)
    output_buffer: String,
    /// Command history
    history: Vec<String>,
    /// Current history index (for up/down navigation)
    history_index: usize,
    /// Current input buffer
    input_buffer: String,
    /// Whether we've sent the initial banner
    initialized: bool,
}

impl TerminalApp {
    const PROMPT: &'static str = "orbital> ";

    /// Print text to console output via SYS_CONSOLE_WRITE syscall
    ///
    /// This writes directly to the console via syscall, which the supervisor
    /// drains and forwards to the browser UI.
    fn print(&mut self, text: &str) {
        // Buffer the output for batch sending
        self.output_buffer.push_str(text);
    }

    /// Print a line (with newline)
    fn println(&mut self, text: &str) {
        self.print(text);
        self.print("\n");
    }

    /// Flush output buffer to console via SYS_CONSOLE_WRITE syscall
    ///
    /// Uses the console_write syscall which the supervisor drains and
    /// forwards to the browser UI.
    fn flush_output(&mut self, _ctx: &AppContext) -> Result<(), AppError> {
        if self.output_buffer.is_empty() {
            return Ok(());
        }

        // Send output via SYS_CONSOLE_WRITE syscall
        let output = core::mem::take(&mut self.output_buffer);
        syscall::console_write(&output);

        Ok(())
    }

    /// Handle user input (a complete line or special action)
    fn handle_input(&mut self, input: &TerminalInput, ctx: &AppContext) -> Result<(), AppError> {
        match input.action {
            InputAction::Enter => {
                let line = input.text.trim();
                if !line.is_empty() {
                    // Add to history
                    self.history.push(line.to_string());
                    self.history_index = self.history.len();

                    // Execute command
                    self.execute_command(line);
                } else {
                    self.print(Self::PROMPT);
                }
                self.flush_output(ctx)?;
            }
            InputAction::Interrupt => {
                self.println("^C");
                self.print(Self::PROMPT);
                self.flush_output(ctx)?;
            }
            InputAction::Clear => {
                self.print("\x1B[2J\x1B[H");
                self.print(Self::PROMPT);
                self.flush_output(ctx)?;
            }
            InputAction::Up => {
                if self.history_index > 0 {
                    self.history_index -= 1;
                    if let Some(cmd) = self.history.get(self.history_index) {
                        self.input_buffer = cmd.clone();
                    }
                }
            }
            InputAction::Down => {
                if self.history_index < self.history.len() {
                    self.history_index += 1;
                    if self.history_index < self.history.len() {
                        if let Some(cmd) = self.history.get(self.history_index) {
                            self.input_buffer = cmd.clone();
                        }
                    } else {
                        self.input_buffer.clear();
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle raw console input (from supervisor fallback path)
    fn handle_raw_input(&mut self, text: &str, ctx: &AppContext) -> Result<(), AppError> {
        let line = text.trim();
        if !line.is_empty() {
            self.history.push(line.to_string());
            self.history_index = self.history.len();
            self.execute_command(line);
        }
        self.print(Self::PROMPT);
        self.flush_output(ctx)
    }

    /// Handle capability revocation notification from supervisor
    ///
    /// Payload format: [slot: u32, object_type: u8, object_id: u64, reason: u8]
    fn handle_cap_revoked(&mut self, data: &[u8], ctx: &AppContext) -> Result<(), AppError> {
        if data.len() >= 14 {
            let slot = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let object_type = data[4];
            // object_id at bytes 5-12 (u64)
            // reason at byte 13
            
            let type_name = ObjectType::from_u8(object_type)
                .map(|t| t.name())
                .unwrap_or("Unknown");
            
            // Print warning in yellow (ANSI escape code)
            self.println("");
            self.println(&format!(
                "\x1B[33mWarning: {} capability (slot {}) was revoked\x1B[0m",
                type_name, slot
            ));
            self.print(Self::PROMPT);
            self.flush_output(ctx)?;
        }
        Ok(())
    }

    /// Format a capability error for user-friendly display
    fn format_cap_error(&self, error_code: u32) -> String {
        match error_code {
            e if e == error::E_BADF => "Permission denied: capability has been revoked".to_string(),
            e if e == error::E_PERM => "Permission denied: insufficient capability permissions".to_string(),
            e if e == error::E_NOENT => "Resource not found: capability may have been revoked".to_string(),
            code => format!("Operation failed (error code {})", code),
        }
    }

    /// Execute a command
    fn execute_command(&mut self, line: &str) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let (cmd, args) = match parts.split_first() {
            Some((c, a)) => (*c, a),
            None => return,
        };

        match cmd {
            "help" => self.cmd_help(),
            "ps" => self.cmd_ps(),
            "caps" => self.cmd_caps(),
            "spawn" => self.cmd_spawn(args),
            "kill" => self.cmd_kill(args),
            "grant" => self.cmd_grant(args),
            "revoke" => self.cmd_revoke(args),
            "echo" => self.cmd_echo(args),
            "time" | "uptime" => self.cmd_time(),
            "clear" => self.cmd_clear(),
            "exit" => self.cmd_exit(),
            _ => {
                self.println(&format!("Unknown command: {}", cmd));
                self.println("Type 'help' for available commands.");
            }
        }
    }

    fn cmd_help(&mut self) {
        self.println("=== Orbital Terminal (userspace shell) ===");
        self.println("");
        self.println("Process Management:");
        self.println("  ps                - List running processes");
        self.println("  spawn <type>      - Request process spawn");
        self.println("  kill <pid>        - Request process termination");
        self.println("");
        self.println("Capabilities:");
        self.println("  caps              - List my capabilities");
        self.println("  grant <slot> <pid> <perms> - Grant capability");
        self.println("  revoke <slot>     - Revoke own capability");
        self.println("");
        self.println("System:");
        self.println("  echo <text>       - Echo text");
        self.println("  time              - Show system uptime");
        self.println("  clear             - Clear the screen");
        self.println("  exit              - Exit the terminal");
    }

    fn cmd_ps(&mut self) {
        // Use real syscall to get process list
        let procs = syscall::list_processes();

        self.println("PID  STATE    NAME");
        self.println("---  -----    ----");

        if procs.is_empty() {
            self.println("(no process data available)");
        } else {
            for proc in procs {
                let state = match proc.state {
                    0 => "Running",
                    1 => "Blocked",
                    2 => "Zombie",
                    _ => "???",
                };
                self.println(&format!("{:<4} {:<8} {}", proc.pid, state, proc.name));
            }
        }
    }

    fn cmd_caps(&mut self) {
        // Use real syscall to get capability list
        let caps = syscall::list_caps();

        self.println("SLOT  TYPE      PERMS  OBJECT");
        self.println("----  ----      -----  ------");

        if caps.is_empty() {
            self.println("(no capabilities)");
        } else {
            for cap in caps {
                let type_str = match cap.object_type {
                    1 => "Endpoint",
                    2 => "Process",
                    3 => "Memory",
                    4 => "IRQ",
                    5 => "I/O Port",
                    6 => "Console",
                    7 => "Storage",
                    8 => "Network",
                    _ => "???",
                };
                let perms = format!(
                    "{}{}{}",
                    if cap.can_read { "R" } else { "-" },
                    if cap.can_write { "W" } else { "-" },
                    if cap.can_grant { "G" } else { "-" },
                );
                self.println(&format!(
                    "{:<5} {:<9} {}    {}",
                    cap.slot, type_str, perms, cap.object_id
                ));
            }
        }
    }

    fn cmd_spawn(&mut self, args: &[&str]) {
        if args.is_empty() {
            self.println("Usage: spawn <process_type>");
            self.println("Types: memhog, sender, receiver, pingpong, idle, clock, calculator");
            return;
        }

        let proc_type = args[0];

        // Send spawn request via debug channel (supervisor intercepts this)
        syscall::debug(&format!("INIT:SPAWN:{}", proc_type));
        self.println(&format!("Requested spawn of '{}'...", proc_type));
    }

    fn cmd_kill(&mut self, args: &[&str]) {
        if args.is_empty() {
            self.println("Usage: kill <pid>");
            return;
        }

        let pid_str = args[0];
        let _pid: u32 = match pid_str.parse() {
            Ok(p) => p,
            Err(_) => {
                self.println("Error: Invalid PID");
                return;
            }
        };

        // For now, killing requires supervisor privilege
        self.println("Error: Kill requires supervisor privilege");
        self.println("(Terminal cannot directly kill processes yet)");
    }

    fn cmd_grant(&mut self, args: &[&str]) {
        if args.len() < 3 {
            self.println("Usage: grant <from_slot> <to_pid> <perms>");
            self.println("  perms: r=read, w=write, g=grant (e.g., 'rw')");
            return;
        }

        let from_slot: u32 = match args[0].parse() {
            Ok(s) => s,
            Err(_) => {
                self.println("Error: Invalid slot number");
                return;
            }
        };

        let to_pid: u32 = match args[1].parse() {
            Ok(p) => p,
            Err(_) => {
                self.println("Error: Invalid PID");
                return;
            }
        };

        let perms_str = args[2];
        let perms = syscall::Permissions {
            read: perms_str.contains('r'),
            write: perms_str.contains('w'),
            grant: perms_str.contains('g'),
        };

        match syscall::cap_grant(from_slot, to_pid, perms) {
            Ok(new_slot) => {
                self.println(&format!(
                    "Granted capability (slot {} -> PID {} at slot {})",
                    from_slot, to_pid, new_slot
                ));
            }
            Err(e) => {
                self.println(&format!("Error: {}", self.format_cap_error(e)));
            }
        }
    }

    fn cmd_revoke(&mut self, args: &[&str]) {
        if args.is_empty() {
            self.println("Usage: revoke <slot>");
            return;
        }

        let slot: u32 = match args[0].parse() {
            Ok(s) => s,
            Err(_) => {
                self.println("Error: Invalid slot number");
                return;
            }
        };

        match syscall::cap_delete(slot) {
            Ok(()) => {
                self.println(&format!("Deleted capability at slot {}", slot));
            }
            Err(e) => {
                self.println(&format!("Error: {}", self.format_cap_error(e)));
            }
        }
    }

    fn cmd_echo(&mut self, args: &[&str]) {
        let text = args.join(" ");
        self.println(&text);
    }

    fn cmd_time(&mut self) {
        let nanos = syscall::get_time();
        let secs = nanos / 1_000_000_000;
        let ms = (nanos % 1_000_000_000) / 1_000_000;
        self.println(&format!("Uptime: {}.{:03}s", secs, ms));
    }

    fn cmd_clear(&mut self) {
        // Send ANSI clear sequence
        self.print("\x1B[2J\x1B[H");
    }

    fn cmd_exit(&mut self) {
        self.println("Goodbye!");
        syscall::exit(0);
    }
}

impl OrbitalApp for TerminalApp {
    fn manifest() -> &'static AppManifest {
        &TERMINAL_MANIFEST
    }

    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError> {
        syscall::debug(&format!("Terminal starting (PID {})", ctx.pid));

        // Print banner
        self.println("Orbital OS Terminal");
        self.println("Type 'help' for available commands.");
        self.println("");
        self.print(Self::PROMPT);

        self.initialized = true;
        self.flush_output(ctx)
    }

    fn update(&mut self, _ctx: &AppContext) -> ControlFlow {
        ControlFlow::Yield
    }

    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError> {
        // Handle app protocol input
        if msg.tag == tags::MSG_APP_INPUT {
            // Try to decode as TerminalInput
            if let Ok(input) = TerminalInput::from_bytes(&msg.data) {
                return self.handle_input(&input, ctx);
            }
        }

        // Handle raw console input (from supervisor fallback)
        if msg.tag == MSG_CONSOLE_INPUT {
            if let Ok(text) = core::str::from_utf8(&msg.data) {
                return self.handle_raw_input(text, ctx);
            }
        }

        // Handle capability revocation notification
        if msg.tag == MSG_CAP_REVOKED {
            return self.handle_cap_revoked(&msg.data, ctx);
        }

        Ok(())
    }

    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("Terminal: shutting down");
    }
}

// Entry point - works for both WASM and native
app_main!(TerminalApp);

// Native main is provided by app_main! macro for WASM
// For native builds, this allows cargo check to work
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("Terminal app is meant to run as WASM in Orbital OS");
}
