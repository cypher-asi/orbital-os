//! Capability operations for KernelCore.
//!
//! This module contains methods for:
//! - Granting capabilities between processes
//! - Granting capabilities to specific endpoints
//! - Revoking capabilities (with permission check)
//! - Deleting capabilities (without permission check)
//! - Deriving capabilities with reduced permissions

use alloc::vec;
use alloc::vec::Vec;

use crate::axiom_check;
use crate::error::KernelError;
use crate::types::{CapSlot, EndpointId, ObjectType, ProcessId};
use crate::{Capability, Permissions};
use zos_axiom::{Commit, CommitType};
use zos_hal::HAL;

use super::{map_axiom_error, KernelCore};

impl<H: HAL> KernelCore<H> {
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

        // Get and validate source capability
        let source_cap = match self.validate_grant_source(from_pid, from_slot, timestamp) {
            Ok(cap) => cap,
            Err(e) => return (Err(e), commits),
        };

        // Attenuate permissions (can only reduce, never amplify)
        let granted_perms = attenuate_permissions(&source_cap.permissions, &new_perms);

        // Create and insert new capability
        let (to_slot, cap_commits) =
            match self.create_derived_cap(to_pid, &source_cap, granted_perms, timestamp) {
                Ok(result) => result,
                Err(e) => return (Err(e), commits),
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
                new_cap_id: cap_commits
                    .last()
                    .map(|c| {
                        if let CommitType::CapInserted { cap_id, .. } = c.commit_type {
                            cap_id
                        } else {
                            0
                        }
                    })
                    .unwrap_or(0),
                perms: zos_axiom::Permissions {
                    read: granted_perms.read,
                    write: granted_perms.write,
                    grant: granted_perms.grant,
                },
            },
            caused_by: None,
        });
        commits.extend(cap_commits);

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
            expires_at: 0,
        };

        // Insert into destination
        let to_slot = match self.cap_spaces.get_mut(&to_pid) {
            Some(cspace) => cspace.insert(new_cap),
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        // Log CapInserted commit
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

        // Validate capability with grant permission
        let cap_id = match self.validate_revoke_permission(pid, slot, timestamp) {
            Ok(id) => id,
            Err(e) => return (Err(e), commits),
        };

        // Log and remove
        commits.push(create_cap_removed_commit(pid, slot, timestamp));

        match self.cap_spaces.get_mut(&pid) {
            Some(cspace) => cspace.remove(slot),
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

        // Check capability exists
        let cap_id = match self.cap_spaces.get(&pid) {
            Some(cspace) => match cspace.get(slot) {
                Some(cap) => cap.id,
                None => return (Err(KernelError::InvalidCapability), commits),
            },
            None => return (Err(KernelError::ProcessNotFound), commits),
        };

        // Log and remove
        commits.push(create_cap_removed_commit(pid, slot, timestamp));

        match self.cap_spaces.get_mut(&pid) {
            Some(cspace) => cspace.remove(slot),
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

        // Validate source capability exists and is not expired
        let source_cap = match self.validate_derive_source(pid, slot, timestamp) {
            Ok(cap) => cap,
            Err(e) => return (Err(e), commits),
        };

        // Attenuate permissions
        let derived_perms = attenuate_permissions(&source_cap.permissions, &new_perms);

        // Create and insert derived capability
        let (new_slot, cap_commits) =
            match self.create_derived_cap(pid, &source_cap, derived_perms, timestamp) {
                Ok(result) => result,
                Err(e) => return (Err(e), commits),
            };

        commits.extend(cap_commits);
        (Ok(new_slot), commits)
    }

    // ========================================================================
    // Private helper methods
    // ========================================================================

    /// Validate source capability for grant operation
    fn validate_grant_source(
        &self,
        from_pid: ProcessId,
        from_slot: CapSlot,
        timestamp: u64,
    ) -> Result<Capability, KernelError> {
        let cspace = self
            .cap_spaces
            .get(&from_pid)
            .ok_or(KernelError::ProcessNotFound)?;

        let grant_perms = Permissions {
            read: false,
            write: false,
            grant: true,
        };

        axiom_check(cspace, from_slot, &grant_perms, None, timestamp)
            .cloned()
            .map_err(map_axiom_error)
    }

    /// Validate capability for revoke operation (needs grant permission)
    fn validate_revoke_permission(
        &self,
        pid: ProcessId,
        slot: CapSlot,
        timestamp: u64,
    ) -> Result<u64, KernelError> {
        let cspace = self
            .cap_spaces
            .get(&pid)
            .ok_or(KernelError::ProcessNotFound)?;

        let grant_perms = Permissions {
            read: false,
            write: false,
            grant: true,
        };

        axiom_check(cspace, slot, &grant_perms, None, timestamp)
            .map(|cap| cap.id)
            .map_err(map_axiom_error)
    }

    /// Validate source capability for derive operation
    fn validate_derive_source(
        &self,
        pid: ProcessId,
        slot: CapSlot,
        timestamp: u64,
    ) -> Result<Capability, KernelError> {
        let cspace = self
            .cap_spaces
            .get(&pid)
            .ok_or(KernelError::ProcessNotFound)?;

        // No specific permissions required - derive just creates a weaker copy
        let no_perms = Permissions::default();
        axiom_check(cspace, slot, &no_perms, None, timestamp)
            .cloned()
            .map_err(map_axiom_error)
    }

    /// Create a derived capability and insert it into target process
    fn create_derived_cap(
        &mut self,
        to_pid: ProcessId,
        source_cap: &Capability,
        new_perms: Permissions,
        timestamp: u64,
    ) -> Result<(CapSlot, Vec<Commit>), KernelError> {
        let new_cap_id = self.next_cap_id();
        let new_cap = Capability {
            id: new_cap_id,
            object_type: source_cap.object_type,
            object_id: source_cap.object_id,
            permissions: new_perms,
            generation: source_cap.generation,
            expires_at: source_cap.expires_at,
        };

        let to_slot = self
            .cap_spaces
            .get_mut(&to_pid)
            .ok_or(KernelError::ProcessNotFound)?
            .insert(new_cap);

        let commit = Commit {
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
                perms: new_perms.to_byte(),
            },
            caused_by: None,
        };

        Ok((to_slot, vec![commit]))
    }
}

/// Attenuate permissions (can only reduce, never amplify)
fn attenuate_permissions(source: &Permissions, requested: &Permissions) -> Permissions {
    Permissions {
        read: source.read && requested.read,
        write: source.write && requested.write,
        grant: source.grant && requested.grant,
    }
}

/// Create a CapRemoved commit
fn create_cap_removed_commit(pid: ProcessId, slot: CapSlot, timestamp: u64) -> Commit {
    Commit {
        id: [0u8; 32],
        prev_commit: [0u8; 32],
        seq: 0,
        timestamp,
        commit_type: CommitType::CapRemoved { pid: pid.0, slot },
        caused_by: None,
    }
}
