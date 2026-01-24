//! Endpoint management for KernelCore.
//!
//! This module contains methods for:
//! - Creating IPC endpoints
//! - Listing endpoints
//! - Getting endpoint details

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::KernelError;
use crate::ipc::{Endpoint, EndpointDetail, EndpointInfo, MessageSummary};
use crate::types::{CapSlot, EndpointId, EndpointMetrics, ObjectType, ProcessId};
use crate::{Capability, Permissions};
use zos_axiom::{Commit, CommitType};
use zos_hal::HAL;

use super::KernelCore;

impl<H: HAL> KernelCore<H> {
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

        // Create and insert the endpoint
        let endpoint = Endpoint {
            id,
            owner,
            pending_messages: VecDeque::new(),
            metrics: EndpointMetrics::default(),
        };
        self.endpoints.insert(id, endpoint);

        // Grant full capability to owner
        let (slot, cap_commits) = match self.grant_owner_endpoint_cap(owner, id, timestamp) {
            Ok((slot, commits)) => (slot, commits),
            Err(e) => {
                // Rollback endpoint creation
                self.endpoints.remove(&id);
                return (Err(e), Vec::new());
            }
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
        commits.extend(cap_commits);

        self.hal.debug_write(&alloc::format!(
            "[kernel] Created endpoint {} for PID {}, cap slot {}",
            id.0,
            owner.0,
            slot
        ));

        (Ok((id, slot)), commits)
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

    /// Get endpoint by ID
    pub fn get_endpoint(&self, id: EndpointId) -> Option<&Endpoint> {
        self.endpoints.get(&id)
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

    // ========================================================================
    // Private helper methods
    // ========================================================================

    /// Grant full capability to endpoint owner and return (slot, commits)
    fn grant_owner_endpoint_cap(
        &mut self,
        owner: ProcessId,
        endpoint_id: EndpointId,
        timestamp: u64,
    ) -> Result<(CapSlot, Vec<Commit>), KernelError> {
        let cap_id = self.next_cap_id();
        let perms = Permissions::full();
        let cap = Capability {
            id: cap_id,
            object_type: ObjectType::Endpoint,
            object_id: endpoint_id.0,
            permissions: perms,
            generation: 0,
            expires_at: 0, // Never expires
        };

        let cspace = self
            .cap_spaces
            .get_mut(&owner)
            .ok_or(KernelError::ProcessNotFound)?;
        let slot = cspace.insert(cap);

        let commit = Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::CapInserted {
                pid: owner.0,
                slot,
                cap_id,
                object_type: ObjectType::Endpoint as u8,
                object_id: endpoint_id.0,
                perms: perms.to_byte(),
            },
            caused_by: None,
        };

        Ok((slot, vec![commit]))
    }
}
