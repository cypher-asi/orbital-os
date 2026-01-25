//! Time Service (PID 5)
//!
//! The TimeService manages time-related settings. It:
//! - Stores user time format preferences (12h/24h)
//! - Stores user timezone preferences
//! - Persists settings via VFS service IPC (async pattern)
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

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use zos_apps::manifest::TIME_SERVICE_MANIFEST;
use zos_apps::syscall;
use zos_apps::vfs_async;
use zos_apps::{app_main, AppContext, AppError, ControlFlow, Message, ZeroApp};
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

/// Time settings that can be persisted
#[derive(Clone, Debug, Default)]
pub struct TimeSettings {
    /// Use 24-hour time format (false = 12-hour with AM/PM)
    pub time_format_24h: bool,
    /// Timezone identifier (e.g., "America/New_York", "UTC")
    pub timezone: String,
}

impl TimeSettings {
    /// Storage path for time settings
    pub fn storage_path() -> &'static str {
        "/system/settings/time.json"
    }

    /// Serialize to JSON bytes
    pub fn to_json(&self) -> Vec<u8> {
        format!(
            r#"{{"time_format_24h":{},"timezone":"{}"}}"#,
            self.time_format_24h, self.timezone
        )
        .into_bytes()
    }

    /// Parse from JSON bytes
    pub fn from_json(data: &[u8]) -> Option<Self> {
        let json_str = core::str::from_utf8(data).ok()?;

        // Simple JSON parsing (production would use serde)
        let time_format_24h = json_str.contains(r#""time_format_24h":true"#);

        // Extract timezone
        let timezone = if let Some(start) = json_str.find(r#""timezone":""#) {
            let rest = &json_str[start + 12..];
            if let Some(end) = rest.find('"') {
                String::from(&rest[..end])
            } else {
                String::from("UTC")
            }
        } else {
            String::from("UTC")
        };

        Some(Self {
            time_format_24h,
            timezone,
        })
    }
}

// =============================================================================
// Pending VFS Operations
// =============================================================================

/// Tracks pending VFS operations awaiting responses.
///
/// Note: VFS IPC doesn't use request IDs like storage syscalls. For the Time Service's
/// simple case (at most one pending read or write), we track by operation type.
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

// =============================================================================
// TimeService Application
// =============================================================================

/// TimeService - manages time display settings
pub struct TimeService {
    /// Whether we have registered with init
    registered: bool,
    /// Current time settings (cached in memory)
    settings: TimeSettings,
    /// Pending VFS operation (at most one at a time for this simple service)
    pending_op: Option<PendingOp>,
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
            pending_op: None,
            settings_loaded: false,
        }
    }
}

impl TimeService {
    // =========================================================================
    // VFS IPC helpers (async, non-blocking) - Invariant 31 compliant
    // =========================================================================

    /// Start async VFS read and track the pending operation.
    /// Uses VFS IPC instead of direct storage syscalls per Invariant 31.
    fn start_vfs_read(&mut self, path: &str, pending_op: PendingOp) -> Result<(), AppError> {
        syscall::debug(&format!("TimeService: sending VFS read request for {}", path));
        vfs_async::send_read_request(path)?;
        self.pending_op = Some(pending_op);
        Ok(())
    }

    /// Start async VFS write and track the pending operation.
    /// Uses VFS IPC instead of direct storage syscalls per Invariant 31.
    fn start_vfs_write(
        &mut self,
        path: &str,
        value: &[u8],
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        syscall::debug(&format!(
            "TimeService: sending VFS write request for {} ({} bytes)",
            path,
            value.len()
        ));
        vfs_async::send_write_request(path, value)?;
        self.pending_op = Some(pending_op);
        Ok(())
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
        syscall::debug("TimeService: Handling get time settings request");

        // If settings are loaded, return immediately from cache
        if self.settings_loaded {
            return self.send_settings_response(
                msg.from_pid,
                &msg.cap_slots,
                &self.settings,
                time_msg::MSG_GET_TIME_SETTINGS_RESPONSE,
            );
        }

        // Otherwise, start async VFS read
        self.start_vfs_read(
            TimeSettings::storage_path(),
            PendingOp::GetSettings {
                client_pid: msg.from_pid,
                cap_slots: msg.cap_slots.clone(),
            },
        )
    }

    /// Handle MSG_SET_TIME_SETTINGS
    fn handle_set_time_settings(
        &mut self,
        _ctx: &AppContext,
        msg: &Message,
    ) -> Result<(), AppError> {
        syscall::debug("TimeService: Handling set time settings request");

        // Parse the settings from the request
        let new_settings = match TimeSettings::from_json(&msg.data) {
            Some(s) => s,
            None => {
                syscall::debug("TimeService: Failed to parse settings from request");
                // Send error response
                return self.send_error_response(
                    msg.from_pid,
                    &msg.cap_slots,
                    "Invalid settings format",
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
        )
    }

    // =========================================================================
    // VFS Response Handlers
    // =========================================================================

    /// Handle VFS read response (MSG_VFS_READ_RESPONSE)
    fn handle_vfs_read_response(&mut self, msg: &Message) -> Result<(), AppError> {
        syscall::debug("TimeService: Handling VFS read response");

        // Take the pending operation
        let pending_op = match self.pending_op.take() {
            Some(op) => op,
            None => {
                syscall::debug("TimeService: VFS read response but no pending operation");
                return Ok(());
            }
        };

        // Parse VFS response
        let result = vfs_async::parse_read_response(&msg.data);

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

        // Take the pending operation
        let pending_op = match self.pending_op.take() {
            Some(op) => op,
            None => {
                syscall::debug("TimeService: VFS write response but no pending operation");
                return Ok(());
            }
        };

        // Parse VFS response
        let result = vfs_async::parse_write_response(&msg.data);

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
                        self.send_error_response(client_pid, &cap_slots, "Write failed")
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
        &TIME_SERVICE_MANIFEST
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

// Entry point
app_main!(TimeService);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("TimeService is meant to run as WASM in Zero OS");
}
