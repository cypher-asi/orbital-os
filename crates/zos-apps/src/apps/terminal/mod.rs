//! Terminal Application
//!
//! Command-line interface for Zero OS. Demonstrates:
//! - Console output via SYS_CONSOLE_WRITE syscall
//! - Console input via kernel-delivered messages
//! - Direct syscalls (ps, caps, time)
//!
//! This is a canonical ZeroApp implementation - all command execution
//! happens in userspace, not in the supervisor.
//!
//! ## Console I/O Architecture
//!
//! - **Output**: Terminal calls `console_write()` which uses SYS_CONSOLE_WRITE.
//!   The kernel buffers the output and the supervisor drains it to the UI.
//! - **Input**: Supervisor delivers keyboard input via privileged kernel API
//!   to the terminal's input endpoint (slot 1).

mod command;
mod state;

pub use command::{Command, ParseError};
pub use state::{InputAction, TerminalInput, TerminalState, MSG_CONSOLE_INPUT, TYPE_TERMINAL_INPUT, TYPE_TERMINAL_STATE};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::protocol::tags;
use crate::framework::{
    AppContext, AppError, AppManifest, ControlFlow, Message, ZeroApp,
    TERMINAL_MANIFEST,
};
use crate::syscall;
use zos_process::{error, ObjectType, MSG_CAP_REVOKED};

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
    const PROMPT: &'static str = "zero> ";

    /// Print text to console output via SYS_CONSOLE_WRITE syscall
    fn print(&mut self, text: &str) {
        self.output_buffer.push_str(text);
    }

    /// Print a line (with newline)
    fn println(&mut self, text: &str) {
        self.print(text);
        self.print("\n");
    }

    /// Flush output buffer to console via SYS_CONSOLE_WRITE syscall
    fn flush_output(&mut self, _ctx: &AppContext) -> Result<(), AppError> {
        if self.output_buffer.is_empty() {
            return Ok(());
        }

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
                    self.history.push(line.to_string());
                    self.history_index = self.history.len();
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

    /// Handle raw console input byte-by-byte (from kernel serial input)
    fn handle_raw_input(&mut self, data: &[u8], ctx: &AppContext) -> Result<(), AppError> {
        for &byte in data {
            match byte {
                // Enter (CR or LF) - execute command
                0x0D | 0x0A => {
                    self.print("\n");
                    let line = core::mem::take(&mut self.input_buffer);
                    let line = line.trim();
                    if !line.is_empty() {
                        self.history.push(line.to_string());
                        self.history_index = self.history.len();
                        self.execute_command(line);
                    }
                    self.print(Self::PROMPT);
                }
                // Backspace (DEL or BS)
                0x7F | 0x08 => {
                    if !self.input_buffer.is_empty() {
                        self.input_buffer.pop();
                        // Erase character on screen: backspace, space, backspace
                        self.print("\x08 \x08");
                    }
                }
                // Ctrl+C - interrupt
                0x03 => {
                    self.input_buffer.clear();
                    self.println("^C");
                    self.print(Self::PROMPT);
                }
                // Ctrl+L - clear screen
                0x0C => {
                    self.input_buffer.clear();
                    self.print("\x1B[2J\x1B[H");
                    self.print(Self::PROMPT);
                }
                // Printable ASCII characters
                0x20..=0x7E => {
                    self.input_buffer.push(byte as char);
                    // Echo the character
                    self.print(&format!("{}", byte as char));
                }
                // Ignore other control characters
                _ => {}
            }
        }
        self.flush_output(ctx)
    }

    /// Handle capability revocation notification from supervisor
    fn handle_cap_revoked(&mut self, data: &[u8], ctx: &AppContext) -> Result<(), AppError> {
        if data.len() >= 14 {
            let slot = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let object_type = data[4];

            let type_name = ObjectType::from_u8(object_type)
                .map(|t| t.name())
                .unwrap_or("Unknown");

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
            e if e == error::E_PERM => {
                "Permission denied: insufficient capability permissions".to_string()
            }
            e if e == error::E_NOENT => {
                "Resource not found: capability may have been revoked".to_string()
            }
            code => format!("Operation failed (error code {})", code),
        }
    }

    /// Execute a command
    fn execute_command(&mut self, line: &str) {
        match Command::parse(line) {
            Ok(cmd) => self.run_command(cmd),
            Err(e) => {
                match e {
                    ParseError::MissingArgument { command, argument } => {
                        self.println(&format!("Error: {} requires {}", command, argument));
                    }
                    ParseError::InvalidArgument { argument, reason } => {
                        self.println(&format!("Error: {} {}", argument, reason));
                    }
                }
            }
        }
    }

    /// Run a parsed command
    fn run_command(&mut self, cmd: Command) {
        match cmd {
            Command::Help => self.cmd_help(),
            Command::Ps => self.cmd_ps(),
            Command::Caps => self.cmd_caps(),
            Command::Spawn { process_type } => self.cmd_spawn(&process_type),
            Command::Kill { pid } => self.cmd_kill(pid),
            Command::Grant { from_slot, to_pid, permissions } => {
                self.cmd_grant(from_slot, to_pid, permissions)
            }
            Command::Revoke { slot } => self.cmd_revoke(slot),
            Command::Echo { text } => self.cmd_echo(&text),
            Command::Time => self.cmd_time(),
            Command::Clear => self.cmd_clear(),
            Command::Exit => self.cmd_exit(),
            Command::Unknown { cmd } if cmd.is_empty() => {}
            Command::Unknown { cmd } => {
                self.println(&format!("Unknown command: {}", cmd));
                self.println("Type 'help' for available commands.");
            }
        }
    }

    fn cmd_help(&mut self) {
        self.println("=== Zero Terminal (userspace shell) ===");
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

    fn cmd_spawn(&mut self, process_type: &str) {
        syscall::debug(&format!("INIT:SPAWN:{}", process_type));
        self.println(&format!("Requested spawn of '{}'...", process_type));
    }

    fn cmd_kill(&mut self, _pid: u32) {
        self.println("Error: Kill requires supervisor privilege");
        self.println("(Terminal cannot directly kill processes yet)");
    }

    fn cmd_grant(&mut self, from_slot: u32, to_pid: u32, perms: syscall::Permissions) {
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

    fn cmd_revoke(&mut self, slot: u32) {
        match syscall::cap_delete(slot) {
            Ok(()) => {
                self.println(&format!("Deleted capability at slot {}", slot));
            }
            Err(e) => {
                self.println(&format!("Error: {}", self.format_cap_error(e)));
            }
        }
    }

    fn cmd_echo(&mut self, text: &str) {
        self.println(text);
    }

    fn cmd_time(&mut self) {
        let nanos = syscall::get_time();
        let secs = nanos / 1_000_000_000;
        let ms = (nanos % 1_000_000_000) / 1_000_000;
        self.println(&format!("Uptime: {}.{:03}s", secs, ms));
    }

    fn cmd_clear(&mut self) {
        self.print("\x1B[2J\x1B[H");
    }

    fn cmd_exit(&mut self) {
        self.println("Goodbye!");
        syscall::exit(0);
    }
}

impl ZeroApp for TerminalApp {
    fn manifest() -> &'static AppManifest {
        &TERMINAL_MANIFEST
    }

    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError> {
        syscall::debug(&format!("Terminal starting (PID {})", ctx.pid));

        self.println("Zero OS Terminal");
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
        // Debug: log all received messages
        syscall::debug(&format!("Terminal received msg: tag=0x{:x}, len={}", msg.tag, msg.data.len()));
        
        // Handle app protocol input
        if msg.tag == tags::MSG_APP_INPUT {
            if let Ok(input) = TerminalInput::from_bytes(&msg.data) {
                return self.handle_input(&input, ctx);
            }
        }

        // Handle raw console input bytes (from kernel serial input)
        if msg.tag == MSG_CONSOLE_INPUT {
            syscall::debug(&format!("Terminal: MSG_CONSOLE_INPUT data={:?}", &msg.data));
            return self.handle_raw_input(&msg.data, ctx);
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
