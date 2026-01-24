//! Capability-based access control verification
//!
//! This module implements the Axiom layer's capability verification:
//! - Capability tokens with permissions
//! - Capability spaces (per-process)
//! - The `axiom_check` function for authority verification
//!
//! # Invariants (per docs/invariants/invariants.md)
//!
//! - Invariant 9: Axiom records all requests and responses
//! - Invariant 10: Axiom verifies capabilities before execution
//!
//! All capability verification flows through `axiom_check`, ensuring
//! there is exactly one code path for authority verification.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::types::{CapSlot, ObjectType, Permissions};

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

// ============================================================================
// Axiom Capability Checking
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

#[cfg(test)]
mod tests {
    use super::*;

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

        let result = axiom_check(&cspace, 999, &Permissions::read_only(), None, 0);

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
            Some(ObjectType::Process),
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
            permissions: Permissions::read_only(),
            generation: 0,
            expires_at: 0,
        };
        let slot = cspace.insert(cap);

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
            expires_at: 1000,
        };
        let slot = cspace.insert(cap);

        let result = axiom_check(&cspace, slot, &Permissions::read_only(), None, 2000);

        assert!(matches!(result, Err(AxiomError::Expired)));
    }

    #[test]
    fn test_capability_never_expires() {
        let mut cspace = CapabilitySpace::new();
        let cap = Capability {
            id: 1,
            object_type: ObjectType::Endpoint,
            object_id: 42,
            permissions: Permissions::full(),
            generation: 0,
            expires_at: 0, // 0 = never expires
        };
        let slot = cspace.insert(cap);

        // Even with a huge current_time, should not expire
        let result = axiom_check(&cspace, slot, &Permissions::read_only(), None, u64::MAX);

        assert!(result.is_ok());
    }

    #[test]
    fn test_capability_space_operations() {
        let mut cspace = CapabilitySpace::new();
        assert!(cspace.is_empty());

        let cap = Capability {
            id: 1,
            object_type: ObjectType::Endpoint,
            object_id: 42,
            permissions: Permissions::full(),
            generation: 0,
            expires_at: 0,
        };
        let slot = cspace.insert(cap);

        assert_eq!(cspace.len(), 1);
        assert!(!cspace.is_empty());
        assert!(cspace.get(slot).is_some());

        let removed = cspace.remove(slot);
        assert!(removed.is_some());
        assert!(cspace.is_empty());
    }
}
