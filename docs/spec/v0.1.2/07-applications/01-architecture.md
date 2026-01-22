# Application Architecture

> Applications are isolated processes that communicate via IPC and request capabilities through Init.

## Overview

This document specifies the platform-agnostic application model for Zero OS:

- **Process Model**: Apps as isolated processes (Web Workers on WASM, hardware processes on native)
- **Control Flow**: Complete lifecycle from spawn through kernel to termination
- **Kernel Integration**: How apps interact with Kernel, ProcessManager, CSpace
- **Axiom Integration**: What gets logged to CommitLog (ProcessCreated, CapInserted, etc.)
- **Platform Separation**: Core app logic vs. platform-specific UI

## App Layer Model

```
┌───────────────────────────────────────────────────────────────┐
│              Platform-Agnostic Core (crates/zos-apps/)    │
│                                                               │
│  - ZeroApp trait (all apps implement this)                 │
│  - AppManifest (declarative capabilities)                     │
│  - AppRuntime (event loop, syscall dispatch)                  │
│  - UI Protocol (MSG_APP_OUTPUT, MSG_APP_INPUT)                │
└───────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
┌───────────────┐    ┌───────────────┐    ┌───────────────┐
│  WASM/Browser │    │    QEMU       │    │  Bare Metal   │
│               │    │               │    │               │
│ React UI      │    │ Text/VGA UI   │    │ Framebuffer   │
│ IndexedDB     │    │ VirtIO Block  │    │ NVMe          │
│ useSupervisor │    │ Serial I/O    │    │ Native I/O    │
└───────────────┘    └───────────────┘    └───────────────┘
```

## AppManifest

Applications declare their identity and capability requirements through a manifest:

```rust
/// Application manifest declaring identity and capabilities
pub struct AppManifest {
    /// Unique identifier, reverse-domain format
    /// Example: "com.Zero.clock"
    pub id: &'static str,
    
    /// Human-readable name
    /// Example: "Clock"
    pub name: &'static str,
    
    /// Semantic version
    /// Example: "1.0.0"
    pub version: &'static str,
    
    /// Brief description
    pub description: &'static str,
    
    /// Requested capabilities
    pub capabilities: &'static [CapabilityRequest],
}

/// A capability request with reason for user consent
pub struct CapabilityRequest {
    /// Type of kernel object being requested
    pub object_type: ObjectType,
    
    /// Permissions needed on this object
    pub permissions: Permissions,
    
    /// Human-readable reason (shown to user in permission dialog)
    pub reason: &'static str,
    
    /// Whether the app can function without this capability
    pub required: bool,
}
```

### Example Manifest

```rust
pub static CLOCK_MANIFEST: AppManifest = AppManifest {
    id: "com.Zero.clock",
    name: "Clock",
    version: "1.0.0",
    description: "Displays current time and date",
    capabilities: &[
        CapabilityRequest {
            object_type: ObjectType::Endpoint,
            permissions: Permissions::READ_WRITE,
            reason: "Send time updates to display",
            required: true,
        },
    ],
};

pub static CALCULATOR_MANIFEST: AppManifest = AppManifest {
    id: "com.Zero.calculator",
    name: "Calculator",
    version: "1.0.0", 
    description: "Basic arithmetic calculator",
    capabilities: &[
        CapabilityRequest {
            object_type: ObjectType::Endpoint,
            permissions: Permissions::READ_WRITE,
            reason: "Receive input and send results to display",
            required: true,
        },
    ],
};
```

## ZeroApp Trait

All Zero applications implement this trait:

```rust
/// The Program Interface that all Zero apps implement.
pub trait ZeroApp {
    /// Returns the static application manifest.
    fn manifest() -> &'static AppManifest where Self: Sized;
    
    /// Called once when the app starts.
    /// Initialize state, set up IPC endpoints.
    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError>;
    
    /// Called repeatedly in the event loop.
    /// Perform periodic work, update state.
    fn update(&mut self, ctx: &AppContext) -> ControlFlow;
    
    /// Called when a message is received via IPC.
    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError>;
    
    /// Called before the app exits.
    /// Clean up resources.
    fn shutdown(&mut self, ctx: &AppContext);
}
```

### AppContext

The context provides information about the current execution environment:

```rust
pub struct AppContext {
    /// This process's ID
    pub pid: u32,
    
    /// Monotonic uptime in nanoseconds (via SYS_GET_TIME)
    /// Suitable for measuring durations, scheduling
    pub uptime_ns: u64,
    
    /// Wall-clock time in milliseconds since Unix epoch (via SYS_GET_WALLCLOCK)
    /// Suitable for displaying time-of-day to users
    pub wallclock_ms: u64,
    
    /// Endpoint for communicating with UI (if connected)
    pub ui_endpoint: Option<u32>,
}
```

### ControlFlow

Apps indicate what to do after `update()`:

```rust
pub enum ControlFlow {
    /// Continue to next update cycle
    Continue,
    
    /// Exit with the given code
    Exit(i32),
    
    /// Yield CPU, wait for next scheduling quantum
    Yield,
}
```

## Application Lifecycle

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Application Lifecycle                           │
│                                                                      │
│  1. Spawn Request                                                    │
│     └─ Desktop/Init requests spawn via IPC                          │
│     └─ Supervisor loads WASM binary                                  │
│     └─ Kernel creates process (ProcessCreated commit)               │
│                                                                      │
│  2. Capability Setup                                                 │
│     └─ Desktop reads AppManifest.capabilities[]                     │
│     └─ For factory apps: auto-grant basic capabilities              │
│     └─ For third-party: show permission dialog, await user consent  │
│     └─ Init grants capabilities via SYS_CAP_GRANT                   │
│     └─ Kernel inserts caps (CapabilityInserted commits)             │
│                                                                      │
│  3. Initialize                                                       │
│     └─ AppRuntime calls app.init(ctx)                               │
│     └─ App sets up internal state                                   │
│     └─ App sends MSG_UI_READY when ready                            │
│                                                                      │
│  4. Run Loop                                                         │
│     └─ AppRuntime builds context (time syscalls)                    │
│     └─ Polls for IPC messages                                        │
│     └─ Calls app.on_message() for each message                       │
│     └─ Calls app.update()                                            │
│     └─ Honors ControlFlow (Continue/Yield/Exit)                     │
│                                                                      │
│  5. Terminate                                                        │
│     └─ App calls SYS_EXIT or is killed                              │
│     └─ AppRuntime calls app.shutdown(ctx)                           │
│     └─ Kernel reclaims resources (ProcessTerminated commit)         │
│     └─ Capabilities revoked                                          │
└─────────────────────────────────────────────────────────────────────┘
```

## Axiom Integration

All lifecycle events are recorded in the CommitLog:

| Event | CommitType | Data |
|-------|------------|------|
| Process spawn | `ProcessCreated` | pid, name |
| Capability grant | `CapabilityInserted` | pid, slot, cap_data |
| Message sent | `MessageSent` | from, to, tag, size |
| Process fault | `ProcessFaulted` | pid, reason |
| Process exit | `ProcessTerminated` | pid, exit_code |

This enables:
- **Replay**: Reconstruct any historical state
- **Audit**: Track what each app did
- **Debugging**: Reproduce issues from logs

## Platform-Specific Execution

| Platform | Process Model | Scheduling | Time Sources |
|----------|---------------|------------|--------------|
| WASM | Web Worker | Cooperative (`yield_now()`) | `performance.now()`, `Date.now()` |
| QEMU | Hardware process | Preemptive (timer interrupt) | HPET/PIT, RTC |
| Bare Metal | Hardware process | Preemptive (APIC timer) | TSC/HPET, RTC |

### WASM Implementation

On WASM, each app runs in a Web Worker:

```
┌─────────────────────────────────────────────────────────────────┐
│  Main Thread (React)                                             │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │ useSupervisor() hook                                        ││
│  │ ├── Manages Supervisor                                       ││
│  │ ├── Routes messages to React components                      ││
│  │ └── Handles spawn requests from Desktop                      ││
│  └─────────────────────────────────────────────────────────────┘│
│                              │                                   │
│                        postMessage                               │
│                              ▼                                   │
├─────────────────────────────────────────────────────────────────┤
│  Web Worker Thread (Per App)                                     │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │ WASM Runtime                                                 ││
│  │ ├── zos-apps crate compiled to wasm32                    ││
│  │ ├── AppRuntime event loop                                    ││
│  │ └── syscalls → postMessage → Kernel                          ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

## Message Flow

```
┌──────────────┐    IPC     ┌──────────────┐    IPC     ┌──────────────┐
│   App WASM   │◄──────────►│    Kernel    │◄──────────►│   React UI   │
│   Backend    │            │              │            │  Component   │
└──────────────┘            └──────────────┘            └──────────────┘
      │                                                        │
      │  1. app.update() computes new state                   │
      │  2. Serializes ClockState/CalculatorState             │
      │  3. Sends MSG_APP_STATE via IPC                       │
      │                           │                            │
      │                           └───────────────────────────►│
      │                                                        │
      │                    4. React receives state             │
      │                    5. Re-renders UI                    │
      │                                                        │
      │                    6. User clicks button               │
      │                    7. Sends MSG_APP_INPUT              │
      │◄───────────────────────────────────────────────────────┤
      │                                                        │
      │  8. app.on_message() receives input                   │
      │  9. Updates internal state                             │
      │  10. Back to step 1                                    │
      │                                                        │
```

## Error Handling

### AppError

```rust
pub enum AppError {
    /// Initialization failed
    InitFailed(String),
    
    /// Required capability not granted
    MissingCapability(ObjectType),
    
    /// IPC communication error
    IpcError(String),
    
    /// Protocol error (invalid message format)
    ProtocolError(ProtocolError),
    
    /// Internal application error
    Internal(String),
}
```

### Fault Handling

When an app crashes or faults:

1. Supervisor catches the fault
2. Axiom records `ProcessFaulted` commit
3. Kernel terminates process, reclaims resources
4. Desktop notified to close window
5. User may be shown error dialog

## Factory Apps vs Third-Party Apps

| Aspect | Factory Apps | Third-Party Apps |
|--------|--------------|------------------|
| Trust level | Bundled, trusted | Unknown, sandboxed |
| Permission UI | Auto-granted | User consent required |
| Capabilities | Basic set | Manifest-declared |
| Source | `crates/zos-apps/` | External (future) |

Factory apps (Clock, Calculator) demonstrate the framework while third-party apps require the full permission dialog flow.
