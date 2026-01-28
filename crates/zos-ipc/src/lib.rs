//! IPC Protocol & Syscall Constants for Zero OS
//!
//! This crate defines:
//! - **Syscall numbers** (Process → Kernel operations)
//! - **IPC message tags** (Process ↔ Process communication)
//!
//! It is the **single source of truth** for all protocol constants,
//! eliminating duplication across crates.
//!
//! # Syscall Number Ranges
//!
//! | Range | Category |
//! |-------|----------|
//! | 0x01-0x0F | Misc (debug, time, info) |
//! | 0x10-0x1F | Process (create, exit, kill) |
//! | 0x30-0x3F | Capability (grant, revoke, inspect) |
//! | 0x40-0x4F | IPC (send, receive, call, reply) |
//! | 0x50-0x5F | System (list processes) |
//! | 0x70-0x7F | Platform Storage (async ops) |
//! | 0x80-0x8F | Keystore (async key storage) |
//! | 0x90-0x9F | Network (async HTTP) |
//!
//! # IPC Message Range Allocation
//!
//! | Range         | Service                              |
//! |---------------|--------------------------------------|
//! | 0x0001-0x000F | Console / System                     |
//! | 0x0080        | Storage result (async IPC)           |
//! | 0x1000-0x100F | Init service protocol                |
//! | 0x1010-0x101F | Permission protocol (legacy)         |
//! | 0x2000-0x200F | App protocol (state, input, etc.)    |
//! | 0x2001-0x200F | Supervisor → Init protocol           |
//! | 0x2010-0x201F | PermissionService protocol           |
//! | 0x2020        | Supervisor → PermissionService       |
//! | 0x3000-0x30FF | Kernel notifications                 |
//! | 0x4000-0x4FFF | System diagnostics (memhog, etc.)    |
//! | 0x5000-0x50FF | Identity permission checks           |
//! | 0x7000-0x70FF | Identity service                     |
//! | 0x8000-0x80FF | VFS service                          |
//! | 0x8100-0x810F | Time service                         |
//! | 0x9000-0x901F | Network service                      |
//! | 0xA000-0xA0FF | Keystore service                     |
//!
//! # Usage
//!
//! ```rust
//! use zos_ipc::{init, supervisor, pm};
//!
//! // Service registration
//! let tag = init::MSG_REGISTER_SERVICE;
//!
//! // Supervisor kill request
//! let tag = supervisor::MSG_SUPERVISOR_KILL_PROCESS;
//!
//! // Permission manager request
//! let tag = pm::MSG_REQUEST_CAPABILITY;
//! ```

#![no_std]

// =============================================================================
// Object Types (Canonical definition for capabilities)
// =============================================================================

/// Types of kernel objects that can be accessed via capabilities.
///
/// **CRITICAL**: This is the single source of truth for object type values.
/// All crates MUST use this definition to ensure consistent capability grants.
///
/// # Safety
///
/// Using mismatched object type values between crates can cause:
/// - Granting wrong capabilities (e.g., Storage instead of Console)
/// - Security policy bypass
/// - Undefined behavior in capability checks
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ObjectType {
    /// IPC endpoint - for process-to-process communication
    Endpoint = 1,
    /// Another process - for spawn/kill operations
    Process = 2,
    /// Memory region - for shared memory
    Memory = 3,
    /// IRQ handler - for interrupt handling (reserved for drivers)
    Irq = 4,
    /// I/O port range - for hardware access (reserved for drivers)
    IoPort = 5,
    /// Console/debug output - for terminal I/O
    Console = 6,
    /// Persistent storage - namespaced per-app key-value store
    Storage = 7,
    /// Network access - for HTTP/WebSocket operations
    Network = 8,
    /// Filesystem access - for VFS operations
    Filesystem = 9,
    /// User identity service - for authentication
    Identity = 10,
    /// Cryptographic keystore - for secure key storage
    Keystore = 11,
}

impl ObjectType {
    /// Convert from u8 value.
    ///
    /// Returns `None` for invalid/unknown values.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(ObjectType::Endpoint),
            2 => Some(ObjectType::Process),
            3 => Some(ObjectType::Memory),
            4 => Some(ObjectType::Irq),
            5 => Some(ObjectType::IoPort),
            6 => Some(ObjectType::Console),
            7 => Some(ObjectType::Storage),
            8 => Some(ObjectType::Network),
            9 => Some(ObjectType::Filesystem),
            10 => Some(ObjectType::Identity),
            11 => Some(ObjectType::Keystore),
            _ => None,
        }
    }

    /// Get human-readable display name.
    pub fn name(&self) -> &'static str {
        match self {
            ObjectType::Endpoint => "Endpoint",
            ObjectType::Process => "Process",
            ObjectType::Memory => "Memory",
            ObjectType::Irq => "IRQ",
            ObjectType::IoPort => "I/O Port",
            ObjectType::Console => "Console",
            ObjectType::Storage => "Storage",
            ObjectType::Network => "Network",
            ObjectType::Filesystem => "Filesystem",
            ObjectType::Identity => "Identity",
            ObjectType::Keystore => "Keystore",
        }
    }
}

// =============================================================================
// Syscall Numbers (Process → Kernel operations)
// =============================================================================

/// Syscall numbers - these are used by processes to invoke kernel operations.
pub mod syscall {
    // === Misc (0x01 - 0x0F) ===
    /// Debug print syscall
    pub const SYS_DEBUG: u32 = 0x01;
    /// Yield/cooperative scheduling hint
    pub const SYS_YIELD: u32 = 0x02;
    /// Exit process
    pub const SYS_EXIT: u32 = 0x03;
    /// Get current time (nanos since boot)
    pub const SYS_TIME: u32 = 0x04;
    /// Get wallclock time (milliseconds since Unix epoch)
    pub const SYS_WALLCLOCK: u32 = 0x06;
    /// Console write syscall - write text to console output
    pub const SYS_CONSOLE_WRITE: u32 = 0x07;

    // === Process (0x10 - 0x1F) ===
    /// Create an IPC endpoint
    pub const SYS_CREATE_ENDPOINT: u32 = 0x11;
    /// Delete an endpoint
    pub const SYS_DELETE_ENDPOINT: u32 = 0x12;
    /// Kill a process (requires Process capability with kill permission)
    pub const SYS_KILL: u32 = 0x13;
    /// Register a new process (Init-only syscall for spawn protocol)
    pub const SYS_REGISTER_PROCESS: u32 = 0x14;
    /// Create an endpoint for another process (Init-only syscall for spawn protocol)
    pub const SYS_CREATE_ENDPOINT_FOR: u32 = 0x15;
    /// Load a binary by name from platform storage (Init-only).
    /// - QEMU: Returns embedded binary from HAL
    /// - WASM: Returns NotSupported (use Supervisor async flow)
    /// Payload: [name: UTF-8 bytes]
    /// Returns: Binary data in syscall result buffer, or error code
    pub const SYS_LOAD_BINARY: u32 = 0x16;
    /// Spawn a process from binary data (Init-only).
    /// Payload: [name_len: u32 (LE), name: [u8], binary: [u8]]
    /// Returns: PID on success (>0), negative error code on failure
    pub const SYS_SPAWN_PROCESS: u32 = 0x17;

    // === Capability (0x30 - 0x3F) ===
    /// Grant a capability to another process
    pub const SYS_CAP_GRANT: u32 = 0x30;
    /// Revoke a capability (requires grant permission)
    pub const SYS_CAP_REVOKE: u32 = 0x31;
    /// Delete a capability from own CSpace
    pub const SYS_CAP_DELETE: u32 = 0x32;
    /// Inspect a capability (get info)
    pub const SYS_CAP_INSPECT: u32 = 0x33;
    /// Derive a new capability with reduced permissions
    pub const SYS_CAP_DERIVE: u32 = 0x34;
    /// List all capabilities
    pub const SYS_CAP_LIST: u32 = 0x35;

    // === IPC (0x40 - 0x4F) ===
    /// Send a message
    pub const SYS_SEND: u32 = 0x40;
    /// Receive a message
    pub const SYS_RECV: u32 = 0x41;
    /// Call (send + wait for reply)
    pub const SYS_CALL: u32 = 0x42;
    /// Reply to a call
    pub const SYS_REPLY: u32 = 0x43;
    /// Send with capability transfer
    pub const SYS_SEND_CAP: u32 = 0x44;

    // === System (0x50 - 0x5F) ===
    /// List all processes (supervisor only)
    pub const SYS_PS: u32 = 0x50;

    // === Platform Storage (0x70 - 0x7F) ===
    // HAL-level key-value storage operations. VfsService uses these for persistence.
    // Applications should use zos_vfs::VfsClient. All storage syscalls are ASYNC.
    /// Read blob from platform storage (async - returns request_id)
    pub const SYS_STORAGE_READ: u32 = 0x70;
    /// Write blob to platform storage (async - returns request_id)
    pub const SYS_STORAGE_WRITE: u32 = 0x71;
    /// Delete blob from platform storage (async - returns request_id)
    pub const SYS_STORAGE_DELETE: u32 = 0x72;
    /// List keys with prefix (async - returns request_id)
    pub const SYS_STORAGE_LIST: u32 = 0x73;
    /// Check if key exists (async - returns request_id)
    pub const SYS_STORAGE_EXISTS: u32 = 0x74;

    // === Keystore (0x80 - 0x8F) ===
    // HAL-level key storage operations. VfsService uses these for /keys/ paths.
    // Applications should use VFS IPC with /keys/ paths. All keystore syscalls are ASYNC.
    /// Read key from keystore (async - returns request_id)
    pub const SYS_KEYSTORE_READ: u32 = 0x80;
    /// Write key to keystore (async - returns request_id)
    pub const SYS_KEYSTORE_WRITE: u32 = 0x81;
    /// Delete key from keystore (async - returns request_id)
    pub const SYS_KEYSTORE_DELETE: u32 = 0x82;
    /// List keys with prefix (async - returns request_id)
    pub const SYS_KEYSTORE_LIST: u32 = 0x83;
    /// Check if key exists (async - returns request_id)
    pub const SYS_KEYSTORE_EXISTS: u32 = 0x84;

    // === Network (0x90 - 0x9F) ===
    // HAL-level HTTP fetch operations. Applications use Network Service via IPC.
    // Network syscalls are ASYNC and return a request_id immediately.
    /// Start async HTTP fetch (returns request_id)
    pub const SYS_NETWORK_FETCH: u32 = 0x90;
}

// Re-export syscall constants at crate root for convenience
pub use syscall::*;

// =============================================================================
// Console Messages (0x0001 - 0x000F)
// =============================================================================

/// Console IPC messages.
pub mod console {
    /// Console input message tag - used by terminal for receiving keyboard input.
    /// Payload: raw input bytes
    pub const MSG_CONSOLE_INPUT: u32 = 0x0002;
}

// =============================================================================
// App Protocol (0x2000 - 0x200F)
// =============================================================================

/// App protocol messages for Backend ↔ UI communication.
///
/// These messages are used by app backends (WASM) to communicate with
/// their UI surfaces (React components).
pub mod app {
    /// App → UI: State update.
    /// The payload contains a versioned envelope with app-specific state data.
    pub const MSG_APP_STATE: u32 = 0x2000;

    /// UI → App: User input event.
    /// The payload contains user input (button presses, text input, etc).
    pub const MSG_APP_INPUT: u32 = 0x2001;

    /// UI → App: UI surface ready notification.
    /// Sent when the React component has mounted and is ready to receive state.
    pub const MSG_UI_READY: u32 = 0x2002;

    /// App → UI: Request focus.
    /// The app requests to be brought to the foreground.
    pub const MSG_APP_FOCUS: u32 = 0x2003;

    /// App → UI: Error notification.
    /// The app reports an error to the UI for display.
    pub const MSG_APP_ERROR: u32 = 0x2004;
}

// Re-export console constants at crate root for convenience
pub use console::MSG_CONSOLE_INPUT;

// =============================================================================
// Storage Result (0x0080)
// =============================================================================

/// Storage IPC messages (async platform storage results).
pub mod storage {
    /// Storage operation result delivered via IPC.
    /// Payload format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
    pub const MSG_STORAGE_RESULT: u32 = 0x80;

    /// Storage result types
    pub mod result {
        /// Read succeeded, data follows
        pub const READ_OK: u8 = 0;
        /// Write/delete succeeded
        pub const WRITE_OK: u8 = 1;
        /// Key not found
        pub const NOT_FOUND: u8 = 2;
        /// Operation failed
        pub const ERROR: u8 = 3;
        /// List succeeded, key list follows (JSON array)
        pub const LIST_OK: u8 = 4;
        /// Exists check result: 1 = exists, 0 = not exists
        pub const EXISTS_OK: u8 = 5;
    }
}

/// Keystore IPC messages (async key storage results).
///
/// Note: Keystore uses the same message tag (0x80) as storage but results are
/// distinguished by the requesting PID's pending_keystore_requests vs pending_storage_requests.
pub mod keystore {
    /// Keystore operation result delivered via IPC.
    /// Payload format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
    /// Uses same format as MSG_STORAGE_RESULT for consistency.
    pub const MSG_KEYSTORE_RESULT: u32 = 0x81;

    /// Keystore result types (same as storage for consistency)
    pub mod result {
        /// Read succeeded, data follows
        pub const READ_OK: u8 = 0;
        /// Write/delete succeeded
        pub const WRITE_OK: u8 = 1;
        /// Key not found
        pub const NOT_FOUND: u8 = 2;
        /// Operation failed
        pub const ERROR: u8 = 3;
        /// List succeeded, key list follows (JSON array)
        pub const LIST_OK: u8 = 4;
        /// Exists check result: 1 = exists, 0 = not exists
        pub const EXISTS_OK: u8 = 5;
    }
}

// =============================================================================
// Init Service Protocol (0x1000 - 0x100F)
// =============================================================================

/// Init service protocol messages.
///
/// These are used for service registration and discovery.
pub mod init {
    /// Register a service with init.
    /// Payload: [name_len: u8, name: [u8], endpoint_id_low: u32, endpoint_id_high: u32]
    pub const MSG_REGISTER_SERVICE: u32 = 0x1000;

    /// Lookup a service by name.
    /// Payload: [name_len: u8, name: [u8]]
    pub const MSG_LOOKUP_SERVICE: u32 = 0x1001;

    /// Lookup response.
    /// Payload: [found: u8, endpoint_id_low: u32, endpoint_id_high: u32]
    pub const MSG_LOOKUP_RESPONSE: u32 = 0x1002;

    /// Request spawn.
    /// Payload: [name_len: u8, name: [u8]]
    pub const MSG_SPAWN_SERVICE: u32 = 0x1003;

    /// Spawn response.
    /// Payload: [success: u8, pid: u32]
    pub const MSG_SPAWN_RESPONSE: u32 = 0x1004;

    /// Service ready notification (service → init after registration complete).
    pub const MSG_SERVICE_READY: u32 = 0x1005;

    /// Service capability granted notification (supervisor → init).
    /// Payload: [service_pid: u32, cap_slot: u32]
    pub const MSG_SERVICE_CAP_GRANTED: u32 = 0x1006;

    /// VFS response endpoint capability granted notification (supervisor → init).
    /// Payload: [service_pid: u32, cap_slot: u32]
    pub const MSG_VFS_RESPONSE_CAP_GRANTED: u32 = 0x1007;
}

// =============================================================================
// Permission Protocol (0x1010 - 0x101F) - Legacy Init-based permissions
// =============================================================================

/// Permission protocol messages (Desktop/Supervisor -> Init).
///
/// These are legacy permission messages routed through Init.
/// Prefer using the PermissionService IPC protocol (`pm` module) instead.
pub mod permission {
    /// Request Init to grant a capability to a process.
    pub const MSG_GRANT_PERMISSION: u32 = 0x1010;

    /// Request Init to revoke a capability from a process.
    pub const MSG_REVOKE_PERMISSION: u32 = 0x1011;

    /// Query what permissions a process has.
    pub const MSG_LIST_PERMISSIONS: u32 = 0x1012;

    /// Response from Init with grant/revoke result.
    pub const MSG_PERMISSION_RESPONSE: u32 = 0x1013;
}

// =============================================================================
// Supervisor → Init Protocol (0x2001 - 0x200F)
// =============================================================================

/// Supervisor → Init protocol messages.
///
/// These messages are sent from the supervisor to Init to route operations
/// that need kernel access. The supervisor has no direct kernel access;
/// it must send IPC to Init which then invokes syscalls on its behalf.
pub mod supervisor {
    /// Supervisor requests Init to deliver console input to a terminal process.
    /// Payload: [target_pid: u32, endpoint_slot: u32, data_len: u16, data: [u8]]
    pub const MSG_SUPERVISOR_CONSOLE_INPUT: u32 = 0x2001;

    /// Supervisor requests Init to terminate a process.
    /// Payload: [target_pid: u32]
    pub const MSG_SUPERVISOR_KILL_PROCESS: u32 = 0x2002;

    /// Supervisor requests Init to route an IPC message to a process.
    /// Payload: [target_pid: u32, endpoint_slot: u32, tag: u32, data_len: u16, data: [u8]]
    pub const MSG_SUPERVISOR_IPC_DELIVERY: u32 = 0x2003;

    // =========================================================================
    // Init-Driven Spawn Protocol (0x2004 - 0x200F)
    // =========================================================================
    // These messages implement the Init-driven spawn protocol where ALL process
    // lifecycle operations flow through Init. This ensures:
    // - All operations are logged via SysLog (Invariant 9)
    // - Supervisor has no direct kernel access (Invariant 16)
    // - Init is the capability authority for process creation

    /// Supervisor requests Init to register a new process in kernel.
    /// This is the first step of the Init-driven spawn protocol.
    /// Payload: [name_len: u8, name: [u8]]
    /// Init responds with MSG_SUPERVISOR_SPAWN_RESPONSE.
    pub const MSG_SUPERVISOR_SPAWN_PROCESS: u32 = 0x2004;

    /// Init response with registered PID.
    /// Payload: [success: u8, pid: u32]
    /// success=1: pid contains the new PID
    /// success=0: spawn failed
    pub const MSG_SUPERVISOR_SPAWN_RESPONSE: u32 = 0x2005;

    /// Supervisor requests Init to create endpoint for a process.
    /// This is called after MSG_SUPERVISOR_SPAWN_RESPONSE to set up IPC.
    /// Payload: [target_pid: u32]
    /// Init responds with MSG_SUPERVISOR_ENDPOINT_RESPONSE.
    pub const MSG_SUPERVISOR_CREATE_ENDPOINT: u32 = 0x2006;

    /// Init response with created endpoint info.
    /// Payload: [success: u8, endpoint_id: u64, slot: u32]
    pub const MSG_SUPERVISOR_ENDPOINT_RESPONSE: u32 = 0x2007;

    /// Supervisor requests Init to grant capability.
    /// This enables setting up capabilities during spawn.
    /// Payload: [from_pid: u32, from_slot: u32, to_pid: u32, perms: u8]
    /// Init responds with MSG_SUPERVISOR_CAP_RESPONSE.
    pub const MSG_SUPERVISOR_GRANT_CAP: u32 = 0x2008;

    /// Init response with capability grant result.
    /// Payload: [success: u8, new_slot: u32]
    pub const MSG_SUPERVISOR_CAP_RESPONSE: u32 = 0x2009;

    /// Supervisor requests PermissionService to revoke a capability from a process.
    /// Payload: [target_pid: u32, slot: u32, reason: u8]
    ///
    /// **IMPORTANT**: This is the canonical value (0x2020). The supervisor had
    /// a bug using 0x2010 which conflicts with MSG_REQUEST_CAPABILITY.
    pub const MSG_SUPERVISOR_REVOKE_CAP: u32 = 0x2020;
}

// =============================================================================
// PermissionService Protocol (0x2010 - 0x201F)
// =============================================================================

/// PermissionService protocol messages.
///
/// These messages are used by processes to request capabilities from
/// the PermissionService (PID 2).
pub mod pm {
    /// Request a capability from PermissionService.
    /// Payload: [object_type: u8, object_id: u64, requested_perms: u8]
    pub const MSG_REQUEST_CAPABILITY: u32 = 0x2010;

    /// Request to revoke a capability (self or with grant permission).
    /// Payload: [slot: u32]
    pub const MSG_REVOKE_CAPABILITY: u32 = 0x2011;

    /// List capabilities in own CSpace.
    /// Payload: (empty)
    pub const MSG_LIST_MY_CAPS: u32 = 0x2012;

    /// Capability request response.
    /// Payload: [success: u8, slot: u32] or [success: u8, error_code: u32]
    pub const MSG_CAPABILITY_RESPONSE: u32 = 0x2013;

    /// Capability list response.
    /// Payload: [count: u32, (slot: u32, type: u8, object_id: u64, perms: u8)*]
    pub const MSG_CAPS_LIST_RESPONSE: u32 = 0x2014;
}

// =============================================================================
// Kernel Notifications (0x3000 - 0x30FF)
// =============================================================================

/// Kernel notification messages.
pub mod kernel {
    /// Notification that a capability was revoked from this process.
    /// Payload: [slot: u32, object_type: u8, object_id: u64, reason: u8]
    pub const MSG_CAP_REVOKED: u32 = 0x3010;
}

/// Capability revocation reasons.
pub mod revoke_reason {
    /// Supervisor/user explicitly revoked the capability.
    pub const EXPLICIT: u8 = 1;
    /// Capability expired.
    pub const EXPIRED: u8 = 2;
    /// Source process exited.
    pub const PROCESS_EXIT: u8 = 3;
}

// =============================================================================
// System Diagnostics (0x4000 - 0x4FFF)
// =============================================================================

/// System diagnostic messages (for test processes like memhog).
pub mod diagnostics {
    /// Memory status report.
    pub const MSG_MEMORY_STATUS: u32 = 0x4001;
    /// Sender stats.
    pub const MSG_SENDER_STATS: u32 = 0x4002;
    /// Receiver stats.
    pub const MSG_RECEIVER_STATS: u32 = 0x4003;
    /// Latency stats.
    pub const MSG_LATENCY_STATS: u32 = 0x4004;

    /// Ping message (for pingpong test).
    pub const MSG_PING: u32 = 0x5001;
    /// Pong message (for pingpong test).
    pub const MSG_PONG: u32 = 0x5002;
    /// Data message (for sender/receiver test).
    pub const MSG_DATA: u32 = 0x5003;
}

// =============================================================================
// Identity Permission Service (0x5000 - 0x50FF)
// =============================================================================

/// Identity permission service messages.
///
/// Note: These are different from the PermissionService (pm) messages.
/// This is for checking user-level permissions in the identity layer.
pub mod identity_perm {
    /// Check permission request.
    pub const MSG_CHECK_PERM: u32 = 0x5000;
    /// Check permission response.
    pub const MSG_CHECK_PERM_RESPONSE: u32 = 0x5001;

    /// Query capabilities request.
    pub const MSG_QUERY_CAPS: u32 = 0x5002;
    /// Query capabilities response.
    pub const MSG_QUERY_CAPS_RESPONSE: u32 = 0x5003;

    /// Query history request.
    pub const MSG_QUERY_HISTORY: u32 = 0x5004;
    /// Query history response.
    pub const MSG_QUERY_HISTORY_RESPONSE: u32 = 0x5005;

    /// Get provenance request.
    pub const MSG_GET_PROVENANCE: u32 = 0x5006;
    /// Get provenance response.
    pub const MSG_GET_PROVENANCE_RESPONSE: u32 = 0x5007;

    /// Update policy request (admin only).
    pub const MSG_UPDATE_POLICY: u32 = 0x5008;
    /// Update policy response.
    pub const MSG_UPDATE_POLICY_RESPONSE: u32 = 0x5009;
}

// =============================================================================
// Identity Service (0x7000 - 0x70FF)
// =============================================================================

/// Identity service messages - User Management (0x7000-0x700F).
pub mod identity_user {
    /// Create user request.
    pub const MSG_CREATE_USER: u32 = 0x7000;
    /// Create user response.
    pub const MSG_CREATE_USER_RESPONSE: u32 = 0x7001;
    /// Get user request.
    pub const MSG_GET_USER: u32 = 0x7002;
    /// Get user response.
    pub const MSG_GET_USER_RESPONSE: u32 = 0x7003;
    /// List users request.
    pub const MSG_LIST_USERS: u32 = 0x7004;
    /// List users response.
    pub const MSG_LIST_USERS_RESPONSE: u32 = 0x7005;
    /// Delete user request.
    pub const MSG_DELETE_USER: u32 = 0x7006;
    /// Delete user response.
    pub const MSG_DELETE_USER_RESPONSE: u32 = 0x7007;
}

/// Identity service messages - Session Management (0x7010-0x701F).
pub mod identity_session {
    /// Login challenge request.
    pub const MSG_LOGIN_CHALLENGE: u32 = 0x7010;
    /// Login challenge response.
    pub const MSG_LOGIN_CHALLENGE_RESPONSE: u32 = 0x7011;
    /// Login verify request.
    pub const MSG_LOGIN_VERIFY: u32 = 0x7012;
    /// Login verify response.
    pub const MSG_LOGIN_VERIFY_RESPONSE: u32 = 0x7013;
    /// Logout request.
    pub const MSG_LOGOUT: u32 = 0x7014;
    /// Logout response.
    pub const MSG_LOGOUT_RESPONSE: u32 = 0x7015;
}

/// Identity service messages - Remote Authentication (0x7020-0x702F).
pub mod identity_remote {
    /// Remote auth request.
    pub const MSG_REMOTE_AUTH: u32 = 0x7020;
    /// Remote auth response.
    pub const MSG_REMOTE_AUTH_RESPONSE: u32 = 0x7021;
}

/// Identity service messages - Process Queries (0x7030-0x703F).
pub mod identity_query {
    /// Whoami request.
    pub const MSG_WHOAMI: u32 = 0x7030;
    /// Whoami response.
    pub const MSG_WHOAMI_RESPONSE: u32 = 0x7031;
}

/// Identity service messages - Credentials (0x7040-0x704F).
pub mod identity_cred {
    /// Attach email request.
    pub const MSG_ATTACH_EMAIL: u32 = 0x7040;
    /// Attach email response.
    pub const MSG_ATTACH_EMAIL_RESPONSE: u32 = 0x7041;
    /// Get credentials request.
    pub const MSG_GET_CREDENTIALS: u32 = 0x7042;
    /// Get credentials response.
    pub const MSG_GET_CREDENTIALS_RESPONSE: u32 = 0x7043;
    /// Unlink credential request.
    pub const MSG_UNLINK_CREDENTIAL: u32 = 0x7046;
    /// Unlink credential response.
    pub const MSG_UNLINK_CREDENTIAL_RESPONSE: u32 = 0x7047;
}

/// Identity service messages - Identity Keys (0x7050-0x705F).
pub mod identity_key {
    /// Register identity key request.
    pub const MSG_REGISTER_IDENTITY_KEY: u32 = 0x7050;
    /// Register identity key response.
    pub const MSG_REGISTER_IDENTITY_KEY_RESPONSE: u32 = 0x7051;
    /// Get identity key request.
    pub const MSG_GET_IDENTITY_KEY: u32 = 0x7052;
    /// Get identity key response.
    pub const MSG_GET_IDENTITY_KEY_RESPONSE: u32 = 0x7053;
    /// Generate Neural Key request.
    pub const MSG_GENERATE_NEURAL_KEY: u32 = 0x7054;
    /// Generate Neural Key response.
    pub const MSG_GENERATE_NEURAL_KEY_RESPONSE: u32 = 0x7055;
    /// Recover Neural Key from shards request.
    pub const MSG_RECOVER_NEURAL_KEY: u32 = 0x7056;
    /// Recover Neural Key from shards response.
    pub const MSG_RECOVER_NEURAL_KEY_RESPONSE: u32 = 0x7057;
}

/// Identity service messages - Machine Keys (0x7060-0x706F).
pub mod identity_machine {
    /// Create machine key request.
    pub const MSG_CREATE_MACHINE_KEY: u32 = 0x7060;
    /// Create machine key response.
    pub const MSG_CREATE_MACHINE_KEY_RESPONSE: u32 = 0x7061;
    /// List machine keys request.
    pub const MSG_LIST_MACHINE_KEYS: u32 = 0x7062;
    /// List machine keys response.
    pub const MSG_LIST_MACHINE_KEYS_RESPONSE: u32 = 0x7063;
    /// Get machine key request.
    pub const MSG_GET_MACHINE_KEY: u32 = 0x7064;
    /// Get machine key response.
    pub const MSG_GET_MACHINE_KEY_RESPONSE: u32 = 0x7065;
    /// Revoke machine key request.
    pub const MSG_REVOKE_MACHINE_KEY: u32 = 0x7066;
    /// Revoke machine key response.
    pub const MSG_REVOKE_MACHINE_KEY_RESPONSE: u32 = 0x7067;
    /// Rotate machine key request.
    pub const MSG_ROTATE_MACHINE_KEY: u32 = 0x7068;
    /// Rotate machine key response.
    pub const MSG_ROTATE_MACHINE_KEY_RESPONSE: u32 = 0x7069;
}

/// Identity service messages - ZID Auth (0x7080-0x708F).
///
/// These messages handle authentication with the ZERO-ID remote server
/// using machine key challenge-response flow.
pub mod identity_zid {
    /// ZID login request (machine key challenge-response).
    /// Payload: JSON-serialized ZidLoginRequest
    pub const MSG_ZID_LOGIN: u32 = 0x7080;
    /// ZID login response.
    /// Payload: JSON-serialized ZidLoginResponse
    pub const MSG_ZID_LOGIN_RESPONSE: u32 = 0x7081;
    /// ZID token refresh request.
    /// Payload: JSON-serialized ZidRefreshRequest
    pub const MSG_ZID_REFRESH: u32 = 0x7082;
    /// ZID token refresh response.
    /// Payload: JSON-serialized ZidRefreshResponse
    pub const MSG_ZID_REFRESH_RESPONSE: u32 = 0x7083;
    /// ZID enroll machine request (register with ZID server).
    /// Payload: JSON-serialized ZidEnrollMachineRequest
    pub const MSG_ZID_ENROLL_MACHINE: u32 = 0x7084;
    /// ZID enroll machine response.
    /// Payload: JSON-serialized ZidEnrollMachineResponse
    pub const MSG_ZID_ENROLL_MACHINE_RESPONSE: u32 = 0x7085;
}

/// Identity service messages - Identity Preferences (0x7090-0x7099).
///
/// These messages handle identity preferences stored in VFS
/// such as default key scheme for new machine keys.
pub mod identity_prefs {
    /// Get identity preferences request.
    /// Payload: JSON-serialized GetIdentityPreferencesRequest
    pub const MSG_GET_IDENTITY_PREFERENCES: u32 = 0x7090;
    /// Get identity preferences response.
    /// Payload: JSON-serialized GetIdentityPreferencesResponse
    pub const MSG_GET_IDENTITY_PREFERENCES_RESPONSE: u32 = 0x7091;
    /// Set default key scheme request.
    /// Payload: JSON-serialized SetDefaultKeySchemeRequest
    pub const MSG_SET_DEFAULT_KEY_SCHEME: u32 = 0x7092;
    /// Set default key scheme response.
    /// Payload: JSON-serialized SetDefaultKeySchemeResponse
    pub const MSG_SET_DEFAULT_KEY_SCHEME_RESPONSE: u32 = 0x7093;
}

// =============================================================================
// VFS Service (0x8000 - 0x80FF)
// =============================================================================

/// VFS service messages - Directory Operations (0x8000-0x800F).
pub mod vfs_dir {
    /// Create directory request.
    pub const MSG_VFS_MKDIR: u32 = 0x8000;
    /// Create directory response.
    pub const MSG_VFS_MKDIR_RESPONSE: u32 = 0x8001;
    /// Remove directory request.
    pub const MSG_VFS_RMDIR: u32 = 0x8002;
    /// Remove directory response.
    pub const MSG_VFS_RMDIR_RESPONSE: u32 = 0x8003;
    /// Read directory request.
    pub const MSG_VFS_READDIR: u32 = 0x8004;
    /// Read directory response.
    pub const MSG_VFS_READDIR_RESPONSE: u32 = 0x8005;
}

/// VFS service messages - File Operations (0x8010-0x801F).
pub mod vfs_file {
    /// Write file request.
    pub const MSG_VFS_WRITE: u32 = 0x8010;
    /// Write file response.
    pub const MSG_VFS_WRITE_RESPONSE: u32 = 0x8011;
    /// Read file request.
    pub const MSG_VFS_READ: u32 = 0x8012;
    /// Read file response.
    pub const MSG_VFS_READ_RESPONSE: u32 = 0x8013;
    /// Delete file request.
    pub const MSG_VFS_UNLINK: u32 = 0x8014;
    /// Delete file response.
    pub const MSG_VFS_UNLINK_RESPONSE: u32 = 0x8015;
    /// Rename file request.
    pub const MSG_VFS_RENAME: u32 = 0x8016;
    /// Rename file response.
    pub const MSG_VFS_RENAME_RESPONSE: u32 = 0x8017;
    /// Copy file request.
    pub const MSG_VFS_COPY: u32 = 0x8018;
    /// Copy file response.
    pub const MSG_VFS_COPY_RESPONSE: u32 = 0x8019;
}

/// VFS service messages - Metadata Operations (0x8020-0x802F).
pub mod vfs_meta {
    /// Stat request.
    pub const MSG_VFS_STAT: u32 = 0x8020;
    /// Stat response.
    pub const MSG_VFS_STAT_RESPONSE: u32 = 0x8021;
    /// Exists request.
    pub const MSG_VFS_EXISTS: u32 = 0x8022;
    /// Exists response.
    pub const MSG_VFS_EXISTS_RESPONSE: u32 = 0x8023;
    /// Change permissions request.
    pub const MSG_VFS_CHMOD: u32 = 0x8024;
    /// Change permissions response.
    pub const MSG_VFS_CHMOD_RESPONSE: u32 = 0x8025;
    /// Change owner request.
    pub const MSG_VFS_CHOWN: u32 = 0x8026;
    /// Change owner response.
    pub const MSG_VFS_CHOWN_RESPONSE: u32 = 0x8027;
}

/// VFS service messages - Quota Operations (0x8030-0x803F).
pub mod vfs_quota {
    /// Get usage request.
    pub const MSG_VFS_GET_USAGE: u32 = 0x8030;
    /// Get usage response.
    pub const MSG_VFS_GET_USAGE_RESPONSE: u32 = 0x8031;
    /// Get quota request.
    pub const MSG_VFS_GET_QUOTA: u32 = 0x8032;
    /// Get quota response.
    pub const MSG_VFS_GET_QUOTA_RESPONSE: u32 = 0x8033;
}

// =============================================================================
// Time Service (0x8100 - 0x810F)
// =============================================================================

/// Time service messages (0x8100-0x810F).
///
/// The Time Service manages time-related settings like time format (12h/24h)
/// and timezone preferences. Settings are persisted to VFS.
pub mod time {
    /// Request current time settings.
    /// Payload: (empty)
    pub const MSG_GET_TIME_SETTINGS: u32 = 0x8100;
    /// Response with time settings.
    /// Payload: JSON {"time_format_24h": bool, "timezone": string}
    pub const MSG_GET_TIME_SETTINGS_RESPONSE: u32 = 0x8101;
    /// Set time settings.
    /// Payload: JSON {"time_format_24h": bool, "timezone": string}
    pub const MSG_SET_TIME_SETTINGS: u32 = 0x8102;
    /// Response confirming settings update.
    /// Payload: JSON {"time_format_24h": bool, "timezone": string} or {"error": string}
    pub const MSG_SET_TIME_SETTINGS_RESPONSE: u32 = 0x8103;
}

// =============================================================================
// Network Service (0x9000 - 0x901F)
// =============================================================================

/// Network service messages (0x9000-0x901F).
///
/// The Network Service mediates HTTP requests from other processes,
/// enforcing network access policies and providing a unified network API.
pub mod net {
    /// HTTP request to network service.
    /// Payload: JSON-serialized HttpRequest
    pub const MSG_NET_REQUEST: u32 = 0x9000;
    /// HTTP response from network service.
    /// Payload: JSON-serialized HttpResponse
    pub const MSG_NET_RESPONSE: u32 = 0x9001;
    /// Network result delivered via IPC (async callback).
    /// Payload format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
    pub const MSG_NET_RESULT: u32 = 0x9002;
}

// =============================================================================
// Keystore Service (0xA000 - 0xA0FF)
// =============================================================================

/// Keystore service IPC messages (0xA000-0xA0FF).
///
/// The Keystore Service provides secure storage for cryptographic keys,
/// isolated from general filesystem storage. It uses the zos-keystore
/// IndexedDB database via keystore syscalls.
///
/// This service is used by Identity Service for key material storage,
/// keeping sensitive cryptographic data separate from user files.
pub mod keystore_svc {
    /// Read key request.
    /// Payload: JSON-serialized KeystoreReadRequest { key: String }
    pub const MSG_KEYSTORE_READ: u32 = 0xA000;
    /// Read key response.
    /// Payload: JSON-serialized KeystoreReadResponse { result: Result<Vec<u8>, KeystoreError> }
    pub const MSG_KEYSTORE_READ_RESPONSE: u32 = 0xA001;

    /// Write key request.
    /// Payload: JSON-serialized KeystoreWriteRequest { key: String, value: Vec<u8> }
    pub const MSG_KEYSTORE_WRITE: u32 = 0xA002;
    /// Write key response.
    /// Payload: JSON-serialized KeystoreWriteResponse { result: Result<(), KeystoreError> }
    pub const MSG_KEYSTORE_WRITE_RESPONSE: u32 = 0xA003;

    /// Delete key request.
    /// Payload: JSON-serialized KeystoreDeleteRequest { key: String }
    pub const MSG_KEYSTORE_DELETE: u32 = 0xA004;
    /// Delete key response.
    /// Payload: JSON-serialized KeystoreDeleteResponse { result: Result<(), KeystoreError> }
    pub const MSG_KEYSTORE_DELETE_RESPONSE: u32 = 0xA005;

    /// Check if key exists request.
    /// Payload: JSON-serialized KeystoreExistsRequest { key: String }
    pub const MSG_KEYSTORE_EXISTS: u32 = 0xA006;
    /// Check if key exists response.
    /// Payload: JSON-serialized KeystoreExistsResponse { result: Result<bool, KeystoreError> }
    pub const MSG_KEYSTORE_EXISTS_RESPONSE: u32 = 0xA007;

    /// List keys with prefix request.
    /// Payload: JSON-serialized KeystoreListRequest { prefix: String }
    pub const MSG_KEYSTORE_LIST: u32 = 0xA008;
    /// List keys response.
    /// Payload: JSON-serialized KeystoreListResponse { result: Result<Vec<String>, KeystoreError> }
    pub const MSG_KEYSTORE_LIST_RESPONSE: u32 = 0xA009;
}

// =============================================================================
// Debug Message Protocol (String Prefixes)
// =============================================================================

/// Debug message protocol prefixes.
///
/// These constants define the string prefixes used for supervisor<->process
/// debug message communication. Using constants instead of string literals
/// provides compile-time safety and consistency.
pub mod debug {
    // === Init Protocol ===
    /// Init spawn request: "INIT:SPAWN:{service_name}"
    pub const INIT_SPAWN: &str = "INIT:SPAWN:";
    /// Init grant capability: "INIT:GRANT:{details}"
    pub const INIT_GRANT: &str = "INIT:GRANT:";
    /// Init revoke capability: "INIT:REVOKE:{details}"
    pub const INIT_REVOKE: &str = "INIT:REVOKE:";
    /// Init kill success: "INIT:KILL_OK:{pid}"
    pub const INIT_KILL_OK: &str = "INIT:KILL_OK:";
    /// Init kill failure: "INIT:KILL_FAIL:{pid}:{error}"
    pub const INIT_KILL_FAIL: &str = "INIT:KILL_FAIL:";
    /// Init permission response: "INIT:PERM_RESPONSE:{details}"
    pub const INIT_PERM_RESPONSE: &str = "INIT:PERM_RESPONSE:";
    /// Init permission list: "INIT:PERM_LIST:{details}"
    pub const INIT_PERM_LIST: &str = "INIT:PERM_LIST:";
    /// Generic init message prefix
    pub const INIT_PREFIX: &str = "INIT:";

    // === Service Responses ===
    /// Service IPC response: "SERVICE:RESPONSE:{hex_data}"
    pub const SERVICE_RESPONSE: &str = "SERVICE:RESPONSE:";
    /// VFS service response: "VFS:RESPONSE:{hex_data}"
    pub const VFS_RESPONSE: &str = "VFS:RESPONSE:";
    /// Keystore service response: "KEYSTORE:RESPONSE:{to_pid}:{tag_hex}:{hex_data}"
    pub const KEYSTORE_RESPONSE: &str = "KEYSTORE:RESPONSE:";

    // === Spawn Protocol ===
    /// Spawn response: "SPAWN:RESPONSE:{hex_data}"
    pub const SPAWN_RESPONSE: &str = "SPAWN:RESPONSE:";
    /// Endpoint response: "ENDPOINT:RESPONSE:{hex_data}"
    pub const ENDPOINT_RESPONSE: &str = "ENDPOINT:RESPONSE:";
    /// Capability response: "CAP:RESPONSE:{hex_data}"
    pub const CAP_RESPONSE: &str = "CAP:RESPONSE:";

    // === Debug/Instrumentation ===
    /// Agent log prefix for debug instrumentation: "AGENT_LOG:{message}"
    pub const AGENT_LOG: &str = "AGENT_LOG:";
}

// =============================================================================
// Well-Known Slots
// =============================================================================

/// Well-known capability slots.
pub mod slots {
    /// Init's endpoint slot (every process gets this at spawn).
    pub const INIT_ENDPOINT_SLOT: u32 = 2;
}

// =============================================================================
// Well-Known PIDs
// =============================================================================

/// Well-known process IDs.
///
/// These PIDs are reserved for system processes that are spawned during boot.
/// The exact semantics of PID 0 vary by platform:
/// - WASM: Supervisor process (browser bridge)
/// - QEMU: Kernel itself (no separate entity)
pub mod pid {
    /// Supervisor/Kernel pseudo-process (platform-dependent).
    /// - WASM: Supervisor process (browser bridge)
    /// - QEMU: Kernel itself (no separate entity)
    pub const SUPERVISOR: u32 = 0;
    /// Init process - service registry and IPC router
    pub const INIT: u32 = 1;
    /// PermissionService - capability authority
    pub const PERMISSION_SERVICE: u32 = 2;
    /// VfsService - virtual filesystem (Note: spawned as PID 3 in current boot order)
    pub const VFS_SERVICE: u32 = 4;
    /// IdentityService - user/session management
    pub const IDENTITY_SERVICE: u32 = 5;
    /// TimeService - time settings
    pub const TIME_SERVICE: u32 = 6;
    /// KeystoreService - secure key storage
    pub const KEYSTORE_SERVICE: u32 = 7;
}

// =============================================================================
// Syscall Error Codes (for SYS_LOAD_BINARY, SYS_SPAWN_PROCESS)
// =============================================================================

/// Syscall error codes (negative return values).
///
/// These error codes are returned by syscalls like SYS_LOAD_BINARY and
/// SYS_SPAWN_PROCESS when an operation fails.
pub mod syscall_error {
    /// Invalid UTF-8 in name parameter
    pub const INVALID_UTF8: i32 = -1;
    /// Binary or resource not found
    pub const NOT_FOUND: i32 = -2;
    /// Operation not supported on this platform
    pub const NOT_SUPPORTED: i32 = -3;
    /// Permission denied (caller is not Init)
    pub const PERMISSION_DENIED: i32 = -4;
    /// Invalid argument (e.g., missing or malformed payload)
    pub const INVALID_ARGUMENT: i32 = -5;
    /// Process spawn failed
    pub const SPAWN_FAILED: i32 = -6;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_constant_conflicts() {
        // Ensure critical constants have expected values
        assert_eq!(supervisor::MSG_SUPERVISOR_REVOKE_CAP, 0x2020);
        assert_eq!(pm::MSG_REQUEST_CAPABILITY, 0x2010);

        // These should NOT be equal
        assert_ne!(
            supervisor::MSG_SUPERVISOR_REVOKE_CAP,
            pm::MSG_REQUEST_CAPABILITY
        );

        // Ensure kernel and console don't conflict
        assert_ne!(kernel::MSG_CAP_REVOKED, console::MSG_CONSOLE_INPUT);
    }

    #[test]
    fn test_message_ranges() {
        // Init service in 0x1000-0x100F
        const { assert!(init::MSG_REGISTER_SERVICE >= 0x1000) };
        const { assert!(init::MSG_VFS_RESPONSE_CAP_GRANTED <= 0x100F) };

        // PM in 0x2010-0x201F
        const { assert!(pm::MSG_REQUEST_CAPABILITY >= 0x2010) };
        const { assert!(pm::MSG_CAPS_LIST_RESPONSE <= 0x201F) };

        // Identity in 0x7000-0x70FF
        const { assert!(identity_user::MSG_CREATE_USER >= 0x7000) };
        const { assert!(identity_machine::MSG_ROTATE_MACHINE_KEY_RESPONSE <= 0x70FF) };

        // VFS in 0x8000-0x80FF
        const { assert!(vfs_dir::MSG_VFS_MKDIR >= 0x8000) };
        const { assert!(vfs_quota::MSG_VFS_GET_QUOTA_RESPONSE <= 0x80FF) };

        // Time service in 0x8100-0x810F
        const { assert!(time::MSG_GET_TIME_SETTINGS >= 0x8100) };
        const { assert!(time::MSG_SET_TIME_SETTINGS_RESPONSE <= 0x810F) };

        // Keystore service in 0xA000-0xA0FF
        const { assert!(keystore_svc::MSG_KEYSTORE_READ >= 0xA000) };
        const { assert!(keystore_svc::MSG_KEYSTORE_LIST_RESPONSE <= 0xA0FF) };
    }

    #[test]
    fn test_object_type_canonical_values() {
        // CRITICAL: These values MUST NOT change!
        // Any change would break capability grants across all crates.
        assert_eq!(ObjectType::Endpoint as u8, 1);
        assert_eq!(ObjectType::Process as u8, 2);
        assert_eq!(ObjectType::Memory as u8, 3);
        assert_eq!(ObjectType::Irq as u8, 4);
        assert_eq!(ObjectType::IoPort as u8, 5);
        assert_eq!(ObjectType::Console as u8, 6);
        assert_eq!(ObjectType::Storage as u8, 7);
        assert_eq!(ObjectType::Network as u8, 8);
        assert_eq!(ObjectType::Filesystem as u8, 9);
        assert_eq!(ObjectType::Identity as u8, 10);
        assert_eq!(ObjectType::Keystore as u8, 11);
    }

    #[test]
    fn test_object_type_from_u8_roundtrip() {
        for val in 1..=11u8 {
            let obj_type = ObjectType::from_u8(val).expect("valid value");
            assert_eq!(obj_type as u8, val);
        }
        // Invalid values should return None
        assert!(ObjectType::from_u8(0).is_none());
        assert!(ObjectType::from_u8(12).is_none());
        assert!(ObjectType::from_u8(255).is_none());
    }
}
