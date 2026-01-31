//! Identity service capability grants
//!
//! Handles granting Identity endpoint capabilities to processes and
//! granting Init capabilities to services.

use zos_kernel::ProcessId;

use crate::constants::{IDENTITY_INPUT_SLOT, SERVICE_INPUT_SLOT};
use crate::supervisor::Supervisor;
use crate::util::log;

impl Supervisor {
    /// Grant Identity Service endpoint capability to a specific process
    ///
    /// This enables the process to send IPC requests to the Identity Service.
    /// The process can then transfer a reply endpoint capability with its request
    /// to receive responses via proper capability-mediated IPC.
    pub(in crate::supervisor) fn grant_identity_capability_to_process(
        &mut self,
        target_pid: ProcessId,
        target_name: &str,
    ) {
        // Don't grant identity capability to identity itself
        // (a service doesn't need to send IPC to itself)
        if target_name == "identity" {
            return;
        }

        // Find Identity service process
        let identity_pid = self.find_identity_service_pid_internal();
        if let Some(identity_pid) = identity_pid {
            match self.system.grant_capability(
                identity_pid,
                IDENTITY_INPUT_SLOT,
                target_pid,
                zos_kernel::Permissions {
                    read: false, // Only need write (send) permission
                    write: true,
                    grant: false,
                },
            ) {
                Ok(slot) => {
                    log(&format!(
                        "[supervisor] Granted Identity endpoint cap to {} (PID {}) at slot {}",
                        target_name, target_pid.0, slot
                    ));
                }
                Err(e) => {
                    log(&format!(
                        "[supervisor] Failed to grant Identity cap to {} (PID {}): {:?}",
                        target_name, target_pid.0, e
                    ));
                }
            }
        }
    }

    /// Grant Identity Service endpoint capabilities to existing processes
    ///
    /// Called when identity spawns to grant its endpoint capability
    /// to all existing processes that may need identity operations.
    pub(in crate::supervisor) fn grant_identity_capabilities_to_existing_processes(
        &mut self,
        identity_pid: ProcessId,
    ) {
        // Get list of processes that need Identity access
        let processes: Vec<(ProcessId, String)> = self
            .system
            .list_processes()
            .into_iter()
            .filter(|(pid, proc)| {
                // Grant to all processes except init, supervisor, and identity itself
                // Also exclude vfs since it doesn't need identity access
                pid.0 > 1
                    && *pid != identity_pid
                    && proc.name != "identity"
                    && proc.name != "vfs"
            })
            .map(|(pid, proc)| (pid, proc.name.clone()))
            .collect();

        for (pid, name) in processes {
            match self.system.grant_capability(
                identity_pid,
                IDENTITY_INPUT_SLOT,
                pid,
                zos_kernel::Permissions {
                    read: false, // Only need write (send) permission
                    write: true,
                    grant: false,
                },
            ) {
                Ok(slot) => {
                    log(&format!(
                        "[supervisor] Granted Identity endpoint cap to {} (PID {}) at slot {}",
                        name, pid.0, slot
                    ));
                }
                Err(e) => {
                    log(&format!(
                        "[supervisor] Failed to grant Identity cap to {} (PID {}): {:?}",
                        name, pid.0, e
                    ));
                }
            }
        }
    }

    /// Find the Identity service process ID (internal helper)
    pub(in crate::supervisor) fn find_identity_service_pid_internal(&self) -> Option<ProcessId> {
        for (pid, proc) in self.system.list_processes() {
            if proc.name == "identity" {
                return Some(pid);
            }
        }
        None
    }

    /// Grant Init (PID 1) a capability to a service's input endpoint.
    ///
    /// This enables Init to deliver IPC messages to the service via
    /// capability-checked syscall::send(). After granting the capability,
    /// Init is notified via MSG_SERVICE_CAP_GRANTED so it can track the
    /// PID -> capability slot mapping.
    ///
    /// This is called when identity, vfs, and time spawn.
    pub(in crate::supervisor) fn grant_init_capability_to_service(
        &mut self,
        service_name: &str,
        service_pid: ProcessId,
    ) {
        self.grant_init_capability_to_service_inner(service_name, service_pid, false);
    }

    /// Re-grant Init capability to a service and trigger pending delivery retry.
    ///
    /// This is called during capability recovery when Init was missing a capability
    /// and has pending messages queued for the service. Unlike the initial grant,
    /// this uses MSG_SERVICE_CAP_GRANTED which triggers Init to retry pending deliveries.
    pub(in crate::supervisor) fn regrant_init_capability_to_service(
        &mut self,
        service_name: &str,
        service_pid: ProcessId,
    ) {
        self.grant_init_capability_to_service_inner(service_name, service_pid, true);
    }

    /// Internal implementation for granting Init capability to a service.
    fn grant_init_capability_to_service_inner(
        &mut self,
        service_name: &str,
        service_pid: ProcessId,
        trigger_retry: bool,
    ) {
        log(&format!(
            "AGENT_LOG:grant_init_cap:start:service={}:pid={}:init_slot={:?}:trigger_retry={}",
            service_name, service_pid.0, self.init_endpoint_slot, trigger_retry
        ));
        
        let init_pid = ProcessId(1);

        // Get service's input endpoint ID from SERVICE_INPUT_SLOT
        let endpoint_id = match self.system.get_cap_space(service_pid) {
            Some(cspace) => match cspace.get(SERVICE_INPUT_SLOT) {
                Some(cap) => zos_kernel::EndpointId(cap.object_id),
                None => {
                    log(&format!(
                        "[supervisor] {} has no endpoint at slot {}",
                        service_name, SERVICE_INPUT_SLOT
                    ));
                    return;
                }
            },
            None => {
                log(&format!("[supervisor] {} has no CSpace", service_name));
                return;
            }
        };

        // Grant Init capability to service's endpoint
        match self.system.grant_capability_to_endpoint(
            service_pid,
            endpoint_id,
            init_pid,
            zos_kernel::Permissions {
                read: false,
                write: true, // Can send to service
                grant: false,
            },
        ) {
            Ok(slot) => {
                log(&format!(
                    "[supervisor] Granted {} endpoint to Init at slot {}",
                    service_name, slot
                ));

                // #region agent log - hypothesis B
                log(&format!(
                    "AGENT_LOG:grant_init_cap:granted:service={}:pid={}:slot={}:will_notify:trigger_retry={}",
                    service_name, service_pid.0, slot, trigger_retry
                ));
                // #endregion

                // Notify Init about the capability via IPC message
                self.notify_init_service_cap_with_retry(service_pid.0, slot, trigger_retry);
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to grant {} cap to Init: {:?}",
                    service_name, e
                ));
            }
        }
    }

    /// Notify Init about a service capability with explicit retry control.
    ///
    /// When `trigger_retry` is true, uses MSG_SERVICE_CAP_GRANTED which causes
    /// Init to retry any pending deliveries for this service.
    pub(in crate::supervisor) fn notify_init_service_cap_with_retry(
        &mut self,
        service_pid: u64,
        cap_slot: u32,
        trigger_retry: bool,
    ) {
        let init_slot = match self.init_endpoint_slot {
            Some(slot) => slot,
            None => {
                log("[supervisor] Cannot notify Init of service cap: no Init capability");
                return;
            }
        };

        use zos_ipc::init::{MSG_SERVICE_CAP_GRANTED, MSG_SERVICE_CAP_PREREGISTER};

        // Use GRANTED when we need to trigger pending retry (capability recovery)
        // Use PREREGISTER when this is initial registration before service starts
        let msg_tag = if trigger_retry {
            MSG_SERVICE_CAP_GRANTED
        } else {
            MSG_SERVICE_CAP_PREREGISTER
        };

        let msg_type = if trigger_retry { "granted" } else { "preregister" };

        // Build message: [service_pid: u32, cap_slot: u32]
        let mut payload = Vec::with_capacity(8);
        payload.extend_from_slice(&(service_pid as u32).to_le_bytes());
        payload.extend_from_slice(&cap_slot.to_le_bytes());

        let supervisor_pid = ProcessId(0);

        match self.system.ipc_send(supervisor_pid, init_slot, msg_tag, payload) {
            Ok(()) => {
                // #region agent log - hypothesis B
                log(&format!(
                    "AGENT_LOG:notify_init:{}:service_pid={}:cap_slot={}:init_slot={}",
                    msg_type, service_pid, cap_slot, init_slot
                ));
                // #endregion

                log(&format!(
                    "[supervisor] Notified Init of service PID {} cap at slot {} ({})",
                    service_pid, cap_slot, msg_type
                ));
            }
            Err(e) => {
                // #region agent log - hypothesis B
                log(&format!(
                    "AGENT_LOG:notify_init:{}_failed:service_pid={}:error={:?}",
                    msg_type, service_pid, e
                ));
                // #endregion

                log(&format!(
                    "[supervisor] Failed to notify Init of service cap ({}): {:?}",
                    msg_type, e
                ));
            }
        }
    }
}
