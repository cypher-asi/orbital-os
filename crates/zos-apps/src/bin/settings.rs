//! Settings Application
//!
//! System settings management app. Demonstrates:
//! - Split-pane navigation layout
//! - Drill-down panels with breadcrumbs
//! - Theme and preference management

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use zos_apps::app_protocol::{tags, InputEvent, SettingsState};
use zos_apps::manifest::SETTINGS_MANIFEST;
use zos_apps::syscall;
use zos_apps::{app_main, AppContext, AppError, AppManifest, ControlFlow, Message, ZeroApp};

/// Settings application state
#[derive(Default)]
pub struct SettingsApp {
    /// Current settings state
    state: SettingsState,
}

impl SettingsApp {
    fn handle_input(&mut self, input: &str) {
        // Parse input command
        if let Some(area) = input.strip_prefix("select_area:") {
            self.handle_select_area(area);
        } else if let Some(item) = input.strip_prefix("drill:") {
            self.handle_drill(item);
        } else if input == "back" {
            self.handle_back();
        } else if let Some(setting) = input.strip_prefix("set:") {
            self.handle_set(setting);
        }
    }

    fn handle_select_area(&mut self, area: &str) {
        self.state.active_area = match area {
            "general" => 0,
            "identity" => 1,
            "permissions" => 2,
            "theme" => 3,
            _ => return,
        };
        // Clear drill-down when switching areas
        self.state.active_item.clear();
    }

    fn handle_drill(&mut self, item: &str) {
        self.state.active_item = item.to_string();
    }

    fn handle_back(&mut self) {
        self.state.active_item.clear();
    }

    fn handle_set(&mut self, setting: &str) {
        // Parse "key:value" format
        if let Some((key, value)) = setting.split_once(':') {
            match key {
                // General settings
                "time_format_24h" => {
                    self.state.time_format_24h = value == "true";
                }
                "timezone" => {
                    self.state.timezone = value.to_string();
                }

                // Theme settings
                "theme" => {
                    self.state.theme = value.to_string();
                }
                "accent" => {
                    self.state.accent = value.to_string();
                }
                "background" => {
                    self.state.background = value.to_string();
                }

                _ => {
                    syscall::debug(&format!("Settings: unknown setting key: {}", key));
                }
            }
        }
    }

    fn send_state(&self, ctx: &AppContext) -> Result<(), AppError> {
        let bytes = self.state.to_bytes();

        if let Some(slot) = ctx.ui_endpoint {
            syscall::send(slot, tags::MSG_APP_STATE, &bytes)
                .map_err(|e| AppError::IpcError(format!("Send failed: {}", e)))?;
        }

        Ok(())
    }
}

impl ZeroApp for SettingsApp {
    fn manifest() -> &'static AppManifest {
        &SETTINGS_MANIFEST
    }

    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError> {
        // Initialize with defaults
        self.state = SettingsState::initial();

        // TODO: Load persisted settings from storage capability
        // For now, use sensible defaults
        self.state.theme = String::from("dark");
        self.state.accent = String::from("cyan");
        self.state.background = String::from("grain");
        self.state.timezone = String::from("UTC");

        // TODO: Query identity service for actual counts
        self.state.has_neural_key = false;
        self.state.machine_key_count = 0;
        self.state.linked_account_count = 0;

        // TODO: Query process manager for actual counts
        self.state.running_process_count = 0;
        self.state.total_capability_count = 0;

        self.send_state(ctx)
    }

    fn update(&mut self, _ctx: &AppContext) -> ControlFlow {
        ControlFlow::Yield
    }

    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError> {
        if msg.tag == tags::MSG_APP_INPUT {
            // Decode input event
            let event = InputEvent::from_bytes(&msg.data)?;

            // Handle text input (navigation commands)
            if let Some(text) = event.text_content() {
                self.handle_input(text);
                self.send_state(ctx)?;
            }
            // Handle button press (alternative input)
            else if let Some(name) = event.button_name() {
                self.handle_input(name);
                self.send_state(ctx)?;
            }
        }

        Ok(())
    }

    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("Settings: shutting down");
    }
}

// Entry point
app_main!(SettingsApp);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("Settings app is meant to run as WASM in Zero OS");
}
