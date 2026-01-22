# Application Model

> Applications run in sandboxed environments with capability-controlled access.

## Overview

Applications in Zero OS:

1. **Sandboxed**: Run as isolated processes (Web Workers on WASM, hardware processes on native)
2. **Capability-based**: Must hold capabilities to access system resources
3. **Platform-agnostic**: Core app logic works across WASM, QEMU, and bare metal
4. **Axiom-integrated**: All lifecycle events recorded in CommitLog for replay

## Documentation Structure

```
07-applications/
├── README.md              (this file - overview)
├── 00-system-integrity.md (Axiom-first architecture invariant)
├── 01-architecture.md     (App model, manifest, ZeroApp trait)
├── 02-protocol.md         (Backend ↔ UI IPC protocol)
├── 03-security.md         (Permission model, Init as authority)
├── 04-runtime.md          (AppRuntime, event loop, platforms)
└── 05-factory-apps.md     (Clock and Calculator reference apps)
```

## Quick Reference

### App Layer Model

```
┌───────────────────────────────────────────────────────────────┐
│              Platform-Agnostic Core (crates/zos-apps/)    │
│                                                               │
│  - ZeroApp trait (all apps implement this)                 │
│  - AppManifest (declarative capabilities)                     │
│  - AppRuntime (event loop, syscall dispatch)                  │
│  - UI Protocol (MSG_APP_STATE, MSG_APP_INPUT)                 │
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

### ZeroApp Trait

```rust
pub trait ZeroApp {
    fn manifest() -> &'static AppManifest where Self: Sized;
    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError>;
    fn update(&mut self, ctx: &AppContext) -> ControlFlow;
    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError>;
    fn shutdown(&mut self, ctx: &AppContext);
}
```

### AppManifest

```rust
pub struct AppManifest {
    pub id: &'static str,           // "com.Zero.clock"
    pub name: &'static str,         // "Clock"
    pub version: &'static str,      // "1.0.0"
    pub description: &'static str,
    pub capabilities: &'static [CapabilityRequest],
}
```

### Permission Authority

All permission grants flow through Init (PID 1):

```
Kernel Boot ──► Init (PID 1) ──► App Process
                    │
              grants via
           SYS_CAP_GRANT syscall
           (through Axiom)
```

## Application Lifecycle

```
1. Spawn        ──► Kernel creates process (ProcessCreated commit)
2. Cap Setup    ──► Init grants capabilities (CapabilityInserted commits)
3. Initialize   ──► AppRuntime calls app.init()
4. Run Loop     ──► app.update() + app.on_message() in loop
5. Terminate    ──► Kernel reclaims resources (ProcessTerminated commit)
```

## Factory Apps

Two reference implementations demonstrate the framework:

| App | Purpose | Demonstrates |
|-----|---------|--------------|
| **Clock** | Display time/date | Time syscalls, periodic updates, one-way IPC |
| **Calculator** | Basic arithmetic | Bidirectional IPC, state management, user input |

## Key Concepts

### System Integrity (00-system-integrity.md)

The Axiom-first invariant: ALL state-mutating operations MUST flow through Axiom Gateway. This enables deterministic replay and complete audit trails.

### Architecture (01-architecture.md)

The platform-agnostic app model including process isolation, kernel integration, and the `ZeroApp` trait contract.

### Protocol (02-protocol.md)

Versioned binary protocol for communication between app backends (WASM) and UI surfaces (React). State-based design for loose coupling.

### Security (03-security.md)

Capability-based security with Init (PID 1) as the sole permission authority. All grants recorded in CommitLog.

### Runtime (04-runtime.md)

The `AppRuntime` component that runs inside each WASM process, providing the event loop and syscall interface.

### Factory Apps (05-factory-apps.md)

Complete specifications for Clock and Calculator reference implementations.
