//! Time Service (PID 5)
//!
//! The TimeService manages time-related settings. It:
//! - Stores user time format preferences (12h/24h)
//! - Stores user timezone preferences
//! - Persists settings to VFS (via async storage syscalls)
//!
//! # Protocol
//!
//! Apps communicate with TimeService via IPC:
//!
//! - `MSG_GET_TIME_SETTINGS (0x8001)`: Get current time settings
//! - `MSG_SET_TIME_SETTINGS (0x8002)`: Update time settings
//!
//! # Storage Access
//!
//! This service uses async storage syscalls (routed through supervisor to IndexedDB)
//! instead of blocking VfsClient to avoid IPC deadlock.

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use zos_apps::manifest::TIME_SERVICE_MANIFEST;
use zos_apps::syscall;
use zos_apps::{app_main, AppContext, AppError, ControlFlow, Message, ZeroApp};
use zos_process::{storage_result, MSG_STORAGE_RESULT};

// =============================================================================
// IPC Message Tags
// =============================================================================

/// Message tags for time service
pub mod time_msg {
    /// Request current time settings
    pub const MSG_GET_TIME_SETTINGS: u32 = 0x8001;
    /// Response with time settings
    pub const MSG_GET_TIME_SETTINGS_RESPONSE: u32 = 0x8002;
    /// Set time settings
    pub const MSG_SET_TIME_SETTINGS: u32 = 0x8003;
    /// Response confirming settings update
    pub const MSG_SET_TIME_SETTINGS_RESPONSE: u32 = 0x8004;
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
// Pending Storage Operations
// =============================================================================

/// Tracks pending storage operations awaiting results
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
    /// Pending storage operations: request_id -> operation context
    pending_ops: BTreeMap<u32, PendingOp>,
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
            settings_loaded: false,
        }
    }
}

impl TimeService {
    // =========================================================================
    // Storage syscall helpers (async, non-blocking)
    // =========================================================================

    /// Start async storage read and track the pending operation
    fn start_storage_read(&mut self, key: &str, pending_op: PendingOp) -> Result<(), AppError> {
        match syscall::storage_read_async(key) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "TimeService: storage_read_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("TimeService: storage_read_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage read failed: {}", e)))
            }
        }
    }

    /// Start async storage write and track the pending operation
    fn start_storage_write(
        &mut self,
        key: &str,
        value: &[u8],
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        match syscall::storage_write_async(key, value) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "TimeService: storage_write_async({}, {} bytes) -> request_id={}",
                    key,
                    value.len(),
                    request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("TimeService: storage_write_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage write failed: {}", e)))
            }
        }
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

        // Otherwise, start async read
        let key = format!("content:{}", TimeSettings::storage_path());
        self.start_storage_read(
            &key,
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
                return self.send_error_response(msg.from_pid, &msg.cap_slots, "Invalid settings format");
            }
        };

        syscall::debug(&format!(
            "TimeService: Setting time_format_24h={}, timezone={}",
            new_settings.time_format_24h, new_settings.timezone
        ));

        // Write to storage
        let key = format!("content:{}", TimeSettings::storage_path());
        let value = new_settings.to_json();
        self.start_storage_write(
            &key,
            &value,
            PendingOp::WriteSettings {
                client_pid: msg.from_pid,
                settings: new_settings,
                cap_slots: msg.cap_slots.clone(),
            },
        )
    }

    // =========================================================================
    // Storage result handler
    // =========================================================================

    /// Handle MSG_STORAGE_RESULT - async storage operation completed
    fn handle_storage_result(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        // Parse storage result
        // Format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
        if msg.data.len() < 9 {
            syscall::debug("TimeService: storage result too short");
            return Ok(());
        }

        let request_id = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);
        let result_type = msg.data[4];
        let data_len =
            u32::from_le_bytes([msg.data[5], msg.data[6], msg.data[7], msg.data[8]]) as usize;
        let data = if data_len > 0 && msg.data.len() >= 9 + data_len {
            &msg.data[9..9 + data_len]
        } else {
            &[]
        };

        syscall::debug(&format!(
            "TimeService: storage result request_id={}, type={}, data_len={}",
            request_id, result_type, data_len
        ));

        // Look up pending operation
        let pending_op = match self.pending_ops.remove(&request_id) {
            Some(op) => op,
            None => {
                syscall::debug(&format!("TimeService: unknown request_id {}", request_id));
                return Ok(());
            }
        };

        // Dispatch based on operation type
        match pending_op {
            PendingOp::GetSettings {
                client_pid,
                cap_slots,
            } => {
                let settings = if result_type == storage_result::READ_OK {
                    TimeSettings::from_json(data).unwrap_or_default()
                } else {
                    // Not found or error - return defaults
                    TimeSettings::default()
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

            PendingOp::WriteSettings {
                client_pid,
                settings,
                cap_slots,
            } => {
                if result_type == storage_result::WRITE_OK {
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
                } else {
                    syscall::debug("TimeService: Settings write failed");
                    self.send_error_response(client_pid, &cap_slots, "Write failed")
                }
            }

            PendingOp::InitialLoad => {
                if result_type == storage_result::READ_OK {
                    if let Some(settings) = TimeSettings::from_json(data) {
                        syscall::debug(&format!(
                            "TimeService: Loaded settings: time_format_24h={}, timezone={}",
                            settings.time_format_24h, settings.timezone
                        ));
                        self.settings = settings;
                    }
                } else {
                    syscall::debug("TimeService: No stored settings found, using defaults");
                }
                self.settings_loaded = true;
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
            match syscall::send(reply_slot, time_msg::MSG_SET_TIME_SETTINGS_RESPONSE, &json) {
                Ok(()) => return Ok(()),
                Err(_) => {}
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
        let _ = syscall::send(syscall::INIT_ENDPOINT_SLOT, syscall::MSG_REGISTER_SERVICE, &data);
        self.registered = true;

        syscall::debug("TimeService: Registered with init");

        // Load settings from storage on startup
        let key = format!("content:{}", TimeSettings::storage_path());
        let _ = self.start_storage_read(&key, PendingOp::InitialLoad);

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
            MSG_STORAGE_RESULT => self.handle_storage_result(ctx, &msg),
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
