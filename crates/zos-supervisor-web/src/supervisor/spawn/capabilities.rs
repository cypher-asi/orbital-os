//! Process capability setup
//!
//! Handles capability grants for spawned processes, including:
//! - Init and PermissionManager capabilities
//! - Terminal capabilities
//! - VFS service capabilities
//! - Identity service capabilities

use zos_kernel::ProcessId;
use super::super::{log, Supervisor};

impl Supervisor {
    /// Set up capabilities for a spawned process
    pub(super) fn setup_process_capabilities(&mut self, process_pid: ProcessId, name: &str) {
        if name == "init" {
            self.init_spawned = true;
            log("[supervisor] Init process spawned (PID 1)");
            
            // Grant supervisor (PID 0) capability to Init's endpoint for IPC
            self.grant_supervisor_capability_to_init(process_pid);
        } else if name == "permission_manager" {
            // Grant supervisor (PID 0) capability to PM's endpoint for IPC
            self.grant_supervisor_capability_to_pm(process_pid);
        } else if self.init_spawned {
            // Grant this process a capability to init's endpoint (slot 0 of PID 1)
            let init_pid = ProcessId(1);
            match self.system.grant_capability(
                init_pid,
                0, // init's endpoint at slot 0
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
            // This goes in slot 3 (VFS_ENDPOINT_SLOT) for VfsClient to use
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
        }
        
        // When terminal is spawned, grant Init (PID 1) capability to terminal's input endpoint
        // and grant supervisor capability for console input routing
        if name == "terminal" {
            self.grant_terminal_capabilities(process_pid);
        }
        
        // When vfs_service is spawned, grant its endpoint to processes that need VFS access
        // and grant Init (PID 1) capability to deliver IPC messages to VFS
        if name == "vfs_service" {
            self.grant_vfs_capabilities_to_existing_processes(process_pid);
            self.grant_init_capability_to_service("vfs_service", process_pid);
        }
        
        // When identity_service is spawned, grant its endpoint to processes that need identity access
        // and grant Init (PID 1) capability to deliver IPC messages to Identity
        if name == "identity_service" {
            self.grant_identity_capabilities_to_existing_processes(process_pid);
            self.grant_init_capability_to_service("identity_service", process_pid);
        }
        
        // When time_service is spawned, grant Init (PID 1) capability to deliver IPC messages
        if name == "time_service" {
            self.grant_init_capability_to_service("time_service", process_pid);
        }
    }
    
    /// Grant supervisor (PID 0) capability to Init's endpoint
    ///
    /// This enables the supervisor to send IPC messages to Init for operations
    /// that need capability-checked kernel access.
    fn grant_supervisor_capability_to_init(&mut self, init_pid: ProcessId) {
        // Init's endpoint is at slot 0
        const INIT_ENDPOINT_SLOT: u32 = 0;
        let supervisor_pid = ProcessId(0);
        
        // Get Init's endpoint ID from slot 0
        let endpoint_id = match self.system.get_cap_space(init_pid) {
            Some(cspace) => match cspace.get(INIT_ENDPOINT_SLOT) {
                Some(cap) => zos_kernel::EndpointId(cap.object_id),
                None => {
                    log("[supervisor] Init has no endpoint at slot 0");
                    return;
                }
            },
            None => {
                log("[supervisor] Init has no CSpace");
                return;
            }
        };
        
        // Grant supervisor capability to Init's endpoint
        match self.system.grant_capability_to_endpoint(
            init_pid,
            endpoint_id,
            supervisor_pid,
            zos_kernel::Permissions {
                read: false,
                write: true, // Can send to Init
                grant: false,
            },
        ) {
            Ok(slot) => {
                self.init_endpoint_slot = Some(slot);
                log(&format!(
                    "[supervisor] Granted Init endpoint cap to supervisor at slot {}",
                    slot
                ));
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to grant Init cap to supervisor: {:?}",
                    e
                ));
            }
        }
    }
    
    /// Grant supervisor (PID 0) capability to PermissionManager's endpoint
    fn grant_supervisor_capability_to_pm(&mut self, pm_pid: ProcessId) {
        // PM's input endpoint is at slot 1
        const PM_INPUT_SLOT: u32 = 1;
        let supervisor_pid = ProcessId(0);
        
        // Get PM's endpoint ID from slot 1
        let endpoint_id = match self.system.get_cap_space(pm_pid) {
            Some(cspace) => match cspace.get(PM_INPUT_SLOT) {
                Some(cap) => zos_kernel::EndpointId(cap.object_id),
                None => {
                    log("[supervisor] PM has no endpoint at slot 1");
                    return;
                }
            },
            None => {
                log("[supervisor] PM has no CSpace");
                return;
            }
        };
        
        // Grant supervisor capability to PM's endpoint
        match self.system.grant_capability_to_endpoint(
            pm_pid,
            endpoint_id,
            supervisor_pid,
            zos_kernel::Permissions {
                read: false,
                write: true, // Can send to PM
                grant: false,
            },
        ) {
            Ok(slot) => {
                self.pm_endpoint_slot = Some(slot);
                log(&format!(
                    "[supervisor] Granted PM endpoint cap to supervisor at slot {}",
                    slot
                ));
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to grant PM cap to supervisor: {:?}",
                    e
                ));
            }
        }
    }
    
    /// Grant capabilities for terminal process
    ///
    /// - Grant Init (PID 1) capability to terminal's input endpoint
    /// - Grant supervisor (PID 0) capability to terminal's input endpoint  
    pub(super) fn grant_terminal_capabilities(&mut self, terminal_pid: ProcessId) {
        // Terminal's input endpoint is at slot 1
        const TERMINAL_INPUT_SLOT: u32 = 1;
        let init_pid = ProcessId(1);
        let supervisor_pid = ProcessId(0);
        
        // Get terminal's input endpoint ID
        let endpoint_id = match self.system.get_cap_space(terminal_pid) {
            Some(cspace) => match cspace.get(TERMINAL_INPUT_SLOT) {
                Some(cap) => zos_kernel::EndpointId(cap.object_id),
                None => {
                    log(&format!(
                        "[supervisor] Terminal PID {} has no endpoint at slot {}",
                        terminal_pid.0, TERMINAL_INPUT_SLOT
                    ));
                    return;
                }
            },
            None => {
                log(&format!(
                    "[supervisor] Terminal PID {} has no CSpace",
                    terminal_pid.0
                ));
                return;
            }
        };
        
        // Grant Init capability to terminal's input endpoint
        match self.system.grant_capability_to_endpoint(
            terminal_pid,
            endpoint_id,
            init_pid,
            zos_kernel::Permissions {
                read: false,
                write: true, // Can send to terminal
                grant: false,
            },
        ) {
            Ok(slot) => {
                log(&format!(
                    "[supervisor] Granted terminal {} input cap to Init at slot {}",
                    terminal_pid.0, slot
                ));
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to grant terminal cap to Init: {:?}",
                    e
                ));
            }
        }
        
        // Grant supervisor capability to terminal's input endpoint
        match self.system.grant_capability_to_endpoint(
            terminal_pid,
            endpoint_id,
            supervisor_pid,
            zos_kernel::Permissions {
                read: false,
                write: true, // Can send to terminal
                grant: false,
            },
        ) {
            Ok(slot) => {
                self.terminal_endpoint_slots.insert(terminal_pid.0, slot);
                log(&format!(
                    "[supervisor] Granted terminal {} input cap to supervisor at slot {}",
                    terminal_pid.0, slot
                ));
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to grant terminal cap to supervisor: {:?}",
                    e
                ));
            }
        }
    }
    
    /// Grant VFS endpoint capability to a specific process
    fn grant_vfs_capability_to_process(&mut self, target_pid: ProcessId, target_name: &str) {
        // Find VFS service process
        let vfs_pid = self.find_vfs_service_pid();
        if let Some(vfs_pid) = vfs_pid {
            // VFS service's input endpoint is at slot 1
            const VFS_INPUT_SLOT: u32 = 1;
            
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
    fn grant_vfs_capabilities_to_existing_processes(&mut self, vfs_pid: ProcessId) {
        // VFS service's input endpoint is at slot 1
        const VFS_INPUT_SLOT: u32 = 1;
        
        // Get list of processes that need VFS access
        let processes: Vec<(ProcessId, String)> = self
            .system
            .list_processes()
            .into_iter()
            .filter(|(pid, proc)| {
                // Grant to all processes except init, supervisor, and vfs_service itself
                pid.0 > 1 && *pid != vfs_pid && proc.name != "vfs_service"
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
            if let Ok((eid, slot)) = self.system.create_endpoint(pid) {
                log(&format!(
                    "[supervisor] Created VFS response endpoint {} at slot {} for {} (PID {})",
                    eid.0, slot, name, pid.0
                ));
                
                // Grant Init capability to this VFS response endpoint
                // This enables Init to deliver VFS responses to the correct endpoint (slot 4)
                self.grant_init_vfs_response_capability(&name, pid);
            }
        }
    }
    
    /// Find the VFS service process ID
    fn find_vfs_service_pid(&self) -> Option<ProcessId> {
        for (pid, proc) in self.system.list_processes() {
            if proc.name == "vfs_service" {
                return Some(pid);
            }
        }
        None
    }
    
    /// Grant Identity Service endpoint capability to a specific process
    ///
    /// This enables the process to send IPC requests to the Identity Service.
    /// The process can then transfer a reply endpoint capability with its request
    /// to receive responses via proper capability-mediated IPC.
    fn grant_identity_capability_to_process(&mut self, target_pid: ProcessId, target_name: &str) {
        // Find Identity service process
        let identity_pid = self.find_identity_service_pid_internal();
        if let Some(identity_pid) = identity_pid {
            // Identity service's input endpoint is at slot 1
            const IDENTITY_INPUT_SLOT: u32 = 1;
            
            match self.system.grant_capability(
                identity_pid,
                IDENTITY_INPUT_SLOT,
                target_pid,
                zos_kernel::Permissions {
                    read: false,  // Only need write (send) permission
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
    /// Called when identity_service spawns to grant its endpoint capability
    /// to all existing processes that may need identity operations.
    fn grant_identity_capabilities_to_existing_processes(&mut self, identity_pid: ProcessId) {
        // Identity service's input endpoint is at slot 1
        const IDENTITY_INPUT_SLOT: u32 = 1;
        
        // Get list of processes that need Identity access
        let processes: Vec<(ProcessId, String)> = self
            .system
            .list_processes()
            .into_iter()
            .filter(|(pid, proc)| {
                // Grant to all processes except init, supervisor, and identity_service itself
                // Also exclude vfs_service since it doesn't need identity access
                pid.0 > 1 
                    && *pid != identity_pid 
                    && proc.name != "identity_service"
                    && proc.name != "vfs_service"
            })
            .map(|(pid, proc)| (pid, proc.name.clone()))
            .collect();
        
        for (pid, name) in processes {
            match self.system.grant_capability(
                identity_pid,
                IDENTITY_INPUT_SLOT,
                pid,
                zos_kernel::Permissions {
                    read: false,  // Only need write (send) permission
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
    fn find_identity_service_pid_internal(&self) -> Option<ProcessId> {
        for (pid, proc) in self.system.list_processes() {
            if proc.name == "identity_service" {
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
    /// This is called when identity_service and vfs_service spawn.
    fn grant_init_capability_to_service(&mut self, service_name: &str, service_pid: ProcessId) {
        // Service's input endpoint is at slot 1
        const SERVICE_INPUT_SLOT: u32 = 1;
        let init_pid = ProcessId(1);
        
        // Get service's input endpoint ID from slot 1
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
                
                // Notify Init about the capability via IPC message
                self.notify_init_service_cap(service_pid.0, slot);
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to grant {} cap to Init: {:?}",
                    service_name, e
                ));
            }
        }
    }
    
    /// Notify Init about a granted service capability via IPC.
    ///
    /// Sends MSG_SERVICE_CAP_GRANTED to Init with [service_pid, cap_slot].
    fn notify_init_service_cap(&mut self, service_pid: u64, cap_slot: u32) {
        let init_slot = match self.init_endpoint_slot {
            Some(slot) => slot,
            None => {
                log("[supervisor] Cannot notify Init of service cap: no Init capability");
                return;
            }
        };
        
        use zos_ipc::init::MSG_SERVICE_CAP_GRANTED;
        
        // Build message: [service_pid: u32, cap_slot: u32]
        let mut payload = Vec::with_capacity(8);
        payload.extend_from_slice(&(service_pid as u32).to_le_bytes());
        payload.extend_from_slice(&cap_slot.to_le_bytes());
        
        let supervisor_pid = ProcessId(0);
        
        match self.system.ipc_send(supervisor_pid, init_slot, MSG_SERVICE_CAP_GRANTED, payload) {
            Ok(()) => {
                log(&format!(
                    "[supervisor] Notified Init of service PID {} cap at slot {}",
                    service_pid, cap_slot
                ));
            }
            Err(e) => {
                log(&format!(
                    "[supervisor] Failed to notify Init of service cap: {:?}",
                    e
                ));
            }
        }
    }
    
    /// Grant Init (PID 1) a capability to a process's VFS response endpoint (slot 4).
    ///
    /// This enables Init to deliver VFS responses to the correct endpoint,
    /// preventing the routing issue where VFS responses go to slot 1 (input)
    /// instead of slot 4 (VFS response). After granting the capability,
    /// Init is notified via MSG_VFS_RESPONSE_CAP_GRANTED.
    fn grant_init_vfs_response_capability(&mut self, process_name: &str, process_pid: ProcessId) {
        // VFS response endpoint is at slot 4
        const VFS_RESPONSE_SLOT: u32 = 4;
        let init_pid = ProcessId(1);
        
        // Get process's VFS response endpoint ID from slot 4
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
                log(&format!("[supervisor] {} (PID {}) has no CSpace", process_name, process_pid.0));
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
        
        match self.system.ipc_send(supervisor_pid, init_slot, MSG_VFS_RESPONSE_CAP_GRANTED, payload) {
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
