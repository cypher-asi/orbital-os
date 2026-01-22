# Inter-Process Communication

> IPC is the mechanism by which processes communicate. All IPC is mediated by capabilities.

## Overview

Zero OS uses synchronous message passing with capability transfer. Key features:

1. **Capability-Mediated**: All IPC requires a valid endpoint capability
2. **Message Queuing**: Messages are queued at endpoints
3. **Capability Transfer**: Capabilities can be sent with messages
4. **Non-Blocking Receive**: Receive returns immediately if no message

## Endpoints

An endpoint is a message queue owned by a process.

```rust
/// IPC endpoint.
pub struct Endpoint {
    /// Unique endpoint ID
    pub id: EndpointId,
    
    /// Owning process (can receive)
    pub owner: ProcessId,
    
    /// Queue of pending messages
    pub queue: VecDeque<Message>,
    
    /// Maximum queue depth
    pub max_queue_depth: usize,
    
    /// Metrics for monitoring
    pub metrics: EndpointMetrics,
}

#[derive(Clone, Debug, Default)]
pub struct EndpointMetrics {
    /// Current queue depth
    pub queue_depth: usize,
    /// Total messages ever received
    pub total_messages: u64,
    /// Total bytes received
    pub total_bytes: u64,
    /// High water mark
    pub queue_high_water: usize,
}
```

## Messages

```rust
/// IPC message.
#[derive(Clone, Debug)]
pub struct Message {
    /// Sender process ID
    pub from: ProcessId,
    
    /// Application-defined message tag
    pub tag: u32,
    
    /// Message payload (max 4KB)
    pub data: Vec<u8>,
    
    /// Capabilities being transferred (slots from sender's CSpace)
    pub transferred_caps: Vec<TransferredCap>,
}

/// A capability being transferred in a message.
#[derive(Clone, Debug)]
pub struct TransferredCap {
    /// The capability (copied from sender)
    pub capability: Capability,
    /// Slot it will occupy in receiver's CSpace
    pub receiver_slot: Option<CapSlot>,
}

/// Maximum message payload size.
pub const MAX_MESSAGE_SIZE: usize = 4096;

/// Maximum capabilities per message.
pub const MAX_CAPS_PER_MESSAGE: usize = 8;
```

## IPC Operations

### Create Endpoint

```rust
/// Create a new IPC endpoint.
///
/// # Pre-conditions
/// - Process exists
///
/// # Post-conditions
/// - New endpoint created
/// - Owner receives full capability in CSpace
/// - Logged to Axiom (Create operation)
///
/// # Returns
/// - `(EndpointId, CapSlot)`: Endpoint ID and capability slot
pub fn endpoint_create(
    kernel: &mut Kernel,
    owner: ProcessId,
) -> Result<(EndpointId, CapSlot), KernelError> {
    // Validate process exists
    if !kernel.processes.contains_key(&owner) {
        return Err(KernelError::ProcessNotFound);
    }
    
    // Allocate endpoint ID
    let id = EndpointId(kernel.next_endpoint_id);
    kernel.next_endpoint_id += 1;
    
    // Create endpoint
    let endpoint = Endpoint {
        id,
        owner,
        queue: VecDeque::new(),
        max_queue_depth: 256,
        metrics: EndpointMetrics::default(),
    };
    kernel.endpoints.insert(id, endpoint);
    
    // Create capability for owner (full permissions)
    let cap = Capability {
        id: kernel.next_cap_id(),
        object_type: ObjectType::Endpoint,
        object_id: id.0,
        permissions: Permissions::full(),
        generation: 0,
        expires_at: 0,
    };
    
    // Log to Axiom
    kernel.axiom_log.append(
        owner,
        CapOperation::Create {
            cap_id: cap.id,
            object_type: ObjectType::Endpoint,
            object_id: id.0,
            holder: owner,
        },
        kernel.hal.now_nanos(),
    );
    
    // Insert into owner's CSpace
    let slot = kernel.cap_spaces.get_mut(&owner)
        .ok_or(KernelError::ProcessNotFound)?
        .insert(cap);
    
    Ok((id, slot))
}
```

### Send Message

```rust
/// Send a message to an endpoint.
///
/// # Pre-conditions
/// - Capability in `slot` must:
///   - Exist in sender's CSpace
///   - Reference an Endpoint
///   - Have Write permission
/// - Message size <= MAX_MESSAGE_SIZE
/// - Caps to transfer <= MAX_CAPS_PER_MESSAGE
///
/// # Post-conditions
/// - Message queued at endpoint
/// - If transferring caps: caps moved from sender to message
/// - Sender metrics updated
/// - Endpoint metrics updated
///
/// # Errors
/// - `InvalidCapability`: Slot empty or wrong type
/// - `PermissionDenied`: No write permission
/// - `EndpointNotFound`: Endpoint no longer exists
/// - `QueueFull`: Endpoint queue at maximum
pub fn ipc_send(
    kernel: &mut Kernel,
    sender: ProcessId,
    slot: CapSlot,
    tag: u32,
    data: Vec<u8>,
    cap_slots: Vec<CapSlot>,  // Caps to transfer
) -> Result<(), KernelError> {
    // Axiom check: must have write permission on endpoint
    let cspace = kernel.cap_spaces.get(&sender)
        .ok_or(KernelError::ProcessNotFound)?;
    let cap = axiom_check(
        cspace,
        slot,
        Permissions::write_only(),
        Some(ObjectType::Endpoint),
    ).map_err(|_| KernelError::InvalidCapability)?;
    
    let endpoint_id = EndpointId(cap.object_id);
    
    // Validate message size
    if data.len() > MAX_MESSAGE_SIZE {
        return Err(KernelError::InvalidArgument);
    }
    
    // Validate cap transfer count
    if cap_slots.len() > MAX_CAPS_PER_MESSAGE {
        return Err(KernelError::InvalidArgument);
    }
    
    // Get endpoint
    let endpoint = kernel.endpoints.get_mut(&endpoint_id)
        .ok_or(KernelError::EndpointNotFound)?;
    
    // Check queue capacity
    if endpoint.queue.len() >= endpoint.max_queue_depth {
        return Err(KernelError::QueueFull);
    }
    
    // Collect capabilities to transfer
    let mut transferred_caps = Vec::new();
    let sender_cspace = kernel.cap_spaces.get_mut(&sender)
        .ok_or(KernelError::ProcessNotFound)?;
    
    for cap_slot in cap_slots {
        // Must have grant permission to transfer
        let transfer_cap = sender_cspace.get(cap_slot)
            .ok_or(KernelError::InvalidCapability)?
            .clone();
        
        if !transfer_cap.permissions.grant {
            return Err(KernelError::PermissionDenied);
        }
        
        transferred_caps.push(TransferredCap {
            capability: transfer_cap,
            receiver_slot: None,  // Will be assigned on receive
        });
        
        // Remove from sender's CSpace
        sender_cspace.remove(cap_slot);
        
        // Log transfer
        kernel.axiom_log.append(
            sender,
            CapOperation::Transfer {
                cap_id: transfer_cap.id,
                from_pid: sender,
                to_pid: endpoint.owner,
            },
            kernel.hal.now_nanos(),
        );
    }
    
    // Build message
    let data_len = data.len();
    let message = Message {
        from: sender,
        tag,
        data,
        transferred_caps,
    };
    
    // Queue message
    endpoint.queue.push_back(message);
    
    // Update metrics
    endpoint.metrics.queue_depth = endpoint.queue.len();
    endpoint.metrics.total_messages += 1;
    endpoint.metrics.total_bytes += data_len as u64;
    if endpoint.metrics.queue_depth > endpoint.metrics.queue_high_water {
        endpoint.metrics.queue_high_water = endpoint.metrics.queue_depth;
    }
    
    // Update sender process metrics
    if let Some(proc) = kernel.processes.get_mut(&sender) {
        proc.metrics.ipc_sent += 1;
        proc.metrics.ipc_bytes_sent += data_len as u64;
    }
    
    Ok(())
}
```

### Receive Message

```rust
/// Receive a message from an endpoint (non-blocking).
///
/// # Pre-conditions
/// - Capability in `slot` must:
///   - Exist in receiver's CSpace
///   - Reference an Endpoint owned by receiver
///   - Have Read permission
///
/// # Post-conditions
/// - If message available: dequeued and returned
/// - Transferred caps inserted into receiver's CSpace
/// - Receiver metrics updated
///
/// # Returns
/// - `Ok(Some(message))`: Message received
/// - `Ok(None)`: No message available (would block)
/// - `Err(...)`: Error
pub fn ipc_receive(
    kernel: &mut Kernel,
    receiver: ProcessId,
    slot: CapSlot,
) -> Result<Option<ReceivedMessage>, KernelError> {
    // Axiom check: must have read permission
    let cspace = kernel.cap_spaces.get(&receiver)
        .ok_or(KernelError::ProcessNotFound)?;
    let cap = axiom_check(
        cspace,
        slot,
        Permissions::read_only(),
        Some(ObjectType::Endpoint),
    ).map_err(|_| KernelError::InvalidCapability)?;
    
    let endpoint_id = EndpointId(cap.object_id);
    
    // Get endpoint
    let endpoint = kernel.endpoints.get_mut(&endpoint_id)
        .ok_or(KernelError::EndpointNotFound)?;
    
    // Check ownership
    if endpoint.owner != receiver {
        return Err(KernelError::PermissionDenied);
    }
    
    // Try to dequeue
    let message = match endpoint.queue.pop_front() {
        Some(msg) => msg,
        None => return Ok(None),  // Would block
    };
    
    // Update endpoint metrics
    endpoint.metrics.queue_depth = endpoint.queue.len();
    
    // Insert transferred capabilities into receiver's CSpace
    let receiver_cspace = kernel.cap_spaces.get_mut(&receiver)
        .ok_or(KernelError::ProcessNotFound)?;
    
    let mut received_cap_slots = Vec::new();
    for mut tc in message.transferred_caps {
        let slot = receiver_cspace.insert(tc.capability);
        received_cap_slots.push(slot);
    }
    
    // Update receiver process metrics
    if let Some(proc) = kernel.processes.get_mut(&receiver) {
        proc.metrics.ipc_received += 1;
        proc.metrics.ipc_bytes_received += message.data.len() as u64;
    }
    
    Ok(Some(ReceivedMessage {
        from: message.from,
        tag: message.tag,
        data: message.data,
        cap_slots: received_cap_slots,
    }))
}

/// Message as received by a process.
#[derive(Clone, Debug)]
pub struct ReceivedMessage {
    pub from: ProcessId,
    pub tag: u32,
    pub data: Vec<u8>,
    /// Slots where transferred capabilities were placed
    pub cap_slots: Vec<CapSlot>,
}
```

### Call (Send + Receive)

Synchronous RPC pattern:

```rust
/// Send a message and wait for a reply.
///
/// This is a convenience combining Send + Receive.
/// Creates a temporary reply endpoint.
pub fn ipc_call(
    kernel: &mut Kernel,
    caller: ProcessId,
    endpoint_slot: CapSlot,
    tag: u32,
    data: Vec<u8>,
) -> Result<ReceivedMessage, KernelError> {
    // Create temporary reply endpoint
    let (reply_eid, reply_slot) = endpoint_create(kernel, caller)?;
    
    // Grant write-only capability to reply endpoint (send with message)
    // ... (would need to extend message format)
    
    // Send message with reply endpoint
    ipc_send(kernel, caller, endpoint_slot, tag, data, vec![reply_slot])?;
    
    // Wait for reply (would block in real implementation)
    loop {
        if let Some(reply) = ipc_receive(kernel, caller, reply_slot)? {
            // Cleanup reply endpoint
            kernel.endpoints.remove(&reply_eid);
            return Ok(reply);
        }
        // Yield and retry (on WASM) or block (on native)
        thread_yield(kernel, caller);
    }
}
```

## Capability Transfer Protocol

When transferring capabilities via IPC:

1. **Sender** includes capability slots in send request
2. **Axiom** logs each transfer
3. **Kernel** moves capabilities from sender's CSpace to message
4. **Receiver** gets message with new slots allocated in their CSpace

```
┌───────────────┐                                    ┌───────────────┐
│    Sender     │                                    │   Receiver    │
│               │                                    │               │
│  CSpace:      │                                    │  CSpace:      │
│  [5] = CapA   │─────────── Send [5] ───────────▶  │  [8] = CapA   │
│  [6] = CapB   │                                    │               │
│               │                                    │               │
│  After send:  │                                    │               │
│  [5] = empty  │                                    │               │
│  [6] = CapB   │                                    │               │
└───────────────┘                                    └───────────────┘
```

## WASM IPC Flow

On WASM, IPC is mediated by the JavaScript supervisor:

```
┌─────────────────┐        ┌─────────────────┐        ┌─────────────────┐
│  Worker A       │        │   Supervisor    │        │  Worker B       │
│  (sender)       │        │   (JS + kernel) │        │  (receiver)     │
│                 │        │                 │        │                 │
│  send(slot,     │        │                 │        │                 │
│    tag, data)   │───────▶│  1. Validate    │        │                 │
│                 │        │     capability  │        │                 │
│                 │        │  2. Queue msg   │        │                 │
│                 │        │  3. Return OK   │        │                 │
│  ◀──────────────│────────│                 │        │                 │
│                 │        │                 │        │                 │
│                 │        │                 │        │  receive(slot)  │
│                 │        │  4. Dequeue msg │◀───────│                 │
│                 │        │  5. Return data │───────▶│                 │
│                 │        │                 │        │  got message!   │
└─────────────────┘        └─────────────────┘        └─────────────────┘
```

## Message Tags

Common message tags for system protocols:

```rust
/// System message tags.
pub mod tags {
    /// Console/terminal write
    pub const CONSOLE_WRITE: u32 = 0x0001;
    /// Console input
    pub const CONSOLE_INPUT: u32 = 0x0002;
    /// Process spawn request
    pub const SPAWN_REQUEST: u32 = 0x0010;
    /// Process spawn response
    pub const SPAWN_RESPONSE: u32 = 0x0011;
    /// Capability request
    pub const CAP_REQUEST: u32 = 0x0020;
    /// Capability grant
    pub const CAP_GRANT: u32 = 0x0021;
    /// Storage read
    pub const STORAGE_READ: u32 = 0x0100;
    /// Storage write
    pub const STORAGE_WRITE: u32 = 0x0101;
    /// Network request
    pub const NET_REQUEST: u32 = 0x0200;
    /// Network response
    pub const NET_RESPONSE: u32 = 0x0201;
}
```

## Compatibility with Current Code

The current `Zero-kernel` implementation already includes:

- `Message` struct with `from`, `tag`, `data`, `caps` fields
- `Endpoint` struct with `id`, `owner`, `pending_messages`, `metrics`
- `ipc_send` and `ipc_receive` methods with capability checking
- Endpoint creation via `create_endpoint`

The spec adds:
- Explicit `TransferredCap` structure for cap transfer
- `ipc_call` for RPC pattern
- Axiom logging for transfers

## LOC Budget

Target: ~400 LOC for IPC subsystem.

| Component          | Estimated LOC |
|--------------------|---------------|
| Endpoint struct    | ~30           |
| Message struct     | ~40           |
| endpoint_create    | ~50           |
| ipc_send           | ~100          |
| ipc_receive        | ~80           |
| ipc_call           | ~60           |
| Metrics            | ~40           |
| **Total**          | **~400**      |
