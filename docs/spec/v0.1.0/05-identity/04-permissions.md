# Permissions

> Capability policy enforcement for resource access control.

## Overview

The Permissions system provides policy-based control over capability grants. It answers the question: "Should this process receive this capability?"

This is the *policy* layer. The kernel (via Axiom) handles the *mechanism* of capability checking. The Permissions system implements *policy* decisions.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Permissions Service                                   │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                    Policy Database                                      │ │
│  │                                                                         │ │
│  │  Rules:                                                                 │ │
│  │  • Apps can request: [storage-ro, network]                              │ │
│  │  • Services can request: [storage-rw, spawn]                            │ │
│  │  • Terminal can grant: [console] to children                            │ │
│  │  • Storage cannot grant: [network]                                      │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                    Axiom Log Query                                      │ │
│  │                                                                         │ │
│  │  • Who granted cap X?                                                   │ │
│  │  • What caps does process Y hold?                                       │ │
│  │  • History of capability transfers                                      │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  Message Handlers:                                                           │
│  • CHECK_PERMISSION → check if grant is allowed                             │
│  • QUERY_CAPS       → list capabilities for a process                       │
│  • QUERY_HISTORY    → capability grant history                              │
│  • UPDATE_POLICY    → modify policy rules (admin only)                      │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Data Structures

### Policy Rule

```rust
use serde::{Serialize, Deserialize};

/// A permission policy rule.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyRule {
    /// Rule identifier
    pub id: String,
    
    /// Process class this rule applies to
    pub applies_to: ProcessClass,
    
    /// Capability types this rule covers
    pub capability_types: Vec<String>,
    
    /// Whether the action is allowed
    pub allowed: bool,
    
    /// Required conditions for the rule to apply
    pub conditions: Vec<Condition>,
    
    /// Priority (higher = evaluated first)
    pub priority: u32,
}
```

### Process Classification

```rust
/// Process classification for policy matching.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ProcessClass {
    /// System services (init, terminal, etc.)
    System,
    
    /// Runtime services (storage, network, identity, etc.)
    Runtime,
    
    /// User applications
    Application,
    
    /// Specific process by name pattern
    Named(String),
    
    /// Specific process ID
    Pid(ProcessId),
    
    /// Any process
    Any,
}
```

### Conditions

```rust
/// Condition for a policy rule.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Condition {
    /// Requester must hold specific capability type
    RequesterHolds(String),
    
    /// Parent process must be specific class
    ParentIs(ProcessClass),
    
    /// Grant must attenuate permissions (reduce, not expand)
    MustAttenuate,
    
    /// Time-based restriction (nanos since boot)
    TimeWindow { start: u64, end: u64 },
    
    /// User must have specific role
    UserHasRole(String),
    
    /// Session must have MFA verified
    RequiresMfa,
    
    /// Maximum permission level allowed
    MaxPermissions(Permissions),
}
```

### Permission Check Request/Response

```rust
/// Permission check request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PermissionCheckRequest {
    /// Process requesting the capability
    pub requester: ProcessId,
    
    /// Capability type being requested
    pub capability_type: String,
    
    /// Requested permissions
    pub permissions: Permissions,
    
    /// Context (why is this being requested)
    pub context: Option<String>,
}

/// Permission check response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PermissionCheckResponse {
    /// Whether the request is allowed
    pub allowed: bool,
    
    /// Reason if denied
    pub reason: Option<String>,
    
    /// Suggested alternative if denied
    pub alternative: Option<String>,
    
    /// Granted permission level (may be attenuated)
    pub granted_permissions: Option<Permissions>,
}
```

### Capability Information

```rust
/// Information about a held capability.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapInfo {
    /// Capability slot
    pub slot: CapSlot,
    
    /// Object type the capability refers to
    pub object_type: ObjectType,
    
    /// Object ID
    pub object_id: u64,
    
    /// Permissions granted
    pub permissions: Permissions,
    
    /// Who granted this capability
    pub granted_by: ProcessId,
    
    /// When it was granted (nanos since boot)
    pub granted_at: u64,
}

/// Query for capabilities.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapQuery {
    /// Process to query (or None for self)
    pub pid: Option<ProcessId>,
    
    /// Filter by object type
    pub object_type: Option<ObjectType>,
}

/// Capability query result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapQueryResult {
    /// List of capabilities
    pub capabilities: Vec<CapInfo>,
}
```

### History Query

```rust
/// History query filter.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryQuery {
    /// Filter by actor (who did the action)
    pub actor: Option<ProcessId>,
    
    /// Filter by operation type
    pub operation: Option<CapOperationType>,
    
    /// Time range start (nanos since boot)
    pub from_time: Option<u64>,
    
    /// Time range end (nanos since boot)
    pub to_time: Option<u64>,
    
    /// Maximum entries to return
    pub limit: usize,
}

/// Type of capability operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CapOperationType {
    Grant,
    Revoke,
    Delegate,
}

/// History entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Sequence number from Axiom log
    pub seq: u64,
    
    /// Timestamp (nanos since boot)
    pub timestamp: u64,
    
    /// Process that performed the operation
    pub actor: ProcessId,
    
    /// The operation performed
    pub operation: CapOperation,
}

/// Capability operation details.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CapOperation {
    /// Capability was granted
    Grant {
        source_cap_id: u64,
        target_pid: ProcessId,
        permissions: Permissions,
    },
    
    /// Capability was revoked
    Revoke {
        cap_id: u64,
        target_pid: ProcessId,
    },
    
    /// Capability was delegated (granted from non-owner)
    Delegate {
        source_cap_id: u64,
        target_pid: ProcessId,
        permissions: Permissions,
    },
}
```

## Permission Service

### Trait Definition

```rust
/// Service for permission policy enforcement.
pub trait PermissionService {
    /// Check if a permission grant is allowed.
    fn check_permission(&self, request: &PermissionCheckRequest) -> PermissionCheckResponse;
    
    /// Query capabilities held by a process.
    fn query_capabilities(&self, query: &CapQuery) -> Result<CapQueryResult, PermissionError>;
    
    /// Query capability grant history.
    fn query_history(&self, query: &HistoryQuery) -> Result<Vec<HistoryEntry>, PermissionError>;
    
    /// Get full provenance chain for a capability.
    fn get_provenance(&self, cap_id: u64) -> Result<Vec<HistoryEntry>, PermissionError>;
    
    /// Add a policy rule (admin only).
    fn add_policy_rule(&self, rule: PolicyRule) -> Result<(), PermissionError>;
    
    /// Remove a policy rule (admin only).
    fn remove_policy_rule(&self, rule_id: &str) -> Result<(), PermissionError>;
    
    /// List all policy rules.
    fn list_policy_rules(&self) -> Vec<PolicyRule>;
    
    /// Classify a process.
    fn classify_process(&self, pid: ProcessId) -> ProcessClass;
}

/// Errors from permission operations.
#[derive(Clone, Debug)]
pub enum PermissionError {
    /// Process not found
    ProcessNotFound,
    /// Capability not found
    CapabilityNotFound,
    /// Permission denied
    PermissionDenied,
    /// Invalid policy rule
    InvalidRule(String),
    /// Axiom query failed
    LogQueryFailed(String),
}
```

### Permission Checking Implementation

```rust
impl PermissionService for PermissionServiceImpl {
    fn check_permission(&self, request: &PermissionCheckRequest) -> PermissionCheckResponse {
        // 1. Classify the requester
        let class = self.classify_process(request.requester);
        
        // 2. Find applicable rules (sorted by priority)
        let mut rules: Vec<_> = self.policy.iter()
            .filter(|r| self.rule_applies(r, &class, &request.capability_type))
            .collect();
        rules.sort_by_key(|r| std::cmp::Reverse(r.priority));
        
        // 3. Evaluate rules (first match wins)
        for rule in rules {
            if self.evaluate_conditions(rule, request) {
                if rule.allowed {
                    // Check if attenuation is needed
                    let granted = self.apply_attenuation(rule, &request.permissions);
                    return PermissionCheckResponse {
                        allowed: true,
                        reason: None,
                        alternative: None,
                        granted_permissions: Some(granted),
                    };
                } else {
                    return PermissionCheckResponse {
                        allowed: false,
                        reason: Some(format!("Denied by rule: {}", rule.id)),
                        alternative: self.suggest_alternative(request),
                        granted_permissions: None,
                    };
                }
            }
        }
        
        // No matching rule - deny by default
        PermissionCheckResponse {
            allowed: false,
            reason: Some("No policy allows this permission".to_string()),
            alternative: None,
            granted_permissions: None,
        }
    }
    
    fn classify_process(&self, pid: ProcessId) -> ProcessClass {
        let info = self.process_manager.get_process_info(pid);
        
        match info.name.as_str() {
            "init" | "terminal" | "supervisor" | "desktop" => ProcessClass::System,
            "storage" | "network" | "identity" | "permissions" | "vfs" => ProcessClass::Runtime,
            name if name.starts_with("system-") => ProcessClass::System,
            name if name.starts_with("service-") => ProcessClass::Runtime,
            _ => ProcessClass::Application,
        }
    }
    
    fn evaluate_conditions(&self, rule: &PolicyRule, request: &PermissionCheckRequest) -> bool {
        for condition in &rule.conditions {
            match condition {
                Condition::RequesterHolds(cap_type) => {
                    if !self.process_holds_cap_type(request.requester, cap_type) {
                        return false;
                    }
                }
                Condition::ParentIs(class) => {
                    let parent_pid = self.get_parent_pid(request.requester);
                    if self.classify_process(parent_pid) != *class {
                        return false;
                    }
                }
                Condition::MustAttenuate => {
                    // This condition is checked during grant, not here
                }
                Condition::TimeWindow { start, end } => {
                    let now = current_timestamp();
                    if now < *start || now > *end {
                        return false;
                    }
                }
                Condition::UserHasRole(role) => {
                    let user_id = self.get_process_user(request.requester);
                    if !self.user_has_role(user_id, role) {
                        return false;
                    }
                }
                Condition::RequiresMfa => {
                    let session = self.get_process_session(request.requester);
                    if !session.mfa_verified {
                        return false;
                    }
                }
                Condition::MaxPermissions(max) => {
                    if !request.permissions.is_subset_of(max) {
                        return false;
                    }
                }
            }
        }
        true
    }
}
```

## Default Policy

```rust
/// Default permission policy rules.
fn default_policy() -> Vec<PolicyRule> {
    vec![
        // System services can spawn and grant
        PolicyRule {
            id: "system-full-access".to_string(),
            applies_to: ProcessClass::System,
            capability_types: vec!["*".to_string()],
            allowed: true,
            conditions: vec![],
            priority: 100,
        },
        
        // Runtime services have elevated access
        PolicyRule {
            id: "runtime-service-access".to_string(),
            applies_to: ProcessClass::Runtime,
            capability_types: vec![
                "storage".to_string(),
                "network".to_string(),
                "spawn".to_string(),
            ],
            allowed: true,
            conditions: vec![],
            priority: 90,
        },
        
        // Applications can request storage (read-only by default)
        PolicyRule {
            id: "app-storage-ro".to_string(),
            applies_to: ProcessClass::Application,
            capability_types: vec!["storage".to_string()],
            allowed: true,
            conditions: vec![
                Condition::MustAttenuate,
                Condition::MaxPermissions(Permissions::READ),
            ],
            priority: 50,
        },
        
        // Applications can request network
        PolicyRule {
            id: "app-network".to_string(),
            applies_to: ProcessClass::Application,
            capability_types: vec!["network".to_string()],
            allowed: true,
            conditions: vec![],
            priority: 50,
        },
        
        // Applications can request their own app data storage (read-write)
        PolicyRule {
            id: "app-own-storage".to_string(),
            applies_to: ProcessClass::Application,
            capability_types: vec!["app-storage".to_string()],
            allowed: true,
            conditions: vec![],
            priority: 60,
        },
        
        // Terminal can grant console to children
        PolicyRule {
            id: "terminal-console".to_string(),
            applies_to: ProcessClass::Named("terminal".to_string()),
            capability_types: vec!["console".to_string()],
            allowed: true,
            conditions: vec![
                Condition::ParentIs(ProcessClass::Named("terminal".to_string())),
            ],
            priority: 70,
        },
        
        // Deny storage service granting network (isolation)
        PolicyRule {
            id: "storage-no-network".to_string(),
            applies_to: ProcessClass::Named("storage".to_string()),
            capability_types: vec!["network".to_string()],
            allowed: false,
            conditions: vec![],
            priority: 80,
        },
        
        // Sensitive operations require MFA
        PolicyRule {
            id: "sensitive-requires-mfa".to_string(),
            applies_to: ProcessClass::Any,
            capability_types: vec![
                "user-management".to_string(),
                "key-management".to_string(),
            ],
            allowed: true,
            conditions: vec![
                Condition::RequiresMfa,
            ],
            priority: 95,
        },
    ]
}
```

## Axiom Log Integration

```rust
impl PermissionServiceImpl {
    /// Query capability grant history from Axiom log.
    fn query_history(&self, query: &HistoryQuery) -> Result<Vec<HistoryEntry>, PermissionError> {
        // Request log range from kernel
        let log_entries = syscall_axiom_query(
            query.from_time,
            query.to_time,
            query.limit,
        ).map_err(|e| PermissionError::LogQueryFailed(e.to_string()))?;
        
        // Filter by criteria
        Ok(log_entries.into_iter()
            .filter(|e| self.matches_query(e, query))
            .map(|e| HistoryEntry {
                seq: e.seq,
                timestamp: e.timestamp,
                actor: e.actor,
                operation: e.operation.clone(),
            })
            .collect())
    }
    
    /// Get full capability provenance (chain of grants).
    fn get_provenance(&self, cap_id: u64) -> Result<Vec<HistoryEntry>, PermissionError> {
        let mut chain = Vec::new();
        let mut current_id = cap_id;
        
        loop {
            // Find grant that created this capability
            let grant = self.find_grant_for_cap(current_id)?;
            
            match grant {
                Some(entry) => {
                    chain.push(entry.clone());
                    
                    // Trace back to source capability
                    if let CapOperation::Grant { source_cap_id, .. } = &entry.operation {
                        current_id = *source_cap_id;
                    } else {
                        break;  // Reached original creation
                    }
                }
                None => break,
            }
        }
        
        Ok(chain)
    }
}
```

## IPC Protocol

### Message Types

```rust
pub mod perm_msg {
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
```

## Invariants

1. **Deny by default**: No permission without explicit policy allowing it
2. **First match wins**: Rules are evaluated in priority order
3. **No escalation**: Capabilities can only be attenuated, never expanded
4. **Audit trail**: All grants are recorded in Axiom log
5. **Policy consistency**: Policy rules have unique IDs

## Security Considerations

1. **Policy isolation**: Only system processes can modify policy
2. **MFA enforcement**: Sensitive operations require MFA
3. **Capability attenuation**: Grants reduce, never increase, permissions
4. **Provenance tracking**: Full chain of custody for capabilities
5. **Time-based access**: Support for temporary permissions

## WASM Notes

- Policy rules are stored in `zos-kernel` IndexedDB database
- Axiom log queries use the kernel's log access syscall
- Process classification uses process metadata from init

## Related Specifications

- [../03-kernel/03-capabilities.md](../03-kernel/03-capabilities.md) - Kernel capability system
- [../02-axiom/01-syslog.md](../02-axiom/01-syslog.md) - Audit logging
- [01-users.md](01-users.md) - User roles
- [02-sessions.md](02-sessions.md) - Session MFA state
