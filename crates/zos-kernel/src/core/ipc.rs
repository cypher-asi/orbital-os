//! IPC (Inter-Process Communication) operations for KernelCore.
//!
//! This module contains methods for:
//! - Sending messages (with and without capability transfer)
//! - Receiving messages (with and without capability transfer)
//! - Checking for pending messages
//! - Direct process-to-process messaging (supervisor override)

use alloc::vec;
use alloc::vec::Vec;

use crate::axiom_check;
use crate::error::KernelError;
use crate::ipc::{Message, TransferredCap, MAX_CAPS_PER_MESSAGE, MAX_MESSAGE_SIZE};
use crate::types::{CapSlot, EndpointId, ObjectType, ProcessId};
use crate::Permissions;
use zos_axiom::{Commit, CommitType};
use zos_hal::HAL;

use super::{map_axiom_error, KernelCore};

/// Result type for IPC receive with capability transfer
type ReceiveWithCapsResult = (
    Result<Option<(Message, Vec<CapSlot>)>, KernelError>,
    Vec<Commit>,
);

impl<H: HAL> KernelCore<H> {
    /// Send IPC message (validates capability via axiom_check).
    ///
    /// Returns (Result<(), KernelError>, Option<Commit>) - optional MessageSent commit.
    pub fn ipc_send(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
        timestamp: u64,
    ) -> (Result<(), KernelError>, Option<Commit>) {
        // Validate endpoint capability
        let endpoint_id = match self.validate_send_cap(from_pid, endpoint_slot, timestamp) {
            Ok(id) => id,
            Err(e) => return (Err(e), None),
        };

        let data_len = data.len();

        // Queue message
        let message = Message {
            from: from_pid,
            tag,
            data,
            transferred_caps: vec![],
        };

        if let Err(e) = self.queue_message(endpoint_id, message) {
            return (Err(e), None);
        }

        // Update metrics
        self.update_send_metrics(from_pid, endpoint_id, data_len, timestamp);

        // Create MessageSent commit
        let commit = Commit {
            id: [0u8; 32],
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::MessageSent {
                from_pid: from_pid.0,
                to_endpoint: endpoint_id.0,
                tag,
                size: data_len,
            },
            caused_by: None,
        };

        (Ok(()), Some(commit))
    }

    /// Send IPC message with capability transfer.
    ///
    /// Capabilities in `cap_slots` are removed from the sender's CSpace and
    /// transferred to the receiver.
    ///
    /// Returns (Result<(), KernelError>, Vec<Commit>) - commits for capability removals.
    pub fn ipc_send_with_caps(
        &mut self,
        from_pid: ProcessId,
        endpoint_slot: CapSlot,
        tag: u32,
        data: Vec<u8>,
        cap_slots: &[CapSlot],
        timestamp: u64,
    ) -> (Result<(), KernelError>, Vec<Commit>) {
        let mut commits = Vec::new();

        // Validate limits
        if let Err(e) = validate_message_limits(data.len(), cap_slots.len()) {
            return (Err(e), commits);
        }

        // Lookup and validate endpoint capability
        let endpoint_id = match self.validate_send_cap_basic(from_pid, endpoint_slot) {
            Ok(id) => id,
            Err(e) => return (Err(e), commits),
        };

        // Verify endpoint exists
        if !self.endpoints.contains_key(&endpoint_id) {
            return (Err(KernelError::EndpointNotFound), commits);
        }

        // Validate all capabilities exist before removing any
        if let Err(e) = self.validate_caps_exist(from_pid, cap_slots) {
            return (Err(e), commits);
        }

        // Remove capabilities and build transfer list
        let (transferred_caps, cap_commits) =
            match self.remove_and_transfer_caps(from_pid, cap_slots, timestamp) {
                Ok(result) => result,
                Err(e) => return (Err(e), commits),
            };
        commits.extend(cap_commits);

        let data_len = data.len();

        // Queue message with transferred capabilities
        let message = Message {
            from: from_pid,
            tag,
            data,
            transferred_caps,
        };

        if let Err(e) = self.queue_message(endpoint_id, message) {
            return (Err(e), commits);
        }

        // Update metrics
        self.update_send_metrics(from_pid, endpoint_id, data_len, timestamp);

        (Ok(()), commits)
    }

    /// Receive IPC message (validates capability via axiom_check).
    pub fn ipc_receive(
        &mut self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
        timestamp: u64,
    ) -> Result<Option<Message>, KernelError> {
        // Validate endpoint capability
        let endpoint_id = self.validate_receive_cap(pid, endpoint_slot, timestamp)?;

        // Pop message
        let endpoint = self
            .endpoints
            .get_mut(&endpoint_id)
            .ok_or(KernelError::EndpointNotFound)?;

        let msg = endpoint.pending_messages.pop_front();

        // Update metrics
        if let Some(ref m) = msg {
            endpoint.metrics.queue_depth = endpoint.pending_messages.len();
            if let Some(receiver) = self.processes.get_mut(&pid) {
                receiver.metrics.ipc_received += 1;
                receiver.metrics.ipc_bytes_received += m.data.len() as u64;
                receiver.metrics.last_active_ns = timestamp;
            }
        }

        Ok(msg)
    }

    /// Receive IPC message and install transferred capabilities.
    ///
    /// Returns (Result<Option<(Message, Vec<CapSlot>)>, KernelError>, Vec<Commit>).
    pub fn ipc_receive_with_caps(
        &mut self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
        timestamp: u64,
    ) -> ReceiveWithCapsResult {
        let mut commits = Vec::new();

        // First do normal receive
        let message = match self.ipc_receive(pid, endpoint_slot, timestamp) {
            Ok(Some(msg)) => msg,
            Ok(None) => return (Ok(None), commits),
            Err(e) => return (Err(e), commits),
        };

        // Install transferred capabilities
        if message.transferred_caps.is_empty() {
            return (Ok(Some((message, vec![]))), commits);
        }

        let (installed_slots, cap_commits) =
            match self.install_transferred_caps(pid, &message.transferred_caps, timestamp) {
                Ok(result) => result,
                Err(e) => return (Err(e), commits),
            };
        commits.extend(cap_commits);

        (Ok(Some((message, installed_slots))), commits)
    }

    /// Check if an IPC endpoint has pending messages (without removing them).
    pub fn ipc_has_message(
        &self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
        timestamp: u64,
    ) -> Result<bool, KernelError> {
        let endpoint_id = self.validate_receive_cap_readonly(pid, endpoint_slot, timestamp)?;

        let endpoint = self
            .endpoints
            .get(&endpoint_id)
            .ok_or(KernelError::EndpointNotFound)?;

        Ok(!endpoint.pending_messages.is_empty())
    }

    /// Send a message to a process's first endpoint (for testing/supervisor use).
    ///
    /// **WARNING: This function BYPASSES capability checks.**
    pub fn send_to_process(
        &mut self,
        from_pid: ProcessId,
        to_pid: ProcessId,
        tag: u32,
        data: Vec<u8>,
        timestamp: u64,
    ) -> Result<(), KernelError> {
        self.hal.debug_write(&alloc::format!(
            "[kernel] SUPERVISOR_OVERRIDE: send_to_process bypassing capabilities \
             (from PID {} to PID {})",
            from_pid.0,
            to_pid.0
        ));

        // Find an endpoint owned by the target process
        let endpoint_id = self
            .endpoints
            .iter()
            .find(|(_, ep)| ep.owner == to_pid)
            .map(|(id, _)| *id)
            .ok_or(KernelError::EndpointNotFound)?;

        let data_len = data.len();

        // Queue the message
        let message = Message {
            from: from_pid,
            tag,
            data,
            transferred_caps: vec![],
        };

        self.queue_message(endpoint_id, message)?;
        self.update_send_metrics(from_pid, endpoint_id, data_len, timestamp);

        self.hal.debug_write(&alloc::format!(
            "[kernel] Message sent from PID {} to PID {} (endpoint {}, tag 0x{:x}) [OVERRIDE]",
            from_pid.0,
            to_pid.0,
            endpoint_id.0,
            tag
        ));

        Ok(())
    }

    // ========================================================================
    // Private helper methods
    // ========================================================================

    /// Validate send capability using axiom_check
    fn validate_send_cap(
        &self,
        from_pid: ProcessId,
        endpoint_slot: CapSlot,
        timestamp: u64,
    ) -> Result<EndpointId, KernelError> {
        let cspace = self
            .cap_spaces
            .get(&from_pid)
            .ok_or(KernelError::ProcessNotFound)?;

        let cap = axiom_check(
            cspace,
            endpoint_slot,
            &Permissions::write_only(),
            Some(ObjectType::Endpoint),
            timestamp,
        )
        .map_err(map_axiom_error)?;

        Ok(EndpointId(cap.object_id))
    }

    /// Validate send capability without axiom_check (for send_with_caps)
    fn validate_send_cap_basic(
        &self,
        from_pid: ProcessId,
        endpoint_slot: CapSlot,
    ) -> Result<EndpointId, KernelError> {
        let cspace = self
            .cap_spaces
            .get(&from_pid)
            .ok_or(KernelError::ProcessNotFound)?;

        let cap = cspace
            .get(endpoint_slot)
            .ok_or(KernelError::InvalidCapability)?;

        if cap.object_type != ObjectType::Endpoint || !cap.permissions.write {
            return Err(KernelError::PermissionDenied);
        }

        Ok(EndpointId(cap.object_id))
    }

    /// Validate receive capability using axiom_check
    fn validate_receive_cap(
        &self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
        timestamp: u64,
    ) -> Result<EndpointId, KernelError> {
        let cspace = self
            .cap_spaces
            .get(&pid)
            .ok_or(KernelError::ProcessNotFound)?;

        let cap = axiom_check(
            cspace,
            endpoint_slot,
            &Permissions::read_only(),
            Some(ObjectType::Endpoint),
            timestamp,
        )
        .map_err(map_axiom_error)?;

        Ok(EndpointId(cap.object_id))
    }

    /// Validate receive capability (readonly, for has_message)
    fn validate_receive_cap_readonly(
        &self,
        pid: ProcessId,
        endpoint_slot: CapSlot,
        timestamp: u64,
    ) -> Result<EndpointId, KernelError> {
        self.validate_receive_cap(pid, endpoint_slot, timestamp)
    }

    /// Validate that all capabilities exist
    fn validate_caps_exist(&self, pid: ProcessId, slots: &[CapSlot]) -> Result<(), KernelError> {
        let cspace = self
            .cap_spaces
            .get(&pid)
            .ok_or(KernelError::ProcessNotFound)?;

        for &slot in slots {
            if cspace.get(slot).is_none() {
                return Err(KernelError::InvalidCapability);
            }
        }
        Ok(())
    }

    /// Remove capabilities from sender and build transfer list
    fn remove_and_transfer_caps(
        &mut self,
        from_pid: ProcessId,
        cap_slots: &[CapSlot],
        timestamp: u64,
    ) -> Result<(Vec<TransferredCap>, Vec<Commit>), KernelError> {
        let mut commits = Vec::new();
        let mut transferred_caps = Vec::with_capacity(cap_slots.len());

        let sender_cspace = self
            .cap_spaces
            .get_mut(&from_pid)
            .ok_or(KernelError::ProcessNotFound)?;

        for &slot in cap_slots {
            if let Some(cap) = sender_cspace.remove(slot) {
                commits.push(Commit {
                    id: [0u8; 32],
                    prev_commit: [0u8; 32],
                    seq: 0,
                    timestamp,
                    commit_type: CommitType::CapRemoved {
                        pid: from_pid.0,
                        slot,
                    },
                    caused_by: None,
                });
                transferred_caps.push(TransferredCap {
                    capability: cap,
                    receiver_slot: None,
                });
            }
        }

        Ok((transferred_caps, commits))
    }

    /// Install transferred capabilities into receiver's CSpace
    fn install_transferred_caps(
        &mut self,
        pid: ProcessId,
        transferred_caps: &[TransferredCap],
        timestamp: u64,
    ) -> Result<(Vec<CapSlot>, Vec<Commit>), KernelError> {
        let mut commits = Vec::new();
        let mut installed_slots = Vec::with_capacity(transferred_caps.len());

        let receiver_cspace = self
            .cap_spaces
            .get_mut(&pid)
            .ok_or(KernelError::ProcessNotFound)?;

        for tcap in transferred_caps {
            let slot = receiver_cspace.insert(tcap.capability.clone());
            installed_slots.push(slot);

            commits.push(Commit {
                id: [0u8; 32],
                prev_commit: [0u8; 32],
                seq: 0,
                timestamp,
                commit_type: CommitType::CapInserted {
                    pid: pid.0,
                    slot,
                    cap_id: tcap.capability.id,
                    object_type: tcap.capability.object_type as u8,
                    object_id: tcap.capability.object_id,
                    perms: tcap.capability.permissions.to_byte(),
                },
                caused_by: None,
            });
        }

        Ok((installed_slots, commits))
    }

    /// Queue a message to an endpoint
    fn queue_message(
        &mut self,
        endpoint_id: EndpointId,
        message: Message,
    ) -> Result<(), KernelError> {
        let endpoint = self
            .endpoints
            .get_mut(&endpoint_id)
            .ok_or(KernelError::EndpointNotFound)?;

        endpoint.pending_messages.push_back(message);
        Ok(())
    }

    /// Update metrics after sending a message
    fn update_send_metrics(
        &mut self,
        from_pid: ProcessId,
        endpoint_id: EndpointId,
        data_len: usize,
        timestamp: u64,
    ) {
        // Update endpoint metrics
        if let Some(endpoint) = self.endpoints.get_mut(&endpoint_id) {
            endpoint.metrics.queue_depth = endpoint.pending_messages.len();
            endpoint.metrics.total_messages += 1;
            endpoint.metrics.total_bytes += data_len as u64;
            if endpoint.metrics.queue_depth > endpoint.metrics.queue_high_water {
                endpoint.metrics.queue_high_water = endpoint.metrics.queue_depth;
            }
        }

        // Update sender process metrics
        if let Some(sender) = self.processes.get_mut(&from_pid) {
            sender.metrics.ipc_sent += 1;
            sender.metrics.ipc_bytes_sent += data_len as u64;
            sender.metrics.last_active_ns = timestamp;
        }

        // Update global IPC count
        self.total_ipc_count += 1;
    }
}

/// Validate message size and cap count limits
fn validate_message_limits(data_len: usize, cap_count: usize) -> Result<(), KernelError> {
    if data_len > MAX_MESSAGE_SIZE {
        return Err(KernelError::PermissionDenied);
    }
    if cap_count > MAX_CAPS_PER_MESSAGE {
        return Err(KernelError::PermissionDenied);
    }
    Ok(())
}
