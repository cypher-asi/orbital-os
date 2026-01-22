# Application Runtime

> AppRuntime runs inside each WASM process, providing the event loop and syscall interface.

## Overview

This document specifies the `AppRuntime` component that:

- Runs **inside** each app's WASM process (Web Worker)
- Provides the event loop that drives `ZeroApp` implementations
- Handles syscall dispatch to the kernel
- Manages context building (time, PID, endpoints)

## Critical Design Point

`AppRuntime` runs **inside the WASM process** (Web Worker), not in the supervisor.

```
┌─────────────────────────────────────────────────────────────────┐
│  Web Worker (clock.wasm process)                                │
│                                                                 │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │  AppRuntime::run()                                         │ │
│  │                                                            │ │
│  │  loop {                                                    │ │
│  │      // Build context (calls syscalls → kernel → HAL)      │ │
│  │      let ctx = self.build_context();                       │ │
│  │                                                            │ │
│  │      // Poll for messages (syscall → kernel)               │ │
│  │      if let Some(msg) = syscall::receive(input_slot) {     │ │
│  │          app.on_message(&ctx, msg);                        │ │
│  │      }                                                     │ │
│  │                                                            │ │
│  │      // Run app update                                     │ │
│  │      match app.update(&ctx) {                              │ │
│  │          ControlFlow::Exit(code) => break,                 │ │
│  │          ControlFlow::Yield => syscall::yield_now(),       │ │
│  │          ControlFlow::Continue => {}                       │ │
│  │      }                                                     │ │
│  │  }                                                         │ │
│  └────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

## AppRuntime Structure

```rust
/// Runtime environment for an Zero application.
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
}
```

## Runtime Lifecycle

```rust
impl AppRuntime {
    /// Create a new runtime for an app.
    pub fn new() -> Self {
        // Get our PID via syscall
        let pid = syscall::get_pid();
        
        AppRuntime {
            pid,
            ui_slot: None,
            input_slot: None,
            last_update_ns: 0,
            update_interval_ns: 16_666_667, // ~60 FPS default
        }
    }
    
    /// Run the main event loop.
    pub fn run<A: ZeroApp>(&mut self, mut app: A) -> ! {
        // Build initial context
        let ctx = self.build_context();
        
        // Initialize the app
        if let Err(e) = app.init(&ctx) {
            // Log error and exit
            syscall::debug(&format!("App init failed: {:?}", e));
            syscall::exit(1);
        }
        
        // Main event loop
        loop {
            // Build fresh context with current time
            let ctx = self.build_context();
            
            // Poll for incoming messages
            if let Some(slot) = self.input_slot {
                while let Some(msg) = syscall::receive_nonblocking(slot) {
                    if let Err(e) = app.on_message(&ctx, msg) {
                        syscall::debug(&format!("Message handling error: {:?}", e));
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
            wallclock_ms: syscall::get_wallclock(),
            ui_endpoint: self.ui_slot,
        }
    }
    
    /// Set the UI endpoint slot.
    pub fn set_ui_endpoint(&mut self, slot: u32) {
        self.ui_slot = Some(slot);
    }
    
    /// Set the input endpoint slot.
    pub fn set_input_endpoint(&mut self, slot: u32) {
        self.input_slot = Some(slot);
    }
    
    /// Set the update interval.
    pub fn set_update_interval_ms(&mut self, ms: u64) {
        self.update_interval_ns = ms * 1_000_000;
    }
}
```

## The app_main! Macro

Eliminates boilerplate (~80 lines reduced to 1):

```rust
/// Generate the entry point and runtime setup for an Zero app.
#[macro_export]
macro_rules! app_main {
    ($app_type:ty) => {
        // Entry point
        #[no_mangle]
        pub extern "C" fn _start() {
            // Initialize allocator
            $crate::init_allocator();
            
            // Create app instance
            let app = <$app_type>::default();
            
            // Create and run runtime
            let mut runtime = $crate::AppRuntime::new();
            
            // Setup endpoints from capability slots
            // Slot 0 is typically the UI output endpoint
            // Slot 1 is typically the input endpoint
            runtime.set_ui_endpoint(0);
            runtime.set_input_endpoint(1);
            
            // Run forever (exits via syscall::exit)
            runtime.run(app);
        }
        
        // Panic handler
        #[panic_handler]
        fn panic(info: &core::panic::PanicInfo) -> ! {
            if let Some(msg) = info.payload().downcast_ref::<&str>() {
                $crate::syscall::debug(&format!("PANIC: {}", msg));
            } else {
                $crate::syscall::debug("PANIC: unknown");
            }
            $crate::syscall::exit(-1);
        }
        
        // Global allocator
        #[global_allocator]
        static ALLOCATOR: $crate::WasmAllocator = $crate::WasmAllocator::new();
    };
}
```

### Usage

```rust
use Zero_apps::{app_main, ZeroApp, AppContext, ControlFlow, AppError, Message};

#[derive(Default)]
struct ClockApp {
    last_update: u64,
}

impl ZeroApp for ClockApp {
    fn manifest() -> &'static AppManifest { &CLOCK_MANIFEST }
    
    fn init(&mut self, _ctx: &AppContext) -> Result<(), AppError> {
        Ok(())
    }
    
    fn update(&mut self, ctx: &AppContext) -> ControlFlow {
        // Update every second
        if ctx.uptime_ns - self.last_update > 1_000_000_000 {
            self.last_update = ctx.uptime_ns;
            self.send_time_update(ctx);
        }
        ControlFlow::Yield
    }
    
    fn on_message(&mut self, _ctx: &AppContext, _msg: Message) -> Result<(), AppError> {
        Ok(())
    }
    
    fn shutdown(&mut self, _ctx: &AppContext) {}
}

app_main!(ClockApp);
```

## Syscall Interface

The runtime uses these syscalls:

```rust
pub mod syscall {
    /// Get this process's ID
    pub fn get_pid() -> u32 {
        // SYS_GET_PID = 0x01
        unsafe { raw_syscall(0x01, 0, 0, 0, 0) as u32 }
    }
    
    /// Get monotonic uptime in nanoseconds
    pub fn get_time() -> u64 {
        // SYS_GET_TIME = 0x02
        let lo = unsafe { raw_syscall(0x02, 0, 0, 0, 0) };
        let hi = unsafe { raw_syscall(0x02, 1, 0, 0, 0) };
        ((hi as u64) << 32) | (lo as u64)
    }
    
    /// Get wall-clock time in milliseconds since Unix epoch
    pub fn get_wallclock() -> u64 {
        // SYS_GET_WALLCLOCK = 0x03
        let lo = unsafe { raw_syscall(0x03, 0, 0, 0, 0) };
        let hi = unsafe { raw_syscall(0x03, 1, 0, 0, 0) };
        ((hi as u64) << 32) | (lo as u64)
    }
    
    /// Yield CPU to scheduler
    pub fn yield_now() {
        // SYS_YIELD = 0x10
        unsafe { raw_syscall(0x10, 0, 0, 0, 0) };
    }
    
    /// Exit with code
    pub fn exit(code: i32) -> ! {
        // SYS_EXIT = 0x11
        unsafe { raw_syscall(0x11, code as u32, 0, 0, 0) };
        unreachable!()
    }
    
    /// Send message via IPC
    pub fn send(slot: u32, tag: u32, data: &[u8]) -> Result<(), IpcError> {
        // SYS_SEND = 0x20
        let ptr = data.as_ptr() as u32;
        let len = data.len() as u32;
        let result = unsafe { raw_syscall(0x20, slot, tag, ptr, len) };
        if result == 0 { Ok(()) } else { Err(IpcError::from_code(result)) }
    }
    
    /// Receive message (blocking)
    pub fn receive(slot: u32) -> Option<Message> {
        // SYS_RECEIVE = 0x21
        // Returns message if available, blocks otherwise
        // ...
    }
    
    /// Receive message (non-blocking)
    pub fn receive_nonblocking(slot: u32) -> Option<Message> {
        // SYS_RECEIVE_NONBLOCK = 0x22
        // ...
    }
    
    /// Debug output to console
    pub fn debug(msg: &str) {
        // SYS_DEBUG = 0x30
        let ptr = msg.as_ptr() as u32;
        let len = msg.len() as u32;
        unsafe { raw_syscall(0x30, ptr, len, 0, 0) };
    }
}
```

## Platform-Specific Execution

| Platform | Process Model | Scheduling | Time Sources |
|----------|---------------|------------|--------------|
| WASM | Web Worker | Cooperative (`yield_now()`) | `performance.now()`, `Date.now()` |
| QEMU | Hardware process | Preemptive (timer interrupt) | HPET/PIT, RTC |
| Bare Metal | Hardware process | Preemptive (APIC timer) | TSC/HPET, RTC |

### WASM Implementation Details

On WASM, syscalls are implemented via:

```rust
// Low-level syscall interface for WASM
#[cfg(target_arch = "wasm32")]
unsafe fn raw_syscall(num: u32, a0: u32, a1: u32, a2: u32, a3: u32) -> u32 {
    // Extern function provided by WASM runtime
    extern "C" {
        fn Zero_syscall(num: u32, a0: u32, a1: u32, a2: u32, a3: u32) -> u32;
    }
    Zero_syscall(num, a0, a1, a2, a3)
}
```

The `Zero_syscall` function is injected by the WASM runtime and routes to the kernel via `postMessage`.

### QEMU/Bare Metal Implementation

On native platforms, syscalls are implemented via software interrupt:

```rust
#[cfg(not(target_arch = "wasm32"))]
unsafe fn raw_syscall(num: u32, a0: u32, a1: u32, a2: u32, a3: u32) -> u32 {
    let result: u32;
    asm!(
        "int 0x80",
        inout("eax") num => result,
        in("ebx") a0,
        in("ecx") a1,
        in("edx") a2,
        in("esi") a3,
        options(nostack)
    );
    result
}
```

## Time Handling

### Monotonic Time (uptime_ns)

- **Purpose**: Measuring durations, scheduling, animations
- **Properties**: Strictly increasing, unaffected by clock changes
- **Syscall**: `SYS_GET_TIME`
- **WASM source**: `performance.now()` in microseconds, converted to nanos

### Wall-Clock Time (wallclock_ms)

- **Purpose**: Displaying time-of-day to users
- **Properties**: Can jump (NTP sync, timezone changes)
- **Syscall**: `SYS_GET_WALLCLOCK`
- **WASM source**: `Date.now()` in milliseconds

### New Syscall: SYS_GET_WALLCLOCK

Required for the Clock app to display real time:

```rust
/// Syscall number for getting wall-clock time
pub const SYS_GET_WALLCLOCK: u32 = 0x03;

/// Implementation in kernel
impl<H: HAL> Kernel<H> {
    fn handle_get_wallclock(&self, arg: u32) -> u64 {
        // Get wall-clock from HAL
        let wallclock_ms = self.hal.wallclock_ms();
        
        // arg=0 returns low 32 bits, arg=1 returns high 32 bits
        if arg == 0 {
            (wallclock_ms & 0xFFFF_FFFF) as u64
        } else {
            (wallclock_ms >> 32) as u64
        }
    }
}

/// HAL trait extension
pub trait HAL {
    // ... existing methods ...
    
    /// Get wall-clock time in milliseconds since Unix epoch.
    /// This is real time-of-day, not monotonic.
    fn wallclock_ms(&self) -> u64;
}

/// WASM HAL implementation
impl HAL for WasmHal {
    fn wallclock_ms(&self) -> u64 {
        // In JavaScript: Date.now()
        js_sys::Date::now() as u64
    }
}
```

## Memory Allocation

WASM apps use a simple bump allocator:

```rust
/// Simple bump allocator for WASM apps.
pub struct WasmAllocator {
    heap_start: Cell<usize>,
    heap_end: Cell<usize>,
}

impl WasmAllocator {
    pub const fn new() -> Self {
        WasmAllocator {
            heap_start: Cell::new(0),
            heap_end: Cell::new(0),
        }
    }
}

unsafe impl GlobalAlloc for WasmAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Simple bump allocation
        let size = layout.size();
        let align = layout.align();
        
        let mut current = self.heap_start.get();
        
        // Align up
        current = (current + align - 1) & !(align - 1);
        
        let new_start = current + size;
        
        if new_start > self.heap_end.get() {
            // Out of memory - try to grow
            if !self.grow(new_start) {
                return core::ptr::null_mut();
            }
        }
        
        self.heap_start.set(new_start);
        current as *mut u8
    }
    
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator doesn't free individual allocations
        // Apps should minimize allocation or use arena patterns
    }
}
```

## Error Handling

Runtime errors are handled gracefully:

```rust
impl AppRuntime {
    fn handle_error(&self, error: AppError) {
        match error {
            AppError::InitFailed(msg) => {
                syscall::debug(&format!("Init failed: {}", msg));
                syscall::exit(1);
            }
            AppError::MissingCapability(cap_type) => {
                syscall::debug(&format!("Missing capability: {:?}", cap_type));
                // Continue running with reduced functionality
            }
            AppError::IpcError(msg) => {
                syscall::debug(&format!("IPC error: {}", msg));
                // Retry or continue
            }
            AppError::ProtocolError(e) => {
                syscall::debug(&format!("Protocol error: {:?}", e));
                // Skip malformed message
            }
            AppError::Internal(msg) => {
                syscall::debug(&format!("Internal error: {}", msg));
                syscall::exit(2);
            }
        }
    }
}
```
