# App Security & Permissions

> All permissions flow through Init (PID 1) as the system's permission authority.

## Overview

This document specifies the security model for Zero applications:

- **Capability-based**: Apps can only access resources they hold capabilities for
- **Least privilege**: Apps start with minimal capabilities
- **User consent**: Third-party apps require explicit user approval
- **Init as authority**: All permission grants flow through Init (PID 1)
- **Auditable**: All grants recorded in CommitLog via Axiom

## Permission Authority Model

```
┌─────────────────────────────────────────────────────────────────┐
│                  PERMISSION AUTHORITY CHAIN                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────────────┐                                           │
│  │   Kernel Boot    │  Creates "root" capabilities              │
│  └────────┬─────────┘                                           │
│           │ grants at spawn                                     │
│           ▼                                                     │
│  ┌──────────────────┐                                           │
│  │   Init Process   │  Receives root caps at spawn              │
│  │     (PID 1)      │  Acts as PERMISSION AUTHORITY             │
│  │                  │  Handles MSG_GRANT_PERMISSION IPC         │
│  └────────┬─────────┘                                           │
│           │ grants (after user consent, via SYS_CAP_GRANT)      │
│           ▼                                                     │
│  ┌──────────────────┐                                           │
│  │   App Process    │  Receives ONLY approved capabilities      │
│  │   (PID N)        │  with grant=FALSE (cannot delegate)       │
│  └──────────────────┘                                           │
│                                                                 │
│  ┌ ─ ─ ─ ─ ─ ─ ─ ─ ─┐                                           │
│      Supervisor      │  Host code, NOT a process                │
│  │   (no PID)       │  Sends IPC to Init for permission requests│
│  │                  │  Does NOT directly call kernel.grant_*()  │
│  └ ─ ─ ─ ─ ─ ─ ─ ─ ─┘                                           │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Why Init (PID 1), Not Supervisor?

| Aspect | Init as Authority | Supervisor as Authority |
|--------|-------------------|-------------------------|
| **Microkernel purity** | ✅ Everything through processes and IPC | ❌ Special backdoors needed |
| **Auditable** | ✅ All grants are syscalls from a process | ❌ Some grants invisible |
| **Platform-agnostic** | ✅ Same Init code on WASM/QEMU/bare metal | ❌ Platform-specific supervisor code |
| **Testable** | ✅ Can test permission logic in isolation | ❌ Requires full environment |
| **Replay** | ✅ All grants in CommitLog | ❌ Supervisor grants missing from log |

## Init's Root Capabilities

At spawn, Init (PID 1) receives root capabilities with `grant=true`:

```rust
/// Root capabilities granted to Init at boot
pub const INIT_ROOT_CAPS: &[CapabilityGrant] = &[
    CapabilityGrant {
        object_type: ObjectType::Console,
        permissions: Permissions {
            read: true,
            write: true,
            grant: true,  // Can grant console access to other processes
        },
    },
    CapabilityGrant {
        object_type: ObjectType::Storage,
        permissions: Permissions {
            read: true,
            write: true,
            grant: true,  // Can grant storage access to other processes
        },
    },
    CapabilityGrant {
        object_type: ObjectType::Network,
        permissions: Permissions {
            read: true,
            write: true,
            grant: true,  // Can grant network access to other processes
        },
    },
    CapabilityGrant {
        object_type: ObjectType::Process,
        permissions: Permissions {
            read: true,
            write: true,
            grant: true,  // Can manage other processes
        },
    },
];
```

## Permission Grant Flow

```
1. User launches third-party app
       │
       ▼
2. Desktop reads AppManifest.capabilities[]
       │
       ▼
3. Platform-specific UI shows permission dialog
       │
       ▼
4. User approves → Desktop sends IPC to Init (PID 1):
   MSG_GRANT_PERMISSION { app_pid, resource_type, perms }
       │
       ▼
5. Init (PID 1) validates and calls SYS_CAP_GRANT syscall
   (goes through Axiom → SysLog + CommitLog)
       │
       ▼
6. App has capability - Kernel axiom_check() enforces on every syscall
```

## Permission IPC Protocol

Messages for permission management (Desktop/Supervisor → Init):

```rust
/// Request Init to grant a capability to a process
pub const MSG_GRANT_PERMISSION: u32 = 0x1010;

/// Request Init to revoke a capability from a process
pub const MSG_REVOKE_PERMISSION: u32 = 0x1011;

/// Query what permissions a process has
pub const MSG_LIST_PERMISSIONS: u32 = 0x1012;

/// Response from Init with grant/revoke result
pub const MSG_PERMISSION_RESPONSE: u32 = 0x1013;
```

### Grant Request Payload

```rust
pub struct GrantRequest {
    /// Target process to grant capability to
    pub target_pid: u32,
    
    /// Type of object being granted
    pub object_type: ObjectType,
    
    /// Permissions to grant
    pub permissions: Permissions,
    
    /// Reason (from AppManifest, for logging)
    pub reason: String,
}
```

### Grant Response Payload

```rust
pub struct GrantResponse {
    /// Whether the grant succeeded
    pub success: bool,
    
    /// Capability slot where cap was inserted (if success)
    pub slot: Option<u32>,
    
    /// Error message (if !success)
    pub error: Option<String>,
}
```

## Init Permission Handling

Init maintains a tracking structure for granted capabilities:

```rust
/// Init's permission tracking state
pub struct PermissionTracker {
    /// Map from (pid, object_type) to granted cap slot
    /// Used for revocation lookups
    granted_caps: BTreeMap<(ProcessId, ObjectType), CapSlot>,
}

impl PermissionTracker {
    /// Handle MSG_GRANT_PERMISSION
    pub fn handle_grant(&mut self, ctx: &AppContext, request: GrantRequest) -> GrantResponse {
        // Validate: Is this a known object type?
        // Validate: Do we have grant permission for this type?
        // Validate: Is target_pid a valid process?
        
        // Call SYS_CAP_GRANT syscall
        // This goes through Axiom automatically
        let result = syscall::cap_grant(
            request.target_pid,
            request.object_type,
            request.permissions,
        );
        
        match result {
            Ok(slot) => {
                // Track for later revocation
                self.granted_caps.insert(
                    (ProcessId(request.target_pid), request.object_type),
                    slot,
                );
                GrantResponse { success: true, slot: Some(slot), error: None }
            }
            Err(e) => {
                GrantResponse { success: false, slot: None, error: Some(e.to_string()) }
            }
        }
    }
    
    /// Handle MSG_REVOKE_PERMISSION
    pub fn handle_revoke(&mut self, ctx: &AppContext, request: RevokeRequest) -> RevokeResponse {
        // Look up the slot we granted
        let key = (ProcessId(request.target_pid), request.object_type);
        
        if let Some(slot) = self.granted_caps.remove(&key) {
            // Call SYS_CAP_REVOKE syscall
            let result = syscall::cap_revoke(request.target_pid, slot);
            RevokeResponse { success: result.is_ok(), error: result.err().map(|e| e.to_string()) }
        } else {
            RevokeResponse { success: false, error: Some("No such grant found".to_string()) }
        }
    }
}
```

## Capability Types

Zero uses the kernel's capability types:

```rust
/// Types of kernel objects that can be accessed via capabilities
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ObjectType {
    /// IPC endpoint for messaging
    Endpoint = 0,
    
    /// Console I/O
    Console = 1,
    
    /// Persistent storage (namespaced per-app)
    Storage = 2,
    
    /// Network access
    Network = 3,
    
    /// Process management (spawn, kill)
    Process = 4,
    
    /// Memory region
    Memory = 5,
}

/// Permission bits for capabilities
#[derive(Copy, Clone, Debug, Default)]
pub struct Permissions {
    /// Can read from the object
    pub read: bool,
    
    /// Can write to the object
    pub write: bool,
    
    /// Can grant this capability to other processes
    /// Only Init typically has grant=true
    pub grant: bool,
}
```

## Factory Apps vs Third-Party Apps

### Factory Apps (Trusted)

Factory apps bundled with the system are auto-granted basic capabilities:

```rust
/// Capabilities automatically granted to factory apps
pub const FACTORY_APP_CAPS: &[ObjectType] = &[
    ObjectType::Endpoint,  // For UI communication
];
```

### Third-Party Apps (Untrusted)

Third-party apps require user consent via permission dialog:

```
┌─────────────────────────────────────────────────────────────────┐
│                     Permission Request                           │
│                                                                 │
│  "Example App" is requesting:                                   │
│                                                                 │
│  ☑ Storage Access (read/write)                                  │
│    "Save user preferences"                                      │
│                                                                 │
│  ☐ Network Access (read only)                                   │
│    "Check for updates"                                          │
│                                                                 │
│                                                                 │
│              [ Deny All ]  [ Allow Selected ]                   │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Kernel Enforcement

Every syscall is checked against the caller's capability space:

```rust
impl<H: HAL> Kernel<H> {
    /// Check if a process has the required capability for an operation.
    /// Called at the start of every syscall handler.
    pub fn axiom_check(
        &self,
        pid: ProcessId,
        required_type: ObjectType,
        required_perms: Permissions,
    ) -> Result<(), KernelError> {
        let cap_space = self.cap_spaces.get(&pid)
            .ok_or(KernelError::ProcessNotFound)?;
        
        // Search for a matching capability
        for cap in cap_space.capabilities() {
            if cap.object_type == required_type {
                if cap.permissions.satisfies(&required_perms) {
                    return Ok(());
                }
            }
        }
        
        Err(KernelError::CapabilityDenied {
            pid: pid.0,
            required_type,
            required_perms,
        })
    }
}
```

## Audit Trail

All permission operations are recorded in CommitLog:

| Event | CommitType | Data |
|-------|------------|------|
| Capability granted | `CapabilityInserted` | pid, slot, object_type, permissions |
| Capability revoked | `CapabilityRevoked` | pid, slot |
| Permission denied | `CapabilityDenied` | pid, requested_type, requested_perms |

This enables:
- **Security audit**: Review what apps have accessed
- **Forensics**: Investigate suspicious activity
- **Compliance**: Prove what permissions were active at any time

## React Components (WASM Phase)

### PermissionDialog

```tsx
// web/components/PermissionDialog/PermissionDialog.tsx

interface PermissionDialogProps {
  app: AppManifest;
  onApprove: (approved: CapabilityRequest[]) => void;
  onDeny: () => void;
}

export function PermissionDialog({ app, onApprove, onDeny }: PermissionDialogProps) {
  const [selected, setSelected] = useState<Set<number>>(
    new Set(app.capabilities.filter(c => c.required).map((_, i) => i))
  );
  
  return (
    <Dialog>
      <DialogTitle>Permission Request</DialogTitle>
      <DialogContent>
        <Typography>"{app.name}" is requesting:</Typography>
        {app.capabilities.map((cap, i) => (
          <FormControlLabel
            key={i}
            control={
              <Checkbox
                checked={selected.has(i)}
                disabled={cap.required}
                onChange={(e) => /* toggle */}
              />
            }
            label={
              <>
                <strong>{cap.objectType}</strong>
                <Typography variant="caption">{cap.reason}</Typography>
              </>
            }
          />
        ))}
      </DialogContent>
      <DialogActions>
        <Button onClick={onDeny}>Deny All</Button>
        <Button onClick={() => onApprove(/* selected caps */)}>
          Allow Selected
        </Button>
      </DialogActions>
    </Dialog>
  );
}
```

### AppPermissions (Settings UI)

```tsx
// web/components/AppPermissions/AppPermissions.tsx

interface AppPermissionsProps {
  app: AppManifest;
  grantedCaps: CapabilityInfo[];
  onRevoke: (objectType: ObjectType) => void;
}

export function AppPermissions({ app, grantedCaps, onRevoke }: AppPermissionsProps) {
  return (
    <Card>
      <CardHeader title={`${app.name} Permissions`} />
      <CardContent>
        <List>
          {grantedCaps.map((cap) => (
            <ListItem
              key={cap.objectType}
              secondaryAction={
                <IconButton onClick={() => onRevoke(cap.objectType)}>
                  <DeleteIcon />
                </IconButton>
              }
            >
              <ListItemText
                primary={cap.objectType}
                secondary={`${cap.permissions.read ? 'R' : ''}${cap.permissions.write ? 'W' : ''}`}
              />
            </ListItem>
          ))}
        </List>
      </CardContent>
    </Card>
  );
}
```

## Security Invariants

1. **Only Init can grant**: No process except Init (PID 1) has `grant=true` permissions
2. **No capability escalation**: Apps cannot increase their own permissions
3. **Revocation is immediate**: Revoked capabilities are removed from cap space instantly
4. **All grants audited**: Every grant/revoke appears in CommitLog
5. **Deny by default**: Missing capability = access denied
