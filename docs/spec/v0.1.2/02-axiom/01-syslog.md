# SysLog: Syscall Audit Trail

> The SysLog records every syscall (request and response) for audit purposes. It is NOT used for state replay.

## Overview

The SysLog provides:

1. **Complete Audit Trail**: Every syscall request and response is recorded
2. **Traceability**: Links between requests, responses, and resulting commits
3. **Debugging**: "What happened and when" for any process

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              SysLog                                         │
│                                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │
│  │ SysEvent    │  │ SysEvent    │  │ SysEvent    │  │ SysEvent    │       │
│  │ (Request)   │──│ (Response)  │──│ (Request)   │──│ (Response)  │─ ...  │
│  │ id: 0xe123  │  │ id: 0xe124  │  │ id: 0xe125  │  │ id: 0xe126  │       │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘       │
│                                                                             │
│  Purpose: Audit trail ("what was asked, what was answered")                 │
│  NOT used for replay                                                        │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Data Structures

### SysEvent

A syscall request or response:

```rust
/// A syscall request or response recorded in the SysLog.
///
/// # Invariants
/// - `id` is computed as SHA-256 of the event contents
/// - `sender` is verified from trusted context, never from payload
/// - Events are append-only; once written, never modified
#[derive(Clone, Debug)]
pub struct SysEvent {
    /// Unique identifier (SHA-256 hash of this event)
    pub id: EventId,
    
    /// Timestamp (nanoseconds since boot)
    pub timestamp: u64,
    
    /// Process that created this event (verified from context)
    pub sender: ProcessId,
    
    /// Event type and payload
    pub event_type: SysEventType,
}

/// SHA-256 hash identifying a SysEvent.
pub type EventId = [u8; 32];
```

### SysEventType

All syscall types and their payloads:

```rust
/// Syscall event types.
///
/// Requests come from applications; responses come from the kernel.
#[derive(Clone, Debug)]
pub enum SysEventType {
    // ═══════════════════════════════════════════════════════════════════════
    // REQUESTS (from applications)
    // ═══════════════════════════════════════════════════════════════════════
    
    /// Grant a capability to another process.
    CapGrant {
        to_pid: ProcessId,
        slot: CapSlot,
        perms: Permissions,
    },
    
    /// Revoke a capability.
    CapRevoke { slot: CapSlot },
    
    /// Delete a capability from CSpace.
    CapDelete { slot: CapSlot },
    
    /// Create a new IPC endpoint.
    EndpointCreate,
    
    /// Send a message to an endpoint.
    Send {
        endpoint_slot: CapSlot,
        tag: u32,
        data: Bytes,
        caps: Vec<CapSlot>,
    },
    
    /// Receive a message from an endpoint.
    Receive { endpoint_slot: CapSlot },
    
    /// Spawn a new process.
    Spawn {
        binary: BinaryRef,
        caps: Vec<CapSlot>,
    },
    
    /// Exit the current process.
    Exit { code: i32 },
    
    /// Yield execution.
    Yield,
    
    /// Debug output.
    Debug { message: Bytes },
    
    /// Get current time.
    GetTime,
    
    /// Get current process ID.
    GetPid,
    
    // ═══════════════════════════════════════════════════════════════════════
    // RESPONSES (from kernel)
    // ═══════════════════════════════════════════════════════════════════════
    
    /// Successful response with optional data.
    Ok {
        /// Reference to the request this responds to
        ref_event: EventId,
        /// Response data (interpretation depends on request type)
        data: Bytes,
    },
    
    /// Error response.
    Err {
        /// Reference to the request this responds to
        ref_event: EventId,
        /// Error code
        code: ErrorCode,
    },
}
```

### ErrorCode

Standard error codes:

```rust
/// Error codes returned in SysEvent::Err responses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ErrorCode {
    /// Permission denied (capability check failed)
    PermissionDenied = 1,
    /// Object not found
    NotFound = 2,
    /// Invalid argument
    InvalidArgument = 3,
    /// Syscall not implemented
    NotImplemented = 4,
    /// Would block (try again)
    WouldBlock = 5,
    /// Out of memory
    OutOfMemory = 6,
    /// Invalid capability slot
    InvalidSlot = 7,
    /// Resource busy
    Busy = 8,
    /// Already exists
    AlreadyExists = 9,
    /// Buffer too small
    BufferTooSmall = 10,
}
```

### BinaryRef

Reference to a process binary:

```rust
/// Reference to a process binary.
#[derive(Clone, Debug)]
pub enum BinaryRef {
    /// Load from storage path
    Path(String),
    /// Content hash (for content-addressed storage)
    Hash([u8; 32]),
    /// Inline binary data (for small/embedded binaries)
    Inline(Bytes),
}
```

## SysLog Structure

```rust
/// The SysLog: append-only log of all syscall events.
///
/// # Invariants
/// - Events are append-only; once written, never modified or removed
/// - Event IDs are unique (SHA-256 of event contents)
/// - Timestamps are monotonically increasing
pub struct SysLog {
    /// Log entries (append-only)
    entries: Vec<SysEvent>,
    /// Next entry index
    next_index: u64,
}

impl SysLog {
    /// Create an empty SysLog.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_index: 0,
        }
    }
    
    /// Append a syscall request to the log.
    ///
    /// # Arguments
    /// - `sender`: Verified sender PID (from trusted context)
    /// - `event_type`: The syscall request type
    /// - `timestamp`: Current timestamp
    ///
    /// # Returns
    /// The EventId of the logged event
    pub fn append_request(
        &mut self,
        sender: ProcessId,
        event_type: SysEventType,
        timestamp: u64,
    ) -> EventId {
        let event = SysEvent {
            id: [0u8; 32],  // Computed below
            timestamp,
            sender,
            event_type,
        };
        
        let id = compute_event_id(&event);
        let event = SysEvent { id, ..event };
        
        self.entries.push(event);
        self.next_index += 1;
        
        id
    }
    
    /// Append a syscall response to the log.
    ///
    /// # Arguments
    /// - `ref_event`: The request this responds to
    /// - `result`: Ok with data or Err with code
    /// - `timestamp`: Current timestamp
    ///
    /// # Returns
    /// The EventId of the logged response
    pub fn append_response(
        &mut self,
        ref_event: EventId,
        result: Result<Bytes, ErrorCode>,
        timestamp: u64,
    ) -> EventId {
        let event_type = match result {
            Ok(data) => SysEventType::Ok { ref_event, data },
            Err(code) => SysEventType::Err { ref_event, code },
        };
        
        // Response sender is the kernel (PID 0)
        let event = SysEvent {
            id: [0u8; 32],
            timestamp,
            sender: ProcessId(0),
            event_type,
        };
        
        let id = compute_event_id(&event);
        let event = SysEvent { id, ..event };
        
        self.entries.push(event);
        self.next_index += 1;
        
        id
    }
    
    /// Get events by index range.
    pub fn get_range(&self, start: u64, end: u64) -> &[SysEvent] {
        let start = start as usize;
        let end = (end as usize).min(self.entries.len());
        &self.entries[start..end]
    }
    
    /// Get event by ID.
    pub fn get_by_id(&self, id: &EventId) -> Option<&SysEvent> {
        self.entries.iter().find(|e| &e.id == id)
    }
    
    /// Get all events from a specific sender.
    pub fn get_by_sender(&self, sender: ProcessId) -> impl Iterator<Item = &SysEvent> {
        self.entries.iter().filter(move |e| e.sender == sender)
    }
}

/// Compute SHA-256 hash of a SysEvent (excluding the id field).
fn compute_event_id(event: &SysEvent) -> EventId {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(&event.timestamp.to_le_bytes());
    hasher.update(&event.sender.0.to_le_bytes());
    hasher.update(&serialize_event_type(&event.event_type));
    hasher.finalize().into()
}
```

## Request-Response Linking

Every response links back to its request via `ref_event`:

```
Request                              Response
┌─────────────────────────┐          ┌─────────────────────────┐
│ SysEvent                │          │ SysEvent                │
│                         │          │                         │
│ id: 0xe123...           │◀─────────│ event_type: Ok {        │
│ sender: PID 5           │          │   ref_event: 0xe123...  │
│ event_type: CapGrant    │          │   data: { slot: 7 }     │
│                         │          │ }                       │
└─────────────────────────┘          └─────────────────────────┘
```

This enables:

- **Tracing**: Follow any request to its response
- **Debugging**: "What happened to this syscall?"
- **Auditing**: Complete request/response pairs

## Commit Linking

Commits link back to SysEvents via `caused_by`:

```
SysEvent (Request)        Commit                    SysEvent (Response)
┌─────────────────┐       ┌─────────────────┐       ┌─────────────────┐
│ id: 0xe123...   │◀──────│ caused_by:      │       │ ref_event:      │
│ type: CapGrant  │       │   0xe123...     │       │   0xe123...     │
│                 │       │ type: CapInsert │       │ type: Ok        │
└─────────────────┘       └─────────────────┘       └─────────────────┘
         │                                                   │
         └───────────────────────────────────────────────────┘
                          Same request ID
```

This enables:

- **Why did this state change?** → trace `caused_by` to SysEvent
- **What commits did this syscall generate?** → search CommitLog by `caused_by`

## Error Handling

Failed syscalls are recorded in SysLog but generate NO commits:

```rust
// Syscall flow for failed request
fn handle_cap_grant(request: CapGrantRequest) {
    // 1. Log request to SysLog
    let event_id = syslog.append_request(sender, SysEventType::CapGrant { ... });
    
    // 2. Kernel processes, finds error
    let result = kernel.process_cap_grant(request);
    
    match result {
        Ok(slot) => {
            // 3a. Success: emit commit AND log response
            commitlog.append(Commit::CapInserted { ... });
            syslog.append_response(event_id, Ok(slot.encode()));
        }
        Err(e) => {
            // 3b. Failure: NO commit, just log error response
            syslog.append_response(event_id, Err(e));
        }
    }
}
```

The SysLog captures both successful and failed syscalls; the CommitLog only captures state changes.

## SysLog vs CommitLog

| Aspect | SysLog | CommitLog |
|--------|--------|-----------|
| **Contents** | All requests + responses | State mutations only |
| **Errors** | Records errors | Never records errors |
| **Replay** | Not used | Source of truth |
| **Deletable** | Yes (audit only) | No (state source) |
| **Size** | Larger | Smaller |
| **Purpose** | "What happened" | "What changed" |

## Querying the SysLog

Common queries:

```rust
impl SysLog {
    /// Get all syscalls from a process in a time range.
    pub fn query_by_sender_and_time(
        &self,
        sender: ProcessId,
        start_time: u64,
        end_time: u64,
    ) -> Vec<&SysEvent> {
        self.entries
            .iter()
            .filter(|e| {
                e.sender == sender &&
                e.timestamp >= start_time &&
                e.timestamp <= end_time
            })
            .collect()
    }
    
    /// Get the response for a request.
    pub fn get_response(&self, request_id: &EventId) -> Option<&SysEvent> {
        self.entries.iter().find(|e| {
            match &e.event_type {
                SysEventType::Ok { ref_event, .. } => ref_event == request_id,
                SysEventType::Err { ref_event, .. } => ref_event == request_id,
                _ => false,
            }
        })
    }
    
    /// Get all failed syscalls from a process.
    pub fn get_failures(&self, sender: ProcessId) -> Vec<&SysEvent> {
        let request_ids: Vec<_> = self.entries
            .iter()
            .filter(|e| e.sender == sender && !matches!(e.event_type, SysEventType::Ok { .. } | SysEventType::Err { .. }))
            .map(|e| e.id)
            .collect();
        
        self.entries
            .iter()
            .filter(|e| {
                if let SysEventType::Err { ref_event, .. } = &e.event_type {
                    request_ids.contains(ref_event)
                } else {
                    false
                }
            })
            .collect()
    }
}
```

## WASM Persistence

On WASM, the SysLog is persisted to IndexedDB:

```rust
/// WASM-specific SysLog persistence.
#[cfg(target_arch = "wasm32")]
impl SysLog {
    /// Persist recent entries to IndexedDB.
    ///
    /// # Note
    /// This is async; entries may not be durable immediately.
    pub async fn persist(&self, db: &IndexedDb) -> Result<(), StorageError> {
        let tx = db.transaction("syslog", TransactionMode::ReadWrite)?;
        let store = tx.object_store("syslog")?;
        
        for entry in &self.entries {
            store.put(&entry.id, &serialize(entry))?;
        }
        
        tx.commit().await
    }
    
    /// Load SysLog from IndexedDB.
    pub async fn load(db: &IndexedDb) -> Result<Self, StorageError> {
        let tx = db.transaction("syslog", TransactionMode::ReadOnly)?;
        let store = tx.object_store("syslog")?;
        
        let entries: Vec<SysEvent> = store
            .get_all()?
            .await?
            .into_iter()
            .map(|bytes| deserialize(&bytes))
            .collect::<Result<_, _>>()?;
        
        Ok(Self {
            next_index: entries.len() as u64,
            entries,
        })
    }
}
```

## Retention Policy

The SysLog can grow large. Retention policies:

1. **Time-based**: Keep last N days of syscalls
2. **Size-based**: Keep last N entries
3. **Checkpoint-based**: Keep entries since last verified checkpoint

```rust
/// Truncate SysLog to entries after a checkpoint.
///
/// # Safety
/// Only call after verifying the checkpoint is valid.
pub fn truncate_before(&mut self, checkpoint_time: u64) {
    self.entries.retain(|e| e.timestamp >= checkpoint_time);
}
```

Unlike the CommitLog, truncating the SysLog does NOT affect system state.
