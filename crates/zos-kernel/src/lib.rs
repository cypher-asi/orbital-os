//! Zero OS Kernel Core
//!
//! This crate implements the core kernel functionality:
//! - Process management
//! - Capability-based access control
//! - IPC endpoints and message passing
//! - Syscall dispatch
//!
//! # Module Organization
//!
//! - `types` - Core kernel types (ProcessId, EndpointId, etc.)
//! - `capability` - Capability tokens and permission checking
//! - `ipc` - Inter-process communication types
//! - `syscall` - Syscall definitions and results
//! - `error` - Kernel error types
//! - `dispatch` - Raw syscall dispatch
//! - `core` - KernelCore implementation (internal)
//! - `wrapper` - Kernel wrapper with Axiom integration
//! - `replay` - Deterministic replay support

#![no_std]
extern crate alloc;

// Submodules
pub mod capability;
pub mod dispatch;
pub mod error;
pub mod ipc;
pub mod syscall;
pub mod types;

// Internal modules
mod core;
mod kernel;
mod replay;

// Re-export all public types for backwards compatibility
pub use capability::{axiom_check, AxiomError, Capability, CapabilitySpace, Permissions};
pub use error::KernelError;
pub use ipc::{
    Endpoint, EndpointDetail, EndpointInfo, MAX_CAPS_PER_MESSAGE, MAX_MESSAGE_SIZE, Message,
    MessageSummary, TransferredCap,
};
pub use syscall::{
    CapInfo, RevokeNotification, Syscall, SyscallResult, MSG_CAP_REVOKED, MSG_CONSOLE_INPUT,
    SYS_CALL, SYS_CAP_DELETE, SYS_CAP_DERIVE, SYS_CAP_GRANT, SYS_CAP_INSPECT, SYS_CAP_LIST,
    SYS_CAP_REVOKE, SYS_CONSOLE_WRITE, SYS_CREATE_ENDPOINT, SYS_DEBUG, SYS_DELETE_ENDPOINT,
    SYS_EXIT, SYS_KILL, SYS_PS, SYS_RECV, SYS_REPLY, SYS_SEND, SYS_SEND_CAP, SYS_TIME, SYS_YIELD,
};
pub use types::{
    CapSlot, EndpointId, EndpointMetrics, ObjectType, Process, ProcessId, ProcessMetrics,
    ProcessState, SystemMetrics,
};

// Re-export HAL types
pub use zos_hal::{HalError, HAL as HalTrait};

// Re-export Axiom types
pub use zos_axiom::{
    apply_commit, replay as axiom_replay, replay_and_verify, AxiomGateway, Commit, CommitId,
    CommitLog, CommitType, ReplayError, ReplayResult, Replayable, StateHasher, SysEvent,
    SysEventType, SysLog,
};

// Re-export main types from internal modules
pub use core::KernelCore;
pub use kernel::Kernel;
