//! Deterministic replay implementation for the System.
//!
//! This module implements the `Replayable` trait, allowing system state to be
//! reconstructed from a commit log for auditing and verification purposes.

use alloc::collections::VecDeque;
use alloc::string::String;

use crate::ipc::Endpoint;
use crate::system::System;
use crate::types::{
    EndpointId, EndpointMetrics, ObjectType, Process, ProcessId, ProcessMetrics, ProcessState,
};
use crate::{Capability, CapabilitySpace, Permissions};
use zos_axiom::{ReplayError, ReplayResult, Replayable, StateHasher};
use zos_hal::HAL;

impl<H: HAL> Replayable for System<H> {
    fn replay_genesis(&mut self) -> ReplayResult<()> {
        Ok(())
    }

    fn replay_create_process(&mut self, pid: u64, _parent: u64, name: String) -> ReplayResult<()> {
        let process = Process {
            pid: ProcessId(pid),
            name,
            state: ProcessState::Running,
            metrics: ProcessMetrics::default(),
        };
        self.kernel.processes.insert(ProcessId(pid), process);
        self.kernel
            .cap_spaces
            .insert(ProcessId(pid), CapabilitySpace::new());

        // Update next_pid to avoid collisions
        if pid >= self.kernel.next_pid {
            self.kernel.next_pid = pid + 1;
        }

        Ok(())
    }

    fn replay_exit_process(&mut self, pid: u64, _code: i32) -> ReplayResult<()> {
        let process = self
            .kernel
            .processes
            .get_mut(&ProcessId(pid))
            .ok_or(ReplayError::ProcessNotFound(pid))?;
        process.state = ProcessState::Zombie;
        Ok(())
    }

    fn replay_process_faulted(
        &mut self,
        pid: u64,
        _reason: u32,
        _description: String,
    ) -> ReplayResult<()> {
        let process = self
            .kernel
            .processes
            .get_mut(&ProcessId(pid))
            .ok_or(ReplayError::ProcessNotFound(pid))?;
        process.state = ProcessState::Zombie;
        Ok(())
    }

    fn replay_insert_capability(
        &mut self,
        pid: u64,
        slot: u32,
        cap_id: u64,
        object_type: u8,
        object_id: u64,
        perms: u8,
    ) -> ReplayResult<()> {
        let obj_type = map_object_type(object_type)?;

        let cap = Capability {
            id: cap_id,
            object_type: obj_type,
            object_id,
            permissions: Permissions::from_byte(perms),
            generation: 0,
            expires_at: 0,
        };

        let cspace = self
            .kernel
            .cap_spaces
            .get_mut(&ProcessId(pid))
            .ok_or(ReplayError::ProcessNotFound(pid))?;

        cspace.slots.insert(slot, cap);

        // Update next_slot to avoid collisions
        if slot >= cspace.next_slot {
            cspace.next_slot = slot + 1;
        }

        // Update next_cap_id to avoid collisions
        if cap_id >= self.kernel.next_cap_id {
            self.kernel.next_cap_id = cap_id + 1;
        }

        Ok(())
    }

    fn replay_remove_capability(&mut self, pid: u64, slot: u32) -> ReplayResult<()> {
        let cspace = self
            .kernel
            .cap_spaces
            .get_mut(&ProcessId(pid))
            .ok_or(ReplayError::ProcessNotFound(pid))?;
        cspace.slots.remove(&slot);
        Ok(())
    }

    fn replay_cap_granted(
        &mut self,
        _from_pid: u64,
        _to_pid: u64,
        _from_slot: u32,
        _to_slot: u32,
        new_cap_id: u64,
        _perms: zos_axiom::Permissions,
    ) -> ReplayResult<()> {
        // Just update next_cap_id (actual insertion handled by CapInserted)
        if new_cap_id >= self.kernel.next_cap_id {
            self.kernel.next_cap_id = new_cap_id + 1;
        }
        Ok(())
    }

    fn replay_create_endpoint(&mut self, id: u64, owner: u64) -> ReplayResult<()> {
        if !self.kernel.processes.contains_key(&ProcessId(owner)) {
            return Err(ReplayError::ProcessNotFound(owner));
        }

        let endpoint = Endpoint {
            id: EndpointId(id),
            owner: ProcessId(owner),
            pending_messages: VecDeque::new(),
            metrics: EndpointMetrics::default(),
        };
        self.kernel.endpoints.insert(EndpointId(id), endpoint);

        // Update next_endpoint_id to avoid collisions
        if id >= self.kernel.next_endpoint_id {
            self.kernel.next_endpoint_id = id + 1;
        }

        Ok(())
    }

    fn replay_destroy_endpoint(&mut self, id: u64) -> ReplayResult<()> {
        self.kernel.endpoints.remove(&EndpointId(id));
        Ok(())
    }

    fn replay_message_sent(
        &mut self,
        _from_pid: u64,
        _to_endpoint: u64,
        _tag: u32,
        _size: usize,
    ) -> ReplayResult<()> {
        // Messages are transient and not replayed into state
        Ok(())
    }

    fn state_hash(&self) -> [u8; 32] {
        let mut hasher = StateHasher::new();

        // Hash process table
        hasher.write_u64(self.kernel.processes.len() as u64);
        for (pid, proc) in &self.kernel.processes {
            hasher.write_u64(pid.0);
            hasher.write_str(&proc.name);
            hasher.write_u8(process_state_to_u8(proc.state));
        }

        // Hash capability spaces
        hasher.write_u64(self.kernel.cap_spaces.len() as u64);
        for (pid, cspace) in &self.kernel.cap_spaces {
            hasher.write_u64(pid.0);
            hasher.write_u64(cspace.slots.len() as u64);
            for (slot, cap) in &cspace.slots {
                hasher.write_u32(*slot);
                hasher.write_u64(cap.id);
                hasher.write_u8(cap.object_type as u8);
                hasher.write_u64(cap.object_id);
                hasher.write_u8(cap.permissions.to_byte());
                hasher.write_u32(cap.generation);
                hasher.write_u64(cap.expires_at);
            }
        }

        // Hash endpoints
        hasher.write_u64(self.kernel.endpoints.len() as u64);
        for (id, ep) in &self.kernel.endpoints {
            hasher.write_u64(id.0);
            hasher.write_u64(ep.owner.0);
        }

        hasher.finalize()
    }
}

/// Map object type byte to ObjectType enum
fn map_object_type(object_type: u8) -> ReplayResult<ObjectType> {
    match object_type {
        1 => Ok(ObjectType::Endpoint),
        2 => Ok(ObjectType::Process),
        3 => Ok(ObjectType::Memory),
        4 => Ok(ObjectType::Irq),
        5 => Ok(ObjectType::IoPort),
        6 => Ok(ObjectType::Console),
        _ => Err(ReplayError::UnknownObjectType(object_type)),
    }
}

/// Convert ProcessState to u8 for hashing
fn process_state_to_u8(state: ProcessState) -> u8 {
    match state {
        ProcessState::Running => 0,
        ProcessState::Blocked => 1,
        ProcessState::Zombie => 2,
    }
}
