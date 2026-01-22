//! Orbital OS Kernel Core
//!
//! This crate implements the core kernel functionality:
//! - Process management
//! - Capability-based access control
//! - IPC endpoints and message passing
//! - Syscall dispatch

#![no_std]
extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use orbital_hal::HAL;

// Re-export HAL types
pub use orbital_hal::{HalError, HAL as HalTrait};

// Re-export Axiom types
pub use orbital_axiom::{
    apply_commit, replay, replay_and_verify, AxiomGateway, Commit, CommitId, CommitLog, CommitType,
    ReplayError, ReplayResult, Replayable, StateHasher, SysEvent, SysEventType, SysLog,
};

/// Process identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

/// IPC endpoint identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EndpointId(pub u64);

/// Capability slot index (per-process)
pub type CapSlot = u32;

/// Process state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    /// Process is running
    Running,
    /// Process is blocked waiting for IPC
    Blocked,
    /// Process has exited
    Zombie,
}

/// Process descriptor
pub struct Process {
    /// Process ID
    pub pid: ProcessId,
    /// Process name
    pub name: String,
    /// Current state
    pub state: ProcessState,
    /// Detailed metrics for this process
    pub metrics: ProcessMetrics,
}

/// Per-process resource tracking
#[derive(Clone, Debug, Default)]
pub struct ProcessMetrics {
    /// Memory size (bytes)
    pub memory_size: usize,
    /// Messages sent
    pub ipc_sent: u64,
    /// Messages received
    pub ipc_received: u64,
    /// Bytes sent via IPC
    pub ipc_bytes_sent: u64,
    /// Bytes received via IPC
    pub ipc_bytes_received: u64,
    /// Syscalls made
    pub syscall_count: u64,
    /// Time of last activity (nanos since boot)
    pub last_active_ns: u64,
    /// Process start time (nanos since boot)
    pub start_time_ns: u64,
}

/// Per-endpoint tracking
#[derive(Clone, Debug, Default)]
pub struct EndpointMetrics {
    /// Messages currently queued
    pub queue_depth: usize,
    /// Total messages ever sent to this endpoint
    pub total_messages: u64,
    /// Total bytes received
    pub total_bytes: u64,
    /// High water mark (max queue depth seen)
    pub queue_high_water: usize,
}

/// System-wide metrics
#[derive(Clone, Debug)]
pub struct SystemMetrics {
    /// Process count
    pub process_count: usize,
    /// Total memory across all processes
    pub total_memory: usize,
    /// Endpoint count
    pub endpoint_count: usize,
    /// Total pending messages
    pub total_pending_messages: usize,
    /// Total IPC messages since boot
    pub total_ipc_messages: u64,
    /// Uptime in nanoseconds
    pub uptime_ns: u64,
}

/// Object types that capabilities can reference
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ObjectType {
    /// IPC endpoint
    Endpoint = 1,
    /// Another process
    Process = 2,
    /// Memory region (for VMM)
    Memory = 3,
    /// IRQ handler
    Irq = 4,
    /// I/O port range
    IoPort = 5,
    /// Console/debug output
    Console = 6,
}

// ============================================================================
// Axiom Module - Capability Checking
// ============================================================================

/// Errors returned by Axiom capability checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxiomError {
    /// Capability slot is empty or invalid
    InvalidSlot,
    /// Capability references wrong object type
    WrongType,
    /// Capability lacks required permissions
    InsufficientRights,
    /// Capability has expired
    Expired,
    /// Object no longer exists
    ObjectNotFound,
}

/// Check if a process has authority to perform an operation.
///
/// This is the Axiom gatekeeper function. Every syscall that requires
/// authority calls this before executing.
///
/// # Arguments
/// - `cspace`: The process's capability space
/// - `slot`: The capability slot being used
/// - `required`: Minimum permissions needed
/// - `expected_type`: Expected object type (optional)
/// - `current_time`: Current time in nanos for expiration check
///
/// # Returns
/// - `Ok(&Capability)`: Authority granted, reference to the capability
/// - `Err(AxiomError)`: Authority denied with reason
///
/// # Invariants
/// - This function never modifies any state
/// - All kernel operations call this before executing
pub fn axiom_check<'a>(
    cspace: &'a CapabilitySpace,
    slot: CapSlot,
    required: &Permissions,
    expected_type: Option<ObjectType>,
    current_time: u64,
) -> Result<&'a Capability, AxiomError> {
    // 1. Lookup capability
    let cap = cspace.get(slot).ok_or(AxiomError::InvalidSlot)?;

    // 2. Check object type (if specified)
    if let Some(expected) = expected_type {
        if cap.object_type != expected {
            return Err(AxiomError::WrongType);
        }
    }

    // 3. Check permissions
    if (required.read && !cap.permissions.read)
        || (required.write && !cap.permissions.write)
        || (required.grant && !cap.permissions.grant)
    {
        return Err(AxiomError::InsufficientRights);
    }

    // 4. Check expiration
    if cap.is_expired(current_time) {
        return Err(AxiomError::Expired);
    }

    Ok(cap)
}

/// Capability permissions
#[derive(Clone, Copy, Debug, Default)]
pub struct Permissions {
    /// Can read/receive
    pub read: bool,
    /// Can write/send
    pub write: bool,
    /// Can grant to others
    pub grant: bool,
}

impl Permissions {
    /// Full permissions (read, write, grant)
    pub fn full() -> Self {
        Self {
            read: true,
            write: true,
            grant: true,
        }
    }

    /// Read-only permission
    pub fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            grant: false,
        }
    }

    /// Write-only permission
    pub fn write_only() -> Self {
        Self {
            read: false,
            write: true,
            grant: false,
        }
    }

    /// Convert to byte representation for CommitLog
    pub fn to_byte(&self) -> u8 {
        let mut b = 0u8;
        if self.read {
            b |= 0x01;
        }
        if self.write {
            b |= 0x02;
        }
        if self.grant {
            b |= 0x04;
        }
        b
    }

    /// Create from byte representation
    pub fn from_byte(b: u8) -> Self {
        Self {
            read: (b & 0x01) != 0,
            write: (b & 0x02) != 0,
            grant: (b & 0x04) != 0,
        }
    }
}

/// A capability token - proof of authority to access a resource
#[derive(Clone, Debug)]
pub struct Capability {
    /// Unique capability ID
    pub id: u64,
    /// Type of object this capability references
    pub object_type: ObjectType,
    /// ID of the referenced object
    pub object_id: u64,
    /// Permissions granted by this capability
    pub permissions: Permissions,
    /// Generation number (for revocation tracking)
    pub generation: u32,
    /// Expiration timestamp (nanos since boot, 0 = never expires)
    pub expires_at: u64,
}

impl Capability {
    /// Check if this capability has expired.
    pub fn is_expired(&self, current_time: u64) -> bool {
        self.expires_at != 0 && current_time > self.expires_at
    }
}

/// Per-process capability table
pub struct CapabilitySpace {
    /// Capability slots (public for replay)
    pub slots: BTreeMap<CapSlot, Capability>,
    /// Next slot to allocate (public for replay)
    pub next_slot: CapSlot,
}

impl CapabilitySpace {
    /// Create a new empty capability space
    pub fn new() -> Self {
        Self {
            slots: BTreeMap::new(),
            next_slot: 0,
        }
    }

    /// Insert a capability, returning its slot
    pub fn insert(&mut self, cap: Capability) -> CapSlot {
        let slot = self.next_slot;
        self.next_slot += 1;
        self.slots.insert(slot, cap);
        slot
    }

    /// Get a capability by slot
    pub fn get(&self, slot: CapSlot) -> Option<&Capability> {
        self.slots.get(&slot)
    }

    /// Remove a capability
    pub fn remove(&mut self, slot: CapSlot) -> Option<Capability> {
        self.slots.remove(&slot)
    }

    /// List all capabilities
    pub fn list(&self) -> Vec<(CapSlot, Capability)> {
        self.slots.iter().map(|(&s, c)| (s, c.clone())).collect()
    }

    /// Number of capabilities
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

impl Default for CapabilitySpace {
    fn default() -> Self {
        Self::new()
    }
}

/// Maximum capabilities per IPC message
pub const MAX_CAPS_PER_MESSAGE: usize = 8;

/// Maximum message payload size in bytes
pub const MAX_MESSAGE_SIZE: usize = 4096;

/// A capability being transferred via IPC.
///
/// When a capability is transferred, it is moved from the sender's CSpace
/// to the receiver's CSpace. The sender loses the capability.
#[derive(Clone, Debug)]
pub struct TransferredCap {
    /// The capability being transferred
    pub capability: Capability,
    /// Hint for receiver slot placement (None = kernel assigns)
    pub receiver_slot: Option<CapSlot>,
}

/// IPC message
#[derive(Clone, Debug)]
pub struct Message {
    /// Sender process
    pub from: ProcessId,
    /// Message tag (application-defined)
    pub tag: u32,
    /// Message payload
    pub data: Vec<u8>,
    /// Capabilities transferred with this message
    pub transferred_caps: Vec<TransferredCap>,
}

/// IPC endpoint
pub struct Endpoint {
    /// Endpoint ID
    pub id: EndpointId,
    /// Owning process
    pub owner: ProcessId,
    /// Queue of pending messages
    pub pending_messages: VecDeque<Message>,
    /// Endpoint metrics
    pub metrics: EndpointMetrics,
}

/// Kernel errors
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelError {
    /// Process not found
    ProcessNotFound,
    /// Endpoint not found
    EndpointNotFound,
    /// Invalid capability (not found or wrong type)
    InvalidCapability,
    /// Permission denied
    PermissionDenied,
    /// No message available (would block)
    WouldBlock,
    /// HAL error
    Hal(HalError),
}

impl From<HalError> for KernelError {
    fn from(e: HalError) -> Self {
        KernelError::Hal(e)
    }
}

// ============================================================================
// Canonical Syscall Numbers (ABI)
// ============================================================================

/// Debug print syscall
pub const SYS_DEBUG: u32 = 0x01;
/// Yield/cooperative scheduling hint
pub const SYS_YIELD: u32 = 0x02;
/// Exit process
pub const SYS_EXIT: u32 = 0x03;
/// Get current time (nanos since boot)
pub const SYS_TIME: u32 = 0x04;
/// Console write syscall - write text to console output
/// The supervisor receives a callback notification after this syscall completes.
pub const SYS_CONSOLE_WRITE: u32 = 0x07;

// Console input message tag (supervisor -> terminal input endpoint)
pub const MSG_CONSOLE_INPUT: u32 = 0x0002;

// Capability revocation notification message tag (supervisor -> process input endpoint)
pub const MSG_CAP_REVOKED: u32 = 0x3010;

/// Create an IPC endpoint
pub const SYS_CREATE_ENDPOINT: u32 = 0x11;
/// Delete an endpoint
pub const SYS_DELETE_ENDPOINT: u32 = 0x12;

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

/// List all processes (supervisor only)
pub const SYS_PS: u32 = 0x50;

/// Syscall request from a process
#[derive(Clone, Debug)]
pub enum Syscall {
    /// Print debug message (SYS_DEBUG 0x01)
    Debug { msg: String },
    /// Create a new IPC endpoint (SYS_CREATE_ENDPOINT 0x11)
    CreateEndpoint,
    /// Send a message to an endpoint (SYS_SEND 0x40)
    Send {
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
    },
    /// Receive a message from an endpoint (SYS_RECV 0x41)
    Receive { endpoint_slot: CapSlot },
    /// List this process's capabilities (SYS_CAP_LIST 0x35)
    ListCaps,
    /// List all processes (SYS_PS 0x50)
    ListProcesses,
    /// Exit process (SYS_EXIT 0x03)
    Exit { code: i32 },
    /// Get current time (SYS_TIME 0x04)
    GetTime,
    /// Yield CPU (SYS_YIELD 0x02)
    Yield,

    // === Capability syscalls ===
    /// Grant capability to another process (SYS_CAP_GRANT 0x30)
    CapGrant {
        from_slot: CapSlot,
        to_pid: ProcessId,
        permissions: Permissions,
    },
    /// Revoke a capability (SYS_CAP_REVOKE 0x31)
    CapRevoke { slot: CapSlot },
    /// Delete capability from own CSpace (SYS_CAP_DELETE 0x32)
    CapDelete { slot: CapSlot },
    /// Inspect a capability (SYS_CAP_INSPECT 0x33)
    CapInspect { slot: CapSlot },
    /// Derive capability with reduced permissions (SYS_CAP_DERIVE 0x34)
    CapDerive {
        slot: CapSlot,
        new_permissions: Permissions,
    },

    // === Enhanced IPC syscalls ===
    /// Send with capability transfer (SYS_SEND_CAP 0x44)
    SendWithCaps {
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
        cap_slots: Vec<CapSlot>,
    },
    /// Call (send + wait for reply) (SYS_CALL 0x42)
    Call {
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
    },
    /// Reply to a call (SYS_REPLY 0x43)
    Reply {
        caller_pid: ProcessId,
        tag: u32,
        data: Vec<u8>,
    },
}

/// Information about a capability (returned by CapInspect)
#[derive(Clone, Debug)]
pub struct CapInfo {
    /// Capability ID
    pub id: u64,
    /// Object type
    pub object_type: ObjectType,
    /// Object ID
    pub object_id: u64,
    /// Permissions
    pub permissions: Permissions,
    /// Generation (for revocation)
    pub generation: u32,
    /// Expiration (0 = never)
    pub expires_at: u64,
}

impl From<&Capability> for CapInfo {
    fn from(cap: &Capability) -> Self {
        Self {
            id: cap.id,
            object_type: cap.object_type,
            object_id: cap.object_id,
            permissions: cap.permissions,
            generation: cap.generation,
            expires_at: cap.expires_at,
        }
    }
}

/// Information about a revoked capability for notification delivery
#[derive(Clone, Debug)]
pub struct RevokeNotification {
    /// Process ID of the affected process
    pub pid: ProcessId,
    /// Capability slot that was revoked
    pub slot: CapSlot,
    /// Object type of the revoked capability
    pub object_type: u8,
    /// Object ID of the revoked capability
    pub object_id: u64,
    /// Reason for revocation
    pub reason: u8,
}

impl RevokeNotification {
    /// Create an empty notification (for cases where cap didn't exist)
    pub fn empty() -> Self {
        Self {
            pid: ProcessId(0),
            slot: 0,
            object_type: 0,
            object_id: 0,
            reason: 0,
        }
    }

    /// Check if this notification has valid data
    pub fn is_valid(&self) -> bool {
        self.object_type != 0
    }
}

/// Syscall result
#[derive(Clone, Debug)]
pub enum SyscallResult {
    /// Success with optional value
    Ok(u64),
    /// Error occurred
    Err(KernelError),
    /// Message received
    Message(Message),
    /// Message received with installed capability slots
    MessageWithCaps(Message, Vec<CapSlot>),
    /// Would block (no message available)
    WouldBlock,
    /// Capability info (from inspect)
    CapInfo(CapInfo),
    /// Capability list
    CapList(Vec<(CapSlot, Capability)>),
    /// Process list
    ProcessList(Vec<(ProcessId, String, ProcessState)>),
}

// ============================================================================
// KernelCore - Holds all mutable kernel state
// ============================================================================

/// The kernel core holds all mutable state.
///
/// All mutation methods on KernelCore return `(Result, Vec<Commit>)` where
/// the commits describe the state mutations that occurred. The caller is
/// responsible for appending these commits to the CommitLog via AxiomGateway.
///
/// This pattern ensures all state-mutating operations flow through AxiomGateway,
/// making Axiom-bypass violations impossible at compile time.
pub struct KernelCore<H: HAL> {
    /// HAL reference for debug output only (no state changes)
    hal: H,
    /// Process table
    pub(crate) processes: BTreeMap<ProcessId, Process>,
    /// Capability spaces (per-process)
    pub(crate) cap_spaces: BTreeMap<ProcessId, CapabilitySpace>,
    /// IPC endpoints
    pub(crate) endpoints: BTreeMap<EndpointId, Endpoint>,
    /// Next process ID
    pub(crate) next_pid: u64,
    /// Next endpoint ID
    pub(crate) next_endpoint_id: u64,
    /// Next capability ID
    pub(crate) next_cap_id: u64,
    /// Total IPC messages since boot
    pub(crate) total_ipc_count: u64,
    /// IPC traffic hook callback data (for live monitoring)
    pub(crate) ipc_traffic_log: VecDeque<IpcTrafficEntry>,
}

/// Console output entry (from SYS_CONSOLE_WRITE syscalls)
#[derive(Clone, Debug)]
pub struct ConsoleOutputEntry {
    /// Source process ID
    pub from: ProcessId,
    /// Output text
    pub text: Vec<u8>,
    /// Timestamp (nanos since boot)
    pub timestamp: u64,
}

/// The kernel, generic over HAL implementation.
///
/// The Kernel is a thin wrapper that provides:
/// - Read-only access to kernel state via public accessor methods
/// - The `axiom` gateway for all state-mutating operations
///
/// All mutations MUST flow through the Kernel's public methods, which ensure
/// proper audit logging and commit recording via axiom. The `core` field is
/// intentionally private to enforce this invariant at compile time.
pub struct Kernel<H: HAL> {
    /// The kernel core holding all mutable state (private to enforce Axiom routing)
    core: KernelCore<H>,
    /// Axiom gateway (SysLog + CommitLog) - entry point for all mutations
    pub axiom: AxiomGateway,
    /// Boot time (for uptime calculation)
    boot_time: u64,
    /// Console output buffer - populated by SYS_CONSOLE_WRITE syscalls
    /// The supervisor should drain this buffer and forward to the UI
    console_output_buffer: VecDeque<ConsoleOutputEntry>,
}

/// IPC traffic log entry for observability
#[derive(Clone, Debug)]
pub struct IpcTrafficEntry {
    /// Sender PID
    pub from: ProcessId,
    /// Receiver PID (endpoint owner)
    pub to: ProcessId,
    /// Endpoint ID
    pub endpoint: EndpointId,
    /// Message tag
    pub tag: u32,
    /// Message size in bytes
    pub size: usize,
    /// Timestamp (nanos since boot)
    pub timestamp: u64,
}

/// Maximum IPC traffic log entries to keep
const MAX_IPC_TRAFFIC_LOG: usize = 100;

// ============================================================================
// KernelCore Implementation - All mutation methods return commits
// ============================================================================

impl<H: HAL> KernelCore<H> {
    /// Create a new kernel core with the given HAL
    pub fn new(hal: H) -> Self {
        Self {
            hal,
            processes: BTreeMap::new(),
            cap_spaces: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            next_pid: 1,
            next_endpoint_id: 1,
            next_cap_id: 1,
            total_ipc_count: 0,
            ipc_traffic_log: VecDeque::new(),
        }
    }

    /// Get a reference to the HAL (for debug output)
    pub fn hal(&self) -> &H {
        &self.hal
    }

    /// Generate next capability ID
    fn next_cap_id(&mut self) -> u64 {
        let id = self.next_cap_id;
        self.next_cap_id += 1;
        id
    }

    /// Register a process (used by supervisor to register spawned workers).
    ///
    /// Returns (ProcessId, Vec<Commit>) - the commits describe the mutation.
    pub fn register_process(&mut self, name: &str, timestamp: u64) -> (ProcessId, Vec<Commit>) {
        self.register_process_with_parent(name, ProcessId(0), timestamp)
    }

    /// Register a process with a specific parent (for fork/spawn tracking).
    ///
    /// Returns (ProcessId, Vec<Commit>) - the commits describe the mutation.
    pub fn register_process_with_parent(
        &mut self,
        name: &str,
        parent: ProcessId,
        timestamp: u64,
    ) -> (ProcessId, Vec<Commit>) {
        let pid = ProcessId(self.next_pid);
        self.next_pid += 1;

        let process = Process {
            pid,
            name: String::from(name),
            state: ProcessState::Running,
            metrics: ProcessMetrics {
                memory_size: 65536, // Initial 64KB (1 WASM page)
                ipc_sent: 0,
                ipc_received: 0,
                ipc_bytes_sent: 0,
                ipc_bytes_received: 0,
                syscall_count: 0,
                last_active_ns: timestamp,
                start_time_ns: timestamp,
            },
        };
        self.processes.insert(pid, process);
        self.cap_spaces.insert(pid, CapabilitySpace::new());

        self.hal.debug_write(&alloc::format!(
            "[kernel] Registered process: {} (PID {})",
            name,
            pid.0
        ));

        // Create the commit for this mutation
        let commit = Commit {
            id: [0u8; 32], // Will be computed by CommitLog
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::ProcessCreated {
                pid: pid.0,
                parent: parent.0,
                name: String::from(name),
            },
            caused_by: None,
        };

        (pid, vec![commit])
    }

    /// Register a process with a specific PID (used for supervisor and special processes).
    ///
    /// Returns (ProcessId, Vec<Commit>) - the commits describe the mutation.
    /// If the PID already exists, returns the existing PID without creating a new process.
    pub fn register_process_with_pid(
        &mut self,
        pid: ProcessId,
        name: &str,
        timestamp: u64,
    ) -> (ProcessId, Vec<Commit>) {
        // If process with this PID already exists, return it
        if self.processes.contains_key(&pid) {
            self.hal.debug_write(&alloc::format!(
                "[kernel] Process {} (PID {}) already exists",
                name,
                pid.0
            ));
            return (pid, vec![]);
        }

        // Update next_pid if necessary to avoid collisions
        if pid.0 >= self.next_pid {
            self.next_pid = pid.0 + 1;
        }

        let process = Process {
            pid,
            name: String::from(name),
            state: ProcessState::Running,
            metrics: ProcessMetrics {
                memory_size: 0, // Supervisor has no memory allocation
                ipc_sent: 0,
                ipc_received: 0,
                ipc_bytes_sent: 0,
                ipc_bytes_received: 0,
                syscall_count: 0,
                last_active_ns: timestamp,
                start_time_ns: timestamp,
            },
        };
        self.processes.insert(pid, process);
        self.cap_spaces.insert(pid, CapabilitySpace::new());

        self.hal.debug_write(&alloc::format!(
            "[kernel] Registered process: {} (PID {})",
            name,
            pid.0
        ));

        // Create the commit for this mutation
        let commit = Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::ProcessCreated {
                pid: pid.0,
                parent: 0, // Supervisor has no parent
                name: String::from(name),
            },
            caused_by: None,
        };

        (pid, vec![commit])
    }

    /// Kill a process and clean up its resources.
    ///
    /// Returns Vec<Commit> describing the mutations.
    pub fn kill_process(&mut self, pid: ProcessId, timestamp: u64) -> Vec<Commit> {
        let mut commits = Vec::new();

        // Remove the process
        if let Some(proc) = self.processes.remove(&pid) {
            self.hal.debug_write(&alloc::format!(
                "[kernel] Killed process: {} (PID {})",
                proc.name,
                pid.0
            ));

            // Log process exit
            commits.push(Commit {
                id: [0u8; 32],
                prev_commit: [0u8; 32],
                seq: 0,
                timestamp,
                commit_type: CommitType::ProcessExited { pid: pid.0, code: -1 },
                caused_by: None,
            });
        }

        // Remove its capability space
        self.cap_spaces.remove(&pid);

        // Remove endpoints owned by this process
        let owned_endpoints: Vec<EndpointId> = self
            .endpoints
            .iter()
            .filter(|(_, ep)| ep.owner == pid)
            .map(|(id, _)| *id)
            .collect();

        for eid in owned_endpoints {
            self.endpoints.remove(&eid);
            commits.push(Commit {
                id: [0u8; 32],
                prev_commit: [0u8; 32],
                seq: 0,
                timestamp,
                commit_type: CommitType::EndpointDestroyed { id: eid.0 },
                caused_by: None,
            });
        }

        commits
    }

    /// Record a process fault and terminate it.
    ///
    /// This is used when a process crashes, performs an invalid syscall,
    /// or otherwise faults. The fault is recorded in the commit log before
    /// the process is terminated.
    ///
    /// # Fault reason codes:
    /// - 1: Invalid syscall
    /// - 2: Memory access violation
    /// - 3: Capability violation
    /// - 4: Panic / abort
    /// - 5: Timeout / watchdog
    /// - 0xFF: Unknown / unspecified
    pub fn fault_process(
        &mut self,
        pid: ProcessId,
        reason: u32,
        description: String,
        timestamp: u64,
    ) -> Vec<Commit> {
        let mut commits = Vec::new();

        // Only record fault if process exists
        if self.processes.contains_key(&pid) {
            self.hal.debug_write(&alloc::format!(
                "[kernel] Process {} faulted: {} (reason {})",
                pid.0, description, reason
            ));

            // Record the fault
            commits.push(Commit {
                id: [0u8; 32],
                prev_commit: [0u8; 32],
                seq: 0,
                timestamp,
                commit_type: CommitType::ProcessFaulted {
                    pid: pid.0,
                    reason,
                    description,
                },
                caused_by: None,
            });
        }

        // Now kill the process (this adds ProcessExited and EndpointDestroyed commits)
        commits.extend(self.kill_process(pid, timestamp));

        commits
    }

    /// Create an IPC endpoint owned by a process.
    ///
    /// Returns (Result<(EndpointId, CapSlot), KernelError>, Vec<Commit>).
    pub fn create_endpoint(
        &mut self,
        owner: ProcessId,
        timestamp: u64,
    ) -> (Result<(EndpointId, CapSlot), KernelError>, Vec<Commit>) {
        let mut commits = Vec::new();

        if !self.processes.contains_key(&owner) {
            return (Err(KernelError::ProcessNotFound), commits);
        }

        let id = EndpointId(self.next_endpoint_id);
        self.next_endpoint_id += 1;

        let endpoint = Endpoint {
            id,
            owner,
            pending_messages: VecDeque::new(),
            metrics: EndpointMetrics::default(),
        };
        self.endpoints.insert(id, endpoint);

        // Grant full capability to owner
        let cap_id = self.next_cap_id();
        let perms = Permissions::full();
        let cap = Capability {
            id: cap_id,
            object_type: ObjectType::Endpoint,
            object_id: id.0,
            permissions: perms,
            generation: 0,
            expires_at: 0, // Never expires
        };

        let slot = match self.cap_spaces.get_mut(&owner) {
            Some(cspace) => cspace.insert(cap),
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        // Log endpoint creation
        commits.push(Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::EndpointCreated {
                id: id.0,
                owner: owner.0,
            },
            caused_by: None,
        });

        // Log capability insertion
        commits.push(Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::CapInserted {
                pid: owner.0,
                slot,
                cap_id,
                object_type: ObjectType::Endpoint as u8,
                object_id: id.0,
                perms: perms.to_byte(),
            },
            caused_by: None,
        });

        self.hal.debug_write(&alloc::format!(
            "[kernel] Created endpoint {} for PID {}, cap slot {}",
            id.0,
            owner.0,
            slot
        ));

        (Ok((id, slot)), commits)
    }

    /// Grant a capability from one process to another (validates via axiom_check).
    ///
    /// Returns (Result<CapSlot, KernelError>, Vec<Commit>).
    pub fn grant_capability(
        &mut self,
        from_pid: ProcessId,
        from_slot: CapSlot,
        to_pid: ProcessId,
        new_perms: Permissions,
        timestamp: u64,
    ) -> (Result<CapSlot, KernelError>, Vec<Commit>) {
        let mut commits = Vec::new();

        // Get capability space for the granter
        let cspace = match self.cap_spaces.get(&from_pid) {
            Some(cs) => cs,
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        // Use axiom_check to verify capability with grant permission (includes expiration check)
        let grant_perms = Permissions {
            read: false,
            write: false,
            grant: true,
        };
        let source_cap = match axiom_check(cspace, from_slot, &grant_perms, None, timestamp) {
            Ok(cap) => cap.clone(),
            Err(e) => {
                let err = match e {
                    AxiomError::InvalidSlot => KernelError::InvalidCapability,
                    AxiomError::WrongType => KernelError::InvalidCapability,
                    AxiomError::InsufficientRights => KernelError::PermissionDenied,
                    AxiomError::Expired => KernelError::PermissionDenied,
                    AxiomError::ObjectNotFound => KernelError::InvalidCapability,
                };
                return (Err(err), commits);
            }
        };

        // Attenuate permissions (can only reduce, never amplify)
        let granted_perms = Permissions {
            read: source_cap.permissions.read && new_perms.read,
            write: source_cap.permissions.write && new_perms.write,
            grant: source_cap.permissions.grant && new_perms.grant,
        };

        // Create new capability with new ID
        let new_cap_id = self.next_cap_id();
        let new_cap = Capability {
            id: new_cap_id,
            object_type: source_cap.object_type,
            object_id: source_cap.object_id,
            permissions: granted_perms,
            generation: source_cap.generation,
            expires_at: source_cap.expires_at,
        };

        // Insert into destination
        let to_slot = match self.cap_spaces.get_mut(&to_pid) {
            Some(cspace) => cspace.insert(new_cap),
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        // Log CapGranted commit
        commits.push(Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::CapGranted {
                from_pid: from_pid.0,
                to_pid: to_pid.0,
                from_slot,
                to_slot,
                new_cap_id,
                perms: orbital_axiom::Permissions {
                    read: granted_perms.read,
                    write: granted_perms.write,
                    grant: granted_perms.grant,
                },
            },
            caused_by: None,
        });

        // Also log CapInserted for the receiver (needed for replay)
        commits.push(Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::CapInserted {
                pid: to_pid.0,
                slot: to_slot,
                cap_id: new_cap_id,
                object_type: source_cap.object_type as u8,
                object_id: source_cap.object_id,
                perms: granted_perms.to_byte(),
            },
            caused_by: None,
        });

        (Ok(to_slot), commits)
    }

    /// Grant a capability to a specific endpoint directly (used for initial setup).
    ///
    /// This creates a new capability for the target process pointing to the given endpoint.
    /// The owner must own the endpoint. This is used during process spawn to set up
    /// the initial capability graph.
    ///
    /// Returns (Result<CapSlot, KernelError>, Vec<Commit>).
    pub fn grant_capability_to_endpoint(
        &mut self,
        owner_pid: ProcessId,
        endpoint_id: EndpointId,
        to_pid: ProcessId,
        perms: Permissions,
        timestamp: u64,
    ) -> (Result<CapSlot, KernelError>, Vec<Commit>) {
        let mut commits = Vec::new();

        // Verify the endpoint exists and is owned by owner_pid
        let endpoint = match self.endpoints.get(&endpoint_id) {
            Some(ep) => ep,
            None => return (Err(KernelError::EndpointNotFound), commits),
        };

        if endpoint.owner != owner_pid {
            return (Err(KernelError::PermissionDenied), commits);
        }

        // Create new capability with new ID
        let new_cap_id = self.next_cap_id();
        let new_cap = Capability {
            id: new_cap_id,
            object_type: ObjectType::Endpoint,
            object_id: endpoint_id.0,
            permissions: perms,
            generation: 0,
            expires_at: 0, // 0 = never expires
        };

        // Insert into destination
        let to_slot = match self.cap_spaces.get_mut(&to_pid) {
            Some(cspace) => cspace.insert(new_cap),
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        // Log CapInserted commit (this is the authoritative log for replay)
        commits.push(Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::CapInserted {
                pid: to_pid.0,
                slot: to_slot,
                cap_id: new_cap_id,
                object_type: ObjectType::Endpoint as u8,
                object_id: endpoint_id.0,
                perms: perms.to_byte(),
            },
            caused_by: None,
        });

        self.hal.debug_write(&alloc::format!(
            "[kernel] Granted endpoint {} capability to PID {} at slot {}",
            endpoint_id.0,
            to_pid.0,
            to_slot
        ));

        (Ok(to_slot), commits)
    }

    /// Revoke a capability (validates via axiom_check).
    ///
    /// Revocation requires the caller to have grant permission on the capability.
    /// This removes the capability from the caller's CSpace.
    ///
    /// Returns (Result<(), KernelError>, Vec<Commit>).
    pub fn revoke_capability(
        &mut self,
        pid: ProcessId,
        slot: CapSlot,
        timestamp: u64,
    ) -> (Result<(), KernelError>, Vec<Commit>) {
        let mut commits = Vec::new();

        // Get capability space
        let cspace = match self.cap_spaces.get(&pid) {
            Some(cs) => cs,
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        // Use axiom_check to verify capability with grant permission (includes expiration check)
        let grant_perms = Permissions {
            read: false,
            write: false,
            grant: true,
        };
        let cap = match axiom_check(cspace, slot, &grant_perms, None, timestamp) {
            Ok(c) => c,
            Err(e) => {
                let err = match e {
                    AxiomError::InvalidSlot => KernelError::InvalidCapability,
                    AxiomError::WrongType => KernelError::InvalidCapability,
                    AxiomError::InsufficientRights => KernelError::PermissionDenied,
                    AxiomError::Expired => KernelError::PermissionDenied,
                    AxiomError::ObjectNotFound => KernelError::InvalidCapability,
                };
                return (Err(err), commits);
            }
        };

        let cap_id = cap.id;

        // Log CapRemoved commit
        commits.push(Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::CapRemoved { pid: pid.0, slot },
            caused_by: None,
        });

        // Remove from CSpace
        match self.cap_spaces.get_mut(&pid) {
            Some(cspace) => {
                cspace.remove(slot);
            }
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        self.hal.debug_write(&alloc::format!(
            "[kernel] PID {} revoked capability {} (slot {})",
            pid.0,
            cap_id,
            slot
        ));

        (Ok(()), commits)
    }

    /// Delete a capability from a process's own CSpace.
    ///
    /// Unlike revoke, delete does not require grant permission. A process can
    /// always delete capabilities from its own CSpace.
    ///
    /// Returns (Result<(), KernelError>, Vec<Commit>).
    pub fn delete_capability(
        &mut self,
        pid: ProcessId,
        slot: CapSlot,
        timestamp: u64,
    ) -> (Result<(), KernelError>, Vec<Commit>) {
        let mut commits = Vec::new();

        // Get capability space
        let cspace = match self.cap_spaces.get(&pid) {
            Some(cs) => cs,
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        // Check capability exists
        let cap = match cspace.get(slot) {
            Some(c) => c,
            None => return (Err(KernelError::InvalidCapability), commits),
        };
        let cap_id = cap.id;

        // Log CapRemoved commit
        commits.push(Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::CapRemoved { pid: pid.0, slot },
            caused_by: None,
        });

        // Remove from CSpace
        match self.cap_spaces.get_mut(&pid) {
            Some(cspace) => {
                cspace.remove(slot);
            }
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        self.hal.debug_write(&alloc::format!(
            "[kernel] PID {} deleted capability {} (slot {})",
            pid.0,
            cap_id,
            slot
        ));

        (Ok(()), commits)
    }

    /// Send IPC message (validates capability via axiom_check).
    ///
    /// Note: IPC messages are not replayed (volatile), so no commits are returned
    /// for the message itself. Only metrics are updated.
    /// Send IPC message.
    ///
    /// Returns (Result<(), KernelError>, Option<Commit>) - optional MessageSent commit for audit.
    pub fn ipc_send(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
        timestamp: u64,
    ) -> (Result<(), KernelError>, Option<Commit>) {
        // Get capability space for the sender
        let cspace = match self.cap_spaces.get(&from_pid) {
            Some(cs) => cs,
            None => return (Err(KernelError::ProcessNotFound), None),
        };

        // Use axiom_check to verify capability (includes expiration check)
        let cap = match axiom_check(
            cspace,
            endpoint_slot,
            &Permissions::write_only(),
            Some(ObjectType::Endpoint),
            timestamp,
        ) {
            Ok(c) => c,
            Err(e) => {
                let err = match e {
                    AxiomError::InvalidSlot => KernelError::InvalidCapability,
                    AxiomError::WrongType => KernelError::InvalidCapability,
                    AxiomError::InsufficientRights => KernelError::PermissionDenied,
                    AxiomError::Expired => KernelError::PermissionDenied,
                    AxiomError::ObjectNotFound => KernelError::EndpointNotFound,
                };
                return (Err(err), None);
            }
        };

        let endpoint_id = EndpointId(cap.object_id);
        let data_len = data.len();

        // Queue message
        let endpoint = match self.endpoints.get_mut(&endpoint_id) {
            Some(ep) => ep,
            None => return (Err(KernelError::EndpointNotFound), None),
        };

        let to_pid = endpoint.owner;
        let message = Message {
            from: from_pid,
            tag,
            data,
            transferred_caps: vec![],
        };

        endpoint.pending_messages.push_back(message);

        // Update endpoint metrics
        endpoint.metrics.queue_depth = endpoint.pending_messages.len();
        endpoint.metrics.total_messages += 1;
        endpoint.metrics.total_bytes += data_len as u64;
        if endpoint.metrics.queue_depth > endpoint.metrics.queue_high_water {
            endpoint.metrics.queue_high_water = endpoint.metrics.queue_depth;
        }

        // Update sender process metrics
        if let Some(sender) = self.processes.get_mut(&from_pid) {
            sender.metrics.ipc_sent += 1;
            sender.metrics.ipc_bytes_sent += data_len as u64;
            sender.metrics.last_active_ns = timestamp;
        }

        // Update global IPC count
        self.total_ipc_count += 1;

        // Log to traffic monitor
        let entry = IpcTrafficEntry {
            from: from_pid,
            to: to_pid,
            endpoint: endpoint_id,
            tag,
            size: data_len,
            timestamp,
        };
        self.ipc_traffic_log.push_back(entry);
        while self.ipc_traffic_log.len() > MAX_IPC_TRAFFIC_LOG {
            self.ipc_traffic_log.pop_front();
        }

        // Create MessageSent commit for audit trail
        let commit = Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::MessageSent {
                from_pid: from_pid.0,
                to_endpoint: endpoint_id.0,
                tag,
                size: data_len,
            },
            caused_by: None,
        };

        (Ok(()), Some(commit))
    }

    /// Send IPC message with capability transfer.
    ///
    /// Capabilities in `cap_slots` are removed from the sender's CSpace and
    /// transferred to the receiver.
    ///
    /// Returns (Result<(), KernelError>, Vec<Commit>) - commits for capability removals.
    pub fn ipc_send_with_caps(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
        cap_slots: &[CapSlot],
        timestamp: u64,
    ) -> (Result<(), KernelError>, Vec<Commit>) {
        let mut commits = Vec::new();

        // Validate limits
        if data.len() > MAX_MESSAGE_SIZE {
            return (Err(KernelError::PermissionDenied), commits);
        }
        if cap_slots.len() > MAX_CAPS_PER_MESSAGE {
            return (Err(KernelError::PermissionDenied), commits);
        }

        // Lookup endpoint capability
        let cap = match self.cap_spaces.get(&from_pid) {
            Some(cspace) => match cspace.get(endpoint_slot) {
                Some(c) => c,
                None => return (Err(KernelError::InvalidCapability), commits),
            },
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        // Check it's an endpoint capability with write permission
        if cap.object_type != ObjectType::Endpoint || !cap.permissions.write {
            return (Err(KernelError::PermissionDenied), commits);
        }

        let endpoint_id = EndpointId(cap.object_id);

        // Verify endpoint exists and get receiver
        let to_pid = match self.endpoints.get(&endpoint_id) {
            Some(ep) => ep.owner,
            None => return (Err(KernelError::EndpointNotFound), commits),
        };

        // Collect capabilities to transfer (validate they exist first)
        let sender_cspace = match self.cap_spaces.get(&from_pid) {
            Some(cs) => cs,
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        for &slot in cap_slots {
            if sender_cspace.get(slot).is_none() {
                return (Err(KernelError::InvalidCapability), commits);
            }
        }

        // Remove capabilities from sender and build transfer list
        let mut transferred_caps = Vec::with_capacity(cap_slots.len());

        let sender_cspace = match self.cap_spaces.get_mut(&from_pid) {
            Some(cs) => cs,
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        for &slot in cap_slots {
            if let Some(cap) = sender_cspace.remove(slot) {
                // Log capability removal from sender
                commits.push(Commit {
                    id: [0u8; 32],
                    prev_commit: [0u8; 32],
                    seq: 0,
                    timestamp,
                    commit_type: CommitType::CapRemoved {
                        pid: from_pid.0,
                        slot,
                    },
                    caused_by: None,
                });
                transferred_caps.push(TransferredCap {
                    capability: cap,
                    receiver_slot: None,
                });
            }
        }

        let data_len = data.len();

        // Queue message with transferred capabilities
        let endpoint = match self.endpoints.get_mut(&endpoint_id) {
            Some(ep) => ep,
            None => return (Err(KernelError::EndpointNotFound), commits),
        };

        let message = Message {
            from: from_pid,
            tag,
            data,
            transferred_caps,
        };

        endpoint.pending_messages.push_back(message);

        // Update endpoint metrics
        endpoint.metrics.queue_depth = endpoint.pending_messages.len();
        endpoint.metrics.total_messages += 1;
        endpoint.metrics.total_bytes += data_len as u64;
        if endpoint.metrics.queue_depth > endpoint.metrics.queue_high_water {
            endpoint.metrics.queue_high_water = endpoint.metrics.queue_depth;
        }

        // Update sender process metrics
        if let Some(sender) = self.processes.get_mut(&from_pid) {
            sender.metrics.ipc_sent += 1;
            sender.metrics.ipc_bytes_sent += data_len as u64;
            sender.metrics.last_active_ns = timestamp;
        }

        // Update global IPC count
        self.total_ipc_count += 1;

        // Log to traffic monitor
        let entry = IpcTrafficEntry {
            from: from_pid,
            to: to_pid,
            endpoint: endpoint_id,
            tag,
            size: data_len,
            timestamp,
        };
        self.ipc_traffic_log.push_back(entry);
        while self.ipc_traffic_log.len() > MAX_IPC_TRAFFIC_LOG {
            self.ipc_traffic_log.pop_front();
        }

        (Ok(()), commits)
    }

    /// Receive IPC message and install transferred capabilities.
    ///
    /// Returns (Result<Option<(Message, Vec<CapSlot>)>, KernelError>, Vec<Commit>).
    /// Commits are generated for installed capabilities.
    pub fn ipc_receive_with_caps(
        &mut self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
        timestamp: u64,
    ) -> (Result<Option<(Message, Vec<CapSlot>)>, KernelError>, Vec<Commit>) {
        let mut commits = Vec::new();

        // First do normal receive to get the message
        let message = match self.ipc_receive(pid, endpoint_slot, timestamp) {
            Ok(Some(msg)) => msg,
            Ok(None) => return (Ok(None), commits),
            Err(e) => return (Err(e), commits),
        };

        // Install transferred capabilities into receiver's CSpace
        let mut installed_slots = Vec::with_capacity(message.transferred_caps.len());

        if !message.transferred_caps.is_empty() {
            let receiver_cspace = match self.cap_spaces.get_mut(&pid) {
                Some(cs) => cs,
                None => return (Err(KernelError::ProcessNotFound), commits),
            };

            for tcap in &message.transferred_caps {
                let slot = receiver_cspace.insert(tcap.capability.clone());
                installed_slots.push(slot);

                // Log capability insertion for the receiver
                commits.push(Commit {
                    id: [0u8; 32],
                    prev_commit: [0u8; 32],
                    seq: 0,
                    timestamp,
                    commit_type: CommitType::CapInserted {
                        pid: pid.0,
                        slot,
                        cap_id: tcap.capability.id,
                        object_type: tcap.capability.object_type as u8,
                        object_id: tcap.capability.object_id,
                        perms: tcap.capability.permissions.to_byte(),
                    },
                    caused_by: None,
                });
            }
        }

        (Ok(Some((message, installed_slots))), commits)
    }

    /// Check if an IPC endpoint has pending messages (without removing them).
    ///
    /// This is used by SYS_RECEIVE to check for messages before the actual receive.
    pub fn ipc_has_message(
        &self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
        timestamp: u64,
    ) -> Result<bool, KernelError> {
        // Get capability space for the receiver
        let cspace = self
            .cap_spaces
            .get(&pid)
            .ok_or(KernelError::ProcessNotFound)?;

        // Use axiom_check to verify capability (includes expiration check)
        let cap = axiom_check(
            cspace,
            endpoint_slot,
            &Permissions::read_only(),
            Some(ObjectType::Endpoint),
            timestamp,
        )
        .map_err(|e| match e {
            AxiomError::InvalidSlot => KernelError::InvalidCapability,
            AxiomError::WrongType => KernelError::InvalidCapability,
            AxiomError::InsufficientRights => KernelError::PermissionDenied,
            AxiomError::Expired => KernelError::PermissionDenied,
            AxiomError::ObjectNotFound => KernelError::EndpointNotFound,
        })?;

        let endpoint_id = EndpointId(cap.object_id);

        // Check if endpoint has messages
        let endpoint = self
            .endpoints
            .get(&endpoint_id)
            .ok_or(KernelError::EndpointNotFound)?;

        Ok(!endpoint.pending_messages.is_empty())
    }

    /// Receive IPC message.
    ///
    /// Note: IPC messages are volatile and not replayed, so no commits are returned.
    /// Receive IPC message (validates capability via axiom_check).
    pub fn ipc_receive(
        &mut self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
        timestamp: u64,
    ) -> Result<Option<Message>, KernelError> {
        // Get capability space for the receiver
        let cspace = self
            .cap_spaces
            .get(&pid)
            .ok_or(KernelError::ProcessNotFound)?;

        // Use axiom_check to verify capability (includes expiration check)
        let cap = axiom_check(
            cspace,
            endpoint_slot,
            &Permissions::read_only(),
            Some(ObjectType::Endpoint),
            timestamp,
        )
        .map_err(|e| match e {
            AxiomError::InvalidSlot => KernelError::InvalidCapability,
            AxiomError::WrongType => KernelError::InvalidCapability,
            AxiomError::InsufficientRights => KernelError::PermissionDenied,
            AxiomError::Expired => KernelError::PermissionDenied,
            AxiomError::ObjectNotFound => KernelError::EndpointNotFound,
        })?;

        let endpoint_id = EndpointId(cap.object_id);

        // Get message
        let endpoint = self
            .endpoints
            .get_mut(&endpoint_id)
            .ok_or(KernelError::EndpointNotFound)?;

        let message = endpoint.pending_messages.pop_front();

        // Update endpoint queue depth
        endpoint.metrics.queue_depth = endpoint.pending_messages.len();

        // Update receiver process metrics if we got a message
        if let Some(ref msg) = message {
            if let Some(receiver) = self.processes.get_mut(&pid) {
                receiver.metrics.ipc_received += 1;
                receiver.metrics.ipc_bytes_received += msg.data.len() as u64;
                receiver.metrics.last_active_ns = timestamp;
            }
        }

        Ok(message)
    }

    /// Handle syscall from a process.
    ///
    /// Returns (SyscallResult, Vec<Commit>) - the result and any commits generated.
    pub fn handle_syscall(
        &mut self,
        from_pid: ProcessId,
        syscall: Syscall,
        timestamp: u64,
    ) -> (SyscallResult, Vec<Commit>) {
        let mut commits = Vec::new();

        // Update syscall count
        if let Some(proc) = self.processes.get_mut(&from_pid) {
            proc.metrics.syscall_count += 1;
            proc.metrics.last_active_ns = timestamp;
        }

        let result = match syscall {
            Syscall::Debug { msg } => {
                self.hal
                    .debug_write(&alloc::format!("[PID {}] {}", from_pid.0, msg));
                SyscallResult::Ok(0)
            }

            Syscall::CreateEndpoint => {
                let (result, ep_commits) = self.create_endpoint(from_pid, timestamp);
                commits.extend(ep_commits);
                match result {
                    Ok((eid, slot)) => SyscallResult::Ok((eid.0 << 32) | (slot as u64)),
                    Err(e) => SyscallResult::Err(e),
                }
            }

            Syscall::Send {
                endpoint_slot,
                tag,
                data,
            } => {
                let (result, commit) = self.ipc_send(from_pid, endpoint_slot, tag, data, timestamp);
                if let Some(c) = commit {
                    commits.push(c);
                }
                match result {
                    Ok(()) => SyscallResult::Ok(0),
                    Err(e) => SyscallResult::Err(e),
                }
            }

            Syscall::Receive { endpoint_slot } => {
                match self.ipc_receive(from_pid, endpoint_slot, timestamp) {
                    Ok(Some(msg)) => SyscallResult::Message(msg),
                    Ok(None) => SyscallResult::WouldBlock,
                    Err(e) => SyscallResult::Err(e),
                }
            }

            Syscall::ListCaps => {
                let caps = self
                    .cap_spaces
                    .get(&from_pid)
                    .map(|cs| cs.list())
                    .unwrap_or_default();
                SyscallResult::CapList(caps)
            }

            Syscall::ListProcesses => {
                let procs: Vec<_> = self
                    .processes
                    .iter()
                    .map(|(pid, p)| (*pid, p.name.clone(), p.state))
                    .collect();
                SyscallResult::ProcessList(procs)
            }

            Syscall::Exit { code } => {
                if let Some(proc) = self.processes.get_mut(&from_pid) {
                    proc.state = ProcessState::Zombie;
                }
                // Create process exit commit
                commits.push(Commit {
                    id: [0u8; 32],
                    prev_commit: [0u8; 32],
                    seq: 0,
                    timestamp,
                    commit_type: CommitType::ProcessExited {
                        pid: from_pid.0,
                        code,
                    },
                    caused_by: None,
                });
                SyscallResult::Ok(code as u64)
            }

            Syscall::GetTime => SyscallResult::Ok(timestamp),

            Syscall::Yield => {
                // Cooperative yield - just return success
                SyscallResult::Ok(0)
            }

            // === Capability syscalls ===
            Syscall::CapGrant {
                from_slot,
                to_pid,
                permissions,
            } => {
                let (result, grant_commits) =
                    self.grant_capability(from_pid, from_slot, to_pid, permissions, timestamp);
                commits.extend(grant_commits);
                match result {
                    Ok(new_slot) => SyscallResult::Ok(new_slot as u64),
                    Err(e) => SyscallResult::Err(e),
                }
            }

            Syscall::CapRevoke { slot } => {
                let (result, revoke_commits) = self.revoke_capability(from_pid, slot, timestamp);
                commits.extend(revoke_commits);
                match result {
                    Ok(()) => SyscallResult::Ok(0),
                    Err(e) => SyscallResult::Err(e),
                }
            }

            Syscall::CapDelete { slot } => {
                let (result, delete_commits) = self.delete_capability(from_pid, slot, timestamp);
                commits.extend(delete_commits);
                match result {
                    Ok(()) => SyscallResult::Ok(0),
                    Err(e) => SyscallResult::Err(e),
                }
            }

            Syscall::CapInspect { slot } => match self.cap_spaces.get(&from_pid) {
                Some(cspace) => match cspace.get(slot) {
                    Some(cap) => SyscallResult::CapInfo(CapInfo::from(cap)),
                    None => SyscallResult::Err(KernelError::InvalidCapability),
                },
                None => SyscallResult::Err(KernelError::ProcessNotFound),
            },

            Syscall::CapDerive {
                slot,
                new_permissions,
            } => {
                let (result, derive_commits) =
                    self.derive_capability(from_pid, slot, new_permissions, timestamp);
                commits.extend(derive_commits);
                match result {
                    Ok(new_slot) => SyscallResult::Ok(new_slot as u64),
                    Err(e) => SyscallResult::Err(e),
                }
            }

            // === Enhanced IPC syscalls ===
            Syscall::SendWithCaps {
                endpoint_slot,
                tag,
                data,
                cap_slots,
            } => {
                let (result, send_commits) =
                    self.ipc_send_with_caps(from_pid, endpoint_slot, tag, data, &cap_slots, timestamp);
                commits.extend(send_commits);
                match result {
                    Ok(()) => SyscallResult::Ok(0),
                    Err(e) => SyscallResult::Err(e),
                }
            }

            Syscall::Call {
                endpoint_slot,
                tag,
                data,
            } => {
                // Call = send + block for reply
                let (result, commit) = self.ipc_send(from_pid, endpoint_slot, tag, data, timestamp);
                if let Some(c) = commit {
                    commits.push(c);
                }
                match result {
                    Ok(()) => SyscallResult::WouldBlock,
                    Err(e) => SyscallResult::Err(e),
                }
            }

            Syscall::Reply {
                caller_pid,
                tag,
                data,
            } => {
                // Reply sends back to the caller's endpoint
                match self.send_to_process(from_pid, caller_pid, tag, data, timestamp) {
                    Ok(()) => SyscallResult::Ok(0),
                    Err(e) => SyscallResult::Err(e),
                }
            }
        };

        (result, commits)
    }

    /// Derive a capability with reduced permissions (validates via axiom_check).
    ///
    /// Returns (Result<CapSlot, KernelError>, Vec<Commit>).
    pub fn derive_capability(
        &mut self,
        pid: ProcessId,
        slot: CapSlot,
        new_perms: Permissions,
        timestamp: u64,
    ) -> (Result<CapSlot, KernelError>, Vec<Commit>) {
        let mut commits = Vec::new();

        // Get capability space
        let cspace = match self.cap_spaces.get(&pid) {
            Some(cs) => cs,
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        // Use axiom_check to verify capability exists and is not expired
        // No specific permissions required - derive just creates a weaker copy
        let no_perms = Permissions::default();
        let source_cap = match axiom_check(cspace, slot, &no_perms, None, timestamp) {
            Ok(cap) => cap.clone(),
            Err(e) => {
                let err = match e {
                    AxiomError::InvalidSlot => KernelError::InvalidCapability,
                    AxiomError::WrongType => KernelError::InvalidCapability,
                    AxiomError::InsufficientRights => KernelError::PermissionDenied,
                    AxiomError::Expired => KernelError::PermissionDenied,
                    AxiomError::ObjectNotFound => KernelError::InvalidCapability,
                };
                return (Err(err), commits);
            }
        };

        // Attenuate permissions (can only reduce)
        let derived_perms = Permissions {
            read: source_cap.permissions.read && new_perms.read,
            write: source_cap.permissions.write && new_perms.write,
            grant: source_cap.permissions.grant && new_perms.grant,
        };

        // Create new capability with new ID
        let new_cap_id = self.next_cap_id();
        let new_cap = Capability {
            id: new_cap_id,
            object_type: source_cap.object_type,
            object_id: source_cap.object_id,
            permissions: derived_perms,
            generation: source_cap.generation,
            expires_at: source_cap.expires_at,
        };

        // Insert into same process's CSpace
        let new_slot = match self.cap_spaces.get_mut(&pid) {
            Some(cspace) => cspace.insert(new_cap),
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        // Log derivation commit
        commits.push(Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::CapInserted {
                pid: pid.0,
                slot: new_slot,
                cap_id: new_cap_id,
                object_type: source_cap.object_type as u8,
                object_id: source_cap.object_id,
                perms: derived_perms.to_byte(),
            },
            caused_by: None,
        });

        (Ok(new_slot), commits)
    }

    /// Get process info
    pub fn get_process(&self, pid: ProcessId) -> Option<&Process> {
        self.processes.get(&pid)
    }

    /// Get mutable process info
    pub fn get_process_mut(&mut self, pid: ProcessId) -> Option<&mut Process> {
        self.processes.get_mut(&pid)
    }

    /// Get all processes
    pub fn list_processes(&self) -> Vec<(ProcessId, &Process)> {
        self.processes.iter().map(|(&pid, p)| (pid, p)).collect()
    }

    /// List all endpoints with their details
    pub fn list_endpoints(&self) -> Vec<EndpointInfo> {
        self.endpoints
            .iter()
            .map(|(id, ep)| EndpointInfo {
                id: *id,
                owner: ep.owner,
                queue_depth: ep.pending_messages.len(),
            })
            .collect()
    }

    /// Get detailed info about an endpoint
    pub fn get_endpoint(&self, id: EndpointId) -> Option<&Endpoint> {
        self.endpoints.get(&id)
    }

    /// Allocate memory to a process (simulated)
    pub fn allocate_memory(&mut self, pid: ProcessId, bytes: usize) -> Result<usize, KernelError> {
        let proc = self
            .processes
            .get_mut(&pid)
            .ok_or(KernelError::ProcessNotFound)?;
        proc.metrics.memory_size += bytes;
        self.hal.debug_write(&alloc::format!(
            "[kernel] PID {} allocated {} bytes (total: {} bytes)",
            pid.0,
            bytes,
            proc.metrics.memory_size
        ));
        Ok(proc.metrics.memory_size)
    }

    /// Free memory from a process (simulated)
    pub fn free_memory(&mut self, pid: ProcessId, bytes: usize) -> Result<usize, KernelError> {
        let proc = self
            .processes
            .get_mut(&pid)
            .ok_or(KernelError::ProcessNotFound)?;
        proc.metrics.memory_size = proc.metrics.memory_size.saturating_sub(bytes);
        self.hal.debug_write(&alloc::format!(
            "[kernel] PID {} freed {} bytes (total: {} bytes)",
            pid.0,
            bytes,
            proc.metrics.memory_size
        ));
        Ok(proc.metrics.memory_size)
    }

    /// Send a message to a process's first endpoint (for testing/supervisor use).
    ///
    /// **WARNING: This function BYPASSES capability checks.**
    ///
    /// This is a supervisor override intended ONLY for:
    /// - System initialization (before capabilities are set up)
    /// - Testing and debugging
    ///
    /// For normal IPC, use `ipc_send()` which enforces capabilities.
    ///
    /// Note: IPC messages are volatile and not replayed, so no commits are returned.
    /// However, the override IS logged to the IPC traffic log for auditing.
    pub fn send_to_process(
        &mut self,
        from_pid: ProcessId,
        to_pid: ProcessId,
        tag: u32,
        data: Vec<u8>,
        timestamp: u64,
    ) -> Result<(), KernelError> {
        // Log supervisor override warning
        self.hal.debug_write(&alloc::format!(
            "[kernel] SUPERVISOR_OVERRIDE: send_to_process bypassing capabilities (from PID {} to PID {})",
            from_pid.0,
            to_pid.0
        ));

        // Find an endpoint owned by the target process
        let endpoint_id = self
            .endpoints
            .iter()
            .find(|(_, ep)| ep.owner == to_pid)
            .map(|(id, _)| *id)
            .ok_or(KernelError::EndpointNotFound)?;

        let data_len = data.len();

        // Queue the message
        let endpoint = self
            .endpoints
            .get_mut(&endpoint_id)
            .ok_or(KernelError::EndpointNotFound)?;
        let message = Message {
            from: from_pid,
            tag,
            data,
            transferred_caps: vec![],
        };
        endpoint.pending_messages.push_back(message);

        // Update endpoint metrics
        endpoint.metrics.queue_depth = endpoint.pending_messages.len();
        endpoint.metrics.total_messages += 1;
        endpoint.metrics.total_bytes += data_len as u64;
        if endpoint.metrics.queue_depth > endpoint.metrics.queue_high_water {
            endpoint.metrics.queue_high_water = endpoint.metrics.queue_depth;
        }

        // Update sender stats
        if let Some(sender) = self.processes.get_mut(&from_pid) {
            sender.metrics.ipc_sent += 1;
            sender.metrics.ipc_bytes_sent += data_len as u64;
            sender.metrics.last_active_ns = timestamp;
        }

        // Update global IPC count
        self.total_ipc_count += 1;

        // Log to traffic monitor (auditable even for override)
        let entry = IpcTrafficEntry {
            from: from_pid,
            to: to_pid,
            endpoint: endpoint_id,
            tag,
            size: data_len,
            timestamp,
        };
        self.ipc_traffic_log.push_back(entry);
        while self.ipc_traffic_log.len() > MAX_IPC_TRAFFIC_LOG {
            self.ipc_traffic_log.pop_front();
        }

        self.hal.debug_write(&alloc::format!(
            "[kernel] Message sent from PID {} to PID {} (endpoint {}, tag 0x{:x}) [OVERRIDE]",
            from_pid.0,
            to_pid.0,
            endpoint_id.0,
            tag
        ));

        Ok(())
    }

    /// Get capability space for a process
    pub fn get_cap_space(&self, pid: ProcessId) -> Option<&CapabilitySpace> {
        self.cap_spaces.get(&pid)
    }

    /// Get total system memory usage
    pub fn total_memory(&self) -> usize {
        self.processes.values().map(|p| p.metrics.memory_size).sum()
    }

    /// Get total message count in all endpoint queues
    pub fn total_pending_messages(&self) -> usize {
        self.endpoints
            .values()
            .map(|e| e.pending_messages.len())
            .sum()
    }

    /// Get system-wide metrics
    pub fn get_system_metrics(&self, uptime_ns: u64) -> SystemMetrics {
        SystemMetrics {
            process_count: self.processes.len(),
            total_memory: self.total_memory(),
            endpoint_count: self.endpoints.len(),
            total_pending_messages: self.total_pending_messages(),
            total_ipc_messages: self.total_ipc_count,
            uptime_ns,
        }
    }

    /// Get recent IPC traffic for monitoring
    pub fn get_ipc_traffic(&self) -> Vec<IpcTrafficEntry> {
        self.ipc_traffic_log.iter().cloned().collect()
    }

    /// Get recent IPC traffic (last N entries)
    pub fn get_recent_ipc_traffic(&self, count: usize) -> Vec<IpcTrafficEntry> {
        self.ipc_traffic_log
            .iter()
            .rev()
            .take(count)
            .cloned()
            .collect()
    }

    /// Update process memory size (called when WASM memory grows)
    pub fn update_process_memory(&mut self, pid: ProcessId, new_size: usize) {
        if let Some(proc) = self.processes.get_mut(&pid) {
            proc.metrics.memory_size = new_size;
        }
    }

    /// Get detailed endpoint info including metrics
    pub fn get_endpoint_detail(&self, id: EndpointId) -> Option<EndpointDetail> {
        let ep = self.endpoints.get(&id)?;
        let queued_messages: Vec<MessageSummary> = ep
            .pending_messages
            .iter()
            .take(10)
            .map(|m| MessageSummary {
                from: m.from,
                tag: m.tag,
                size: m.data.len(),
            })
            .collect();

        Some(EndpointDetail {
            id,
            owner: ep.owner,
            queue_depth: ep.pending_messages.len(),
            metrics: ep.metrics.clone(),
            queued_messages,
        })
    }
}

/// Detailed info about an endpoint
#[derive(Clone, Debug)]
pub struct EndpointDetail {
    pub id: EndpointId,
    pub owner: ProcessId,
    pub queue_depth: usize,
    pub metrics: EndpointMetrics,
    pub queued_messages: Vec<MessageSummary>,
}

/// Summary of a queued message
#[derive(Clone, Debug)]
pub struct MessageSummary {
    pub from: ProcessId,
    pub tag: u32,
    pub size: usize,
}

/// Summary info about an endpoint
#[derive(Clone, Debug)]
pub struct EndpointInfo {
    pub id: EndpointId,
    pub owner: ProcessId,
    pub queue_depth: usize,
}

// ============================================================================
// Kernel Implementation - Thin Wrapper
// ============================================================================

impl<H: HAL> Kernel<H> {
    /// Create a new kernel with the given HAL
    pub fn new(hal: H) -> Self {
        let boot_time = hal.now_nanos();
        Self {
            core: KernelCore::new(hal),
            axiom: AxiomGateway::new(boot_time),
            boot_time,
            console_output_buffer: VecDeque::new(),
        }
    }

    /// Get the HAL
    pub fn hal(&self) -> &H {
        self.core.hal()
    }

    /// Get uptime in nanoseconds
    pub fn uptime_nanos(&self) -> u64 {
        self.core.hal().now_nanos() - self.boot_time
    }

    /// Get the SysLog (syscall audit trail)
    pub fn syslog(&self) -> &SysLog {
        self.axiom.syslog()
    }

    /// Get the CommitLog (state mutations for replay)
    pub fn commitlog(&self) -> &CommitLog {
        self.axiom.commitlog()
    }

    // ========================================================================
    // Read-only accessors (delegate to core)
    // ========================================================================

    /// Get process info
    pub fn get_process(&self, pid: ProcessId) -> Option<&Process> {
        self.core.get_process(pid)
    }

    /// Get all processes
    pub fn list_processes(&self) -> Vec<(ProcessId, &Process)> {
        self.core.list_processes()
    }

    /// List all endpoints
    pub fn list_endpoints(&self) -> Vec<EndpointInfo> {
        self.core.list_endpoints()
    }

    /// Get endpoint info
    pub fn get_endpoint(&self, id: EndpointId) -> Option<&Endpoint> {
        self.core.get_endpoint(id)
    }

    /// Get capability space for a process
    pub fn get_cap_space(&self, pid: ProcessId) -> Option<&CapabilitySpace> {
        self.core.get_cap_space(pid)
    }

    /// Get total system memory usage
    pub fn total_memory(&self) -> usize {
        self.core.total_memory()
    }

    /// Get total pending messages
    pub fn total_pending_messages(&self) -> usize {
        self.core.total_pending_messages()
    }

    /// Get system-wide metrics
    pub fn get_system_metrics(&self) -> SystemMetrics {
        self.core.get_system_metrics(self.uptime_nanos())
    }

    /// Get IPC traffic log
    pub fn get_ipc_traffic(&self) -> Vec<IpcTrafficEntry> {
        self.core.get_ipc_traffic()
    }

    /// Get recent IPC traffic
    pub fn get_recent_ipc_traffic(&self, count: usize) -> Vec<IpcTrafficEntry> {
        self.core.get_recent_ipc_traffic(count)
    }

    /// Get detailed endpoint info
    pub fn get_endpoint_detail(&self, id: EndpointId) -> Option<EndpointDetail> {
        self.core.get_endpoint_detail(id)
    }

    // ========================================================================
    // Mutation methods - route through AxiomGateway
    // ========================================================================

    /// Register a process and log the mutation.
    pub fn register_process(&mut self, name: &str) -> ProcessId {
        let timestamp = self.uptime_nanos();
        let (pid, commits) = self.core.register_process(name, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
        pid
    }

    /// Register a process with a specific PID (used for supervisor and special processes).
    pub fn register_process_with_pid(&mut self, pid: ProcessId, name: &str) -> ProcessId {
        let timestamp = self.uptime_nanos();
        let (actual_pid, commits) = self.core.register_process_with_pid(pid, name, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
        actual_pid
    }

    /// Kill a process and log the mutations.
    pub fn kill_process(&mut self, pid: ProcessId) {
        let timestamp = self.uptime_nanos();
        let commits = self.core.kill_process(pid, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
    }

    /// Record a process fault and terminate it.
    ///
    /// Records a `ProcessFaulted` commit before terminating the process.
    /// Use this instead of `kill_process` when a process crashes or faults.
    ///
    /// # Arguments
    /// - `pid`: Process ID to fault
    /// - `reason`: Fault reason code (see KernelCore::fault_process)
    /// - `description`: Human-readable fault description
    pub fn fault_process(&mut self, pid: ProcessId, reason: u32, description: String) {
        let timestamp = self.uptime_nanos();
        let commits = self.core.fault_process(pid, reason, description, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
    }

    /// Create an endpoint and log the mutations.
    pub fn create_endpoint(&mut self, owner: ProcessId) -> Result<(EndpointId, CapSlot), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.create_endpoint(owner, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
        result
    }

    /// Grant a capability and log the mutations.
    pub fn grant_capability(
        &mut self,
        from_pid: ProcessId,
        from_slot: CapSlot,
        to_pid: ProcessId,
        new_perms: Permissions,
    ) -> Result<CapSlot, KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.grant_capability(from_pid, from_slot, to_pid, new_perms, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
        result
    }

    /// Grant a capability to a specific endpoint directly (used for initial setup).
    ///
    /// This is used when setting up the capability graph during process spawn,
    /// allowing the supervisor to grant capabilities to endpoints it owns.
    pub fn grant_capability_to_endpoint(
        &mut self,
        owner_pid: ProcessId,
        endpoint_id: EndpointId,
        to_pid: ProcessId,
        perms: Permissions,
    ) -> Result<CapSlot, KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.grant_capability_to_endpoint(owner_pid, endpoint_id, to_pid, perms, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
        result
    }

    /// Send a message to a process (SUPERVISOR OVERRIDE - bypasses capability checks).
    ///
    /// **DEPRECATED**: Use `ipc_send()` for normal IPC which enforces capabilities.
    /// This function bypasses capability checks and should only be used for:
    /// - System initialization
    /// - Testing and debugging
    ///
    /// All calls are logged to SysLog for auditing (syscall 0xFFFF = SUPERVISOR_OVERRIDE).
    #[deprecated(
        since = "0.2.0",
        note = "Use ipc_send() which enforces capability checks. This method bypasses the capability system."
    )]
    pub fn send_to_process(
        &mut self,
        from_pid: ProcessId,
        to_pid: ProcessId,
        tag: u32,
        data: Vec<u8>,
    ) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();

        // Log supervisor override to SysLog for auditing
        // Use a special "syscall" number (0xFFFF) to indicate supervisor override
        let args = [to_pid.0 as u32, tag, data.len() as u32, 0];
        let req_id = self.axiom.syslog_mut().log_request(from_pid.0, 0xFFFF, args, timestamp);
        
        let result = self.core.send_to_process(from_pid, to_pid, tag, data, timestamp);
        
        // Log the result
        let result_code = match &result {
            Ok(()) => 0,
            Err(_) => -1,
        };
        self.axiom.syslog_mut().log_response(from_pid.0, req_id, result_code, timestamp);
        
        result
    }

    /// Allocate memory to a process.
    pub fn allocate_memory(&mut self, pid: ProcessId, bytes: usize) -> Result<usize, KernelError> {
        self.core.allocate_memory(pid, bytes)
    }

    /// Free memory from a process.
    pub fn free_memory(&mut self, pid: ProcessId, bytes: usize) -> Result<usize, KernelError> {
        self.core.free_memory(pid, bytes)
    }

    /// Update process memory size.
    pub fn update_process_memory(&mut self, pid: ProcessId, new_size: usize) {
        self.core.update_process_memory(pid, new_size)
    }

    /// IPC send with audit logging.
    ///
    /// Sends a message and logs a `MessageSent` commit for audit trail.
    pub fn ipc_send(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
    ) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commit) = self.core.ipc_send(from_pid, endpoint_slot, tag, data, timestamp);
        if let Some(c) = commit {
            self.axiom.append_internal_commit(c.commit_type, timestamp);
        }
        result
    }

    /// IPC receive.
    pub fn ipc_receive(
        &mut self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
    ) -> Result<Option<Message>, KernelError> {
        let timestamp = self.uptime_nanos();
        self.core.ipc_receive(pid, endpoint_slot, timestamp)
    }

    /// IPC send with capabilities and log mutations.
    pub fn ipc_send_with_caps(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
        cap_slots: &[CapSlot],
    ) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.ipc_send_with_caps(from_pid, endpoint_slot, tag, data, cap_slots, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
        result
    }

    /// IPC receive with capabilities and log mutations.
    pub fn ipc_receive_with_caps(
        &mut self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
    ) -> Result<Option<(Message, Vec<CapSlot>)>, KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.ipc_receive_with_caps(pid, endpoint_slot, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
        result
    }

    /// Handle syscall and log mutations.
    pub fn handle_syscall(&mut self, from_pid: ProcessId, syscall: Syscall) -> SyscallResult {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.handle_syscall(from_pid, syscall, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
        result
    }

    /// Revoke capability and log the mutation.
    pub fn revoke_capability(&mut self, pid: ProcessId, slot: CapSlot) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.revoke_capability(pid, slot, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
        result
    }

    /// Delete capability and log the mutation.
    pub fn delete_capability(&mut self, pid: ProcessId, slot: CapSlot) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.delete_capability(pid, slot, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
        result
    }

    /// Delete a capability and return information for notification.
    ///
    /// This method is used by the supervisor when revoking capabilities externally.
    /// It captures the capability info before deletion so a notification message
    /// can be delivered to the affected process.
    ///
    /// # Arguments
    /// - `pid`: Process whose capability to delete
    /// - `slot`: Capability slot to delete
    /// - `reason`: Revocation reason code (see REVOKE_REASON_* constants)
    ///
    /// # Returns
    /// - `Ok(RevokeNotification)`: Deletion succeeded, notification info captured
    /// - `Err(KernelError)`: Deletion failed
    pub fn delete_capability_with_notification(
        &mut self,
        pid: ProcessId,
        slot: CapSlot,
        reason: u8,
    ) -> Result<RevokeNotification, KernelError> {
        // Get cap info before deletion (for the notification payload)
        let cap_info = self
            .get_cap_space(pid)
            .and_then(|cs| cs.get(slot))
            .map(|cap| (cap.object_type as u8, cap.object_id));

        // Perform the deletion
        self.delete_capability(pid, slot)?;

        // Build notification if cap existed
        if let Some((object_type, object_id)) = cap_info {
            Ok(RevokeNotification {
                pid,
                slot,
                object_type,
                object_id,
                reason,
            })
        } else {
            // This shouldn't happen since delete_capability succeeded,
            // but return an empty notification as fallback
            Ok(RevokeNotification::empty())
        }
    }

    /// Derive capability and log the mutation.
    pub fn derive_capability(
        &mut self,
        pid: ProcessId,
        slot: CapSlot,
        new_perms: Permissions,
    ) -> Result<CapSlot, KernelError> {
        let timestamp = self.uptime_nanos();
        let (result, commits) = self.core.derive_capability(pid, slot, new_perms, timestamp);
        for commit in commits {
            self.axiom.append_internal_commit(commit.commit_type, timestamp);
        }
        result
    }

    // ========================================================================
    // Privileged Supervisor APIs (WASM-only, PID 0)
    // ========================================================================
    //
    // These methods are for the supervisor (PID 0) to use in the WASM/browser
    // environment. The supervisor is the trusted boundary layer and does NOT
    // use IPC endpoints - it uses these privileged kernel methods instead.
    //
    // All operations are logged to SysLog with PID 0 for auditing.

    /// Drain all pending console output entries.
    ///
    /// Called by the supervisor after poll_syscalls() to get console output
    /// from processes that called SYS_CONSOLE_WRITE.
    ///
    /// Returns a vector of (ProcessId, text) pairs.
    pub fn drain_console_output(&mut self) -> Vec<(ProcessId, Vec<u8>)> {
        self.console_output_buffer
            .drain(..)
            .map(|entry| (entry.from, entry.text))
            .collect()
    }

    /// Deliver console input to a process (PRIVILEGED - supervisor only).
    ///
    /// This is a privileged operation for the supervisor (PID 0) to deliver
    /// keyboard input to a terminal process. The operation is logged to SysLog
    /// with PID 0 as the sender.
    ///
    /// # Arguments
    /// - `target_pid`: The process to receive the input
    /// - `endpoint_slot`: The target process's input endpoint slot
    /// - `data`: The input data (typically keyboard text)
    ///
    /// # Returns
    /// - `Ok(())`: Input was delivered successfully
    /// - `Err(KernelError)`: Delivery failed
    pub fn deliver_console_input(
        &mut self,
        target_pid: ProcessId,
        endpoint_slot: CapSlot,
        data: &[u8],
    ) -> Result<(), KernelError> {
        let timestamp = self.uptime_nanos();
        let supervisor_pid = ProcessId(0);

        // Log to SysLog with PID 0 (supervisor operation)
        // Use a special syscall number 0xF0 = SUPERVISOR_CONSOLE_INPUT
        let args = [target_pid.0 as u32, endpoint_slot, MSG_CONSOLE_INPUT, data.len() as u32];
        let req_id = self.axiom.syslog_mut().log_request(
            supervisor_pid.0,
            0xF0, // SUPERVISOR_CONSOLE_INPUT
            args,
            timestamp,
        );

        // Find the endpoint and deliver the message directly
        // (bypasses capability check since this is a privileged operation)
        let result = self.deliver_to_endpoint_internal(
            supervisor_pid,
            target_pid,
            endpoint_slot,
            MSG_CONSOLE_INPUT,
            data,
            timestamp,
        );

        // Log the result
        let result_code = match &result {
            Ok(()) => 0,
            Err(_) => -1,
        };
        self.axiom.syslog_mut().log_response(supervisor_pid.0, req_id, result_code, timestamp);

        result
    }

    /// Internal: Deliver a message to an endpoint (privileged, no capability check).
    ///
    /// This is used by supervisor-initiated operations.
    fn deliver_to_endpoint_internal(
        &mut self,
        from_pid: ProcessId,
        target_pid: ProcessId,
        endpoint_slot: CapSlot,
        tag: u32,
        data: &[u8],
        timestamp: u64,
    ) -> Result<(), KernelError> {
        // Get the target process's capability space
        let cspace = self.core.cap_spaces.get(&target_pid)
            .ok_or(KernelError::ProcessNotFound)?;

        // Get the capability at the specified slot
        let cap = cspace.get(endpoint_slot)
            .ok_or(KernelError::InvalidCapability)?;

        // Verify it's an endpoint capability
        if cap.object_type != ObjectType::Endpoint {
            return Err(KernelError::InvalidCapability);
        }

        let endpoint_id = EndpointId(cap.object_id);

        // Get the endpoint
        let endpoint = self.core.endpoints.get_mut(&endpoint_id)
            .ok_or(KernelError::EndpointNotFound)?;

        // Create and queue the message
        let message = Message {
            from: from_pid,
            tag,
            data: data.to_vec(),
            transferred_caps: Vec::new(),
        };

        endpoint.pending_messages.push_back(message);
        endpoint.metrics.queue_depth = endpoint.pending_messages.len();
        endpoint.metrics.total_messages += 1;
        endpoint.metrics.total_bytes += data.len() as u64;
        if endpoint.pending_messages.len() > endpoint.metrics.queue_high_water {
            endpoint.metrics.queue_high_water = endpoint.pending_messages.len();
        }

        // Update sender metrics
        if let Some(proc) = self.core.processes.get_mut(&from_pid) {
            proc.metrics.ipc_sent += 1;
            proc.metrics.ipc_bytes_sent += data.len() as u64;
            proc.metrics.last_active_ns = timestamp;
        }

        Ok(())
    }

    /// Deliver a capability revocation notification to a process (PRIVILEGED - supervisor only).
    ///
    /// This is a privileged operation for the supervisor (PID 0) to notify a process
    /// that one of its capabilities has been revoked. The notification is delivered
    /// to the process's input endpoint (slot 1) with the MSG_CAP_REVOKED tag.
    ///
    /// # Arguments
    /// - `notif`: The revocation notification containing cap info and reason
    ///
    /// # Returns
    /// - `Ok(())`: Notification was delivered successfully
    /// - `Err(KernelError)`: Delivery failed (e.g., no input endpoint)
    pub fn deliver_revoke_notification(
        &mut self,
        notif: &RevokeNotification,
    ) -> Result<(), KernelError> {
        if !notif.is_valid() {
            return Ok(()); // Nothing to deliver
        }

        let timestamp = self.uptime_nanos();
        let supervisor_pid = ProcessId(0);

        // Build payload: [slot: u32, object_type: u8, object_id: u64, reason: u8]
        let mut payload = alloc::vec::Vec::with_capacity(14);
        payload.extend_from_slice(&notif.slot.to_le_bytes());
        payload.push(notif.object_type);
        payload.extend_from_slice(&notif.object_id.to_le_bytes());
        payload.push(notif.reason);

        // Log to SysLog with PID 0 (supervisor operation)
        // Use a special syscall number 0xF1 = SUPERVISOR_CAP_REVOKE_NOTIFY
        let args = [notif.pid.0 as u32, notif.slot, MSG_CAP_REVOKED, payload.len() as u32];
        let req_id = self.axiom.syslog_mut().log_request(
            supervisor_pid.0,
            0xF1, // SUPERVISOR_CAP_REVOKE_NOTIFY
            args,
            timestamp,
        );

        // Terminal's input endpoint is at slot 1
        const INPUT_ENDPOINT_SLOT: CapSlot = 1;

        // Deliver to process's input endpoint
        let result = self.deliver_to_endpoint_internal(
            supervisor_pid,
            notif.pid,
            INPUT_ENDPOINT_SLOT,
            MSG_CAP_REVOKED,
            &payload,
            timestamp,
        );

        // Log the result
        let result_code = match &result {
            Ok(()) => 0,
            Err(_) => -1,
        };
        self.axiom.syslog_mut().log_response(supervisor_pid.0, req_id, result_code, timestamp);

        result
    }

    /// Log a syscall request to the SysLog.
    ///
    /// **DEPRECATED**: Use `execute_raw_syscall()` which routes through AxiomGateway.
    /// This method bypasses the gateway pattern. Keeping for migration compatibility.
    #[deprecated(note = "Use execute_raw_syscall() which routes through AxiomGateway")]
    pub fn log_syscall_request(&mut self, pid: ProcessId, syscall_num: u32, args: [u32; 4]) -> u64 {
        let timestamp = self.uptime_nanos();
        self.axiom.syslog_mut().log_request(pid.0, syscall_num, args, timestamp)
    }

    /// Log a syscall response to the SysLog.
    ///
    /// **DEPRECATED**: Use `execute_raw_syscall()` which routes through AxiomGateway.
    /// This method bypasses the gateway pattern. Keeping for migration compatibility.
    #[deprecated(note = "Use execute_raw_syscall() which routes through AxiomGateway")]
    pub fn log_syscall_response(&mut self, pid: ProcessId, request_id: u64, result: i64) {
        let timestamp = self.uptime_nanos();
        self.axiom.syslog_mut().log_response(pid.0, request_id, result, timestamp);
    }

    // ========================================================================
    // Axiom Gateway Entry Point
    // ========================================================================

    /// Execute a raw syscall through the Axiom gateway protocol.
    ///
    /// This is the proper entry point for syscalls from processes. It implements
    /// the AxiomGateway protocol:
    /// 1. Syscall request is logged to SysLog
    /// 2. Kernel operation is executed  
    /// 3. State mutations are recorded to CommitLog
    /// 4. Syscall response is logged to SysLog
    ///
    /// # Arguments
    /// - `sender`: Process ID making the syscall
    /// - `syscall_num`: Raw syscall number
    /// - `args`: Syscall arguments [arg0, arg1, arg2, arg3]
    /// - `data`: Additional data payload (e.g., message bytes for IPC)
    ///
    /// # Returns
    /// - `(i64, SyscallResult, Vec<u8>)`: (result code, rich result, response data)
    ///
    /// # Syscall Numbers
    /// - 0x00: NOP - No operation
    /// - 0x01: SYS_DEBUG - Debug output (handled by supervisor)
    /// - 0x02: SYS_GET_TIME - Get uptime (low/high word based on arg0)
    /// - 0x03: SYS_GET_PID - Get caller's PID
    /// - 0x04: SYS_LIST_CAPS - List caller's capabilities
    /// - 0x05: SYS_LIST_PROCS - List all processes
    /// - 0x06: SYS_GET_WALLCLOCK - Get wall clock time
    /// - 0x11: SYS_EXIT - Exit process
    /// - 0x12: SYS_YIELD - Yield execution
    /// - 0x30: SYS_CAP_GRANT - Grant capability to another process
    /// - 0x31: SYS_CAP_REVOKE - Revoke a capability
    /// - 0x35: SYS_EP_CREATE - Create endpoint
    /// - 0x40: SYS_SEND - Send IPC message
    /// - 0x41: SYS_RECEIVE - Receive IPC message
    pub fn execute_raw_syscall(
        &mut self,
        sender: ProcessId,
        syscall_num: u32,
        args: [u32; 4],
        data: &[u8],
    ) -> (i64, SyscallResult, Vec<u8>) {
        let timestamp = self.uptime_nanos();

        // Determine if this syscall should be logged based on syscall type
        // Read-only / high-frequency syscalls skip logging for performance
        let should_log = match syscall_num {
            0x00 => false, // NOP
            0x02 => false, // SYS_GET_TIME
            0x03 => false, // SYS_GET_PID
            0x04 => false, // SYS_LIST_CAPS
            0x05 => false, // SYS_LIST_PROCS
            0x06 => false, // SYS_GET_WALLCLOCK
            0x12 => false, // SYS_YIELD
            0x41 => false, // SYS_RECEIVE (polling)
            _ => true,
        };

        if should_log {
            // Implement Axiom gateway protocol manually to avoid borrow issues:
            // 1. Log syscall request to SysLog
            let request_id = self.axiom.syslog_mut().log_request(
                sender.0,
                syscall_num,
                args,
                timestamp,
            );

            // 2. Execute kernel operation
            let (result, commit_types) =
                self.execute_syscall_kernel_fn(sender, syscall_num, args, data, timestamp);

            // 3. Append commits to CommitLog
            for ct in commit_types {
                self.axiom.commitlog_mut().append(ct, Some(request_id), timestamp);
            }

            // 4. Log syscall response to SysLog
            self.axiom.syslog_mut().log_response(
                sender.0,
                request_id,
                result,
                timestamp,
            );

            // Get the rich result for the supervisor to use
            let (rich_result, response_data) =
                self.get_syscall_rich_result(sender, syscall_num, args, data, result);

            (result, rich_result, response_data)
        } else {
            // Fast path for read-only syscalls - no gateway logging
            let (result, _commits) =
                self.execute_syscall_kernel_fn(sender, syscall_num, args, data, timestamp);
            let (rich_result, response_data) =
                self.get_syscall_rich_result(sender, syscall_num, args, data, result);
            (result, rich_result, response_data)
        }
    }

    /// Internal: Execute syscall kernel operation and return (result, commits).
    ///
    /// This is the kernel function passed to AxiomGateway.syscall().
    fn execute_syscall_kernel_fn(
        &mut self,
        sender: ProcessId,
        syscall_num: u32,
        args: [u32; 4],
        data: &[u8],
        timestamp: u64,
    ) -> (i64, Vec<CommitType>) {
        match syscall_num {
            // NOP
            0x00 => (0, Vec::new()),

            // SYS_DEBUG - Just returns 0, supervisor handles the message
            0x01 => (0, Vec::new()),

            // SYS_GET_TIME
            0x02 => {
                let nanos = self.uptime_nanos();
                let result = if args[0] == 0 {
                    (nanos & 0xFFFFFFFF) as i64
                } else {
                    ((nanos >> 32) & 0xFFFFFFFF) as i64
                };
                (result, Vec::new())
            }

            // SYS_GET_PID
            0x03 => (sender.0 as i64, Vec::new()),

            // SYS_LIST_CAPS
            0x04 => {
                // Just returns 0, data is written separately
                (0, Vec::new())
            }

            // SYS_LIST_PROCS
            0x05 => {
                // Just returns 0, data is written separately
                (0, Vec::new())
            }

            // SYS_GET_WALLCLOCK
            0x06 => {
                let millis = self.core.hal().wallclock_ms();
                let result = if args[0] == 0 {
                    (millis & 0xFFFFFFFF) as i64
                } else {
                    ((millis >> 32) & 0xFFFFFFFF) as i64
                };
                (result, Vec::new())
            }

            // SYS_CONSOLE_WRITE - Write to console output
            // The text is placed in the console_output_buffer for the supervisor to drain
            0x07 => {
                let text = data.to_vec();
                self.console_output_buffer.push_back(ConsoleOutputEntry {
                    from: sender,
                    text,
                    timestamp,
                });
                // Trim buffer if too large (keep last 100 entries)
                while self.console_output_buffer.len() > 100 {
                    self.console_output_buffer.pop_front();
                }
                (0, Vec::new())
            }

            // SYS_EXIT
            0x11 => {
                let commits = self.core.kill_process(sender, timestamp);
                let commit_types: Vec<CommitType> =
                    commits.into_iter().map(|c| c.commit_type).collect();
                (0, commit_types)
            }

            // SYS_YIELD
            0x12 => (0, Vec::new()),

            // SYS_CAP_GRANT - Grant capability to another process
            0x30 => {
                let from_slot = args[0];
                let to_pid = ProcessId(args[1] as u64);
                let perms = Permissions::from_byte(args[2] as u8);

                match self
                    .core
                    .grant_capability(sender, from_slot, to_pid, perms, timestamp)
                {
                    (Ok(new_slot), commits) => {
                        let commit_types: Vec<CommitType> =
                            commits.into_iter().map(|c| c.commit_type).collect();
                        (new_slot as i64, commit_types)
                    }
                    (Err(_), _) => (-1, Vec::new()),
                }
            }

            // SYS_CAP_REVOKE - Revoke a capability
            0x31 => {
                let target_pid = ProcessId(args[0] as u64);
                let slot = args[1];

                match self.core.delete_capability(target_pid, slot, timestamp) {
                    (Ok(()), commits) => {
                        let commit_types: Vec<CommitType> =
                            commits.into_iter().map(|c| c.commit_type).collect();
                        (0, commit_types)
                    }
                    (Err(_), _) => (-1, Vec::new()),
                }
            }

            // SYS_EP_CREATE
            0x35 => {
                let (result, commits) = self.core.create_endpoint(sender, timestamp);
                let commit_types: Vec<CommitType> =
                    commits.into_iter().map(|c| c.commit_type).collect();
                match result {
                    Ok((eid, _slot)) => (eid.0 as i64, commit_types),
                    Err(_) => (-1, commit_types),
                }
            }

            // SYS_SEND
            0x40 => {
                let slot = args[0];
                let tag = args[1];
                let (result, commit) =
                    self.core
                        .ipc_send(sender, slot, tag, data.to_vec(), timestamp);
                let commit_types: Vec<CommitType> = commit
                    .into_iter()
                    .map(|c| c.commit_type)
                    .collect();
                match result {
                    Ok(()) => (0, commit_types),
                    Err(_) => (-1, commit_types),
                }
            }

            // SYS_RECEIVE
            // Note: We use ipc_has_message here to check for messages WITHOUT popping.
            // The actual message retrieval happens in get_syscall_rich_result().
            0x41 => {
                let slot = args[0];
                match self.core.ipc_has_message(sender, slot, timestamp) {
                    Ok(true) => (1, Vec::new()),  // Message available
                    Ok(false) => (0, Vec::new()), // No message (WouldBlock)
                    Err(_) => (-1, Vec::new()),
                }
            }

            // Unknown syscall
            _ => (-1, Vec::new()),
        }
    }

    /// Get rich result and response data for a syscall.
    ///
    /// Some syscalls need to return data (like SYS_LIST_CAPS) which can't
    /// fit in the i64 result. This method provides both the rich result
    /// and any additional response data.
    fn get_syscall_rich_result(
        &mut self,
        sender: ProcessId,
        syscall_num: u32,
        args: [u32; 4],
        _data: &[u8],
        result: i64,
    ) -> (SyscallResult, Vec<u8>) {
        match syscall_num {
            // SYS_LIST_CAPS
            0x04 => {
                let syscall = Syscall::ListCaps;
                let timestamp = self.uptime_nanos();
                let (rich_result, _) = self.core.handle_syscall(sender, syscall, timestamp);
                if let SyscallResult::CapList(ref caps) = rich_result {
                    let mut bytes = Vec::new();
                    bytes.extend_from_slice(&(caps.len() as u32).to_le_bytes());
                    for (slot, cap) in caps {
                        bytes.extend_from_slice(&slot.to_le_bytes());
                        bytes.push(cap.object_type as u8);
                        bytes.extend_from_slice(&cap.object_id.to_le_bytes());
                    }
                    (rich_result, bytes)
                } else {
                    (SyscallResult::Ok(result as u64), Vec::new())
                }
            }

            // SYS_LIST_PROCS
            0x05 => {
                let syscall = Syscall::ListProcesses;
                let timestamp = self.uptime_nanos();
                let (rich_result, _) = self.core.handle_syscall(sender, syscall, timestamp);
                if let SyscallResult::ProcessList(ref procs) = rich_result {
                    let mut bytes = Vec::new();
                    bytes.extend_from_slice(&(procs.len() as u32).to_le_bytes());
                    for (proc_pid, name, _state) in procs {
                        bytes.extend_from_slice(&(proc_pid.0 as u32).to_le_bytes());
                        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
                        bytes.extend_from_slice(name.as_bytes());
                    }
                    (rich_result, bytes)
                } else {
                    (SyscallResult::Ok(result as u64), Vec::new())
                }
            }

            // SYS_RECEIVE
            0x41 => {
                if result == 1 {
                    // Message was received - get it again for the data
                    let slot = args[0];
                    let timestamp = self.uptime_nanos();
                    match self.core.ipc_receive(sender, slot, timestamp) {
                        Ok(Some(msg)) => {
                            let mut msg_bytes = Vec::new();
                            msg_bytes.extend_from_slice(&(msg.from.0 as u32).to_le_bytes());
                            msg_bytes.extend_from_slice(&msg.tag.to_le_bytes());
                            msg_bytes.extend_from_slice(&msg.data);
                            (SyscallResult::Message(msg), msg_bytes)
                        }
                        _ => (SyscallResult::Ok(result as u64), Vec::new()),
                    }
                } else if result == 0 {
                    (SyscallResult::WouldBlock, Vec::new())
                } else {
                    (SyscallResult::Err(KernelError::PermissionDenied), Vec::new())
                }
            }

            // Default: wrap result in SyscallResult::Ok
            _ => {
                if result >= 0 {
                    (SyscallResult::Ok(result as u64), Vec::new())
                } else {
                    (SyscallResult::Err(KernelError::PermissionDenied), Vec::new())
                }
            }
        }
    }
}

impl<H: HAL + Default> Kernel<H> {
    /// Create a kernel for replay mode.
    pub fn new_for_replay() -> Self {
        let hal = H::default();
        Self {
            core: KernelCore::new(hal),
            axiom: AxiomGateway::new(0),
            boot_time: 0,
            console_output_buffer: VecDeque::new(),
        }
    }
}

// ============================================================================
// Deterministic Replay Implementation
// ============================================================================

impl<H: HAL> Replayable for Kernel<H> {
    fn replay_genesis(&mut self) -> ReplayResult<()> {
        // Genesis is implicit - kernel starts in genesis state
        // Nothing to do here
        Ok(())
    }

    fn replay_create_process(&mut self, pid: u64, _parent: u64, name: String) -> ReplayResult<()> {
        let process = Process {
            pid: ProcessId(pid),
            name,
            state: ProcessState::Running,
            metrics: ProcessMetrics::default(),
        };
        self.core.processes.insert(ProcessId(pid), process);
        self.core.cap_spaces
            .insert(ProcessId(pid), CapabilitySpace::new());

        // Update next_pid if needed to avoid collisions
        if pid >= self.core.next_pid {
            self.core.next_pid = pid + 1;
        }

        Ok(())
    }

    fn replay_exit_process(&mut self, pid: u64, _code: i32) -> ReplayResult<()> {
        let process = self.core
            .processes
            .get_mut(&ProcessId(pid))
            .ok_or(ReplayError::ProcessNotFound(pid))?;
        process.state = ProcessState::Zombie;
        Ok(())
    }

    fn replay_process_faulted(
        &mut self,
        pid: u64,
        _reason: u32,
        _description: String,
    ) -> ReplayResult<()> {
        // Mark the process as faulted (transitions to Zombie state)
        let process = self.core
            .processes
            .get_mut(&ProcessId(pid))
            .ok_or(ReplayError::ProcessNotFound(pid))?;
        process.state = ProcessState::Zombie;
        Ok(())
    }

    fn replay_insert_capability(
        &mut self,
        pid: u64,
        slot: u32,
        cap_id: u64,
        object_type: u8,
        object_id: u64,
        perms: u8,
    ) -> ReplayResult<()> {
        let obj_type = match object_type {
            1 => ObjectType::Endpoint,
            2 => ObjectType::Process,
            3 => ObjectType::Memory,
            4 => ObjectType::Irq,
            5 => ObjectType::IoPort,
            6 => ObjectType::Console,
            _ => return Err(ReplayError::UnknownObjectType(object_type)),
        };

        let cap = Capability {
            id: cap_id,
            object_type: obj_type,
            object_id,
            permissions: Permissions::from_byte(perms),
            generation: 0,
            expires_at: 0,
        };

        let cspace = self.core
            .cap_spaces
            .get_mut(&ProcessId(pid))
            .ok_or(ReplayError::ProcessNotFound(pid))?;

        // Insert at specific slot
        cspace.slots.insert(slot, cap);

        // Update next_slot if needed
        if slot >= cspace.next_slot {
            cspace.next_slot = slot + 1;
        }

        // Update next_cap_id if needed
        if cap_id >= self.core.next_cap_id {
            self.core.next_cap_id = cap_id + 1;
        }

        Ok(())
    }

    fn replay_remove_capability(&mut self, pid: u64, slot: u32) -> ReplayResult<()> {
        let cspace = self.core
            .cap_spaces
            .get_mut(&ProcessId(pid))
            .ok_or(ReplayError::ProcessNotFound(pid))?;
        cspace.slots.remove(&slot);
        Ok(())
    }

    fn replay_cap_granted(
        &mut self,
        _from_pid: u64,
        _to_pid: u64,
        _from_slot: u32,
        _to_slot: u32,
        new_cap_id: u64,
        _perms: orbital_axiom::Permissions,
    ) -> ReplayResult<()> {
        // CapGranted is followed by a CapInserted commit for the receiver.
        // The actual capability insertion is handled by replay_insert_capability.
        // This method just records the grant relationship and updates counters.

        // Update next_cap_id if needed
        if new_cap_id >= self.core.next_cap_id {
            self.core.next_cap_id = new_cap_id + 1;
        }

        Ok(())
    }

    fn replay_create_endpoint(&mut self, id: u64, owner: u64) -> ReplayResult<()> {
        if !self.core.processes.contains_key(&ProcessId(owner)) {
            return Err(ReplayError::ProcessNotFound(owner));
        }

        let endpoint = Endpoint {
            id: EndpointId(id),
            owner: ProcessId(owner),
            pending_messages: VecDeque::new(),
            metrics: EndpointMetrics::default(),
        };
        self.core.endpoints.insert(EndpointId(id), endpoint);

        // Update next_endpoint_id if needed
        if id >= self.core.next_endpoint_id {
            self.core.next_endpoint_id = id + 1;
        }

        Ok(())
    }

    fn replay_destroy_endpoint(&mut self, id: u64) -> ReplayResult<()> {
        self.core.endpoints.remove(&EndpointId(id));
        Ok(())
    }

    fn replay_message_sent(
        &mut self,
        _from_pid: u64,
        _to_endpoint: u64,
        _tag: u32,
        _size: usize,
    ) -> ReplayResult<()> {
        // MessageSent commits are for audit trail verification only.
        // The actual message content is volatile and not replayed.
        // During replay, we just record that a message was sent for
        // consistency with the original execution.
        //
        // Note: We don't update metrics here because metrics are
        // non-deterministic and excluded from state_hash().
        Ok(())
    }

    fn state_hash(&self) -> [u8; 32] {
        let mut hasher = StateHasher::new();

        // Hash processes (BTreeMap is sorted by key, so order is deterministic)
        hasher.write_u64(self.core.processes.len() as u64);
        for (pid, proc) in &self.core.processes {
            hasher.write_u64(pid.0);
            hasher.write_str(&proc.name);
            hasher.write_u8(match proc.state {
                ProcessState::Running => 0,
                ProcessState::Blocked => 1,
                ProcessState::Zombie => 2,
            });
            // Note: We don't hash metrics as they are non-deterministic
        }

        // Hash capability spaces (sorted by PID)
        hasher.write_u64(self.core.cap_spaces.len() as u64);
        for (pid, cspace) in &self.core.cap_spaces {
            hasher.write_u64(pid.0);
            hasher.write_u64(cspace.slots.len() as u64);
            // BTreeMap is sorted by key
            for (slot, cap) in &cspace.slots {
                hasher.write_u32(*slot);
                hasher.write_u64(cap.id);
                hasher.write_u8(cap.object_type as u8);
                hasher.write_u64(cap.object_id);
                hasher.write_u8(cap.permissions.to_byte());
                hasher.write_u32(cap.generation);
                hasher.write_u64(cap.expires_at);
            }
        }

        // Hash endpoints (sorted by ID)
        hasher.write_u64(self.core.endpoints.len() as u64);
        for (id, ep) in &self.core.endpoints {
            hasher.write_u64(id.0);
            hasher.write_u64(ep.owner.0);
            // Note: We don't hash pending_messages or metrics as they are volatile
        }

        hasher.finalize()
    }
}

// ============================================================================
// Mock HAL for Testing
// ============================================================================

#[cfg(test)]
mod mock_hal {
    use alloc::collections::BTreeMap;
    use alloc::string::String;
    use alloc::vec::Vec;
    use core::cell::RefCell;
    use core::sync::atomic::{AtomicU64, Ordering};
    use orbital_hal::{HalError, NumericProcessHandle, HAL};

    /// Simulated process state
    struct MockProcess {
        name: String,
        alive: bool,
        memory_size: usize,
        pending_messages: Vec<Vec<u8>>,
    }

    /// Mock HAL for unit testing
    ///
    /// Provides simulated process spawning, time, memory, and message passing
    /// for testing kernel logic without a real platform.
    pub struct MockHal {
        time: AtomicU64,
        wallclock: AtomicU64,
        debug_log: RefCell<Vec<String>>,
        random_seed: AtomicU64,
        next_pid: AtomicU64,
        processes: RefCell<BTreeMap<u64, MockProcess>>,
        incoming_messages: RefCell<Vec<(NumericProcessHandle, Vec<u8>)>>,
    }

    impl MockHal {
        pub fn new() -> Self {
            Self {
                time: AtomicU64::new(0),
                wallclock: AtomicU64::new(1737504000000), // Jan 22, 2025 00:00:00 UTC
                debug_log: RefCell::new(Vec::new()),
                random_seed: AtomicU64::new(12345),
                next_pid: AtomicU64::new(1),
                processes: RefCell::new(BTreeMap::new()),
                incoming_messages: RefCell::new(Vec::new()),
            }
        }

        pub fn with_time(nanos: u64) -> Self {
            Self {
                time: AtomicU64::new(nanos),
                wallclock: AtomicU64::new(1737504000000),
                debug_log: RefCell::new(Vec::new()),
                random_seed: AtomicU64::new(12345),
                next_pid: AtomicU64::new(1),
                processes: RefCell::new(BTreeMap::new()),
                incoming_messages: RefCell::new(Vec::new()),
            }
        }
    }

    impl Default for MockHal {
        fn default() -> Self {
            Self::new()
        }
    }

    // MockHal is Send + Sync because it uses atomic operations and RefCell
    // is only accessed in single-threaded test contexts
    unsafe impl Send for MockHal {}
    unsafe impl Sync for MockHal {}

    impl HAL for MockHal {
        type ProcessHandle = NumericProcessHandle;

        fn spawn_process(&self, name: &str, _binary: &[u8]) -> Result<Self::ProcessHandle, HalError> {
            let pid = self.next_pid.fetch_add(1, Ordering::SeqCst);
            let handle = NumericProcessHandle::new(pid);

            let process = MockProcess {
                name: String::from(name),
                alive: true,
                memory_size: 65536,
                pending_messages: Vec::new(),
            };

            self.processes.borrow_mut().insert(pid, process);
            self.debug_log.borrow_mut().push(alloc::format!(
                "[mock-hal] Spawned process '{}' with PID {}",
                name, pid
            ));

            Ok(handle)
        }

        fn kill_process(&self, handle: &Self::ProcessHandle) -> Result<(), HalError> {
            let mut processes = self.processes.borrow_mut();
            if let Some(proc) = processes.get_mut(&handle.id()) {
                if proc.alive {
                    proc.alive = false;
                    self.debug_log.borrow_mut().push(alloc::format!(
                        "[mock-hal] Killed process PID {}",
                        handle.id()
                    ));
                    Ok(())
                } else {
                    Err(HalError::ProcessNotFound)
                }
            } else {
                Err(HalError::ProcessNotFound)
            }
        }

        fn send_to_process(&self, handle: &Self::ProcessHandle, msg: &[u8]) -> Result<(), HalError> {
            let mut processes = self.processes.borrow_mut();
            if let Some(proc) = processes.get_mut(&handle.id()) {
                if proc.alive {
                    proc.pending_messages.push(msg.to_vec());
                    Ok(())
                } else {
                    Err(HalError::ProcessNotFound)
                }
            } else {
                Err(HalError::ProcessNotFound)
            }
        }

        fn is_process_alive(&self, handle: &Self::ProcessHandle) -> bool {
            self.processes
                .borrow()
                .get(&handle.id())
                .map(|p| p.alive)
                .unwrap_or(false)
        }

        fn get_process_memory_size(&self, handle: &Self::ProcessHandle) -> Result<usize, HalError> {
            self.processes
                .borrow()
                .get(&handle.id())
                .filter(|p| p.alive)
                .map(|p| p.memory_size)
                .ok_or(HalError::ProcessNotFound)
        }

        fn allocate(&self, size: usize, _align: usize) -> Result<*mut u8, HalError> {
            let layout =
                core::alloc::Layout::from_size_align(size, 8).map_err(|_| HalError::InvalidArgument)?;
            let ptr = unsafe { alloc::alloc::alloc(layout) };
            if ptr.is_null() {
                Err(HalError::OutOfMemory)
            } else {
                Ok(ptr)
            }
        }

        fn deallocate(&self, ptr: *mut u8, size: usize, _align: usize) {
            if !ptr.is_null() {
                let layout = core::alloc::Layout::from_size_align(size, 8).unwrap();
                unsafe { alloc::alloc::dealloc(ptr, layout) };
            }
        }

        fn now_nanos(&self) -> u64 {
            self.time.load(Ordering::SeqCst)
        }

        fn wallclock_ms(&self) -> u64 {
            self.wallclock.load(Ordering::SeqCst)
        }

        fn random_bytes(&self, buf: &mut [u8]) -> Result<(), HalError> {
            let mut seed = self.random_seed.load(Ordering::SeqCst);
            for byte in buf.iter_mut() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                *byte = (seed >> 33) as u8;
            }
            self.random_seed.store(seed, Ordering::SeqCst);
            Ok(())
        }

        fn debug_write(&self, msg: &str) {
            self.debug_log.borrow_mut().push(String::from(msg));
        }

        fn poll_messages(&self) -> Vec<(Self::ProcessHandle, Vec<u8>)> {
            let mut messages = self.incoming_messages.borrow_mut();
            messages.drain(..).collect()
        }
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use super::mock_hal::MockHal;

    #[test]
    fn test_kernel_creation() {
        let hal = MockHal::new();
        let kernel = Kernel::new(hal);

        assert_eq!(kernel.list_processes().len(), 0);
        assert_eq!(kernel.list_endpoints().len(), 0);
    }

    #[test]
    fn test_process_registration() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid1 = kernel.register_process("init");
        let pid2 = kernel.register_process("terminal");

        assert_eq!(pid1, ProcessId(1));
        assert_eq!(pid2, ProcessId(2));
        assert_eq!(kernel.list_processes().len(), 2);

        let proc = kernel.get_process(pid1).expect("process should exist");
        assert_eq!(proc.name, "init");
        assert_eq!(proc.state, ProcessState::Running);
    }

    #[test]
    fn test_process_kill() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");
        assert!(kernel.get_process(pid).is_some());

        kernel.kill_process(pid);
        assert!(kernel.get_process(pid).is_none());
    }

    #[test]
    fn test_process_fault() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("crashy");
        assert!(kernel.get_process(pid).is_some());

        let commit_count_before = kernel.commitlog().len();

        // Fault the process
        kernel.fault_process(pid, 4, String::from("panic: assertion failed"));

        let commit_count_after = kernel.commitlog().len();

        // Process should be removed
        assert!(kernel.get_process(pid).is_none());

        // Should have ProcessFaulted + ProcessExited commits
        assert!(commit_count_after > commit_count_before);

        // Verify ProcessFaulted commit exists
        let commits = kernel.commitlog().commits();
        let has_fault = commits.iter().any(|c| {
            matches!(&c.commit_type, CommitType::ProcessFaulted { pid: fpid, reason, description }
                if *fpid == pid.0 && *reason == 4 && description.contains("panic"))
        });
        assert!(has_fault, "Must have ProcessFaulted commit");

        // Verify ProcessExited commit exists
        let has_exit = commits.iter().any(|c| {
            matches!(&c.commit_type, CommitType::ProcessExited { pid: epid, .. } if *epid == pid.0)
        });
        assert!(has_exit, "Must have ProcessExited commit after fault");
    }

    #[test]
    fn test_endpoint_creation() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");
        let (eid, slot) = kernel
            .create_endpoint(pid)
            .expect("endpoint creation should succeed");

        assert_eq!(eid, EndpointId(1));
        assert_eq!(slot, 0);

        let endpoints = kernel.list_endpoints();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].owner, pid);
    }

    #[test]
    fn test_endpoint_creation_requires_process() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let result = kernel.create_endpoint(ProcessId(999));
        assert!(matches!(result, Err(KernelError::ProcessNotFound)));
    }

    #[test]
    fn test_capability_grant() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid1 = kernel.register_process("owner");
        let pid2 = kernel.register_process("recipient");

        // Create endpoint (owner gets full capability in slot 0)
        let (eid, owner_slot) = kernel.create_endpoint(pid1).unwrap();

        // Grant capability from pid1 to pid2 with reduced permissions
        let recipient_slot = kernel
            .grant_capability(
                pid1,
                owner_slot,
                pid2,
                Permissions {
                    read: true,
                    write: true,
                    grant: false,
                },
            )
            .expect("grant should succeed");

        // Verify recipient got the capability
        let cap_space = kernel.get_cap_space(pid2).expect("cap space should exist");
        let cap = cap_space
            .get(recipient_slot)
            .expect("capability should exist");

        assert_eq!(cap.object_type, ObjectType::Endpoint);
        assert_eq!(cap.object_id, eid.0);
        assert!(cap.permissions.read);
        assert!(cap.permissions.write);
        assert!(!cap.permissions.grant); // Attenuated
    }

    #[test]
    fn test_capability_grant_requires_grant_permission() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid1 = kernel.register_process("owner");
        let pid2 = kernel.register_process("middleman");
        let pid3 = kernel.register_process("recipient");

        // Create endpoint
        kernel.create_endpoint(pid1).unwrap();

        // Grant to pid2 without grant permission
        let middleman_slot = kernel
            .grant_capability(
                pid1,
                0,
                pid2,
                Permissions {
                    read: true,
                    write: true,
                    grant: false,
                },
            )
            .unwrap();

        // pid2 should not be able to grant further (no grant permission)
        let result = kernel.grant_capability(
            pid2,
            middleman_slot,
            pid3,
            Permissions {
                read: true,
                write: false,
                grant: false,
            },
        );

        assert!(matches!(result, Err(KernelError::PermissionDenied)));
    }

    #[test]
    fn test_ipc_send_receive() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let sender_pid = kernel.register_process("sender");
        let receiver_pid = kernel.register_process("receiver");

        // Create endpoint owned by receiver
        let (_, receiver_slot) = kernel.create_endpoint(receiver_pid).unwrap();

        // Grant send capability to sender (write-only)
        let sender_slot = kernel
            .grant_capability(
                receiver_pid,
                receiver_slot,
                sender_pid,
                Permissions {
                    read: false,
                    write: true,
                    grant: false,
                },
            )
            .unwrap();

        // Send message
        let data = b"hello world".to_vec();
        kernel
            .ipc_send(sender_pid, sender_slot, 42, data.clone())
            .expect("send should succeed");

        // Verify message is queued
        let ep = kernel
            .get_endpoint(EndpointId(1))
            .expect("endpoint should exist");
        assert_eq!(ep.pending_messages.len(), 1);

        // Receive message
        let msg = kernel
            .ipc_receive(receiver_pid, receiver_slot)
            .expect("receive should succeed")
            .expect("message should be present");

        assert_eq!(msg.from, sender_pid);
        assert_eq!(msg.tag, 42);
        assert_eq!(msg.data, b"hello world");

        // Queue should now be empty
        let ep = kernel
            .get_endpoint(EndpointId(1))
            .expect("endpoint should exist");
        assert_eq!(ep.pending_messages.len(), 0);
    }

    #[test]
    fn test_ipc_requires_capability() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid1 = kernel.register_process("proc1");
        let pid2 = kernel.register_process("proc2");

        // Create endpoint owned by pid1
        kernel.create_endpoint(pid1).unwrap();

        // pid2 tries to send without capability - should fail
        let result = kernel.ipc_send(pid2, 0, 0, vec![]);
        assert!(matches!(result, Err(KernelError::InvalidCapability)));
    }

    #[test]
    fn test_ipc_requires_write_permission() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let owner = kernel.register_process("owner");
        let reader = kernel.register_process("reader");

        // Create endpoint
        kernel.create_endpoint(owner).unwrap();

        // Grant read-only capability
        let reader_slot = kernel
            .grant_capability(
                owner,
                0,
                reader,
                Permissions {
                    read: true,
                    write: false,
                    grant: false,
                },
            )
            .unwrap();

        // Try to send with read-only capability - should fail
        let result = kernel.ipc_send(reader, reader_slot, 0, vec![]);
        assert!(matches!(result, Err(KernelError::PermissionDenied)));
    }

    #[test]
    fn test_ipc_metrics() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let sender = kernel.register_process("sender");
        let receiver = kernel.register_process("receiver");

        // Create endpoint and grant capability
        kernel.create_endpoint(receiver).unwrap();
        let sender_slot = kernel
            .grant_capability(
                receiver,
                0,
                sender,
                Permissions {
                    read: false,
                    write: true,
                    grant: false,
                },
            )
            .unwrap();

        // Send several messages
        for i in 0..5 {
            kernel
                .ipc_send(sender, sender_slot, i, vec![0u8; 100])
                .unwrap();
        }

        // Check sender metrics
        let sender_proc = kernel.get_process(sender).unwrap();
        assert_eq!(sender_proc.metrics.ipc_sent, 5);
        assert_eq!(sender_proc.metrics.ipc_bytes_sent, 500);

        // Check system metrics
        let sys = kernel.get_system_metrics();
        assert_eq!(sys.total_ipc_messages, 5);
        assert_eq!(sys.total_pending_messages, 5);

        // Receive messages and check receiver metrics
        for _ in 0..5 {
            kernel.ipc_receive(receiver, 0).unwrap();
        }

        let receiver_proc = kernel.get_process(receiver).unwrap();
        assert_eq!(receiver_proc.metrics.ipc_received, 5);
        assert_eq!(receiver_proc.metrics.ipc_bytes_received, 500);
    }

    #[test]
    fn test_memory_allocation() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");
        let initial = kernel.get_process(pid).unwrap().metrics.memory_size;

        // Allocate memory
        let new_total = kernel.allocate_memory(pid, 65536).unwrap();
        assert_eq!(new_total, initial + 65536);

        // Free memory
        let after_free = kernel.free_memory(pid, 32768).unwrap();
        assert_eq!(after_free, initial + 65536 - 32768);
    }

    #[test]
    fn test_syscall_dispatch() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");

        // Test GetTime syscall
        let result = kernel.handle_syscall(pid, Syscall::GetTime);
        if let SyscallResult::Ok(time) = result {
            assert!(time >= 0);
        } else {
            panic!("GetTime should return Ok");
        }

        // Test ListProcesses syscall
        let result = kernel.handle_syscall(pid, Syscall::ListProcesses);
        if let SyscallResult::ProcessList(procs) = result {
            assert_eq!(procs.len(), 1);
            assert_eq!(procs[0].0, pid);
        } else {
            panic!("ListProcesses should return ProcessList");
        }

        // Verify syscall count incremented
        let proc = kernel.get_process(pid).unwrap();
        assert_eq!(proc.metrics.syscall_count, 2);
    }

    #[test]
    fn test_process_cleanup_removes_endpoints() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");
        kernel.create_endpoint(pid).unwrap();
        kernel.create_endpoint(pid).unwrap();

        assert_eq!(kernel.list_endpoints().len(), 2);

        kernel.kill_process(pid);

        assert_eq!(kernel.list_endpoints().len(), 0);
    }

    #[test]
    fn test_capability_revoke() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("owner");

        // Create endpoint (owner gets full capability in slot 0)
        let (_eid, slot) = kernel.create_endpoint(pid).unwrap();

        // Verify capability exists
        let cap_space = kernel.get_cap_space(pid).unwrap();
        assert!(cap_space.get(slot).is_some());

        // Revoke the capability
        kernel
            .revoke_capability(pid, slot)
            .expect("revoke should succeed");

        // Verify capability is gone
        let cap_space = kernel.get_cap_space(pid).unwrap();
        assert!(cap_space.get(slot).is_none());

        // Verify CommitLog contains the revoke operation (CapRemoved)
        let commits = kernel.commitlog().commits();
        // Should have Genesis, ProcessCreated, EndpointCreated, CapInserted, CapRemoved
        assert!(commits.len() >= 5);
        assert!(matches!(
            &commits[commits.len() - 1].commit_type,
            CommitType::CapRemoved { .. }
        ));
    }

    #[test]
    fn test_capability_revoke_requires_grant_permission() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let owner = kernel.register_process("owner");
        let holder = kernel.register_process("holder");

        // Create endpoint
        kernel.create_endpoint(owner).unwrap();

        // Grant to holder without grant permission
        let holder_slot = kernel
            .grant_capability(
                owner,
                0,
                holder,
                Permissions {
                    read: true,
                    write: true,
                    grant: false,
                },
            )
            .unwrap();

        // Holder cannot revoke (no grant permission)
        let result = kernel.revoke_capability(holder, holder_slot);
        assert!(matches!(result, Err(KernelError::PermissionDenied)));
    }

    #[test]
    fn test_capability_delete() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let owner = kernel.register_process("owner");
        let holder = kernel.register_process("holder");

        // Create endpoint
        kernel.create_endpoint(owner).unwrap();

        // Grant to holder without grant permission
        let holder_slot = kernel
            .grant_capability(
                owner,
                0,
                holder,
                Permissions {
                    read: true,
                    write: true,
                    grant: false,
                },
            )
            .unwrap();

        // Holder can delete their own capability (no grant permission required)
        kernel
            .delete_capability(holder, holder_slot)
            .expect("delete should succeed");

        // Verify capability is gone
        let cap_space = kernel.get_cap_space(holder).unwrap();
        assert!(cap_space.get(holder_slot).is_none());

        // Verify CommitLog contains the delete operation (CapRemoved)
        let commits = kernel.commitlog().commits();
        // Should have Genesis, ProcessCreated x2, EndpointCreated, CapInserted, CapGranted, CapRemoved
        assert!(commits.len() >= 7);
        assert!(matches!(
            &commits[commits.len() - 1].commit_type,
            CommitType::CapRemoved { .. }
        ));
    }

    #[test]
    fn test_capability_delete_invalid_slot() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");

        // Try to delete non-existent capability
        let result = kernel.delete_capability(pid, 999);
        assert!(matches!(result, Err(KernelError::InvalidCapability)));
    }

    #[test]
    fn test_ipc_traffic_log() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let sender = kernel.register_process("sender");
        let receiver = kernel.register_process("receiver");

        kernel.create_endpoint(receiver).unwrap();
        let sender_slot = kernel
            .grant_capability(
                receiver,
                0,
                sender,
                Permissions {
                    read: false,
                    write: true,
                    grant: false,
                },
            )
            .unwrap();

        // Send messages
        kernel
            .ipc_send(sender, sender_slot, 0x1234, vec![0u8; 64])
            .unwrap();
        kernel
            .ipc_send(sender, sender_slot, 0x5678, vec![0u8; 128])
            .unwrap();

        // Check traffic log
        let traffic = kernel.get_recent_ipc_traffic(10);
        assert_eq!(traffic.len(), 2);
        assert_eq!(traffic[0].from, sender);
        assert_eq!(traffic[0].to, receiver);
        assert_eq!(traffic[0].size, 128); // Most recent first
        assert_eq!(traffic[0].tag, 0x5678);
    }

    #[test]
    fn test_system_metrics() {
        let hal = MockHal::with_time(1_000_000_000); // Start at 1 second
        let mut kernel = Kernel::new(hal);

        kernel.register_process("p1");
        kernel.register_process("p2");
        kernel.create_endpoint(ProcessId(1)).unwrap();
        kernel.create_endpoint(ProcessId(2)).unwrap();
        kernel.create_endpoint(ProcessId(2)).unwrap();

        let metrics = kernel.get_system_metrics();

        assert_eq!(metrics.process_count, 2);
        assert_eq!(metrics.endpoint_count, 3);
        assert!(metrics.total_memory > 0);
        assert_eq!(metrics.total_pending_messages, 0);
        assert_eq!(metrics.total_ipc_messages, 0);
    }

    // ========================================================================
    // Axiom Module Tests
    // ========================================================================

    #[test]
    fn test_axiom_check_valid_capability() {
        let mut cspace = CapabilitySpace::new();
        let cap = Capability {
            id: 1,
            object_type: ObjectType::Endpoint,
            object_id: 42,
            permissions: Permissions::full(),
            generation: 0,
            expires_at: 0,
        };
        let slot = cspace.insert(cap);

        let result = axiom_check(
            &cspace,
            slot,
            &Permissions::read_only(),
            Some(ObjectType::Endpoint),
            0,
        );

        assert!(result.is_ok());
        let cap = result.unwrap();
        assert_eq!(cap.object_id, 42);
    }

    #[test]
    fn test_axiom_check_invalid_slot() {
        let cspace = CapabilitySpace::new();

        let result = axiom_check(
            &cspace,
            999, // Invalid slot
            &Permissions::read_only(),
            None,
            0,
        );

        assert!(matches!(result, Err(AxiomError::InvalidSlot)));
    }

    #[test]
    fn test_axiom_check_wrong_type() {
        let mut cspace = CapabilitySpace::new();
        let cap = Capability {
            id: 1,
            object_type: ObjectType::Endpoint,
            object_id: 42,
            permissions: Permissions::full(),
            generation: 0,
            expires_at: 0,
        };
        let slot = cspace.insert(cap);

        let result = axiom_check(
            &cspace,
            slot,
            &Permissions::read_only(),
            Some(ObjectType::Process), // Wrong type
            0,
        );

        assert!(matches!(result, Err(AxiomError::WrongType)));
    }

    #[test]
    fn test_axiom_check_insufficient_permissions() {
        let mut cspace = CapabilitySpace::new();
        let cap = Capability {
            id: 1,
            object_type: ObjectType::Endpoint,
            object_id: 42,
            permissions: Permissions::read_only(), // Only read
            generation: 0,
            expires_at: 0,
        };
        let slot = cspace.insert(cap);

        // Require write
        let result = axiom_check(&cspace, slot, &Permissions::write_only(), None, 0);

        assert!(matches!(result, Err(AxiomError::InsufficientRights)));
    }

    #[test]
    fn test_axiom_check_expired_capability() {
        let mut cspace = CapabilitySpace::new();
        let cap = Capability {
            id: 1,
            object_type: ObjectType::Endpoint,
            object_id: 42,
            permissions: Permissions::full(),
            generation: 0,
            expires_at: 1000, // Expires at time 1000
        };
        let slot = cspace.insert(cap);

        // Check at time 2000 (after expiration)
        let result = axiom_check(&cspace, slot, &Permissions::read_only(), None, 2000);

        assert!(matches!(result, Err(AxiomError::Expired)));
    }

    #[test]
    fn test_axiom_check_non_expiring_capability() {
        let mut cspace = CapabilitySpace::new();
        let cap = Capability {
            id: 1,
            object_type: ObjectType::Endpoint,
            object_id: 42,
            permissions: Permissions::full(),
            generation: 0,
            expires_at: 0, // Never expires
        };
        let slot = cspace.insert(cap);

        // Check at very large time
        let result = axiom_check(&cspace, slot, &Permissions::read_only(), None, u64::MAX);

        assert!(result.is_ok());
    }

    // ========================================================================
    // Integration Tests - Capability Flow
    // ========================================================================

    #[test]
    fn test_capability_grant_chain() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let alice = kernel.register_process("alice");
        let bob = kernel.register_process("bob");
        let charlie = kernel.register_process("charlie");

        // Alice creates endpoint
        let (_, alice_slot) = kernel.create_endpoint(alice).unwrap();

        // Alice grants to Bob with full permissions
        let bob_slot = kernel
            .grant_capability(alice, alice_slot, bob, Permissions::full())
            .unwrap();

        // Bob grants to Charlie with reduced permissions (no grant)
        let charlie_slot = kernel
            .grant_capability(
                bob,
                bob_slot,
                charlie,
                Permissions {
                    read: true,
                    write: true,
                    grant: false,
                },
            )
            .unwrap();

        // Verify Charlie's capability
        let charlie_cap = kernel
            .get_cap_space(charlie)
            .unwrap()
            .get(charlie_slot)
            .unwrap();
        assert!(charlie_cap.permissions.read);
        assert!(charlie_cap.permissions.write);
        assert!(!charlie_cap.permissions.grant);

        // Charlie cannot grant further (no grant permission)
        let dave = kernel.register_process("dave");
        let result = kernel.grant_capability(charlie, charlie_slot, dave, Permissions::read_only());
        assert!(matches!(result, Err(KernelError::PermissionDenied)));
    }

    #[test]
    fn test_capability_ipc_with_transfer() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let sender = kernel.register_process("sender");
        let receiver = kernel.register_process("receiver");

        // Both create endpoints
        let (_, sender_ep) = kernel.create_endpoint(sender).unwrap();
        let (_, receiver_ep) = kernel.create_endpoint(receiver).unwrap();

        // Receiver grants send capability to sender
        let send_cap = kernel
            .grant_capability(receiver, receiver_ep, sender, Permissions::write_only())
            .unwrap();

        // Sender sends message with its own endpoint capability attached
        kernel
            .ipc_send_with_caps(
                sender,
                send_cap,
                0x1234,
                b"hello with cap".to_vec(),
                &[sender_ep],
            )
            .expect("send with caps should succeed");

        // Verify sender no longer has the endpoint capability
        assert!(kernel
            .get_cap_space(sender)
            .unwrap()
            .get(sender_ep)
            .is_none());

        // Receiver receives message with transferred capability
        let (msg, installed_slots) = kernel
            .ipc_receive_with_caps(receiver, receiver_ep)
            .expect("receive should succeed")
            .expect("message should exist");

        assert_eq!(msg.tag, 0x1234);
        assert_eq!(msg.data, b"hello with cap");
        assert_eq!(installed_slots.len(), 1);

        // Receiver now has the sender's endpoint capability
        let received_cap = kernel
            .get_cap_space(receiver)
            .unwrap()
            .get(installed_slots[0])
            .unwrap();
        assert_eq!(received_cap.object_type, ObjectType::Endpoint);
    }

    #[test]
    fn test_capability_derive() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");
        let (_, slot) = kernel.create_endpoint(pid).unwrap();

        // Derive a read-only capability
        let derived_slot = kernel
            .derive_capability(pid, slot, Permissions::read_only())
            .expect("derive should succeed");

        // Verify original still has full permissions
        let orig_cap = kernel.get_cap_space(pid).unwrap().get(slot).unwrap();
        assert!(orig_cap.permissions.grant);

        // Verify derived has reduced permissions
        let derived_cap = kernel
            .get_cap_space(pid)
            .unwrap()
            .get(derived_slot)
            .unwrap();
        assert!(derived_cap.permissions.read);
        assert!(!derived_cap.permissions.write);
        assert!(!derived_cap.permissions.grant);

        // Both reference the same object
        assert_eq!(orig_cap.object_id, derived_cap.object_id);
    }

    #[test]
    fn test_commitlog_full_workflow() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let alice = kernel.register_process("alice");
        let bob = kernel.register_process("bob");

        // Create endpoint (logs EndpointCreated + CapInserted)
        let (_, alice_slot) = kernel.create_endpoint(alice).unwrap();

        // Grant to Bob (logs CapGranted)
        let bob_slot = kernel
            .grant_capability(
                alice,
                alice_slot,
                bob,
                Permissions {
                    read: true,
                    write: true,
                    grant: false,
                },
            )
            .unwrap();

        // Bob deletes their capability (logs CapRemoved)
        kernel.delete_capability(bob, bob_slot).unwrap();

        // Alice revokes her capability (logs CapRemoved)
        kernel.revoke_capability(alice, alice_slot).unwrap();

        // Verify CommitLog contents
        let commits = kernel.commitlog().commits();

        // Should have: Genesis, ProcessCreated x2, EndpointCreated, CapInserted, CapGranted, CapRemoved x2
        assert!(commits.len() >= 8);

        // Verify first commit is Genesis
        assert!(matches!(&commits[0].commit_type, CommitType::Genesis));

        // Verify process creation
        assert!(matches!(
            &commits[1].commit_type,
            CommitType::ProcessCreated { .. }
        ));
        assert!(matches!(
            &commits[2].commit_type,
            CommitType::ProcessCreated { .. }
        ));

        // Verify endpoint creation
        assert!(matches!(
            &commits[3].commit_type,
            CommitType::EndpointCreated { .. }
        ));

        // Verify log integrity
        assert!(kernel.commitlog().verify_integrity());
    }

    #[test]
    fn test_syscall_cap_inspect() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");
        let (eid, slot) = kernel.create_endpoint(pid).unwrap();

        // Inspect the capability via syscall
        let result = kernel.handle_syscall(pid, Syscall::CapInspect { slot });

        match result {
            SyscallResult::CapInfo(info) => {
                assert_eq!(info.object_type, ObjectType::Endpoint);
                assert_eq!(info.object_id, eid.0);
                assert!(info.permissions.read);
                assert!(info.permissions.write);
                assert!(info.permissions.grant);
            }
            _ => panic!("Expected CapInfo result"),
        }
    }

    #[test]
    fn test_syscall_cap_derive() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");
        let (_, slot) = kernel.create_endpoint(pid).unwrap();

        // Derive via syscall
        let result = kernel.handle_syscall(
            pid,
            Syscall::CapDerive {
                slot,
                new_permissions: Permissions::read_only(),
            },
        );

        match result {
            SyscallResult::Ok(new_slot) => {
                let cap = kernel
                    .get_cap_space(pid)
                    .unwrap()
                    .get(new_slot as u32)
                    .unwrap();
                assert!(cap.permissions.read);
                assert!(!cap.permissions.write);
            }
            _ => panic!("Expected Ok result with new slot"),
        }
    }

    // ========================================================================
    // Deterministic Replay Tests
    // ========================================================================

    #[test]
    fn test_replay_empty_commitlog() {
        // Replay just genesis
        let hal = MockHal::new();
        let kernel = Kernel::new(hal);
        let commits = kernel.commitlog().commits().to_vec();

        // Create fresh kernel for replay
        let mut replay_kernel: Kernel<MockHal> = Kernel::new_for_replay();
        replay(&mut replay_kernel, &commits).expect("replay should succeed");

        // Should have no processes or endpoints
        assert_eq!(replay_kernel.list_processes().len(), 0);
        assert_eq!(replay_kernel.list_endpoints().len(), 0);
    }

    #[test]
    fn test_replay_single_process() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("init");
        let commits = kernel.commitlog().commits().to_vec();

        // Replay
        let mut replay_kernel: Kernel<MockHal> = Kernel::new_for_replay();
        replay(&mut replay_kernel, &commits).expect("replay should succeed");

        // Should have the process
        assert_eq!(replay_kernel.list_processes().len(), 1);
        let proc = replay_kernel
            .get_process(pid)
            .expect("process should exist");
        assert_eq!(proc.name, "init");
        assert_eq!(proc.state, ProcessState::Running);
    }

    #[test]
    fn test_replay_multiple_processes() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        kernel.register_process("init");
        kernel.register_process("terminal");
        kernel.register_process("idle");
        let commits = kernel.commitlog().commits().to_vec();

        // Replay
        let mut replay_kernel: Kernel<MockHal> = Kernel::new_for_replay();
        replay(&mut replay_kernel, &commits).expect("replay should succeed");

        assert_eq!(replay_kernel.list_processes().len(), 3);
    }

    #[test]
    fn test_replay_endpoint_creation() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");
        let (eid, _slot) = kernel.create_endpoint(pid).unwrap();
        let commits = kernel.commitlog().commits().to_vec();

        // Replay
        let mut replay_kernel: Kernel<MockHal> = Kernel::new_for_replay();
        replay(&mut replay_kernel, &commits).expect("replay should succeed");

        // Should have the endpoint
        assert_eq!(replay_kernel.list_endpoints().len(), 1);
        let ep = replay_kernel
            .get_endpoint(eid)
            .expect("endpoint should exist");
        assert_eq!(ep.owner, pid);
    }

    #[test]
    fn test_replay_capability_lifecycle() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let owner = kernel.register_process("owner");
        let recipient = kernel.register_process("recipient");

        // Create endpoint and grant capability
        let (_, owner_slot) = kernel.create_endpoint(owner).unwrap();
        let recipient_slot = kernel
            .grant_capability(
                owner,
                owner_slot,
                recipient,
                Permissions {
                    read: true,
                    write: true,
                    grant: false,
                },
            )
            .unwrap();

        // Recipient deletes their capability
        kernel.delete_capability(recipient, recipient_slot).unwrap();

        let commits = kernel.commitlog().commits().to_vec();

        // Replay
        let mut replay_kernel: Kernel<MockHal> = Kernel::new_for_replay();
        replay(&mut replay_kernel, &commits).expect("replay should succeed");

        // Owner should still have capability
        let owner_cspace = replay_kernel.get_cap_space(owner).unwrap();
        assert!(owner_cspace.get(owner_slot).is_some());

        // Recipient's capability should be gone
        let recipient_cspace = replay_kernel.get_cap_space(recipient).unwrap();
        assert!(recipient_cspace.get(recipient_slot).is_none());
    }

    #[test]
    fn test_replay_determinism_single() {
        // Run system, record commits, replay, verify identical state hash
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid1 = kernel.register_process("init");
        let (_, slot) = kernel.create_endpoint(pid1).unwrap();
        let pid2 = kernel.register_process("terminal");
        kernel
            .grant_capability(pid1, slot, pid2, Permissions::read_only())
            .unwrap();

        // Get state hash and commits
        let hash1 = kernel.state_hash();
        let commits = kernel.commitlog().commits().to_vec();

        // Replay
        let mut replay_kernel: Kernel<MockHal> = Kernel::new_for_replay();
        replay(&mut replay_kernel, &commits).expect("replay should succeed");
        let hash2 = replay_kernel.state_hash();

        // Hashes must match
        assert_eq!(hash1, hash2, "Replay must produce identical state hash");
    }

    #[test]
    fn test_replay_determinism_multiple() {
        // Create commits from a more complex workflow
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        // Complex workflow
        let alice = kernel.register_process("alice");
        let bob = kernel.register_process("bob");
        let charlie = kernel.register_process("charlie");

        let (ep1, slot1) = kernel.create_endpoint(alice).unwrap();
        let (ep2, slot2) = kernel.create_endpoint(bob).unwrap();

        kernel
            .grant_capability(alice, slot1, bob, Permissions::full())
            .unwrap();
        kernel
            .grant_capability(bob, slot2, charlie, Permissions::read_only())
            .unwrap();

        let derived_slot = kernel
            .derive_capability(alice, slot1, Permissions::write_only())
            .unwrap();
        kernel.delete_capability(alice, derived_slot).unwrap();

        let hash1 = kernel.state_hash();
        let commits = kernel.commitlog().commits().to_vec();

        // Replay 10 times
        let hashes: Vec<[u8; 32]> = (0..10)
            .map(|_| {
                let mut replay_kernel: Kernel<MockHal> = Kernel::new_for_replay();
                replay(&mut replay_kernel, &commits).expect("replay should succeed");
                replay_kernel.state_hash()
            })
            .collect();

        // All hashes must match the original
        for (i, hash) in hashes.iter().enumerate() {
            assert_eq!(*hash, hash1, "Replay {} must produce identical hash", i);
        }
    }

    #[test]
    fn test_replay_process_exit() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");
        kernel.handle_syscall(pid, Syscall::Exit { code: 42 });

        let commits = kernel.commitlog().commits().to_vec();

        // Replay
        let mut replay_kernel: Kernel<MockHal> = Kernel::new_for_replay();
        replay(&mut replay_kernel, &commits).expect("replay should succeed");

        // Process should be zombie
        let proc = replay_kernel
            .get_process(pid)
            .expect("process should exist");
        assert_eq!(proc.state, ProcessState::Zombie);
    }

    #[test]
    fn test_replay_and_verify_success() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        kernel.register_process("init");
        kernel.register_process("terminal");

        let expected_hash = kernel.state_hash();
        let commits = kernel.commitlog().commits().to_vec();

        // Replay and verify
        let mut replay_kernel: Kernel<MockHal> = Kernel::new_for_replay();
        let result = replay_and_verify(&mut replay_kernel, &commits, expected_hash);

        assert!(result.is_ok(), "replay_and_verify should succeed");
    }

    #[test]
    fn test_replay_and_verify_hash_mismatch() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        kernel.register_process("init");

        let commits = kernel.commitlog().commits().to_vec();

        // Use wrong expected hash
        let wrong_hash = [0xFFu8; 32];

        let mut replay_kernel: Kernel<MockHal> = Kernel::new_for_replay();
        let result = replay_and_verify(&mut replay_kernel, &commits, wrong_hash);

        match result {
            Err(ReplayError::HashMismatch { expected, actual }) => {
                assert_eq!(expected, wrong_hash);
                assert_ne!(actual, wrong_hash);
            }
            _ => panic!("Expected HashMismatch error"),
        }
    }

    #[test]
    fn test_state_hash_consistency() {
        // Calling state_hash multiple times should return the same result
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        kernel.register_process("test");
        kernel.create_endpoint(ProcessId(1)).unwrap();

        let hash1 = kernel.state_hash();
        let hash2 = kernel.state_hash();
        let hash3 = kernel.state_hash();

        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);
    }

    #[test]
    fn test_state_hash_changes_with_state() {
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let hash_empty = kernel.state_hash();

        kernel.register_process("test");
        let hash_with_process = kernel.state_hash();

        kernel.create_endpoint(ProcessId(1)).unwrap();
        let hash_with_endpoint = kernel.state_hash();

        // All hashes should be different
        assert_ne!(hash_empty, hash_with_process);
        assert_ne!(hash_with_process, hash_with_endpoint);
    }

    // ========================================================================
    // Stage 5: System Integrity & Audit Trail Tests
    // ========================================================================

    #[test]
    fn test_audit_trail_completeness_process_lifecycle() {
        // Verify every process lifecycle operation creates commits
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let initial_commits = kernel.commitlog().len();

        // Register process - should create ProcessCreated commit
        let pid = kernel.register_process("audit_test");
        let after_register = kernel.commitlog().len();
        assert!(
            after_register > initial_commits,
            "register_process must create a commit"
        );

        // Verify the commit type
        let last_commit = kernel.commitlog().commits().last().unwrap();
        match &last_commit.commit_type {
            CommitType::ProcessCreated { pid: cpid, name, .. } => {
                assert_eq!(*cpid, pid.0);
                assert_eq!(name, "audit_test");
            }
            _ => panic!("Expected ProcessCreated commit"),
        }

        // Kill process - should create ProcessExited commit
        kernel.kill_process(pid);
        let after_kill = kernel.commitlog().len();
        assert!(
            after_kill > after_register,
            "kill_process must create a commit"
        );

        // Verify the commit type
        let last_commit = kernel.commitlog().commits().last().unwrap();
        assert!(matches!(
            &last_commit.commit_type,
            CommitType::ProcessExited { pid: cpid, .. } if *cpid == pid.0
        ));
    }

    #[test]
    fn test_audit_trail_completeness_endpoint_lifecycle() {
        // Verify every endpoint operation creates commits
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");
        let before_endpoint = kernel.commitlog().len();

        // Create endpoint - should create EndpointCreated + CapInserted commits
        let (eid, _slot) = kernel.create_endpoint(pid).unwrap();
        let after_endpoint = kernel.commitlog().len();
        assert!(
            after_endpoint >= before_endpoint + 2,
            "create_endpoint must create at least 2 commits (endpoint + cap)"
        );

        // Verify we have EndpointCreated commit
        let commits = kernel.commitlog().commits();
        let has_endpoint_created = commits.iter().any(|c| {
            matches!(&c.commit_type, CommitType::EndpointCreated { id, owner } 
                if *id == eid.0 && *owner == pid.0)
        });
        assert!(has_endpoint_created, "Must have EndpointCreated commit");

        // Kill process (which destroys endpoints) - should create EndpointDestroyed commit
        kernel.kill_process(pid);
        let commits = kernel.commitlog().commits();
        let has_endpoint_destroyed = commits.iter().any(|c| {
            matches!(&c.commit_type, CommitType::EndpointDestroyed { id } if *id == eid.0)
        });
        assert!(has_endpoint_destroyed, "Must have EndpointDestroyed commit");
    }

    #[test]
    fn test_audit_trail_completeness_capability_lifecycle() {
        // Verify every capability operation creates commits
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let owner = kernel.register_process("owner");
        let recipient = kernel.register_process("recipient");
        let (eid, owner_slot) = kernel.create_endpoint(owner).unwrap();

        let before_grant = kernel.commitlog().len();

        // Grant capability - should create CapGranted + CapInserted commits
        let recipient_slot = kernel
            .grant_capability(owner, owner_slot, recipient, Permissions::read_only())
            .unwrap();
        let after_grant = kernel.commitlog().len();
        assert!(
            after_grant >= before_grant + 2,
            "grant_capability must create at least 2 commits"
        );

        // Verify CapGranted commit exists
        let commits = kernel.commitlog().commits();
        let has_cap_granted = commits.iter().any(|c| {
            matches!(&c.commit_type, CommitType::CapGranted { from_pid, to_pid, .. } 
                if *from_pid == owner.0 && *to_pid == recipient.0)
        });
        assert!(has_cap_granted, "Must have CapGranted commit");

        // Delete capability - should create CapRemoved commit
        let before_delete = kernel.commitlog().len();
        kernel.delete_capability(recipient, recipient_slot).unwrap();
        let after_delete = kernel.commitlog().len();
        assert!(
            after_delete > before_delete,
            "delete_capability must create a commit"
        );

        // Verify CapRemoved commit
        let last_commit = kernel.commitlog().commits().last().unwrap();
        assert!(matches!(
            &last_commit.commit_type,
            CommitType::CapRemoved { pid, slot } 
                if *pid == recipient.0 && *slot == recipient_slot
        ));

        // Derive capability - should create CapInserted commit
        let before_derive = kernel.commitlog().len();
        kernel
            .derive_capability(owner, owner_slot, Permissions::write_only())
            .unwrap();
        let after_derive = kernel.commitlog().len();
        assert!(
            after_derive > before_derive,
            "derive_capability must create a commit"
        );

        // Revoke capability - should create CapRemoved commit
        let before_revoke = kernel.commitlog().len();
        kernel.revoke_capability(owner, owner_slot).unwrap();
        let after_revoke = kernel.commitlog().len();
        assert!(
            after_revoke > before_revoke,
            "revoke_capability must create a commit"
        );
    }

    #[test]
    fn test_audit_trail_completeness_syscall_exit() {
        // Verify Exit syscall creates ProcessExited commit
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");
        let before_exit = kernel.commitlog().len();

        kernel.handle_syscall(pid, Syscall::Exit { code: 42 });

        let after_exit = kernel.commitlog().len();
        assert!(
            after_exit > before_exit,
            "Exit syscall must create a commit"
        );

        // Verify ProcessExited commit
        let last_commit = kernel.commitlog().commits().last().unwrap();
        match &last_commit.commit_type {
            CommitType::ProcessExited { pid: cpid, code } => {
                assert_eq!(*cpid, pid.0);
                assert_eq!(*code, 42);
            }
            _ => panic!("Expected ProcessExited commit"),
        }
    }

    #[test]
    fn test_full_system_integrity_comprehensive() {
        // Comprehensive test: perform many operations, verify complete audit trail,
        // replay from genesis, verify identical state hash
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        // --- Phase 1: Complex operations ---
        
        // Create multiple processes
        let alice = kernel.register_process("alice");
        let bob = kernel.register_process("bob");
        let charlie = kernel.register_process("charlie");
        let dave = kernel.register_process("dave");

        // Create endpoints
        let (ep_alice, slot_alice) = kernel.create_endpoint(alice).unwrap();
        let (ep_bob, slot_bob) = kernel.create_endpoint(bob).unwrap();
        let (_ep_charlie, slot_charlie) = kernel.create_endpoint(charlie).unwrap();

        // Grant capabilities in a chain
        let bob_cap_alice = kernel
            .grant_capability(alice, slot_alice, bob, Permissions::full())
            .unwrap();
        let charlie_cap_bob = kernel
            .grant_capability(bob, slot_bob, charlie, Permissions::full())
            .unwrap();
        let dave_cap_charlie = kernel
            .grant_capability(charlie, charlie_cap_bob, dave, Permissions::read_only())
            .unwrap();

        // Derive some capabilities
        let alice_derived = kernel
            .derive_capability(alice, slot_alice, Permissions::read_only())
            .unwrap();

        // Delete some capabilities
        kernel.delete_capability(alice, alice_derived).unwrap();
        kernel.delete_capability(dave, dave_cap_charlie).unwrap();

        // Exit processes via syscall (proper way that creates consistent commits)
        kernel.handle_syscall(dave, Syscall::Exit { code: 0 });
        kernel.handle_syscall(charlie, Syscall::Exit { code: 1 });

        // --- Phase 2: Verify commit log integrity ---
        assert!(
            kernel.commitlog().verify_integrity(),
            "CommitLog hash chain must be valid"
        );

        // --- Phase 3: Record state for replay verification ---
        let original_hash = kernel.state_hash();
        let commits = kernel.commitlog().commits().to_vec();
        let commit_count = commits.len();

        // Verify we have a substantial number of commits
        assert!(
            commit_count >= 15,
            "Should have many commits for comprehensive test, got {}",
            commit_count
        );

        // --- Phase 4: Replay and verify determinism ---
        let mut replay_kernel: Kernel<MockHal> = Kernel::new_for_replay();
        replay(&mut replay_kernel, &commits).expect("Replay should succeed");

        let replayed_hash = replay_kernel.state_hash();
        assert_eq!(
            original_hash, replayed_hash,
            "Replayed state hash must match original"
        );

        // --- Phase 5: Verify replayed state matches ---
        
        // Check processes exist (alice and bob running, dave and charlie zombies)
        assert!(replay_kernel.get_process(alice).is_some());
        assert!(replay_kernel.get_process(bob).is_some());
        assert!(replay_kernel.get_process(dave).is_some());
        assert!(replay_kernel.get_process(charlie).is_some());
        assert_eq!(
            replay_kernel.get_process(dave).unwrap().state,
            ProcessState::Zombie
        );
        assert_eq!(
            replay_kernel.get_process(charlie).unwrap().state,
            ProcessState::Zombie
        );

        // Check endpoints (all should exist - Exit doesn't destroy endpoints)
        assert!(replay_kernel.get_endpoint(ep_alice).is_some());
        assert!(replay_kernel.get_endpoint(ep_bob).is_some());

        // Check capabilities
        assert!(replay_kernel
            .get_cap_space(alice)
            .unwrap()
            .get(slot_alice)
            .is_some());
        assert!(replay_kernel
            .get_cap_space(bob)
            .unwrap()
            .get(bob_cap_alice)
            .is_some());
    }

    #[test]
    fn test_syslog_request_response_pairs() {
        // Verify SysLog contains request/response pairs for syscalls
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let pid = kernel.register_process("test");

        // Log a syscall request
        let request_id = kernel.log_syscall_request(pid, 0x04, [0, 0, 0, 0]);

        // Log the response
        kernel.log_syscall_response(pid, request_id, 42);

        // Verify SysLog has both events
        let events = kernel.syslog().events();
        assert!(events.len() >= 2);

        // Find request event
        let request = events.iter().find(|e| {
            matches!(&e.event_type, SysEventType::Request { syscall_num, .. } 
                if *syscall_num == 0x04)
        });
        assert!(request.is_some(), "SysLog must contain request");

        // Find corresponding response
        let response = events.iter().find(|e| {
            matches!(&e.event_type, SysEventType::Response { request_id: rid, result } 
                if *rid == request_id && *result == 42)
        });
        assert!(response.is_some(), "SysLog must contain matching response");
    }

    #[test]
    fn test_ipc_messages_create_audit_commits() {
        // Verify IPC messages create MessageSent commits for audit trail
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        let sender = kernel.register_process("sender");
        let receiver = kernel.register_process("receiver");

        kernel.create_endpoint(receiver).unwrap();
        let sender_cap = kernel
            .grant_capability(receiver, 0, sender, Permissions::write_only())
            .unwrap();

        let commit_count_before = kernel.commitlog().len();

        // Send IPC message
        kernel
            .ipc_send(sender, sender_cap, 0x1234, vec![1, 2, 3, 4])
            .unwrap();

        let commit_count_after = kernel.commitlog().len();

        // IPC send should create a MessageSent commit for audit
        assert_eq!(
            commit_count_before + 1, commit_count_after,
            "IPC messages should create MessageSent commits for audit trail"
        );

        // Verify the commit is a MessageSent
        let last_commit = kernel.commitlog().commits().last().unwrap();
        match &last_commit.commit_type {
            CommitType::MessageSent { from_pid, to_endpoint, tag, size } => {
                assert_eq!(*from_pid, sender.0);
                assert_eq!(*tag, 0x1234);
                assert_eq!(*size, 4);
                // to_endpoint is the endpoint ID
                let _ = to_endpoint;
            }
            _ => panic!("Expected MessageSent commit, got {:?}", last_commit.commit_type),
        }
    }

    #[test]
    fn test_replay_idempotence() {
        // Replaying the same commits multiple times should always produce same state
        let hal = MockHal::new();
        let mut kernel = Kernel::new(hal);

        kernel.register_process("test1");
        kernel.register_process("test2");
        kernel.create_endpoint(ProcessId(1)).unwrap();
        kernel
            .grant_capability(ProcessId(1), 0, ProcessId(2), Permissions::full())
            .unwrap();

        let commits = kernel.commitlog().commits().to_vec();

        // Replay multiple times, collect all hashes
        let hashes: Vec<[u8; 32]> = (0..5)
            .map(|_| {
                let mut replay_kernel: Kernel<MockHal> = Kernel::new_for_replay();
                replay(&mut replay_kernel, &commits).unwrap();
                replay_kernel.state_hash()
            })
            .collect();

        // All hashes must be identical
        let first_hash = hashes[0];
        for (i, hash) in hashes.iter().enumerate() {
            assert_eq!(
                *hash, first_hash,
                "Replay {} produced different hash",
                i
            );
        }
    }
}
