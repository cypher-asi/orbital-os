//! Process capability setup
//!
//! Handles capability grants for spawned processes, including:
//! - Init and PermissionService capabilities
//! - Terminal capabilities
//! - VFS service capabilities
//! - Identity service capabilities
//! - Keystore service capabilities
//!
//! This module is organized into submodules by capability domain.

mod identity;
mod keystore;
mod supervisor;
mod terminal;
mod vfs;

use zos_kernel::ProcessId;

use crate::constants::{INIT_ENDPOINT_SLOT, VFS_RESPONSE_SLOT};
use crate::util::log;

use super::super::Supervisor;

impl Supervisor {
    /// Set up capabilities for a spawned process
    pub(in crate::supervisor) fn setup_process_capabilities(
        &mut self,
        process_pid: ProcessId,
        name: &str,
    ) {
        log(&format!(
            "AGENT_LOG:setup_caps:name={}:pid={}:init_spawned={}",
            name, process_pid.0, self.init_spawned
        ));
        
        if name == "init" {
            self.init_spawned = true;
            log("[supervisor] Init process spawned (PID 1)");

            // Grant supervisor (PID 0) capability to Init's endpoint for IPC
            self.grant_supervisor_capability_to_init(process_pid);
        } else if name == "permission" {
            // Grant supervisor (PID 0) capability to PS's endpoint for IPC
            self.grant_supervisor_capability_to_ps(process_pid);
        } else if self.init_spawned {
            // Grant this process a capability to init's endpoint (slot 0 of PID 1)
            let init_pid = ProcessId(1);
            match self.system.grant_capability(
                init_pid,
                INIT_ENDPOINT_SLOT, // init's endpoint at slot 0
                process_pid,
                zos_kernel::Permissions {
                    read: false,
                    write: true,
                    grant: false,
                },
            ) {
                Ok(slot) => {
                    log(&format!(
                        "[supervisor] Granted init endpoint cap to {} at slot {}",
                        name, slot
                    ));
                }
                Err(e) => {
                    log(&format!(
                        "[supervisor] Failed to grant init cap to {}: {:?}",
                        name, e
                    ));
                }
            }

            // If VFS service is running, grant this process a capability to VFS endpoint
            // This goes in slot 3 for VfsClient to use
            self.grant_vfs_capability_to_process(process_pid, name);

            // Create a dedicated endpoint for VFS responses (slot 4)
            // This prevents race conditions where the VFS client's blocking receive
            // on the general input endpoint (slot 1) could consume other IPC messages.
            // VFS responses are routed here by the supervisor via Init.
            if let Ok((eid, slot)) = self.system.create_endpoint(process_pid) {
                log(&format!(
                    "[supervisor] Created VFS response endpoint {} at slot {} for {}",
                    eid.0, slot, name
                ));

                // Grant Init capability to this VFS response endpoint
                // This enables Init to deliver VFS responses to the correct endpoint (slot 4)
                self.grant_init_vfs_response_capability(name, process_pid);
            }

            // If Identity service is running, grant this process a capability to Identity endpoint
            // This enables proper capability-mediated IPC for identity operations
            self.grant_identity_capability_to_process(process_pid, name);

            // If this is identity and Keystore service is running,
            // grant keystore capability to Identity
            // This enables Identity to use keystore IPC for /keys/ paths (Invariant 32)
            if name == "identity" {
                log(&format!(
                    "[supervisor] Identity service spawned (PID {}), looking for keystore service...",
                    process_pid.0
                ));
                if let Some(keystore_pid) = self.find_keystore_service_pid() {
                    log(&format!(
                        "[supervisor] Found keystore service at PID {}, granting capability to Identity",
                        keystore_pid.0
                    ));
                    self.grant_keystore_capability_to_identity(keystore_pid);
                } else {
                    log("[supervisor] WARNING: Keystore service not found when Identity spawned!");
                }
            }
        }

        // When terminal is spawned, grant Init (PID 1) capability to terminal's input endpoint
        // and grant supervisor capability for console input routing
        if name == "terminal" {
            self.grant_terminal_capabilities(process_pid);
        }

        // When vfs is spawned, grant its endpoint to processes that need VFS access
        // and grant Init (PID 1) capability to deliver IPC messages to VFS
        if name == "vfs" {
            log(&format!(
                "AGENT_LOG:spawn_caps:vfs_spawned:pid={}:init_spawned={}:will_grant",
                process_pid.0, self.init_spawned
            ));
            self.grant_vfs_capabilities_to_existing_processes(process_pid);
            self.grant_init_capability_to_service("vfs", process_pid);
        }

        // When identity is spawned, grant its endpoint to processes that need identity access
        // and grant Init (PID 1) capability to deliver IPC messages to Identity
        if name == "identity" {
            // #region agent log - hypothesis B,E
            log(&format!(
                "AGENT_LOG:spawn_caps:identity_spawned:pid={}:will_grant",
                process_pid.0
            ));
            // #endregion

            self.grant_identity_capabilities_to_existing_processes(process_pid);
            self.grant_init_capability_to_service("identity", process_pid);
        }

        // When time is spawned, grant Init (PID 1) capability to deliver IPC messages
        if name == "time" {
            self.grant_init_capability_to_service("time", process_pid);
        }

        // When keystore is spawned, grant its endpoint to Identity service
        // and grant Init (PID 1) capability to deliver IPC messages
        if name == "keystore" {
            log(&format!(
                "[supervisor] Keystore service spawned (PID {}), setting up capabilities",
                process_pid.0
            ));
            self.grant_keystore_capability_to_identity(process_pid);
            self.grant_init_capability_to_service("keystore", process_pid);
        }
    }

    /// Create a VFS response endpoint for a process
    ///
    /// Helper used by VFS capability grant functions.
    pub(super) fn create_vfs_response_endpoint_for_process(
        &mut self,
        pid: ProcessId,
        name: &str,
    ) -> Option<u32> {
        if let Ok((eid, slot)) = self.system.create_endpoint(pid) {
            log(&format!(
                "[supervisor] Created VFS response endpoint {} at slot {} for {} (PID {})",
                eid.0, slot, name, pid.0
            ));

            // Grant Init capability to this VFS response endpoint
            self.grant_init_vfs_response_capability(name, pid);
            Some(slot)
        } else {
            None
        }
    }

    /// Grant Init (PID 1) a capability to a process's VFS response endpoint (slot 4).
    ///
    /// This enables Init to deliver VFS responses to the correct endpoint,
    /// preventing the routing issue where VFS responses go to slot 1 (input)
    /// instead of slot 4 (VFS response). After granting the capability,
    /// Init is notified via MSG_VFS_RESPONSE_CAP_GRANTED.
    fn grant_init_vfs_response_capability(&mut self, process_name: &str, process_pid: ProcessId) {
        let init_pid = ProcessId(1);

        // Get process's VFS response endpoint ID from VFS_RESPONSE_SLOT
        let endpoint_id = match self.system.get_cap_space(process_pid) {
            Some(cspace) => match cspace.get(VFS_RESPONSE_SLOT) {
                Some(cap) => zos_kernel::EndpointId(cap.object_id),
                None => {
                    log(&format!(
                        "[supervisor] {} (PID {}) has no VFS response endpoint at slot {}",
                        process_name, process_pid.0, VFS_RESPONSE_SLOT
                    ));
                    return;
                }
            },
            None => {
                log(&format!(
                    "[supervisor] {} (PID {}) has no CSpace",
                    process_name, process_pid.0
                ));
                return;
            }
        };

        // Grant Init capability to process's VFS response endpoint
        match self.system.grant_capability_to_endpoint(
            process_pid,
            endpoint_id,
            init_pid,
            zos_kernel::Permissions {
                read: false,
                write: true, // Can send VFS responses to process
                grant: false,
            },
        ) {
            Ok(slot) => {
                log(&format!(
                    "[supervisor] Granted {} (PID {}) VFS response endpoint to Init at slot {}",
                    process_name, process_pid.0, slot
                ));

                // Notify Init about the VFS response capability via IPC message
                self.notify_init_vfs_response_cap(process_pid.0, slot);
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to grant {} VFS response cap to Init: {:?}",
                    process_name, e
                ));
            }
        }
    }

    /// Notify Init about a granted VFS response endpoint capability via IPC.
    ///
    /// Sends MSG_VFS_RESPONSE_CAP_GRANTED to Init with [service_pid, cap_slot].
    fn notify_init_vfs_response_cap(&mut self, process_pid: u64, cap_slot: u32) {
        let init_slot = match self.init_endpoint_slot {
            Some(slot) => slot,
            None => {
                log("[supervisor] Cannot notify Init of VFS response cap: no Init capability");
                return;
            }
        };

        use zos_ipc::init::MSG_VFS_RESPONSE_CAP_GRANTED;

        // Build message: [process_pid: u32, cap_slot: u32]
        let mut payload = Vec::with_capacity(8);
        payload.extend_from_slice(&(process_pid as u32).to_le_bytes());
        payload.extend_from_slice(&cap_slot.to_le_bytes());

        let supervisor_pid = ProcessId(0);

        match self.system.ipc_send(
            supervisor_pid,
            init_slot,
            MSG_VFS_RESPONSE_CAP_GRANTED,
            payload,
        ) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Notified Init of PID {} VFS response cap at slot {}",
                    process_pid, cap_slot
                ));
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to notify Init of VFS response cap: {:?}",
                    e
                ));
            }
        }
    }
}
