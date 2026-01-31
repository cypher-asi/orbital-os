//! VFS service capability grants
//!
//! Handles granting VFS endpoint capabilities to processes.

use zos_kernel::ProcessId;

use crate::constants::VFS_INPUT_SLOT;
use crate::supervisor::Supervisor;
use crate::util::log;

impl Supervisor {
    /// Grant VFS endpoint capability to a specific process
    pub(in crate::supervisor) fn grant_vfs_capability_to_process(
        &mut self,
        target_pid: ProcessId,
        target_name: &str,
    ) {
        // Find VFS service process
        let vfs_pid = self.find_vfs_service_pid();
        if let Some(vfs_pid) = vfs_pid {
            match self.system.grant_capability(
                vfs_pid,
                VFS_INPUT_SLOT,
                target_pid,
                zos_kernel::Permissions {
                    read: true,
                    write: true,
                    grant: false,
                },
            ) {
                Ok(slot) => {
                    log(&format!(
                        "[supervisor] Granted VFS endpoint cap to {} (PID {}) at slot {}",
                        target_name, target_pid.0, slot
                    ));
                }
                Err(e) => {
                    log(&format!(
                        "[supervisor] Failed to grant VFS cap to {} (PID {}): {:?}",
                        target_name, target_pid.0, e
                    ));
                }
            }
        }
    }

    /// Grant VFS endpoint capabilities to existing processes that need VFS access
    pub(in crate::supervisor) fn grant_vfs_capabilities_to_existing_processes(
        &mut self,
        vfs_pid: ProcessId,
    ) {
        // Get list of processes that need VFS access
        let processes: Vec<(ProcessId, String)> = self
            .system
            .list_processes()
            .into_iter()
            .filter(|(pid, proc)| {
                // Grant to all processes except init, supervisor, and vfs itself
                pid.0 > 1 && *pid != vfs_pid && proc.name != "vfs"
            })
            .map(|(pid, proc)| (pid, proc.name.clone()))
            .collect();

        for (pid, name) in processes {
            match self.system.grant_capability(
                vfs_pid,
                VFS_INPUT_SLOT,
                pid,
                zos_kernel::Permissions {
                    read: true,
                    write: true,
                    grant: false,
                },
            ) {
                Ok(slot) => {
                    log(&format!(
                        "[supervisor] Granted VFS endpoint cap to {} (PID {}) at slot {}",
                        name, pid.0, slot
                    ));
                }
                Err(e) => {
                    log(&format!(
                        "[supervisor] Failed to grant VFS cap to {} (PID {}): {:?}",
                        name, pid.0, e
                    ));
                }
            }

            // Also create a dedicated VFS response endpoint for this process (slot 4)
            // This prevents race conditions where VFS client's blocking receive
            // could consume other IPC messages on the general input endpoint.
            self.create_vfs_response_endpoint_for_process(pid, &name);
        }
    }

    /// Find the VFS service process ID
    pub(in crate::supervisor) fn find_vfs_service_pid(&self) -> Option<ProcessId> {
        for (pid, proc) in self.system.list_processes() {
            if proc.name == "vfs" {
                return Some(pid);
            }
        }
        None
    }
}
