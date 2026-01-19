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

/// The kernel, generic over HAL implementation
pub struct Kernel<H: HAL> {
    hal: H,
    /// Process table
    processes: BTreeMap<ProcessId, Process>,
    /// Capability spaces (per-process)
    cap_spaces: BTreeMap<ProcessId, CapabilitySpace>,
    /// IPC endpoints
    endpoints: BTreeMap<EndpointId, Endpoint>,
    /// Axiom gateway (SysLog + CommitLog) - unified logging interface
    axiom_gateway: AxiomGateway,
    /// Next process ID
    next_pid: u64,
    /// Next endpoint ID
    next_endpoint_id: u64,
    /// Next capability ID
    next_cap_id: u64,
    /// Boot time (for uptime calculation)
    boot_time: u64,
    /// Total IPC messages since boot
    total_ipc_count: u64,
    /// IPC traffic hook callback data (for live monitoring)
    ipc_traffic_log: VecDeque<IpcTrafficEntry>,
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

impl<H: HAL> Kernel<H> {
    /// Create a new kernel with the given HAL
    pub fn new(hal: H) -> Self {
        let boot_time = hal.now_nanos();
        Self {
            hal,
            processes: BTreeMap::new(),
            cap_spaces: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            axiom_gateway: AxiomGateway::new(boot_time),
            next_pid: 1,
            next_endpoint_id: 1,
            next_cap_id: 1,
            boot_time,
            total_ipc_count: 0,
            ipc_traffic_log: VecDeque::new(),
        }
    }

    /// Get the HAL
    pub fn hal(&self) -> &H {
        &self.hal
    }

    /// Get the Axiom gateway (SysLog + CommitLog)
    pub fn axiom(&self) -> &AxiomGateway {
        &self.axiom_gateway
    }

    /// Get the SysLog (syscall audit trail)
    pub fn syslog(&self) -> &SysLog {
        self.axiom_gateway.syslog()
    }

    /// Get the CommitLog (state mutations for replay)
    pub fn commitlog(&self) -> &CommitLog {
        self.axiom_gateway.commitlog()
    }

    /// Log a syscall request to the SysLog.
    /// Returns the event ID for correlation with the response.
    pub fn log_syscall_request(&mut self, pid: ProcessId, syscall_num: u32, args: [u32; 4]) -> u64 {
        let timestamp = self.uptime_nanos();
        self.axiom_gateway
            .syslog_mut()
            .log_request(pid.0, syscall_num, args, timestamp)
    }

    /// Log a syscall response to the SysLog.
    pub fn log_syscall_response(&mut self, pid: ProcessId, request_id: u64, result: i64) {
        let timestamp = self.uptime_nanos();
        self.axiom_gateway
            .syslog_mut()
            .log_response(pid.0, request_id, result, timestamp);
    }

    /// Get uptime in nanoseconds
    pub fn uptime_nanos(&self) -> u64 {
        self.hal.now_nanos() - self.boot_time
    }

    /// Generate next capability ID
    fn next_cap_id(&mut self) -> u64 {
        let id = self.next_cap_id;
        self.next_cap_id += 1;
        id
    }

    /// Register a process (used by supervisor to register spawned workers)
    pub fn register_process(&mut self, name: &str) -> ProcessId {
        self.register_process_with_parent(name, ProcessId(0))
    }

    /// Register a process with a specific parent (for fork/spawn tracking)
    pub fn register_process_with_parent(&mut self, name: &str, parent: ProcessId) -> ProcessId {
        let pid = ProcessId(self.next_pid);
        self.next_pid += 1;

        let now = self.uptime_nanos();
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
                last_active_ns: now,
                start_time_ns: now,
            },
        };
        self.processes.insert(pid, process);
        self.cap_spaces.insert(pid, CapabilitySpace::new());

        // Log process creation to CommitLog
        self.axiom_gateway.append_internal_commit(
            CommitType::ProcessCreated {
                pid: pid.0,
                parent: parent.0,
                name: String::from(name),
            },
            now,
        );

        self.hal.debug_write(&alloc::format!(
            "[kernel] Registered process: {} (PID {})",
            name,
            pid.0
        ));

        pid
    }

    /// Kill a process and clean up its resources
    pub fn kill_process(&mut self, pid: ProcessId) {
        // Remove the process
        if let Some(proc) = self.processes.remove(&pid) {
            self.hal.debug_write(&alloc::format!(
                "[kernel] Killed process: {} (PID {})",
                proc.name,
                pid.0
            ));
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
        }
    }

    /// Create an IPC endpoint owned by a process
    pub fn create_endpoint(
        &mut self,
        owner: ProcessId,
    ) -> Result<(EndpointId, CapSlot), KernelError> {
        if !self.processes.contains_key(&owner) {
            return Err(KernelError::ProcessNotFound);
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

        let slot = self
            .cap_spaces
            .get_mut(&owner)
            .ok_or(KernelError::ProcessNotFound)?
            .insert(cap);

        // Log to CommitLog
        let timestamp = self.uptime_nanos();
        self.axiom_gateway.append_internal_commit(
            CommitType::EndpointCreated {
                id: id.0,
                owner: owner.0,
            },
            timestamp,
        );
        self.axiom_gateway.append_internal_commit(
            CommitType::CapInserted {
                pid: owner.0,
                slot,
                cap_id,
                object_type: ObjectType::Endpoint as u8,
                object_id: id.0,
                perms: perms.to_byte(),
            },
            timestamp,
        );

        self.hal.debug_write(&alloc::format!(
            "[kernel] Created endpoint {} for PID {}, cap slot {}",
            id.0,
            owner.0,
            slot
        ));

        Ok((id, slot))
    }

    /// Grant a capability from one process to another
    pub fn grant_capability(
        &mut self,
        from_pid: ProcessId,
        from_slot: CapSlot,
        to_pid: ProcessId,
        new_perms: Permissions,
    ) -> Result<CapSlot, KernelError> {
        // Get source capability
        let source_cap = self
            .cap_spaces
            .get(&from_pid)
            .ok_or(KernelError::ProcessNotFound)?
            .get(from_slot)
            .ok_or(KernelError::InvalidCapability)?
            .clone();

        // Check grant permission
        if !source_cap.permissions.grant {
            return Err(KernelError::PermissionDenied);
        }

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
        let to_slot = self
            .cap_spaces
            .get_mut(&to_pid)
            .ok_or(KernelError::ProcessNotFound)?
            .insert(new_cap);

        // Log to CommitLog
        let timestamp = self.uptime_nanos();
        self.axiom_gateway.append_internal_commit(
            CommitType::CapGranted {
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
            timestamp,
        );
        // Also log CapInserted for the receiver (needed for replay)
        self.axiom_gateway.append_internal_commit(
            CommitType::CapInserted {
                pid: to_pid.0,
                slot: to_slot,
                cap_id: new_cap_id,
                object_type: source_cap.object_type as u8,
                object_id: source_cap.object_id,
                perms: granted_perms.to_byte(),
            },
            timestamp,
        );

        Ok(to_slot)
    }

    /// Revoke a capability.
    ///
    /// Revocation requires the caller to have grant permission on the capability.
    /// This removes the capability from the caller's CSpace.
    ///
    /// # Arguments
    /// - `pid`: Process revoking the capability
    /// - `slot`: Capability slot to revoke
    ///
    /// # Returns
    /// - `Ok(())`: Capability revoked
    /// - `Err(KernelError::InvalidCapability)`: Slot empty
    /// - `Err(KernelError::PermissionDenied)`: No grant permission
    pub fn revoke_capability(&mut self, pid: ProcessId, slot: CapSlot) -> Result<(), KernelError> {
        // Get capability space
        let cspace = self
            .cap_spaces
            .get(&pid)
            .ok_or(KernelError::ProcessNotFound)?;

        // Check we have the capability and it has grant permission
        let cap = cspace.get(slot).ok_or(KernelError::InvalidCapability)?;
        if !cap.permissions.grant {
            return Err(KernelError::PermissionDenied);
        }

        let cap_id = cap.id;

        // Log to CommitLog
        let timestamp = self.uptime_nanos();
        self.axiom_gateway
            .append_internal_commit(CommitType::CapRemoved { pid: pid.0, slot }, timestamp);

        // Remove from CSpace
        self.cap_spaces
            .get_mut(&pid)
            .ok_or(KernelError::ProcessNotFound)?
            .remove(slot);

        self.hal.debug_write(&alloc::format!(
            "[kernel] PID {} revoked capability {} (slot {})",
            pid.0,
            cap_id,
            slot
        ));

        Ok(())
    }

    /// Delete a capability from a process's own CSpace.
    ///
    /// Unlike revoke, delete does not require grant permission. A process can
    /// always delete capabilities from its own CSpace.
    ///
    /// # Arguments
    /// - `pid`: Process deleting the capability
    /// - `slot`: Capability slot to delete
    ///
    /// # Returns
    /// - `Ok(())`: Capability deleted
    /// - `Err(KernelError::ProcessNotFound)`: Process does not exist
    /// - `Err(KernelError::InvalidCapability)`: Slot is empty
    pub fn delete_capability(&mut self, pid: ProcessId, slot: CapSlot) -> Result<(), KernelError> {
        // Get capability space
        let cspace = self
            .cap_spaces
            .get(&pid)
            .ok_or(KernelError::ProcessNotFound)?;

        // Check capability exists
        let cap = cspace.get(slot).ok_or(KernelError::InvalidCapability)?;
        let cap_id = cap.id;

        // Log to CommitLog
        let timestamp = self.uptime_nanos();
        self.axiom_gateway
            .append_internal_commit(CommitType::CapRemoved { pid: pid.0, slot }, timestamp);

        // Remove from CSpace
        self.cap_spaces
            .get_mut(&pid)
            .ok_or(KernelError::ProcessNotFound)?
            .remove(slot);

        self.hal.debug_write(&alloc::format!(
            "[kernel] PID {} deleted capability {} (slot {})",
            pid.0,
            cap_id,
            slot
        ));

        Ok(())
    }

    /// Send IPC message (validates capability)
    pub fn ipc_send(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
    ) -> Result<(), KernelError> {
        // Lookup capability
        let cap = self
            .cap_spaces
            .get(&from_pid)
            .ok_or(KernelError::ProcessNotFound)?
            .get(endpoint_slot)
            .ok_or(KernelError::InvalidCapability)?;

        // Check it's an endpoint capability with write permission
        if cap.object_type != ObjectType::Endpoint || !cap.permissions.write {
            return Err(KernelError::PermissionDenied);
        }

        let endpoint_id = EndpointId(cap.object_id);
        let data_len = data.len();

        // Queue message
        let endpoint = self
            .endpoints
            .get_mut(&endpoint_id)
            .ok_or(KernelError::EndpointNotFound)?;

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
        let now = self.uptime_nanos();
        if let Some(sender) = self.processes.get_mut(&from_pid) {
            sender.metrics.ipc_sent += 1;
            sender.metrics.ipc_bytes_sent += data_len as u64;
            sender.metrics.last_active_ns = now;
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
            timestamp: now,
        };
        self.ipc_traffic_log.push_back(entry);
        while self.ipc_traffic_log.len() > MAX_IPC_TRAFFIC_LOG {
            self.ipc_traffic_log.pop_front();
        }

        Ok(())
    }

    /// Send IPC message with capability transfer.
    ///
    /// Capabilities in `cap_slots` are removed from the sender's CSpace and
    /// transferred to the receiver. Each transfer is logged to the Axiom log.
    ///
    /// # Arguments
    /// - `from_pid`: Sending process
    /// - `endpoint_slot`: Capability slot for the endpoint
    /// - `tag`: Application-defined message tag
    /// - `data`: Message payload (max `MAX_MESSAGE_SIZE` bytes)
    /// - `cap_slots`: Capability slots to transfer (max `MAX_CAPS_PER_MESSAGE`)
    ///
    /// # Returns
    /// - `Ok(())`: Message sent successfully
    /// - `Err(KernelError)`: Various errors (see `ipc_send`)
    pub fn ipc_send_with_caps(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
        cap_slots: &[CapSlot],
    ) -> Result<(), KernelError> {
        // Validate limits
        if data.len() > MAX_MESSAGE_SIZE {
            return Err(KernelError::PermissionDenied); // TODO: add specific error
        }
        if cap_slots.len() > MAX_CAPS_PER_MESSAGE {
            return Err(KernelError::PermissionDenied); // TODO: add specific error
        }

        // Lookup endpoint capability
        let cap = self
            .cap_spaces
            .get(&from_pid)
            .ok_or(KernelError::ProcessNotFound)?
            .get(endpoint_slot)
            .ok_or(KernelError::InvalidCapability)?;

        // Check it's an endpoint capability with write permission
        if cap.object_type != ObjectType::Endpoint || !cap.permissions.write {
            return Err(KernelError::PermissionDenied);
        }

        let endpoint_id = EndpointId(cap.object_id);

        // Verify endpoint exists and get receiver
        let to_pid = self
            .endpoints
            .get(&endpoint_id)
            .ok_or(KernelError::EndpointNotFound)?
            .owner;

        // Collect capabilities to transfer (validate they exist first)
        let sender_cspace = self
            .cap_spaces
            .get(&from_pid)
            .ok_or(KernelError::ProcessNotFound)?;

        for &slot in cap_slots {
            if sender_cspace.get(slot).is_none() {
                return Err(KernelError::InvalidCapability);
            }
        }

        // Remove capabilities from sender and build transfer list
        let timestamp = self.uptime_nanos();
        let mut transferred_caps = Vec::with_capacity(cap_slots.len());

        let sender_cspace = self
            .cap_spaces
            .get_mut(&from_pid)
            .ok_or(KernelError::ProcessNotFound)?;

        for &slot in cap_slots {
            if let Some(cap) = sender_cspace.remove(slot) {
                // Log capability removal from sender to CommitLog
                self.axiom_gateway.append_internal_commit(
                    CommitType::CapRemoved {
                        pid: from_pid.0,
                        slot,
                    },
                    timestamp,
                );
                transferred_caps.push(TransferredCap {
                    capability: cap,
                    receiver_slot: None,
                });
            }
        }

        let data_len = data.len();

        // Queue message with transferred capabilities
        let endpoint = self
            .endpoints
            .get_mut(&endpoint_id)
            .ok_or(KernelError::EndpointNotFound)?;

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
        let now = self.uptime_nanos();
        if let Some(sender) = self.processes.get_mut(&from_pid) {
            sender.metrics.ipc_sent += 1;
            sender.metrics.ipc_bytes_sent += data_len as u64;
            sender.metrics.last_active_ns = now;
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
            timestamp: now,
        };
        self.ipc_traffic_log.push_back(entry);
        while self.ipc_traffic_log.len() > MAX_IPC_TRAFFIC_LOG {
            self.ipc_traffic_log.pop_front();
        }

        Ok(())
    }

    /// Receive IPC message and install transferred capabilities.
    ///
    /// If the message contains transferred capabilities, they are automatically
    /// installed into the receiver's CSpace.
    ///
    /// # Arguments
    /// - `pid`: Receiving process
    /// - `endpoint_slot`: Capability slot for the endpoint
    ///
    /// # Returns
    /// - `Ok(Some(Message))`: Message received with capabilities installed
    /// - `Ok(None)`: No message available
    /// - `Err(KernelError)`: Various errors
    pub fn ipc_receive_with_caps(
        &mut self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
    ) -> Result<Option<(Message, Vec<CapSlot>)>, KernelError> {
        // First do normal receive to get the message
        let message = match self.ipc_receive(pid, endpoint_slot)? {
            Some(msg) => msg,
            None => return Ok(None),
        };

        // Install transferred capabilities into receiver's CSpace
        let mut installed_slots = Vec::with_capacity(message.transferred_caps.len());

        if !message.transferred_caps.is_empty() {
            let receiver_cspace = self
                .cap_spaces
                .get_mut(&pid)
                .ok_or(KernelError::ProcessNotFound)?;

            for tcap in &message.transferred_caps {
                let slot = receiver_cspace.insert(tcap.capability.clone());
                installed_slots.push(slot);
            }
        }

        Ok(Some((message, installed_slots)))
    }

    /// Receive IPC message
    pub fn ipc_receive(
        &mut self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
    ) -> Result<Option<Message>, KernelError> {
        // Lookup capability
        let cap = self
            .cap_spaces
            .get(&pid)
            .ok_or(KernelError::ProcessNotFound)?
            .get(endpoint_slot)
            .ok_or(KernelError::InvalidCapability)?;

        // Check it's an endpoint capability with read permission
        if cap.object_type != ObjectType::Endpoint || !cap.permissions.read {
            return Err(KernelError::PermissionDenied);
        }

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
            let now = self.uptime_nanos();
            if let Some(receiver) = self.processes.get_mut(&pid) {
                receiver.metrics.ipc_received += 1;
                receiver.metrics.ipc_bytes_received += msg.data.len() as u64;
                receiver.metrics.last_active_ns = now;
            }
        }

        Ok(message)
    }

    /// Handle syscall from a process
    pub fn handle_syscall(&mut self, from_pid: ProcessId, syscall: Syscall) -> SyscallResult {
        // Update syscall count (compute time before mutable borrow)
        let now = self.uptime_nanos();
        if let Some(proc) = self.processes.get_mut(&from_pid) {
            proc.metrics.syscall_count += 1;
            proc.metrics.last_active_ns = now;
        }

        match syscall {
            Syscall::Debug { msg } => {
                self.hal
                    .debug_write(&alloc::format!("[PID {}] {}", from_pid.0, msg));
                SyscallResult::Ok(0)
            }

            Syscall::CreateEndpoint => match self.create_endpoint(from_pid) {
                Ok((eid, slot)) => SyscallResult::Ok((eid.0 << 32) | (slot as u64)),
                Err(e) => SyscallResult::Err(e),
            },

            Syscall::Send {
                endpoint_slot,
                tag,
                data,
            } => match self.ipc_send(from_pid, endpoint_slot, tag, data) {
                Ok(()) => SyscallResult::Ok(0),
                Err(e) => SyscallResult::Err(e),
            },

            Syscall::Receive { endpoint_slot } => match self.ipc_receive(from_pid, endpoint_slot) {
                Ok(Some(msg)) => SyscallResult::Message(msg),
                Ok(None) => SyscallResult::WouldBlock,
                Err(e) => SyscallResult::Err(e),
            },

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
                // Log process exit to CommitLog
                let timestamp = self.uptime_nanos();
                self.axiom_gateway.append_internal_commit(
                    CommitType::ProcessExited {
                        pid: from_pid.0,
                        code,
                    },
                    timestamp,
                );
                SyscallResult::Ok(code as u64)
            }

            Syscall::GetTime => SyscallResult::Ok(self.uptime_nanos()),

            Syscall::Yield => {
                // Cooperative yield - just return success
                // In a real scheduler, this would trigger a context switch
                SyscallResult::Ok(0)
            }

            // === Capability syscalls ===
            Syscall::CapGrant {
                from_slot,
                to_pid,
                permissions,
            } => match self.grant_capability(from_pid, from_slot, to_pid, permissions) {
                Ok(new_slot) => SyscallResult::Ok(new_slot as u64),
                Err(e) => SyscallResult::Err(e),
            },

            Syscall::CapRevoke { slot } => match self.revoke_capability(from_pid, slot) {
                Ok(()) => SyscallResult::Ok(0),
                Err(e) => SyscallResult::Err(e),
            },

            Syscall::CapDelete { slot } => match self.delete_capability(from_pid, slot) {
                Ok(()) => SyscallResult::Ok(0),
                Err(e) => SyscallResult::Err(e),
            },

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
                // Derive creates a new capability in the same process's CSpace
                // with attenuated permissions
                match self.derive_capability(from_pid, slot, new_permissions) {
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
            } => match self.ipc_send_with_caps(from_pid, endpoint_slot, tag, data, &cap_slots) {
                Ok(()) => SyscallResult::Ok(0),
                Err(e) => SyscallResult::Err(e),
            },

            Syscall::Call {
                endpoint_slot,
                tag,
                data,
            } => {
                // Call = send + block for reply
                // For now, we implement a simple version that just sends
                // Full blocking semantics would require scheduler support
                match self.ipc_send(from_pid, endpoint_slot, tag, data) {
                    Ok(()) => SyscallResult::WouldBlock, // Caller should poll for reply
                    Err(e) => SyscallResult::Err(e),
                }
            }

            Syscall::Reply {
                caller_pid,
                tag,
                data,
            } => {
                // Reply sends back to the caller's endpoint
                match self.send_to_process(from_pid, caller_pid, tag, data) {
                    Ok(()) => SyscallResult::Ok(0),
                    Err(e) => SyscallResult::Err(e),
                }
            }
        }
    }

    /// Derive a capability with reduced permissions.
    ///
    /// Creates a new capability in the same process's CSpace that references
    /// the same object but with attenuated permissions.
    ///
    /// # Arguments
    /// - `pid`: Process deriving the capability
    /// - `slot`: Source capability slot
    /// - `new_perms`: Requested permissions (will be intersected with source)
    ///
    /// # Returns
    /// - `Ok(CapSlot)`: Slot of the new derived capability
    /// - `Err(KernelError)`: Various errors
    pub fn derive_capability(
        &mut self,
        pid: ProcessId,
        slot: CapSlot,
        new_perms: Permissions,
    ) -> Result<CapSlot, KernelError> {
        // Get source capability
        let source_cap = self
            .cap_spaces
            .get(&pid)
            .ok_or(KernelError::ProcessNotFound)?
            .get(slot)
            .ok_or(KernelError::InvalidCapability)?
            .clone();

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
        let new_slot = self
            .cap_spaces
            .get_mut(&pid)
            .ok_or(KernelError::ProcessNotFound)?
            .insert(new_cap);

        // Log derivation to CommitLog
        let timestamp = self.uptime_nanos();
        self.axiom_gateway.append_internal_commit(
            CommitType::CapInserted {
                pid: pid.0,
                slot: new_slot,
                cap_id: new_cap_id,
                object_type: source_cap.object_type as u8,
                object_id: source_cap.object_id,
                perms: derived_perms.to_byte(),
            },
            timestamp,
        );

        Ok(new_slot)
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

    /// Send a message to a process's first endpoint (for testing/supervisor use)
    pub fn send_to_process(
        &mut self,
        from_pid: ProcessId,
        to_pid: ProcessId,
        tag: u32,
        data: Vec<u8>,
    ) -> Result<(), KernelError> {
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
        let now = self.uptime_nanos();
        if let Some(sender) = self.processes.get_mut(&from_pid) {
            sender.metrics.ipc_sent += 1;
            sender.metrics.ipc_bytes_sent += data_len as u64;
            sender.metrics.last_active_ns = now;
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
            timestamp: now,
        };
        self.ipc_traffic_log.push_back(entry);
        while self.ipc_traffic_log.len() > MAX_IPC_TRAFFIC_LOG {
            self.ipc_traffic_log.pop_front();
        }

        self.hal.debug_write(&alloc::format!(
            "[kernel] Message sent from PID {} to PID {} (endpoint {}, tag 0x{:x})",
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
    pub fn get_system_metrics(&self) -> SystemMetrics {
        SystemMetrics {
            process_count: self.processes.len(),
            total_memory: self.total_memory(),
            endpoint_count: self.endpoints.len(),
            total_pending_messages: self.total_pending_messages(),
            total_ipc_messages: self.total_ipc_count,
            uptime_ns: self.uptime_nanos(),
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
        self.processes.insert(ProcessId(pid), process);
        self.cap_spaces
            .insert(ProcessId(pid), CapabilitySpace::new());

        // Update next_pid if needed to avoid collisions
        if pid >= self.next_pid {
            self.next_pid = pid + 1;
        }

        Ok(())
    }

    fn replay_exit_process(&mut self, pid: u64, _code: i32) -> ReplayResult<()> {
        let process = self
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

        let cspace = self
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
        if cap_id >= self.next_cap_id {
            self.next_cap_id = cap_id + 1;
        }

        Ok(())
    }

    fn replay_remove_capability(&mut self, pid: u64, slot: u32) -> ReplayResult<()> {
        let cspace = self
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
        if new_cap_id >= self.next_cap_id {
            self.next_cap_id = new_cap_id + 1;
        }

        Ok(())
    }

    fn replay_create_endpoint(&mut self, id: u64, owner: u64) -> ReplayResult<()> {
        if !self.processes.contains_key(&ProcessId(owner)) {
            return Err(ReplayError::ProcessNotFound(owner));
        }

        let endpoint = Endpoint {
            id: EndpointId(id),
            owner: ProcessId(owner),
            pending_messages: VecDeque::new(),
            metrics: EndpointMetrics::default(),
        };
        self.endpoints.insert(EndpointId(id), endpoint);

        // Update next_endpoint_id if needed
        if id >= self.next_endpoint_id {
            self.next_endpoint_id = id + 1;
        }

        Ok(())
    }

    fn replay_destroy_endpoint(&mut self, id: u64) -> ReplayResult<()> {
        self.endpoints.remove(&EndpointId(id));
        Ok(())
    }

    fn state_hash(&self) -> [u8; 32] {
        let mut hasher = StateHasher::new();

        // Hash processes (BTreeMap is sorted by key, so order is deterministic)
        hasher.write_u64(self.processes.len() as u64);
        for (pid, proc) in &self.processes {
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
        hasher.write_u64(self.cap_spaces.len() as u64);
        for (pid, cspace) in &self.cap_spaces {
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
        hasher.write_u64(self.endpoints.len() as u64);
        for (id, ep) in &self.endpoints {
            hasher.write_u64(id.0);
            hasher.write_u64(ep.owner.0);
            // Note: We don't hash pending_messages or metrics as they are volatile
        }

        hasher.finalize()
    }
}

impl<H: HAL + Default> Kernel<H> {
    /// Create a kernel for replay mode.
    ///
    /// This creates a minimal kernel suitable for replaying commits.
    /// It uses a default HAL (typically MockHal) since replay doesn't
    /// perform actual HAL operations.
    pub fn new_for_replay() -> Self {
        let hal = H::default();
        Self {
            hal,
            processes: BTreeMap::new(),
            cap_spaces: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            axiom_gateway: AxiomGateway::new(0),
            next_pid: 1,
            next_endpoint_id: 1,
            next_cap_id: 1,
            boot_time: 0,
            total_ipc_count: 0,
            ipc_traffic_log: VecDeque::new(),
        }
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use orbital_hal_mock::MockHal;

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
}
