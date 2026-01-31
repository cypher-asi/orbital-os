# Stage 1.5: Init + Services

> **Status**: ✅ **COMPLETE**
>
> **Goal**: Bootstrap system with init process and service supervision.

## Current Implementation

### What's Complete ✅

| Component | Status | Location |
|-----------|--------|----------|
| Terminal process | ✅ | `crates/zos-apps/src/bin/terminal.rs` |
| Terminal commands | ✅ | help, ps, caps, echo, time, clear, exit |
| Process spawning | ✅ | Via supervisor `spawn` command |
| Console IPC | ✅ | Console output via IPC endpoint |

### What's Implemented ✅

| Component | Status | Description |
|-----------|--------|-------------|
| Init process (PID 1) | ✅ | `zos-init` crate spawns as PID 1 |
| Service registry | ✅ | Init maintains name → endpoint mapping |
| Service discovery | ✅ | MSG_LOOKUP_SERVICE protocol implemented |
| Bootstrap sequence | ✅ | Kernel → init → terminal via INIT:SPAWN |
| Init crate | ✅ | `crates/zos-init/` |
| Terminal registration | ✅ | Terminal registers with init on startup |

## Gap Analysis

### Current Bootstrap

Currently, the supervisor directly spawns processes via user commands:

```javascript
// User types "spawn terminal"
supervisor.send_input('spawn terminal');
```

### Required Bootstrap

The spec requires a proper bootstrap sequence:

```
1. Kernel boots (Genesis commit)
2. Kernel spawns init (PID 1)
3. Init receives root capabilities
4. Init reads config, spawns services
5. Services register with init
6. Services become discoverable
```

## Required Modifications

### Task 1: Create `zos-init` Crate

**File**: `crates/zos-init/Cargo.toml`

```toml
[package]
name = "zos-init"
version.workspace = true
edition.workspace = true

[[bin]]
name = "init"
path = "src/main.rs"

[dependencies]
zos-process = { workspace = true }
```

**File**: `crates/zos-init/src/main.rs`

```rust
#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use Zero_process::{self as syscall, debug, receive_blocking, send, create_endpoint, Permissions};

// Service registry: name → endpoint slot
static mut SERVICES: Option<BTreeMap<String, u32>> = None;

// Well-known message tags
const MSG_REGISTER_SERVICE: u32 = 0x1000;
const MSG_LOOKUP_SERVICE: u32 = 0x1001;
const MSG_LOOKUP_RESPONSE: u32 = 0x1002;

#[no_mangle]
pub extern "C" fn _start() {
    debug("init: Starting Zero OS init process");
    
    unsafe { SERVICES = Some(BTreeMap::new()); }
    
    // Create init's endpoint for service registration
    let (init_ep_id, init_ep_slot) = match create_endpoint() {
        Ok(r) => r,
        Err(e) => {
            debug(&alloc::format!("init: Failed to create endpoint: {}", e));
            syscall::exit(1);
        }
    };
    
    debug(&alloc::format!("init: Created endpoint {} in slot {}", init_ep_id, init_ep_slot));
    
    // Spawn core services (in future: read from config)
    spawn_service("terminal");
    
    // Main loop: handle service registration and discovery
    loop {
        let msg = receive_blocking(init_ep_slot);
        handle_message(&msg, init_ep_slot);
    }
}

fn spawn_service(name: &str) {
    // Request supervisor to spawn service
    // This uses a special syscall or IPC to supervisor
    debug(&alloc::format!("init: Spawning service '{}'", name));
    // TODO: Implement spawn syscall
}

fn handle_message(msg: &syscall::ReceivedMessage, my_slot: u32) {
    match msg.tag {
        MSG_REGISTER_SERVICE => {
            // Data format: [name_len: u8, name: [u8], endpoint_slot: u32]
            if msg.data.len() < 2 { return; }
            let name_len = msg.data[0] as usize;
            if msg.data.len() < 1 + name_len + 4 { return; }
            
            let name = core::str::from_utf8(&msg.data[1..1+name_len]).unwrap_or("");
            let ep_slot = u32::from_le_bytes([
                msg.data[1+name_len], msg.data[2+name_len],
                msg.data[3+name_len], msg.data[4+name_len]
            ]);
            
            debug(&alloc::format!("init: Service '{}' registered at slot {}", name, ep_slot));
            
            unsafe {
                if let Some(services) = SERVICES.as_mut() {
                    services.insert(String::from(name), ep_slot);
                }
            }
        }
        
        MSG_LOOKUP_SERVICE => {
            // Data format: [name_len: u8, name: [u8]]
            if msg.data.is_empty() { return; }
            let name_len = msg.data[0] as usize;
            if msg.data.len() < 1 + name_len { return; }
            
            let name = core::str::from_utf8(&msg.data[1..1+name_len]).unwrap_or("");
            
            let response = unsafe {
                SERVICES.as_ref()
                    .and_then(|s| s.get(name))
                    .copied()
            };
            
            // Send response back to requester
            // This requires knowing the requester's endpoint
            // For now, we'd need to grant them a capability first
            debug(&alloc::format!("init: Lookup '{}' -> {:?}", name, response));
        }
        
        _ => {
            debug(&alloc::format!("init: Unknown message tag 0x{:x}", msg.tag));
        }
    }
}

// Required for no_std
#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    debug(&alloc::format!("init: PANIC: {:?}", info));
    syscall::exit(1);
}

#[cfg(target_arch = "wasm32")]
mod allocator {
    // Same bump allocator as terminal
    // ...
}
```

### Task 2: Add Spawn Syscall

**File**: `crates/Zero-kernel/src/lib.rs`

Add new syscall:

```rust
pub const SYS_SPAWN: u32 = 0x15;

// In Syscall enum:
Syscall::Spawn { name: String, binary_slot: CapSlot },

// In handle_syscall:
Syscall::Spawn { name, binary_slot } => {
    // Verify caller has spawn capability
    // Load binary from memory region referenced by capability
    // Create new process
    // Return new PID
}
```

### Task 3: Update Bootstrap Sequence

**File**: `apps/zos-supervisor/src/lib.rs`

Modify boot to spawn init first:

```rust
pub fn boot(&mut self) {
    // 1. Create init process (PID 1)
    let init_pid = self.kernel.register_process("init");
    assert_eq!(init_pid.0, 1);
    
    // 2. Grant init root capabilities (access to spawn, all endpoints)
    self.grant_root_capabilities(init_pid);
    
    // 3. Load and spawn init WASM
    // This triggers init to run and spawn services
    
    self.console_output("Kernel booted. Init process started.\n");
}

fn grant_root_capabilities(&mut self, init_pid: ProcessId) {
    // Grant init special capabilities:
    // - Spawn capability (can create new processes)
    // - Root endpoint capability (receive service registrations)
}
```

### Task 4: Service Discovery Protocol

Define IPC protocol for service discovery:

```
Register Service:
  Tag: 0x1000
  Data: [name_len: u8, name: [u8; name_len], endpoint_slot: u32]
  
Lookup Service:
  Tag: 0x1001
  Data: [name_len: u8, name: [u8; name_len]]
  
Lookup Response:
  Tag: 0x1002
  Data: [found: u8, endpoint_slot: u32 (if found)]
```

### Task 5: Update Terminal to Register

**File**: `crates/zos-apps/src/bin/terminal.rs`

> **Note**: Terminal is now a canonical `ZeroApp` in `zos-apps`.
> It implements the same `ZeroApp` trait as Clock and Calculator.

```rust
impl ZeroApp for TerminalApp {
    fn manifest() -> &'static AppManifest {
        &TERMINAL_MANIFEST
    }

    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError> {
        // Print banner and prompt
        self.println("Zero OS Terminal");
        self.print(Self::PROMPT);
        self.flush_output(ctx)
    }
    
    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError> {
        // Handle console input, execute commands via syscalls
    }
}
```

### Task 6: Update Cargo Workspace

**File**: `Cargo.toml`

```toml
[workspace]
members = [
    # ... existing ...
    "crates/zos-init",
]
```

### Task 7: Update Build

**File**: `Makefile`

```makefile
build-processes:
	@echo "Building process WASM binaries..."
	cargo build -p zos-init --target wasm32-unknown-unknown --release
	cargo build -p zos-system-procs --target wasm32-unknown-unknown --release
	cargo build -p zos-apps --bins --target wasm32-unknown-unknown --release
	@echo "Copying WASM binaries..."
	mkdir -p web/processes
	cp target/wasm32-unknown-unknown/release/Zero_init.wasm web/processes/init.wasm
	cp target/wasm32-unknown-unknown/release/terminal.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/permission.wasm web/processes/
	# ... other processes ...
```

## Test Criteria

### Manual Tests

1. Boot system → init starts (PID 1)
2. Init spawns terminal
3. Terminal registers with init
4. User can discover terminal via init

### Automated Tests

```rust
#[test]
fn test_init_is_pid_1() {
    let hal = MockHal::new();
    let mut kernel = Kernel::new(hal);
    kernel.boot();
    
    let init = kernel.get_process(ProcessId(1));
    assert!(init.is_some());
    assert_eq!(init.unwrap().name, "init");
}

#[test]
fn test_service_registration() {
    // Create init with service registry
    // Register a service
    // Verify lookup works
}
```

## Invariants

### 1. Init is PID 1 ❌ (Not enforced)

- Init should always be the first process
- Init receives root capabilities at genesis

### 2. Service Registration ❌ (Not implemented)

- Services register by sending message to init
- Init maintains service → endpoint mapping

### 3. Service Discovery ❌ (Not implemented)

- Processes lookup services via init
- Init grants endpoint capabilities on lookup

## Verification Checklist

- [x] `zos-init` crate created
- [x] Init is spawned as PID 1
- [x] Init receives endpoints (slot 0 = init endpoint, slot 1 = console)
- [x] Services can register with init (MSG_REGISTER_SERVICE)
- [x] Service discovery protocol implemented (MSG_LOOKUP_SERVICE)
- [x] Terminal registers on startup
- [x] All existing tests pass (81 tests)

## Estimated Changes

| File | Change Type | Lines |
|------|-------------|-------|
| `crates/zos-init/` | New crate | ~200 |
| `crates/Zero-kernel/src/lib.rs` | Modify | ~50 |
| `crates/zos-apps/src/bin/terminal.rs` | New file | ~300 |
| `crates/zos-supervisor/src/lib.rs` | Modify | ~50 |
| `Makefile` | Modify | ~10 |
| Tests | Add | ~50 |

## Alternative: Simplified Bootstrap

For a simpler initial implementation, we could:

1. Keep supervisor as implicit init
2. Hard-code service endpoints
3. Skip service registry
4. Implement full init in Phase 2

This would allow focusing on Stage 1.6 (replay) first.

## Next Stage

After implementing service bootstrap, proceed to [Stage 1.6: Replay + Testing](stage-1.6-replay-testing.md).
