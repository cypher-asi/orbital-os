# Capabilities: Unforgeable Authority

> Capabilities are unforgeable tokens that grant authority over kernel objects. The kernel verifies capabilities before executing any privileged operation.

## Overview

Capabilities provide:

1. **Unforgeable Authority**: Only the kernel can create capabilities
2. **Fine-Grained Control**: Permissions can be attenuated when granted
3. **Object References**: Each capability references a specific kernel object
4. **Revocable Access**: Capabilities can be revoked by holders with grant permission

## Capability Checking

Every syscall that requires authority calls `check_capability`:

```rust
/// Check if a process has authority to perform an operation.
///
/// # Arguments
/// - `cspace`: The process's capability space
/// - `slot`: The capability slot being used
/// - `required_perms`: Minimum permissions needed
/// - `expected_type`: Expected object type (optional)
///
/// # Returns
/// - `Ok(&Capability)`: Authority granted, reference to the capability
/// - `Err(CapError)`: Authority denied with reason
///
/// # Invariants
/// - This function never modifies any state
/// - All kernel operations call this before executing
pub fn check_capability(
    cspace: &CapabilitySpace,
    slot: CapSlot,
    required_perms: Permissions,
    expected_type: Option<ObjectType>,
) -> Result<&Capability, CapError> {
    // 1. Lookup capability
    let cap = cspace.get(slot).ok_or(CapError::InvalidSlot)?;
    
    // 2. Check object type (if specified)
    if let Some(expected) = expected_type {
        if cap.object_type != expected {
            return Err(CapError::WrongType);
        }
    }
    
    // 3. Check permissions
    if !cap.permissions.permits(&required_perms) {
        return Err(CapError::InsufficientPermissions);
    }
    
    // 4. Check expiration (if applicable)
    if cap.is_expired() {
        return Err(CapError::Expired);
    }
    
    Ok(cap)
}
```

## Data Structures

### Capability

```rust
/// Unforgeable token granting authority over a kernel object.
///
/// # Invariants
/// - `id` is globally unique and never reused
/// - Capabilities can only be created by the kernel
/// - Derived capabilities have permissions ≤ source permissions
#[derive(Clone, Debug)]
pub struct Capability {
    /// Unique identifier (globally unique, never reused)
    pub id: u64,
    
    /// Type of object this capability references
    pub object_type: ObjectType,
    
    /// ID of the referenced object
    pub object_id: u64,
    
    /// Permissions granted by this capability
    pub permissions: Permissions,
    
    /// Generation number (for revocation)
    pub generation: u32,
    
    /// Optional expiration timestamp (nanos since boot, 0 = never)
    pub expires_at: u64,
}

impl Capability {
    /// Check if this capability has expired.
    pub fn is_expired(&self) -> bool {
        self.expires_at != 0 && current_time_nanos() > self.expires_at
    }
    
    /// Attenuate this capability with new permissions.
    ///
    /// Returns a new capability with permissions that are the intersection
    /// of the current permissions and the requested permissions.
    pub fn attenuate(&self, new_perms: Permissions) -> Self {
        Self {
            id: 0,  // Will be assigned by kernel
            object_type: self.object_type,
            object_id: self.object_id,
            permissions: self.permissions.attenuate(&new_perms),
            generation: self.generation,
            expires_at: self.expires_at,
        }
    }
}
```

### Object Types

```rust
/// Types of kernel objects that capabilities can reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ObjectType {
    /// IPC endpoint
    Endpoint = 1,
    /// Process
    Process = 2,
    /// Memory region (VMM)
    Memory = 3,
    /// IRQ handler
    Irq = 4,
    /// I/O port range
    IoPort = 5,
    /// Console/debug output
    Console = 6,
}
```

### Permissions

```rust
/// Permission bits for capabilities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Permissions {
    /// Can read from / receive from object
    pub read: bool,
    /// Can write to / send to object
    pub write: bool,
    /// Can grant (derive) this capability to others
    pub grant: bool,
}

impl Permissions {
    /// Full permissions (read, write, grant).
    pub const fn full() -> Self {
        Self { read: true, write: true, grant: true }
    }
    
    /// Read-only permission.
    pub const fn read_only() -> Self {
        Self { read: true, write: false, grant: false }
    }
    
    /// Write-only permission.
    pub const fn write_only() -> Self {
        Self { read: false, write: true, grant: false }
    }
    
    /// Check if self permits the required permissions.
    pub fn permits(&self, required: &Permissions) -> bool {
        (!required.read || self.read) &&
        (!required.write || self.write) &&
        (!required.grant || self.grant)
    }
    
    /// Attenuate permissions (can only reduce, never amplify).
    ///
    /// Returns permissions that are the intersection of self and mask.
    pub fn attenuate(&self, mask: &Permissions) -> Self {
        Self {
            read: self.read && mask.read,
            write: self.write && mask.write,
            grant: self.grant && mask.grant,
        }
    }
}
```

### Capability Space

```rust
/// Per-process capability table.
///
/// # Invariants
/// - Each slot contains at most one capability
/// - Slot numbers are never reused within a CSpace
pub struct CapabilitySpace {
    /// Slot -> Capability mapping
    slots: BTreeMap<CapSlot, Capability>,
    /// Next available slot
    next_slot: CapSlot,
}

/// Slot index into a capability space.
pub type CapSlot = u32;

impl CapabilitySpace {
    /// Create an empty capability space.
    pub fn new() -> Self {
        Self {
            slots: BTreeMap::new(),
            next_slot: 0,
        }
    }
    
    /// Insert a capability, returning its slot.
    pub fn insert(&mut self, cap: Capability) -> CapSlot {
        let slot = self.next_slot;
        self.next_slot += 1;
        self.slots.insert(slot, cap);
        slot
    }
    
    /// Insert a capability at a specific slot.
    pub fn insert_at(&mut self, slot: CapSlot, cap: Capability) {
        self.slots.insert(slot, cap);
        if slot >= self.next_slot {
            self.next_slot = slot + 1;
        }
    }
    
    /// Get a capability by slot (immutable).
    pub fn get(&self, slot: CapSlot) -> Option<&Capability> {
        self.slots.get(&slot)
    }
    
    /// Get a capability by slot (mutable).
    pub fn get_mut(&mut self, slot: CapSlot) -> Option<&mut Capability> {
        self.slots.get_mut(&slot)
    }
    
    /// Remove a capability from a slot.
    pub fn remove(&mut self, slot: CapSlot) -> Option<Capability> {
        self.slots.remove(&slot)
    }
    
    /// Find the next free slot.
    pub fn next_free_slot(&self) -> CapSlot {
        self.next_slot
    }
    
    /// List all capabilities.
    pub fn list(&self) -> impl Iterator<Item = (CapSlot, &Capability)> {
        self.slots.iter().map(|(&s, c)| (s, c))
    }
    
    /// Count of capabilities in this space.
    pub fn len(&self) -> usize {
        self.slots.len()
    }
    
    /// Check if the space is empty.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}
```

### Capability Errors

```rust
/// Errors returned by capability checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapError {
    /// Capability slot is empty or invalid
    InvalidSlot,
    /// Capability references wrong object type
    WrongType,
    /// Capability lacks required permissions
    InsufficientPermissions,
    /// Capability has expired
    Expired,
    /// Capability has been revoked
    Revoked,
    /// Object no longer exists
    ObjectNotFound,
}
```

## Capability Operations

### Grant (Derive)

Create a new capability derived from an existing one:

```rust
/// Grant a capability from one process to another.
///
/// # Pre-conditions
/// - Source capability must exist and have grant permission
/// - Target process must exist
/// - New permissions must be a subset of source permissions
///
/// # Post-conditions
/// - New capability created in target's CSpace
/// - CapInserted commit emitted
///
/// # Returns
/// - `Ok((CapSlot, Commit))`: New slot and the commit to record
/// - `Err(CapError)`: Grant denied
pub fn grant_capability(
    cap_spaces: &mut BTreeMap<ProcessId, CapabilitySpace>,
    next_cap_id: &mut u64,
    from_pid: ProcessId,
    from_slot: CapSlot,
    to_pid: ProcessId,
    new_perms: Permissions,
) -> Result<(CapSlot, Commit), CapError> {
    // Check source capability
    let source_cspace = cap_spaces.get(&from_pid)
        .ok_or(CapError::ObjectNotFound)?;
    let source_cap = check_capability(
        source_cspace, 
        from_slot, 
        Permissions { read: false, write: false, grant: true },
        None
    )?;
    
    // Create new capability (attenuated permissions)
    let new_cap = Capability {
        id: *next_cap_id,
        object_type: source_cap.object_type,
        object_id: source_cap.object_id,
        permissions: source_cap.permissions.attenuate(&new_perms),
        generation: source_cap.generation,
        expires_at: source_cap.expires_at,
    };
    *next_cap_id += 1;
    
    // Insert into target's CSpace
    let target_cspace = cap_spaces.get_mut(&to_pid)
        .ok_or(CapError::ObjectNotFound)?;
    let new_slot = target_cspace.insert(new_cap.clone());
    
    // Build commit for this state change
    let commit = Commit {
        commit_type: CommitType::CapInserted {
            pid: to_pid,
            slot: new_slot,
            cap: new_cap,
        },
        ..Default::default()
    };
    
    Ok((new_slot, commit))
}
```

### Revoke

Invalidate a capability:

```rust
/// Revoke a capability.
///
/// # Pre-conditions
/// - Capability must exist
/// - Caller must have grant permission on the capability
///
/// # Post-conditions
/// - Capability removed from CSpace
/// - CapRemoved commit emitted
pub fn revoke_capability(
    cap_spaces: &mut BTreeMap<ProcessId, CapabilitySpace>,
    pid: ProcessId,
    slot: CapSlot,
) -> Result<Commit, CapError> {
    let cspace = cap_spaces.get_mut(&pid)
        .ok_or(CapError::ObjectNotFound)?;
    
    // Check we can revoke (need grant permission)
    {
        let cap = cspace.get(slot).ok_or(CapError::InvalidSlot)?;
        if !cap.permissions.grant {
            return Err(CapError::InsufficientPermissions);
        }
    }
    
    // Remove from CSpace
    cspace.remove(slot);
    
    // Build commit for this state change
    let commit = Commit {
        commit_type: CommitType::CapRemoved { pid, slot },
        ..Default::default()
    };
    
    Ok(commit)
}
```

### Delete

Remove a capability without revocation rights:

```rust
/// Delete a capability from a CSpace.
///
/// Unlike revoke, this just removes the capability from the caller's
/// own CSpace without requiring grant permission.
///
/// # Post-conditions
/// - Capability removed from CSpace
/// - CapRemoved commit emitted
pub fn delete_capability(
    cap_spaces: &mut BTreeMap<ProcessId, CapabilitySpace>,
    pid: ProcessId,
    slot: CapSlot,
) -> Result<Commit, CapError> {
    let cspace = cap_spaces.get_mut(&pid)
        .ok_or(CapError::ObjectNotFound)?;
    
    // Verify slot exists
    if cspace.get(slot).is_none() {
        return Err(CapError::InvalidSlot);
    }
    
    // Remove from CSpace
    cspace.remove(slot);
    
    // Build commit
    let commit = Commit {
        commit_type: CommitType::CapRemoved { pid, slot },
        ..Default::default()
    };
    
    Ok(commit)
}
```

## Properties

1. **Unforgeable**: Capabilities can only be created by the kernel at object creation time or derived through authorized grant operations.

2. **Attenuating**: Derived capabilities can only have equal or fewer permissions than their source. Permissions never increase.

3. **Object-Bound**: Each capability references a specific kernel object. The capability is invalid if the object is destroyed.

4. **Generation-Tracked**: Capabilities include a generation number for efficient revocation of derived capabilities.

5. **Time-Limited**: Capabilities can have optional expiration timestamps.

## Commit Integration

All capability mutations generate Commits that are recorded in the Axiom CommitLog:

| Operation | Commit Generated |
|-----------|------------------|
| Grant | `CapInserted { pid, slot, cap }` |
| Revoke | `CapRemoved { pid, slot }` |
| Delete | `CapRemoved { pid, slot }` |
| Transfer (via IPC) | `CapRemoved` + `CapInserted` |

See [../02-axiom/02-commitlog.md](../02-axiom/02-commitlog.md) for the full CommitType enumeration.

## WASM Notes

On WASM, capabilities work the same way, but:

- **No Hardware Protection**: The WASM runtime (supervisor) enforces capability checks in software
- **Trusted Supervisor**: The JavaScript supervisor must be trusted to correctly implement capability semantics
- **Serialization**: Capabilities are serialized when passed between Web Workers

## Compatibility with Current Code

The current `Zero-kernel` implementation includes:

- `Capability`, `CapabilitySpace`, `Permissions` types (compatible)
- `ObjectType` enum (compatible)
- Capability checking in `ipc_send`, `ipc_receive` (implements the check pattern)

The kernel emits Commits for all capability mutations, which are recorded by the Axiom layer.
