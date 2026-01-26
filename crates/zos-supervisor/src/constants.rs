//! Centralized constants for the supervisor crate
//!
//! Slot numbers, syscall numbers, and other magic numbers should be defined here
//! to avoid scatter and enable easy audit.
//!
//! ## Slot Conventions
//!
//! Processes have their endpoints organized by convention:
//! - Slot 0: Primary endpoint (typically init endpoint or process output)
//! - Slot 1: Input endpoint (for receiving IPC messages)
//! - Slot 2: Reserved (process-specific)
//! - Slot 3: VFS endpoint (granted to processes that need VFS access)
//! - Slot 4: VFS response endpoint (dedicated to avoid race conditions)

// =============================================================================
// Capability Slot Conventions
// =============================================================================

/// Init's primary endpoint slot
pub const INIT_ENDPOINT_SLOT: u32 = 0;

/// Service input endpoint slot (for receiving IPC)
///
/// Used by services like PermissionService, Identity, VFS, Terminal, etc.
/// This is where they receive incoming IPC messages.
pub const SERVICE_INPUT_SLOT: u32 = 1;

/// VFS response endpoint slot (dedicated to avoid race conditions)
///
/// VFS responses are routed here instead of the general input endpoint (slot 1)
/// to prevent the VFS client's blocking receive from consuming other IPC messages.
pub const VFS_RESPONSE_SLOT: u32 = 4;

/// Terminal input endpoint slot
pub const TERMINAL_INPUT_SLOT: u32 = SERVICE_INPUT_SLOT;

/// Permission Service input slot  
pub const PS_INPUT_SLOT: u32 = SERVICE_INPUT_SLOT;

/// Identity Service input slot
pub const IDENTITY_INPUT_SLOT: u32 = SERVICE_INPUT_SLOT;

/// VFS Service input slot
pub const VFS_INPUT_SLOT: u32 = SERVICE_INPUT_SLOT;

// =============================================================================
// Syscall Numbers (frequently used in supervisor)
// =============================================================================

/// SYS_DEBUG syscall number - debug message output
pub const SYS_DEBUG: u32 = 0x01;

/// SYS_CONSOLE_WRITE syscall number - console output
pub const SYS_CONSOLE_WRITE: u32 = 0x07;

/// SYS_EXIT syscall number - process exit
pub const SYS_EXIT: u32 = 0x11;

/// SYS_IPC_RECEIVE syscall number - receive IPC message
pub const SYS_IPC_RECEIVE: u32 = 0x41;
