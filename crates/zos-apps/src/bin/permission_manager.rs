//! Permission Manager Service (PID 2)
//!
//! The PermissionManager is the system's capability authority. It:
//! - Receives root capabilities from Init at spawn
//! - Handles capability requests from applications
//! - Grants/revokes capabilities to/from processes
//! - Maintains audit trail of all capability operations
//!
//! # Protocol
//!
//! Apps communicate with PermissionManager via IPC:
//!
//! - `MSG_REQUEST_CAPABILITY (0x2010)`: Request a capability
//! - `MSG_REVOKE_CAPABILITY (0x2011)`: Request capability revocation
//! - `MSG_LIST_MY_CAPS (0x2012)`: Query own capabilities
//! - `MSG_CAPABILITY_RESPONSE (0x2013)`: Response from PermissionManager

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use zos_apps::manifest::PERMISSION_MANAGER_MANIFEST;
use zos_apps::syscall;
use zos_apps::{app_main, AppContext, AppError, AppManifest, ControlFlow, Message, ZeroApp};

// =============================================================================
// Protocol Constants (from zos-ipc via zos-process)
// =============================================================================
// All IPC message constants are defined in zos-ipc as the single source of truth.

pub use zos_apps::pm::{
    MSG_CAPABILITY_RESPONSE, MSG_CAPS_LIST_RESPONSE, MSG_LIST_MY_CAPS, MSG_REQUEST_CAPABILITY,
    MSG_REVOKE_CAPABILITY,
};

pub use zos_apps::supervisor::MSG_SUPERVISOR_REVOKE_CAP;

// =============================================================================
// Object Types (mirrors zero-kernel)
// =============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ObjectType {
    Endpoint = 1,
    Process = 2,
    Memory = 3,
    Irq = 4,
    IoPort = 5,
    Console = 6,
    Storage = 7,
    Network = 8,
}

impl ObjectType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(ObjectType::Endpoint),
            2 => Some(ObjectType::Process),
            3 => Some(ObjectType::Memory),
            4 => Some(ObjectType::Irq),
            5 => Some(ObjectType::IoPort),
            6 => Some(ObjectType::Console),
            7 => Some(ObjectType::Storage),
            8 => Some(ObjectType::Network),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            ObjectType::Endpoint => "Endpoint",
            ObjectType::Process => "Process",
            ObjectType::Memory => "Memory",
            ObjectType::Irq => "IRQ",
            ObjectType::IoPort => "I/O Port",
            ObjectType::Console => "Console",
            ObjectType::Storage => "Storage",
            ObjectType::Network => "Network",
        }
    }
}

// =============================================================================
// Permission Tracking
// =============================================================================

/// Key for tracking granted capabilities: (pid, object_type)
type CapKey = (u32, u8);

/// Information about a granted capability
#[derive(Clone, Debug)]
struct GrantedCap {
    /// Capability slot in the target process's CSpace
    slot: u32,
    /// Permissions granted (read=1, write=2, grant=4)
    permissions: u8,
    /// Reason for the grant
    #[allow(dead_code)]
    reason: String,
}

// =============================================================================
// PermissionManager Application
// =============================================================================

/// PermissionManager - the system's capability authority (PID 2)
#[derive(Default)]
pub struct PermissionManager {
    /// Map from (pid, object_type) to granted capability info
    granted_caps: BTreeMap<CapKey, GrantedCap>,

    /// Root capability slots (received from Init)
    /// These are the source capabilities for grants
    console_cap_slot: Option<u32>,
    spawn_cap_slot: Option<u32>,
    endpoint_cap_slot: Option<u32>,
}

impl PermissionManager {
    /// Record a capability grant
    fn record_grant(
        &mut self,
        target_pid: u32,
        object_type: ObjectType,
        slot: u32,
        permissions: u8,
        reason: String,
    ) {
        let key = (target_pid, object_type as u8);
        self.granted_caps.insert(
            key,
            GrantedCap {
                slot,
                permissions,
                reason,
            },
        );
    }

    /// Remove a grant record
    fn remove_grant(&mut self, target_pid: u32, object_type: ObjectType) -> Option<GrantedCap> {
        let key = (target_pid, object_type as u8);
        self.granted_caps.remove(&key)
    }

    /// Look up a grant
    fn get_grant(&self, target_pid: u32, object_type: ObjectType) -> Option<&GrantedCap> {
        let key = (target_pid, object_type as u8);
        self.granted_caps.get(&key)
    }

    /// List all grants for a process
    fn list_grants(&self, pid: u32) -> Vec<(ObjectType, &GrantedCap)> {
        self.granted_caps
            .iter()
            .filter(|((p, _), _)| *p == pid)
            .filter_map(|((_, obj_type), cap)| ObjectType::from_u8(*obj_type).map(|ot| (ot, cap)))
            .collect()
    }

    /// Handle capability request
    fn handle_cap_request(&mut self, ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        // Parse request: [object_type: u8, permissions: u8, reason_len: u16, reason: [u8]]
        if msg.data.len() < 4 {
            return self.send_error_response(ctx, msg.from_pid, "Invalid request format");
        }

        let object_type = match ObjectType::from_u8(msg.data[0]) {
            Some(ot) => ot,
            None => {
                return self.send_error_response(ctx, msg.from_pid, "Unknown object type");
            }
        };

        let permissions = msg.data[1];
        let reason_len = u16::from_le_bytes([msg.data[2], msg.data[3]]) as usize;

        let reason = if msg.data.len() >= 4 + reason_len {
            core::str::from_utf8(&msg.data[4..4 + reason_len])
                .unwrap_or("(invalid reason)")
                .into()
        } else {
            String::from("(no reason)")
        };

        syscall::debug(&format!(
            "PermMgr: Cap request from PID {} for {} ({:02x}) - {}",
            msg.from_pid,
            object_type.name(),
            permissions,
            reason
        ));

        // Check if already granted
        if let Some(existing) = self.get_grant(msg.from_pid, object_type) {
            syscall::debug(&format!(
                "PermMgr: {} already granted to PID {} at slot {}",
                object_type.name(),
                msg.from_pid,
                existing.slot
            ));
            return self.send_success_response(ctx, msg.from_pid, existing.slot);
        }

        // Determine source slot based on object type
        let source_slot = match object_type {
            ObjectType::Console => self.console_cap_slot,
            ObjectType::Process => self.spawn_cap_slot,
            ObjectType::Endpoint => self.endpoint_cap_slot,
            _ => {
                syscall::debug(&format!(
                    "PermMgr: {} not yet supported",
                    object_type.name()
                ));
                return self.send_error_response(
                    ctx,
                    msg.from_pid,
                    &format!("{} not yet supported", object_type.name()),
                );
            }
        };

        let from_slot = match source_slot {
            Some(s) => s,
            None => {
                syscall::debug(&format!("PermMgr: No root cap for {}", object_type.name()));
                return self.send_error_response(
                    ctx,
                    msg.from_pid,
                    &format!("No root capability for {}", object_type.name()),
                );
            }
        };

        // Grant via syscall
        let perms = syscall::Permissions {
            read: (permissions & 0x01) != 0,
            write: (permissions & 0x02) != 0,
            grant: (permissions & 0x04) != 0,
        };

        match syscall::cap_grant(from_slot, msg.from_pid, perms) {
            Ok(new_slot) => {
                syscall::debug(&format!(
                    "PermMgr: Granted {} to PID {} at slot {}",
                    object_type.name(),
                    msg.from_pid,
                    new_slot
                ));

                self.record_grant(msg.from_pid, object_type, new_slot, permissions, reason);
                self.send_success_response(ctx, msg.from_pid, new_slot)
            }
            Err(e) => {
                syscall::debug(&format!(
                    "PermMgr: Grant syscall failed: {} - using debug fallback",
                    e
                ));

                // Fall back to debug message for supervisor
                syscall::debug(&format!(
                    "PERMMGR:GRANT:{}:{}:{}",
                    msg.from_pid, from_slot, permissions
                ));

                // Optimistically record and respond
                self.record_grant(msg.from_pid, object_type, from_slot, permissions, reason);
                self.send_success_response(ctx, msg.from_pid, from_slot)
            }
        }
    }

    /// Handle capability revocation request
    fn handle_cap_revoke(&mut self, ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        // Parse request: [slot: u32]
        if msg.data.len() < 4 {
            return self.send_error_response(ctx, msg.from_pid, "Invalid revoke format");
        }

        let slot = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);

        syscall::debug(&format!(
            "PermMgr: Revoke request from PID {} for slot {}",
            msg.from_pid, slot
        ));

        // Find which capability this is
        let mut found_type: Option<ObjectType> = None;
        for ((pid, obj_type), grant) in &self.granted_caps {
            if *pid == msg.from_pid && grant.slot == slot {
                found_type = ObjectType::from_u8(*obj_type);
                break;
            }
        }

        if let Some(obj_type) = found_type {
            // Signal supervisor to revoke
            syscall::debug(&format!("PERMMGR:REVOKE:{}:{}", msg.from_pid, slot));

            self.remove_grant(msg.from_pid, obj_type);
            self.send_success_response(ctx, msg.from_pid, slot)
        } else {
            self.send_error_response(ctx, msg.from_pid, "Capability not found")
        }
    }

    /// Handle list capabilities request
    fn handle_list_caps(&self, ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let grants = self.list_grants(msg.from_pid);

        // Build response: [count: u8, (slot: u32, type: u8, perms: u8)...]
        let mut response = Vec::new();
        response.push(grants.len() as u8);

        for (obj_type, grant) in &grants {
            response.extend_from_slice(&grant.slot.to_le_bytes());
            response.push(*obj_type as u8);
            response.push(grant.permissions);
        }

        // Send via debug channel (supervisor routes to process)
        let response_hex: String = response.iter().map(|b| format!("{:02x}", b)).collect();
        syscall::debug(&format!(
            "PERMMGR:CAPS_LIST:{}:{}",
            msg.from_pid, response_hex
        ));

        // Also try direct IPC if we have endpoint
        if let Some(slot) = ctx.ui_endpoint {
            let _ = syscall::send(slot, MSG_CAPS_LIST_RESPONSE, &response);
        }

        Ok(())
    }

    /// Handle supervisor request to revoke a capability from a process.
    ///
    /// The supervisor sends this message when the UI requests capability revocation.
    /// PM performs the revocation and notifies the affected process.
    ///
    /// Payload: [target_pid: u32, slot: u32, reason: u8]
    fn handle_supervisor_revoke(&mut self, msg: &Message) -> Result<(), AppError> {
        // Verify sender is supervisor (PID 0)
        if msg.from_pid != 0 {
            syscall::debug(&format!(
                "PermMgr: SECURITY - Supervisor revoke request from non-supervisor PID {}",
                msg.from_pid
            ));
            return Ok(());
        }

        // Parse payload
        if msg.data.len() < 9 {
            syscall::debug("PermMgr: Invalid supervisor revoke payload (too short)");
            return Ok(());
        }

        let target_pid = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);
        let slot = u32::from_le_bytes([msg.data[4], msg.data[5], msg.data[6], msg.data[7]]);
        let reason = msg.data[8];

        syscall::debug(&format!(
            "PermMgr: Supervisor revoke request for PID {} slot {} reason {}",
            target_pid, slot, reason
        ));

        // Perform the revocation via syscall
        // Uses privileged cap_revoke_from to revoke from another process
        match syscall::cap_revoke_from(target_pid, slot) {
            Ok(()) => {
                syscall::debug(&format!(
                    "PermMgr: Successfully revoked cap from PID {} slot {}",
                    target_pid, slot
                ));

                // Remove from our tracking if we have it
                for obj_type in 1..=8u8 {
                    let key = (target_pid, obj_type);
                    if let Some(grant) = self.granted_caps.get(&key) {
                        if grant.slot == slot {
                            self.granted_caps.remove(&key);
                            break;
                        }
                    }
                }

                // Notify the affected process via debug channel
                // The supervisor or Init can route this notification
                syscall::debug(&format!(
                    "PERMMGR:REVOKED:{}:{}:{}",
                    target_pid, slot, reason
                ));
            }
            Err(e) => {
                syscall::debug(&format!(
                    "PermMgr: Revoke syscall failed for PID {} slot {}: {}",
                    target_pid, slot, e
                ));
            }
        }

        Ok(())
    }

    /// Send success response
    fn send_success_response(
        &self,
        ctx: &AppContext,
        _to_pid: u32,
        slot: u32,
    ) -> Result<(), AppError> {
        let mut response = Vec::new();
        response.push(1u8); // Success
        response.extend_from_slice(&slot.to_le_bytes());

        if let Some(endpoint_slot) = ctx.ui_endpoint {
            syscall::send(endpoint_slot, MSG_CAPABILITY_RESPONSE, &response)
                .map_err(|e| AppError::IpcError(format!("Send failed: {}", e)))?;
        }

        Ok(())
    }

    /// Send error response
    fn send_error_response(
        &self,
        ctx: &AppContext,
        _to_pid: u32,
        error: &str,
    ) -> Result<(), AppError> {
        let mut response = Vec::new();
        response.push(0u8); // Failure
        let error_bytes = error.as_bytes();
        response.extend_from_slice(&(error_bytes.len() as u16).to_le_bytes());
        response.extend_from_slice(error_bytes);

        if let Some(endpoint_slot) = ctx.ui_endpoint {
            syscall::send(endpoint_slot, MSG_CAPABILITY_RESPONSE, &response)
                .map_err(|e| AppError::IpcError(format!("Send failed: {}", e)))?;
        }

        Ok(())
    }
}

impl ZeroApp for PermissionManager {
    fn manifest() -> &'static AppManifest {
        &PERMISSION_MANAGER_MANIFEST
    }

    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError> {
        syscall::debug(&format!("PermissionManager starting (PID {})", ctx.pid));

        // Set up root capability slots
        // In a full implementation, these would be granted by Init at spawn
        // For now, we use well-known slots that supervisor sets up
        self.console_cap_slot = Some(0); // Console output
        self.spawn_cap_slot = Some(2); // Process spawn
        self.endpoint_cap_slot = Some(1); // Endpoint creation

        syscall::debug("PermissionManager: Root capabilities configured");
        syscall::debug(&format!(
            "  Console slot: {:?}, Spawn slot: {:?}, Endpoint slot: {:?}",
            self.console_cap_slot, self.spawn_cap_slot, self.endpoint_cap_slot
        ));

        Ok(())
    }

    fn update(&mut self, _ctx: &AppContext) -> ControlFlow {
        ControlFlow::Yield
    }

    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError> {
        match msg.tag {
            MSG_REQUEST_CAPABILITY => self.handle_cap_request(ctx, &msg),
            MSG_REVOKE_CAPABILITY => self.handle_cap_revoke(ctx, &msg),
            MSG_LIST_MY_CAPS => self.handle_list_caps(ctx, &msg),
            MSG_SUPERVISOR_REVOKE_CAP => self.handle_supervisor_revoke(&msg),
            _ => {
                syscall::debug(&format!(
                    "PermMgr: Unknown message tag 0x{:x} from PID {}",
                    msg.tag, msg.from_pid
                ));
                Ok(())
            }
        }
    }

    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("PermissionManager: shutting down");
        syscall::debug(&format!(
            "  Total grants issued: {}",
            self.granted_caps.len()
        ));
    }
}

// Entry point
app_main!(PermissionManager);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("PermissionManager is meant to run as WASM in Zero OS");
}
