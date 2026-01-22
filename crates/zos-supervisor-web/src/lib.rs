//! Browser-based Supervisor for Zero OS
//!
//! This crate runs in the browser's main thread and acts as the kernel
//! supervisor. It manages Web Workers (processes) and routes IPC messages.
//!
//! ## Module Structure
//!
//! - `hal` - WASM HAL implementation using Web Workers
//! - `supervisor` - Kernel supervisor and process management (pure boundary layer)
//! - `syscall` - Syscall dispatch and handling
//! - `pingpong` - Automated IPC latency testing
//! - `background` - WebGPU background renderer wrapper
//! - `axiom` - IndexedDB persistence helpers
//! - `worker` - Web Worker process types
//!
//! ## Architecture
//!
//! The supervisor is a pure boundary layer that only:
//! - Routes IPC messages between processes
//! - Handles process spawning/killing at the Web Worker level
//! - Dispatches syscalls to the kernel
//!
//! All application logic (including terminal commands) runs in userspace WASM processes.
//! See `zos-apps` for the canonical ZeroApp implementations.

// =============================================================================
// Module declarations
// =============================================================================

pub(crate) mod axiom;
mod background;
pub(crate) mod hal;
pub(crate) mod pingpong;
pub(crate) mod supervisor;
pub(crate) mod syscall;
pub mod vfs;
pub(crate) mod worker;

// =============================================================================
// Public re-exports
// =============================================================================

// Re-export the Supervisor (main public API)
pub use supervisor::Supervisor;

// Re-export the DesktopBackground wrapper
pub use background::DesktopBackground;

// Re-export worker types needed by external code
pub use worker::{PendingSyscall, WasmProcessHandle, WorkerMessage, WorkerMessageType};

// Re-export background renderer from zos-desktop for convenience
pub use zos_desktop::background as desktop_background;
