# Process Manager

> User-space process lifecycle management as a supervision responsibility.

## Overview

The Process Manager handles process lifecycle as part of the init supervision tree. It provides:

1. **Process Creation**: Spawning new processes from binaries
2. **Capability Distribution**: Granting initial capabilities to new processes
3. **Process Queries**: Listing and inspecting processes
4. **Resource Limits**: Enforcing per-process resource quotas

Note: The *kernel* handles low-level process primitives (TCBs, scheduling). The Process Manager is a *policy layer* that decides what capabilities processes receive and manages lifecycle.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      Process Manager (part of Init)                          │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                    Capabilities                                         │ │
│  │                                                                         │ │
│  │  • ProcessSpawn (kernel) - ability to spawn processes                   │ │
│  │  • CapabilityGrant (kernel) - ability to grant capabilities             │ │
│  │  • Storage (read) - load binaries                                       │ │
│  │  • Init endpoint (write) - report status                                │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                    Process Table                                        │ │
│  │                                                                         │ │
│  │  PID │ Name       │ Parent │ State   │ User     │ Caps Granted         │ │
│  │  ────┼────────────┼────────┼─────────┼──────────┼──────────────        │ │
│  │  1   │ init       │ 0      │ Running │ system   │ [full]               │ │
│  │  2   │ terminal   │ 1      │ Running │ system   │ [console]            │ │
│  │  3   │ storage    │ 1      │ Running │ system   │ [storage-rw]         │ │
│  │  4   │ app1       │ 2      │ Running │ user-123 │ [storage-ro, net]    │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  Message Handlers:                                                           │
│  • MSG_SPAWN         → spawn_process()                                      │
│  • MSG_LIST_PROCESSES→ list_processes()                                     │
│  • MSG_KILL          → kill_process()                                       │
│  • MSG_GET_PROCESS   → get_process_info()                                   │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Data Structures

### SpawnRequest

```rust
use serde::{Serialize, Deserialize};
use uuid::Uuid;

/// Request to spawn a new process.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpawnRequest {
    /// Process name (for debugging and logging)
    pub name: String,
    
    /// Binary to execute
    pub binary: BinarySource,
    
    /// Initial capabilities to grant
    pub capabilities: Vec<CapRequest>,
    
    /// Resource limits
    pub limits: ResourceLimits,
    
    /// User context for the process
    pub user_id: Option<Uuid>,
    
    /// Session context
    pub session_id: Option<Uuid>,
    
    /// Environment variables
    pub env: Vec<(String, String)>,
    
    /// Working directory
    pub working_dir: Option<String>,
}

/// Source for process binary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BinarySource {
    /// Load from storage path
    Path(String),
    
    /// Inline binary data (for small programs)
    Inline(Vec<u8>),
    
    /// Well-known service name (built-in)
    WellKnown(String),
}

/// Request for a capability.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapRequest {
    /// Type of capability needed
    pub cap_type: String,  // "storage", "network", "console", etc.
    
    /// Requested permissions
    pub permissions: Permissions,
    
    /// Context/reason for the request
    pub reason: Option<String>,
}
```

### ResourceLimits

```rust
/// Resource limits for the process.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum memory in bytes
    pub max_memory: Option<usize>,
    
    /// Maximum CPU time in nanoseconds (0 = unlimited)
    pub max_cpu_time: Option<u64>,
    
    /// Maximum open capabilities
    pub max_caps: Option<u32>,
    
    /// Maximum IPC message queue depth
    pub max_ipc_queue: Option<usize>,
    
    /// Maximum file descriptors / handles
    pub max_handles: Option<u32>,
    
    /// Maximum children this process can spawn
    pub max_children: Option<u32>,
}
```

### ProcessInfo

```rust
/// Information about a running process.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessInfo {
    /// Process ID
    pub pid: ProcessId,
    
    /// Process name
    pub name: String,
    
    /// Parent process ID
    pub parent_pid: ProcessId,
    
    /// Current state
    pub state: ProcessState,
    
    /// User ID (if user process)
    pub user_id: Option<Uuid>,
    
    /// Session ID (if in session)
    pub session_id: Option<Uuid>,
    
    /// Memory usage in bytes
    pub memory_bytes: usize,
    
    /// CPU time consumed in nanoseconds
    pub cpu_time_ns: u64,
    
    /// Start time (nanos since boot)
    pub start_time_ns: u64,
    
    /// Number of capabilities held
    pub cap_count: u32,
    
    /// Process classification
    pub classification: ProcessClass,
}

/// Process state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessState {
    /// Process is running
    Running,
    
    /// Process is blocked (waiting for IPC, etc.)
    Blocked,
    
    /// Process is suspended
    Suspended,
    
    /// Process has exited
    Exited { exit_code: i32 },
    
    /// Process was killed
    Killed { signal: KillSignal },
}

/// Process classification for policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessClass {
    /// System service (init, compositor, etc.)
    System,
    
    /// Runtime service (storage, network, identity, etc.)
    Runtime,
    
    /// User application
    Application,
}
```

### SpawnResponse

```rust
/// Spawn response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpawnResponse {
    /// Success or error
    pub result: Result<SpawnSuccess, SpawnError>,
}

/// Successful spawn result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpawnSuccess {
    /// New process ID
    pub pid: ProcessId,
    
    /// Capability slots granted to new process
    pub granted_caps: Vec<CapSlot>,
}

/// Spawn errors.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SpawnError {
    /// Binary not found
    BinaryNotFound,
    
    /// Insufficient permissions to spawn
    PermissionDenied,
    
    /// Resource limit exceeded (system-wide)
    ResourceLimitExceeded,
    
    /// Invalid binary format
    InvalidBinary,
    
    /// Requested capability denied
    CapabilityDenied(String),
    
    /// User not found
    UserNotFound,
    
    /// Session not found or expired
    SessionNotFound,
    
    /// Internal error
    InternalError(String),
}
```

## IPC Protocol

### Message Types

```rust
pub mod proc_msg {
    /// Spawn process request.
    pub const MSG_SPAWN: u32 = 0x4000;
    /// Spawn process response.
    pub const MSG_SPAWN_RESPONSE: u32 = 0x4001;
    
    /// List processes request.
    pub const MSG_LIST_PROCESSES: u32 = 0x4002;
    /// List processes response.
    pub const MSG_PROCESS_LIST: u32 = 0x4003;
    
    /// Kill process request.
    pub const MSG_KILL: u32 = 0x4004;
    /// Kill process response.
    pub const MSG_KILL_RESPONSE: u32 = 0x4005;
    
    /// Get process info request.
    pub const MSG_GET_PROCESS: u32 = 0x4006;
    /// Get process info response.
    pub const MSG_GET_PROCESS_RESPONSE: u32 = 0x4007;
    
    /// Process exit notification (to parent).
    pub const MSG_CHILD_EXITED: u32 = 0x4010;
    
    /// Resource warning notification.
    pub const MSG_RESOURCE_WARNING: u32 = 0x4020;
}
```

### Kill Request

```rust
/// Kill process request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KillRequest {
    /// Process to kill
    pub pid: ProcessId,
    
    /// Signal type
    pub signal: KillSignal,
}

/// Kill signal types.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum KillSignal {
    /// Request graceful termination
    Term,
    
    /// Force immediate termination
    Kill,
    
    /// Suspend process
    Stop,
    
    /// Resume suspended process
    Continue,
}

/// Kill response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KillResponse {
    pub result: Result<(), KillError>,
}

/// Kill errors.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum KillError {
    /// Process not found
    ProcessNotFound,
    
    /// Permission denied
    PermissionDenied,
    
    /// Cannot kill system process
    SystemProcess,
}
```

## Spawn Flow

```
   Client                  ProcessManager              Kernel
     │                          │                        │
     │  MSG_SPAWN               │                        │
     │  (name, binary, caps)    │                        │
     │─────────────────────────▶│                        │
     │                          │                        │
     │                          │  Load binary           │
     │                          │  (from VFS)            │
     │                          │                        │
     │                          │  Check permissions     │
     │                          │  (via Permission svc)  │
     │                          │                        │
     │                          │  Syscall: spawn        │
     │                          │───────────────────────▶│
     │                          │                        │  Create process
     │                          │                        │  Allocate PID
     │                          │◀───────────────────────│
     │                          │  { pid: 5 }            │
     │                          │                        │
     │                          │  Grant capabilities    │
     │                          │  to new process        │
     │                          │───────────────────────▶│
     │                          │◀───────────────────────│
     │                          │                        │
     │                          │  Update session        │
     │                          │  (add process)         │
     │                          │                        │
     │  MSG_SPAWN_RESPONSE      │                        │
     │  { pid: 5 }              │                        │
     │◀─────────────────────────│                        │
```

## Implementation

### Permission Checking

```rust
impl ProcessManager {
    fn check_spawn_permission(
        &self,
        requester: ProcessId,
        request: &SpawnRequest,
    ) -> Result<(), SpawnError> {
        // 1. Check requester has spawn capability
        if !self.has_capability(requester, CAP_SPAWN) {
            return Err(SpawnError::PermissionDenied);
        }
        
        // 2. Check user context is valid
        if let Some(user_id) = &request.user_id {
            if !self.user_exists(user_id) {
                return Err(SpawnError::UserNotFound);
            }
        }
        
        // 3. Check session is valid
        if let Some(session_id) = &request.session_id {
            if !self.session_valid(session_id) {
                return Err(SpawnError::SessionNotFound);
            }
        }
        
        // 4. Check requester can grant requested capabilities
        for cap_req in &request.capabilities {
            let check = PermissionCheckRequest {
                requester,
                capability_type: cap_req.cap_type.clone(),
                permissions: cap_req.permissions,
                context: cap_req.reason.clone(),
            };
            
            let response = self.permission_service.check_permission(&check);
            if !response.allowed {
                return Err(SpawnError::CapabilityDenied(
                    cap_req.cap_type.clone()
                ));
            }
        }
        
        // 5. Check resource limits
        if let Some(max_mem) = request.limits.max_memory {
            if max_mem > self.get_available_memory() {
                return Err(SpawnError::ResourceLimitExceeded);
            }
        }
        
        Ok(())
    }
}
```

### Capability Distribution

```rust
impl ProcessManager {
    fn grant_initial_capabilities(
        &mut self,
        pid: ProcessId,
        requests: &[CapRequest],
    ) -> Vec<CapSlot> {
        let mut granted = Vec::new();
        
        for req in requests {
            // Map cap_type to actual capability
            let source_cap = match req.cap_type.as_str() {
                "console" => self.console_cap,
                "storage" => self.storage_cap,
                "storage-ro" => self.storage_ro_cap,
                "network" => self.network_cap,
                "vfs" => self.vfs_cap,
                "identity" => self.identity_cap,
                _ => continue,
            };
            
            // Attenuate permissions
            let perms = self.attenuate(source_cap, &req.permissions);
            
            // Grant to new process
            let slot = syscall_cap_grant(source_cap, pid, perms);
            granted.push(slot);
        }
        
        granted
    }
    
    fn attenuate(&self, source_cap: CapSlot, requested: &Permissions) -> Permissions {
        let source_perms = self.get_cap_permissions(source_cap);
        
        // Result is intersection of source and requested
        Permissions {
            read: source_perms.read && requested.read,
            write: source_perms.write && requested.write,
            execute: source_perms.execute && requested.execute,
            grant: source_perms.grant && requested.grant,
        }
    }
}
```

### Resource Enforcement

```rust
/// Per-process resource tracking.
struct ProcessResources {
    pid: ProcessId,
    limits: ResourceLimits,
    usage: ResourceUsage,
}

struct ResourceUsage {
    memory_bytes: usize,
    cpu_time_ns: u64,
    cap_count: u32,
    ipc_queue_depth: usize,
}

impl ProcessManager {
    fn check_resource_violation(&self, pid: ProcessId) -> Option<ResourceViolation> {
        let resources = self.resources.get(&pid)?;
        
        if let Some(max) = resources.limits.max_memory {
            if resources.usage.memory_bytes > max {
                return Some(ResourceViolation::Memory);
            }
        }
        
        if let Some(max) = resources.limits.max_cpu_time {
            if resources.usage.cpu_time_ns > max {
                return Some(ResourceViolation::CpuTime);
            }
        }
        
        if let Some(max) = resources.limits.max_caps {
            if resources.usage.cap_count > max {
                return Some(ResourceViolation::CapabilityCount);
            }
        }
        
        None
    }
    
    fn handle_violation(&mut self, pid: ProcessId, violation: ResourceViolation) {
        debug(&format!("Resource violation: {:?} for PID {}", violation, pid.0));
        
        // Notify the process (give it a chance to clean up)
        self.send_resource_warning(pid, violation);
        
        // Track violation count
        let count = self.violation_counts.entry(pid).or_insert(0);
        *count += 1;
        
        // If persistent, kill
        if *count > MAX_VIOLATIONS {
            self.kill_process(pid, KillSignal::Kill);
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ResourceViolation {
    Memory,
    CpuTime,
    CapabilityCount,
    IpcQueueDepth,
}

const MAX_VIOLATIONS: u32 = 3;
```

## Integration with Supervision

The Process Manager integrates with init's supervision model:

```rust
impl Init {
    fn spawn_service(&mut self, name: &str) -> Result<ProcessId, SpawnError> {
        let request = SpawnRequest {
            name: name.to_string(),
            binary: BinarySource::WellKnown(name.to_string()),
            capabilities: self.default_caps_for_service(name),
            limits: self.default_limits_for_service(name),
            user_id: None,  // System services have no user
            session_id: None,
            env: vec![],
            working_dir: None,
        };
        
        let pid = self.process_manager.spawn(request)?;
        
        // Register in supervision tree
        self.supervised.insert(pid, SupervisionEntry {
            name: name.to_string(),
            restart_policy: RestartPolicy::Always,
            crash_count: 0,
            last_crash: None,
        });
        
        Ok(pid)
    }
    
    fn handle_child_exit(&mut self, pid: ProcessId, exit_code: i32) {
        if let Some(entry) = self.supervised.get_mut(&pid) {
            match entry.restart_policy {
                RestartPolicy::Always => {
                    self.schedule_restart(&entry.name);
                }
                RestartPolicy::OnFailure if exit_code != 0 => {
                    self.schedule_restart(&entry.name);
                }
                RestartPolicy::Never | RestartPolicy::OnFailure => {
                    // Don't restart
                }
            }
        }
    }
}
```

## WASM Implementation

```rust
// process_manager.rs (part of init)

#![no_std]
extern crate alloc;
extern crate zero_process;

use alloc::collections::BTreeMap;
use zero_process::*;

struct ProcessManager {
    process_table: BTreeMap<u32, ProcessInfo>,
    resources: BTreeMap<u32, ProcessResources>,
    service_ep: CapSlot,
}

impl ProcessManager {
    fn handle_spawn(&mut self, msg: ReceivedMessage) {
        let request: SpawnRequest = decode(&msg.data);
        let reply_ep = msg.cap_slots.get(0);
        
        // Check permissions
        if let Err(e) = self.check_spawn_permission(msg.from, &request) {
            if let Some(ep) = reply_ep {
                send(*ep, MSG_SPAWN_RESPONSE, &encode_error(e));
            }
            return;
        }
        
        // Load binary and spawn
        match self.do_spawn(&request) {
            Ok(result) => {
                // Grant initial capabilities
                let caps = self.grant_initial_capabilities(result.pid, &request.capabilities);
                
                // Update session if applicable
                if let Some(session_id) = &request.session_id {
                    self.add_process_to_session(session_id, result.pid);
                }
                
                if let Some(ep) = reply_ep {
                    send(*ep, MSG_SPAWN_RESPONSE, &encode_success(result.pid, caps));
                }
            }
            Err(e) => {
                if let Some(ep) = reply_ep {
                    send(*ep, MSG_SPAWN_RESPONSE, &encode_error(e));
                }
            }
        }
    }
}
```

## Invariants

1. **PID uniqueness**: Process IDs are unique and never reused during runtime
2. **Parent validity**: Every process (except init) has a valid parent
3. **Capability integrity**: Processes can only grant capabilities they hold
4. **Resource tracking**: Usage never exceeds hard limits
5. **Session consistency**: Processes in session are tracked

## Security Considerations

1. **Spawn authorization**: All spawns go through permission service
2. **Capability attenuation**: Grants can only reduce permissions
3. **Resource isolation**: Per-process limits prevent DoS
4. **User context**: Processes inherit user context from spawner
5. **System protection**: System processes cannot be killed by applications

## Related Specifications

- [02-supervision.md](02-supervision.md) - Service restart policies
- [../05-identity/04-permissions.md](../05-identity/04-permissions.md) - Permission checking
- [../05-identity/02-sessions.md](../05-identity/02-sessions.md) - Session management
- [../03-kernel/03-capabilities.md](../03-kernel/03-capabilities.md) - Kernel capability system
