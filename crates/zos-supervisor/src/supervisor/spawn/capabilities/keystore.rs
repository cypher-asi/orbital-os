//! Keystore service capability grants
//!
//! Handles granting Keystore endpoint capabilities to the Identity service.
//! Unlike VFS which is granted to all processes, Keystore is only accessible
//! by the Identity service for security isolation.

use zos_kernel::ProcessId;

use crate::constants::KEYSTORE_INPUT_SLOT;
use crate::supervisor::Supervisor;
use crate::util::log;

impl Supervisor {
    /// Grant Keystore Service endpoint capability to the Identity service
    ///
    /// This enables the Identity service to send IPC requests to the Keystore Service
    /// for cryptographic key storage operations.
    pub(in crate::supervisor) fn grant_keystore_capability_to_identity(
        &mut self,
        keystore_pid: ProcessId,
    ) {
        log(&format!(
            "[supervisor] grant_keystore_capability_to_identity: keystore_pid={}",
            keystore_pid.0
        ));
        
        // Find Identity service process
        let identity_pid = self.find_identity_service_pid_internal();
        log(&format!(
            "[supervisor] grant_keystore_capability_to_identity: identity_pid={:?}",
            identity_pid.map(|p| p.0)
        ));
        
        if let Some(identity_pid) = identity_pid {
            log(&format!(
                "[supervisor] Granting Keystore (PID {}) slot {} to Identity (PID {})",
                keystore_pid.0, KEYSTORE_INPUT_SLOT, identity_pid.0
            ));
            match self.system.grant_capability(
                keystore_pid,
                KEYSTORE_INPUT_SLOT,
                identity_pid,
                zos_kernel::Permissions {
                    read: false, // Only need write (send) permission
                    write: true,
                    grant: false,
                },
            ) {
                Ok(slot) => {
                    log(&format!(
                        "[supervisor] SUCCESS: Granted Keystore endpoint cap to Identity (PID {}) at slot {} (expected slot 5)",
                        identity_pid.0, slot
                    ));
                    if slot != 5 {
                        log(&format!(
                            "[supervisor] WARNING: Keystore cap at slot {} != expected slot 5!",
                            slot
                        ));
                    }
                }
                Err(e) => {
                    log(&format!(
                        "[supervisor] FAILED to grant Keystore cap to Identity (PID {}): {:?}",
                        identity_pid.0, e
                    ));
                }
            }
        } else {
            log("[supervisor] Cannot grant Keystore cap: Identity service not found");
        }
    }

    /// Find the Keystore service process ID (internal helper)
    pub(in crate::supervisor) fn find_keystore_service_pid(&self) -> Option<ProcessId> {
        let processes = self.system.list_processes();
        log(&format!(
            "[supervisor] find_keystore_service_pid: checking {} processes",
            processes.len()
        ));
        for (pid, proc) in processes {
            log(&format!(
                "[supervisor] find_keystore_service_pid: checking PID {} name='{}'",
                pid.0, proc.name
            ));
            if proc.name == "keystore_service" {
                log(&format!(
                    "[supervisor] find_keystore_service_pid: FOUND at PID {}",
                    pid.0
                ));
                return Some(pid);
            }
        }
        log("[supervisor] find_keystore_service_pid: NOT FOUND");
        None
    }
}
