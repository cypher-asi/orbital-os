//! Terminal Process for Orbital OS
//!
//! This is a user-space process that provides a command-line interface.
//! It runs in its own Web Worker with isolated memory.

// Only use no_std when building for WASM
#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(target_arch = "wasm32")]
extern crate alloc;

#[cfg(target_arch = "wasm32")]
use alloc::format;
#[cfg(target_arch = "wasm32")]
use alloc::string::String;
#[cfg(target_arch = "wasm32")]
use alloc::vec::Vec;

#[cfg(not(target_arch = "wasm32"))]
use std::format;
#[cfg(not(target_arch = "wasm32"))]
use std::string::String;
#[cfg(not(target_arch = "wasm32"))]
use std::vec::Vec;

use orbital_process::{
    self as syscall, INIT_ENDPOINT_SLOT, MSG_CONSOLE_INPUT, MSG_REGISTER_SERVICE,
};

// Well-known capability slots (assigned by supervisor at spawn)
const CONSOLE_OUTPUT_SLOT: u32 = 0;
const CONSOLE_INPUT_SLOT: u32 = 1;
// Note: INIT_ENDPOINT_SLOT (2) is provided by orbital_process

/// Terminal state
struct Terminal {
    /// Input buffer for current line
    #[allow(dead_code)]
    input_buffer: String,
}

impl Terminal {
    fn new() -> Self {
        Self {
            input_buffer: String::new(),
        }
    }

    /// Print text to console
    fn print(&self, text: &str) {
        syscall::console_write(CONSOLE_OUTPUT_SLOT, text);
    }

    /// Print a line (with newline)
    fn println(&self, text: &str) {
        self.print(text);
        self.print("\n");
    }

    /// Register with init service registry
    fn register_with_init(&self) {
        // Build registration message: [name_len: u8, name: [u8], endpoint_id_low: u32, endpoint_id_high: u32]
        // For now, use endpoint_id = 0 since we don't have our endpoint ID yet
        let name = "terminal";
        let mut data = Vec::new();
        data.push(name.len() as u8);
        data.extend_from_slice(name.as_bytes());
        // Use console input endpoint ID (placeholder - we'd need actual endpoint ID)
        data.extend_from_slice(&0u32.to_le_bytes()); // endpoint_id_low
        data.extend_from_slice(&0u32.to_le_bytes()); // endpoint_id_high

        match syscall::send(INIT_ENDPOINT_SLOT, MSG_REGISTER_SERVICE, &data) {
            Ok(()) => syscall::debug("terminal: Registered with init"),
            Err(e) => syscall::debug(&format!("terminal: Failed to register with init: {}", e)),
        }
    }

    /// Run the terminal main loop
    fn run(&mut self) {
        // Register with init service
        self.register_with_init();

        self.println("Orbital OS Terminal");
        self.println("Type 'help' for available commands.");
        self.println("");
        self.print("orbital> ");

        // Main loop: wait for input, execute commands
        loop {
            // Check for input messages
            if let Some(msg) = syscall::receive(CONSOLE_INPUT_SLOT) {
                if msg.tag == MSG_CONSOLE_INPUT {
                    // Convert bytes to string
                    if let Ok(input) = core::str::from_utf8(&msg.data) {
                        self.handle_input(input);
                    }
                }
            }
            syscall::yield_now();
        }
    }

    /// Handle input from user
    fn handle_input(&mut self, input: &str) {
        let line = input.trim();
        if line.is_empty() {
            self.print("orbital> ");
            return;
        }

        // Echo the command (since we received it as a message)
        // The UI should have already displayed it

        self.execute_command(line);
        self.print("orbital> ");
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
            "echo" => self.cmd_echo(args),
            "time" => self.cmd_time(),
            "clear" => self.cmd_clear(),
            "exit" => self.cmd_exit(),
            _ => {
                self.println(&format!("Unknown command: {}", cmd));
                self.println("Type 'help' for available commands.");
            }
        }
    }

    fn cmd_help(&self) {
        self.println("Available commands:");
        self.println("  help   - Show this help message");
        self.println("  ps     - List running processes");
        self.println("  caps   - List my capabilities");
        self.println("  echo   - Echo arguments");
        self.println("  time   - Show system uptime");
        self.println("  clear  - Clear the screen");
        self.println("  exit   - Exit the terminal");
    }

    fn cmd_ps(&self) {
        // For now, use debug syscall to request process list
        // The supervisor will intercept this and return the data
        syscall::debug("SYSCALL:LIST_PROCESSES");
        // The actual process list will come via a response message
        // For the vertical slice, we'll have the supervisor handle this
        self.println("PID  STATE    NAME");
        self.println("---  -----    ----");
        self.println("1    Running  init");
        self.println("2    Running  terminal");
    }

    fn cmd_caps(&self) {
        syscall::debug("SYSCALL:LIST_CAPS");
        self.println("SLOT  TYPE      PERMS  OBJECT");
        self.println("----  ----      -----  ------");
        self.println("0     Endpoint  RWG    console_out");
        self.println("1     Endpoint  R--    console_in");
    }

    fn cmd_echo(&self, args: &[&str]) {
        let text = args.join(" ");
        self.println(&text);
    }

    fn cmd_time(&self) {
        let nanos = syscall::get_time();
        let secs = nanos / 1_000_000_000;
        let ms = (nanos % 1_000_000_000) / 1_000_000;
        self.println(&format!("Uptime: {}.{:03}s", secs, ms));
    }

    fn cmd_clear(&self) {
        // Send special escape sequence that the UI can interpret
        self.print("\x1B[2J\x1B[H");
    }

    fn cmd_exit(&self) {
        self.println("Goodbye!");
        syscall::exit(0);
    }
}

// ============================================================================
// WASM Entry Point
// ============================================================================

/// Process entry point - called by the Web Worker
#[no_mangle]
pub extern "C" fn _start() {
    let mut terminal = Terminal::new();
    terminal.run();
}

// ============================================================================
// Panic Handler (required for no_std on WASM)
// ============================================================================

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use alloc::string::ToString;
    let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
        format!("PANIC: {}", s)
    } else {
        "PANIC: unknown".to_string()
    };
    syscall::debug(&msg);
    syscall::exit(1);
}

// ============================================================================
// Allocator (required for alloc in no_std on WASM)
// ============================================================================

#[cfg(target_arch = "wasm32")]
mod allocator {
    use core::alloc::{GlobalAlloc, Layout};

    struct BumpAllocator {
        head: core::sync::atomic::AtomicUsize,
    }

    #[global_allocator]
    static ALLOCATOR: BumpAllocator = BumpAllocator {
        head: core::sync::atomic::AtomicUsize::new(0),
    };

    const HEAP_START: usize = 0x10000; // 64KB offset
    const HEAP_SIZE: usize = 1024 * 1024; // 1MB heap

    unsafe impl GlobalAlloc for BumpAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let size = layout.size();
            let align = layout.align();

            loop {
                let head = self.head.load(core::sync::atomic::Ordering::Relaxed);
                let aligned = (HEAP_START + head + align - 1) & !(align - 1);
                let new_head = aligned - HEAP_START + size;

                if new_head > HEAP_SIZE {
                    return core::ptr::null_mut();
                }

                if self
                    .head
                    .compare_exchange_weak(
                        head,
                        new_head,
                        core::sync::atomic::Ordering::SeqCst,
                        core::sync::atomic::Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    return aligned as *mut u8;
                }
            }
        }

        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
            // Bump allocator doesn't deallocate
        }
    }
}
