//! Time Service (PID 5)
//!
//! The TimeService manages time-related settings. It:
//! - Stores user time format preferences (12h/24h)
//! - Stores user timezone preferences
//! - Persists settings via VFS service IPC (async pattern)
//!
//! # Safety Invariants
//!
//! **Success means:**
//! - GET: Settings returned to client (from cache or storage)
//! - SET: Settings written to storage AND cache updated AND success response sent
//!
//! **Acceptable partial failure:**
//! - Storage read fails → return default settings (fail-open for read-only)
//! - Cache may be stale if storage write succeeds but cache update fails
//!
//! **Forbidden:**
//! - Returning success for SET before storage write completes
//! - Allowing unauthorized processes to modify system time settings
//! - Unbounded pending operations (DoS vector)
//!
//! # Protocol
//!
//! Apps communicate with TimeService via IPC:
//!
//! - `MSG_GET_TIME_SETTINGS (0x8100)`: Get current time settings
//! - `MSG_SET_TIME_SETTINGS (0x8102)`: Update time settings
//!
//! # Storage Access
//!
//! This service uses VFS IPC (async pattern) to persist settings.
//! All storage operations flow through VFS Service (PID 4) per Invariant 31.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::manifests::TIME_MANIFEST;
use zos_apps::syscall;
use zos_apps::{AppContext, AppError, ControlFlow, Message, ZeroApp};
use zos_vfs::async_client;
use zos_vfs::ipc::vfs_msg;

// =============================================================================
// IPC Message Tags (re-exported from zos-ipc for single source of truth)
// =============================================================================

/// Message tags for time service - re-exported from zos-ipc.
///
/// Note: Constants are defined in zos-ipc as the single source of truth
/// per Invariant 32. This module re-exports for local convenience.
pub mod time_msg {
    pub use zos_ipc::time::*;
}

// =============================================================================
// Time Settings Types
// =============================================================================

/// Default timezone for time settings
fn default_timezone() -> String {
    String::from("UTC")
}

/// Time settings that can be persisted
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TimeSettings {
    /// Use 24-hour time format (false = 12-hour with AM/PM)
    #[serde(default)]
    pub time_format_24h: bool,
    /// Timezone identifier (e.g., "America/New_York", "UTC")
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

impl Default for TimeSettings {
    fn default() -> Self {
        Self {
            time_format_24h: false,
            timezone: default_timezone(),
        }
    }
}

impl TimeSettings {
    /// Storage path for time settings
    pub fn storage_path() -> &'static str {
        "/system/settings/time.json"
    }

    /// Serialize to JSON bytes
    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| {
            // Fallback to manual serialization if serde fails
            format!(
                r#"{{"time_format_24h":{},"timezone":"{}"}}"#,
                self.time_format_24h, self.timezone
            )
            .into_bytes()
        })
    }

    /// Parse from JSON bytes
    pub fn from_json(data: &[u8]) -> Option<Self> {
        serde_json::from_slice(data).ok()
    }
}

// =============================================================================
// Pending VFS Operations
// =============================================================================

/// Tracks pending VFS operations awaiting responses.
///
/// Each operation is assigned a unique request_id for correlation,
/// allowing multiple concurrent VFS operations.
#[derive(Clone)]
enum PendingOp {
    /// Reading settings for get request
    GetSettings {
        client_pid: u32,
        cap_slots: Vec<u32>,
    },
    /// Writing settings after set request
    WriteSettings {
        client_pid: u32,
        settings: TimeSettings,
        cap_slots: Vec<u32>,
    },
    /// Initial load of settings on startup
    InitialLoad,
}

/// Operation type for matching responses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpType {
    Read,
    Write,
}

// =============================================================================
// TimeService Application
// =============================================================================

use alloc::collections::BTreeMap;

// =============================================================================
// Permission Constants
// =============================================================================

/// Maximum number of pending VFS operations (DoS protection per Rule 11)
const MAX_PENDING_OPS: usize = 32;

/// System service PIDs that are trusted for time settings operations.
/// - PID 0: Supervisor
/// - PID 1: Init
/// - PID 2: Permission Service  
/// - PID 3: Desktop/Settings UI
const TRUSTED_PIDS_FOR_TIME_SETTINGS: &[u32] = &[0, 1, 2, 3];

/// TimeService - manages time display settings
pub struct TimeService {
    /// Whether we have registered with init
    registered: bool,
    /// Current time settings (cached in memory)
    settings: TimeSettings,
    /// Pending VFS operations: request_id -> (operation, op_type)
    /// Supports concurrent operations with unique request IDs
    pending_ops: BTreeMap<u32, (PendingOp, OpType)>,
    /// Next request ID for correlation (wraps around at u32::MAX)
    next_request_id: u32,
    /// Whether settings have been loaded from storage
    settings_loaded: bool,
}

impl Default for TimeService {
    fn default() -> Self {
        Self {
            registered: false,
            settings: TimeSettings {
                time_format_24h: false,
                timezone: String::from("UTC"),
            },
            pending_ops: BTreeMap::new(),
            next_request_id: 1,
            settings_loaded: false,
        }
    }
}

impl TimeService {
    /// Allocate a new request ID for operation correlation.
    fn alloc_request_id(&mut self) -> u32 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        if self.next_request_id == 0 {
            self.next_request_id = 1; // Skip 0
        }
        id
    }

    /// Find and remove a pending operation by type (for VFS responses without request IDs).
    ///
    /// VFS responses don't include request IDs, so we match by operation type.
    /// This finds the oldest pending operation of the given type.
    fn take_pending_by_type(&mut self, op_type: OpType) -> Option<(u32, PendingOp)> {
        // Find the first (oldest) operation matching the type
        let request_id = self
            .pending_ops
            .iter()
            .find(|(_, (_, t))| *t == op_type)
            .map(|(id, _)| *id);

        if let Some(id) = request_id {
            self.pending_ops.remove(&id).map(|(op, _)| (id, op))
        } else {
            None
        }
    }

    /// Check if caller is authorized to read time settings.
    /// GET operations are open to all processes (read-only, non-sensitive).
    fn check_get_permission(&self, _from_pid: u32) -> bool {
        // Time settings are non-sensitive, allow all reads
        true
    }

    /// Check if caller is authorized to modify time settings.
    /// SET operations are restricted to trusted system services (fail-closed per Rule 4).
    fn check_set_permission(&self, from_pid: u32) -> bool {
        let allowed = TRUSTED_PIDS_FOR_TIME_SETTINGS.contains(&from_pid);
        if !allowed {
            syscall::debug(&format!(
                "TimeService: SECURITY - SET denied for PID {} (not in trusted list)",
                from_pid
            ));
        }
        allowed
    }

    /// Check and enforce pending operation limits (DoS protection per Rule 11).
    /// Returns true if a new operation can be accepted.
    fn check_pending_limit(&self) -> bool {
        if self.pending_ops.len() >= MAX_PENDING_OPS {
            syscall::debug(&format!(
                "TimeService: Pending operation limit reached ({}/{})",
                self.pending_ops.len(),
                MAX_PENDING_OPS
            ));
            false
        } else {
            true
        }
    }
}

impl TimeService {
    // =========================================================================
    // VFS IPC helpers (async, non-blocking) - Invariant 31 compliant
    // =========================================================================

    /// Start async VFS read and track the pending operation.
    /// Uses VFS IPC instead of direct storage syscalls per Invariant 31.
    /// Returns the request_id for correlation.
    fn start_vfs_read(&mut self, path: &str, pending_op: PendingOp) -> Result<u32, AppError> {
        let request_id = self.alloc_request_id();
        syscall::debug(&format!(
            "TimeService: sending VFS read request for {} (req_id={})",
            path, request_id
        ));
        async_client::send_read_request(path)?;
        self.pending_ops.insert(request_id, (pending_op, OpType::Read));
        Ok(request_id)
    }

    /// Start async VFS write and track the pending operation.
    /// Uses VFS IPC instead of direct storage syscalls per Invariant 31.
    /// Returns the request_id for correlation.
    fn start_vfs_write(
        &mut self,
        path: &str,
        value: &[u8],
        pending_op: PendingOp,
    ) -> Result<u32, AppError> {
        let request_id = self.alloc_request_id();
        syscall::debug(&format!(
            "TimeService: sending VFS write request for {} ({} bytes, req_id={})",
            path,
            value.len(),
            request_id
        ));
        async_client::send_write_request(path, value)?;
        self.pending_ops.insert(request_id, (pending_op, OpType::Write));
        Ok(request_id)
    }

    // =========================================================================
    // Request handlers
    // =========================================================================

    /// Handle MSG_GET_TIME_SETTINGS
    fn handle_get_time_settings(
        &mut self,
        _ctx: &AppContext,
        msg: &Message,
    ) -> Result<(), AppError> {
        syscall::debug(&format!(
            "TimeService: Handling get time settings request from PID {}",
            msg.from_pid
        ));

        // Permission check (Rule 4: fail-closed)
        if !self.check_get_permission(msg.from_pid) {
            return self.send_error_response(
                msg.from_pid,
                &msg.cap_slots,
                "Permission denied: GET_TIME_SETTINGS requires time capability",
            );
        }

        // If settings are loaded, return immediately from cache
        if self.settings_loaded {
            return self.send_settings_response(
                msg.from_pid,
                &msg.cap_slots,
                &self.settings,
                time_msg::MSG_GET_TIME_SETTINGS_RESPONSE,
            );
        }

        // DoS protection: check pending operation limit (Rule 11)
        if !self.check_pending_limit() {
            return self.send_error_response(
                msg.from_pid,
                &msg.cap_slots,
                "Service busy: pending operation limit reached",
            );
        }

        // Otherwise, start async VFS read
        self.start_vfs_read(
            TimeSettings::storage_path(),
            PendingOp::GetSettings {
                client_pid: msg.from_pid,
                cap_slots: msg.cap_slots.clone(),
            },
        ).map(|_| ())
    }

    /// Handle MSG_SET_TIME_SETTINGS
    fn handle_set_time_settings(
        &mut self,
        _ctx: &AppContext,
        msg: &Message,
    ) -> Result<(), AppError> {
        syscall::debug(&format!(
            "TimeService: Handling set time settings request from PID {}",
            msg.from_pid
        ));

        // Permission check (Rule 4: fail-closed)
        if !self.check_set_permission(msg.from_pid) {
            return self.send_error_response(
                msg.from_pid,
                &msg.cap_slots,
                "Permission denied: SET_TIME_SETTINGS requires system privilege",
            );
        }

        // DoS protection: check pending operation limit (Rule 11)
        if !self.check_pending_limit() {
            return self.send_error_response(
                msg.from_pid,
                &msg.cap_slots,
                "Service busy: pending operation limit reached",
            );
        }

        // Parse the settings from the request
        let new_settings = match TimeSettings::from_json(&msg.data) {
            Some(s) => s,
            None => {
                syscall::debug(&format!(
                    "TimeService: Failed to parse settings JSON from PID {} (len={})",
                    msg.from_pid,
                    msg.data.len()
                ));
                // Send error response with context (Rule 9)
                return self.send_error_response(
                    msg.from_pid,
                    &msg.cap_slots,
                    "Invalid settings format: JSON parse failed",
                );
            }
        };

        syscall::debug(&format!(
            "TimeService: Setting time_format_24h={}, timezone={}",
            new_settings.time_format_24h, new_settings.timezone
        ));

        // Write via VFS IPC
        let value = new_settings.to_json();
        self.start_vfs_write(
            TimeSettings::storage_path(),
            &value,
            PendingOp::WriteSettings {
                client_pid: msg.from_pid,
                settings: new_settings,
                cap_slots: msg.cap_slots.clone(),
            },
        ).map(|_| ())
    }

    // =========================================================================
    // VFS Response Handlers
    // =========================================================================

    /// Handle VFS read response (MSG_VFS_READ_RESPONSE)
    fn handle_vfs_read_response(&mut self, msg: &Message) -> Result<(), AppError> {
        syscall::debug("TimeService: Handling VFS read response");

        // Find and take a pending read operation
        let (request_id, pending_op) = match self.take_pending_by_type(OpType::Read) {
            Some((id, op)) => (id, op),
            None => {
                syscall::debug("TimeService: VFS read response but no pending read operation");
                return Ok(());
            }
        };

        syscall::debug(&format!(
            "TimeService: Matched VFS read response to req_id={}",
            request_id
        ));

        // Parse VFS response
        let result = async_client::parse_read_response(&msg.data);

        // Dispatch based on operation type
        match pending_op {
            PendingOp::GetSettings {
                client_pid,
                cap_slots,
            } => {
                let settings = match result {
                    Ok(data) => TimeSettings::from_json(&data).unwrap_or_default(),
                    Err(e) => {
                        syscall::debug(&format!("TimeService: VFS read failed: {}", e));
                        // Not found or error - return defaults
                        TimeSettings::default()
                    }
                };

                // Update cache
                self.settings = settings.clone();
                self.settings_loaded = true;

                self.send_settings_response(
                    client_pid,
                    &cap_slots,
                    &settings,
                    time_msg::MSG_GET_TIME_SETTINGS_RESPONSE,
                )
            }

            PendingOp::InitialLoad => {
                match result {
                    Ok(data) => {
                        if let Some(settings) = TimeSettings::from_json(&data) {
                            syscall::debug(&format!(
                                "TimeService: Loaded settings: time_format_24h={}, timezone={}",
                                settings.time_format_24h, settings.timezone
                            ));
                            self.settings = settings;
                        }
                    }
                    Err(_) => {
                        syscall::debug("TimeService: No stored settings found, using defaults");
                    }
                }
                self.settings_loaded = true;
                Ok(())
            }

            _ => {
                syscall::debug("TimeService: Unexpected pending operation for read response");
                Ok(())
            }
        }
    }

    /// Handle VFS write response (MSG_VFS_WRITE_RESPONSE)
    fn handle_vfs_write_response(&mut self, msg: &Message) -> Result<(), AppError> {
        syscall::debug("TimeService: Handling VFS write response");

        // Find and take a pending write operation
        let (request_id, pending_op) = match self.take_pending_by_type(OpType::Write) {
            Some((id, op)) => (id, op),
            None => {
                syscall::debug("TimeService: VFS write response but no pending write operation");
                return Ok(());
            }
        };

        syscall::debug(&format!(
            "TimeService: Matched VFS write response to req_id={}",
            request_id
        ));

        // Parse VFS response
        let result = async_client::parse_write_response(&msg.data);

        // Dispatch based on operation type
        match pending_op {
            PendingOp::WriteSettings {
                client_pid,
                settings,
                cap_slots,
            } => {
                match result {
                    Ok(()) => {
                        syscall::debug("TimeService: Settings written successfully");
                        // Update cache
                        self.settings = settings.clone();
                        self.settings_loaded = true;
                        self.send_settings_response(
                            client_pid,
                            &cap_slots,
                            &settings,
                            time_msg::MSG_SET_TIME_SETTINGS_RESPONSE,
                        )
                    }
                    Err(e) => {
                        syscall::debug(&format!("TimeService: VFS write failed: {}", e));
                        // Rule 9: Include operation context in error
                        self.send_error_response(
                            client_pid,
                            &cap_slots,
                            &format!("VFS write failed for {}: {}", TimeSettings::storage_path(), e),
                        )
                    }
                }
            }

            _ => {
                syscall::debug("TimeService: Unexpected pending operation for write response");
                Ok(())
            }
        }
    }

    // =========================================================================
    // Response helpers
    // =========================================================================

    /// Send time settings response
    fn send_settings_response(
        &self,
        to_pid: u32,
        cap_slots: &[u32],
        settings: &TimeSettings,
        response_tag: u32,
    ) -> Result<(), AppError> {
        let json = settings.to_json();

        // Try to send via transferred reply capability first
        if let Some(&reply_slot) = cap_slots.first() {
            syscall::debug(&format!(
                "TimeService: Sending settings response via reply cap slot {} (tag 0x{:x})",
                reply_slot, response_tag
            ));
            match syscall::send(reply_slot, response_tag, &json) {
                Ok(()) => {
                    syscall::debug("TimeService: Response sent via reply cap");
                    return Ok(());
                }
                Err(e) => {
                    syscall::debug(&format!(
                        "TimeService: Reply cap send failed ({}), falling back to debug channel",
                        e
                    ));
                }
            }
        }

        // Fallback: send via debug channel for supervisor to route
        let hex: String = json.iter().map(|b| format!("{:02x}", b)).collect();
        syscall::debug(&format!(
            "SERVICE:RESPONSE:{}:{:08x}:{}",
            to_pid, response_tag, hex
        ));
        Ok(())
    }

    /// Send error response
    fn send_error_response(
        &self,
        to_pid: u32,
        cap_slots: &[u32],
        error: &str,
    ) -> Result<(), AppError> {
        let json = format!(r#"{{"error":"{}"}}"#, error).into_bytes();

        // Try to send via transferred reply capability first
        if let Some(&reply_slot) = cap_slots.first() {
            if let Ok(()) =
                syscall::send(reply_slot, time_msg::MSG_SET_TIME_SETTINGS_RESPONSE, &json)
            {
                return Ok(());
            }
        }

        // Fallback: send via debug channel
        let hex: String = json.iter().map(|b| format!("{:02x}", b)).collect();
        syscall::debug(&format!(
            "SERVICE:RESPONSE:{}:{:08x}:{}",
            to_pid,
            time_msg::MSG_SET_TIME_SETTINGS_RESPONSE,
            hex
        ));
        Ok(())
    }
}

impl ZeroApp for TimeService {
    fn manifest() -> &'static zos_apps::AppManifest {
        &TIME_MANIFEST
    }

    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError> {
        syscall::debug(&format!("TimeService starting (PID {})", ctx.pid));

        // Register with init as "time" service
        let service_name = "time";
        let name_bytes = service_name.as_bytes();
        let mut data = Vec::with_capacity(1 + name_bytes.len() + 8);
        data.push(name_bytes.len() as u8);
        data.extend_from_slice(name_bytes);
        // Endpoint ID (placeholder)
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());

        // Send to init's endpoint
        let _ = syscall::send(
            syscall::INIT_ENDPOINT_SLOT,
            syscall::MSG_REGISTER_SERVICE,
            &data,
        );
        self.registered = true;

        syscall::debug("TimeService: Registered with init");

        // Load settings via VFS on startup (Invariant 31 compliant)
        let _ = self.start_vfs_read(TimeSettings::storage_path(), PendingOp::InitialLoad);

        Ok(())
    }

    fn update(&mut self, _ctx: &AppContext) -> ControlFlow {
        ControlFlow::Yield
    }

    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError> {
        syscall::debug(&format!(
            "TimeService: Received message tag 0x{:x} from PID {}",
            msg.tag, msg.from_pid
        ));

        match msg.tag {
            // VFS responses (Invariant 31 compliant - storage via VFS IPC)
            vfs_msg::MSG_VFS_READ_RESPONSE => self.handle_vfs_read_response(&msg),
            vfs_msg::MSG_VFS_WRITE_RESPONSE => self.handle_vfs_write_response(&msg),
            
            // Time service protocol
            time_msg::MSG_GET_TIME_SETTINGS => self.handle_get_time_settings(ctx, &msg),
            time_msg::MSG_SET_TIME_SETTINGS => self.handle_set_time_settings(ctx, &msg),
            
            _ => {
                syscall::debug(&format!(
                    "TimeService: Unknown message tag 0x{:x} from PID {}",
                    msg.tag, msg.from_pid
                ));
                Ok(())
            }
        }
    }

    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("TimeService: shutting down");
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // TimeSettings tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_time_settings_default() {
        let settings = TimeSettings::default();
        assert!(!settings.time_format_24h);
        assert_eq!(settings.timezone, "UTC");
    }

    #[test]
    fn test_time_settings_json_round_trip() {
        let settings = TimeSettings {
            time_format_24h: true,
            timezone: String::from("America/New_York"),
        };
        let json = settings.to_json();
        let parsed = TimeSettings::from_json(&json).expect("should parse");
        assert_eq!(parsed.time_format_24h, true);
        assert_eq!(parsed.timezone, "America/New_York");
    }

    #[test]
    fn test_time_settings_from_invalid_json() {
        let result = TimeSettings::from_json(b"not valid json");
        assert!(result.is_none());
    }

    #[test]
    fn test_time_settings_from_empty_json_uses_defaults() {
        let result = TimeSettings::from_json(b"{}").expect("should parse empty object");
        assert!(!result.time_format_24h); // default
        assert_eq!(result.timezone, "UTC"); // default
    }

    // -------------------------------------------------------------------------
    // Permission check tests (Rule 4: fail-closed)
    // -------------------------------------------------------------------------

    #[test]
    fn test_get_permission_allows_all() {
        let service = TimeService::default();
        // GET is open to all - non-sensitive read operation
        assert!(service.check_get_permission(0)); // supervisor
        assert!(service.check_get_permission(1)); // init
        assert!(service.check_get_permission(100)); // untrusted process
        assert!(service.check_get_permission(9999)); // any process
    }

    #[test]
    fn test_set_permission_trusted_pids() {
        let service = TimeService::default();
        // SET requires trusted PIDs
        for &pid in TRUSTED_PIDS_FOR_TIME_SETTINGS {
            assert!(
                service.check_set_permission(pid),
                "PID {} should be trusted for SET",
                pid
            );
        }
    }

    #[test]
    fn test_set_permission_denies_untrusted() {
        let service = TimeService::default();
        // Untrusted PIDs should be denied
        assert!(!service.check_set_permission(100));
        assert!(!service.check_set_permission(9999));
    }

    // -------------------------------------------------------------------------
    // DoS protection tests (Rule 11: pending-op limits)
    // -------------------------------------------------------------------------

    #[test]
    fn test_pending_limit_allows_under_max() {
        let service = TimeService::default();
        assert!(service.check_pending_limit());
    }

    #[test]
    fn test_pending_limit_denies_at_max() {
        let mut service = TimeService::default();
        // Fill up to the limit
        for i in 0..MAX_PENDING_OPS {
            service.pending_ops.insert(
                i as u32,
                (PendingOp::InitialLoad, OpType::Read),
            );
        }
        assert!(!service.check_pending_limit());
    }

    // -------------------------------------------------------------------------
    // Request ID allocation tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_request_id_allocation() {
        let mut service = TimeService::default();
        let id1 = service.alloc_request_id();
        let id2 = service.alloc_request_id();
        assert_ne!(id1, id2);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn test_request_id_skips_zero() {
        let mut service = TimeService::default();
        service.next_request_id = u32::MAX;
        let id = service.alloc_request_id();
        assert_eq!(id, u32::MAX);
        let next = service.alloc_request_id();
        assert_eq!(next, 1); // Skipped 0
    }
}
