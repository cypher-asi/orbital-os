//! Clock Application
//!
//! Displays current time and date. Demonstrates:
//! - Time syscalls (SYS_GET_WALLCLOCK)
//! - Periodic updates
//! - One-way IPC (state output only)

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use zos_apps::app_protocol::{tags, ClockState};
use zos_apps::manifest::CLOCK_MANIFEST;
use zos_apps::syscall;
use zos_apps::{app_main, AppContext, AppError, AppManifest, ControlFlow, Message, ZeroApp};

/// Clock application state
#[derive(Default)]
pub struct ClockApp {
    /// Last time we sent an update (nanoseconds)
    last_update_ns: u64,

    /// Cached formatted time
    cached_time: String,

    /// Cached formatted date
    cached_date: String,

    /// Update interval (1 second in nanos)
    update_interval: u64,
}

impl ClockApp {
    const UPDATE_INTERVAL_NS: u64 = 1_000_000_000; // 1 second

    /// Format wall-clock time into time and date strings
    fn format_time(wallclock_ms: u64) -> (String, String) {
        // Convert milliseconds since epoch to components
        let total_seconds = wallclock_ms / 1000;
        let seconds = (total_seconds % 60) as u8;
        let minutes = ((total_seconds / 60) % 60) as u8;
        let hours = ((total_seconds / 3600) % 24) as u8;

        // Format time as "HH:MM:SS"
        let time = format!("{:02}:{:02}:{:02}", hours, minutes, seconds);

        // Calculate date (simplified - days since epoch)
        let days_since_epoch = total_seconds / 86400;

        // Simple day-of-week calculation (Jan 1, 1970 was Thursday)
        let day_names = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
        let day_of_week = day_names[(days_since_epoch % 7) as usize];

        // Format date (simplified version)
        let date = format!("{}, Day {}", day_of_week, days_since_epoch);

        (time, date)
    }

    /// Send current state to UI
    fn send_state(&self, ctx: &AppContext) -> Result<(), AppError> {
        let state = ClockState::new(
            self.cached_time.clone(),
            self.cached_date.clone(),
            true,                // is_24_hour
            String::from("UTC"), // timezone
        );

        let bytes = state.to_bytes();

        if let Some(slot) = ctx.ui_endpoint {
            syscall::send(slot, tags::MSG_APP_STATE, &bytes)
                .map_err(|e| AppError::IpcError(format!("Send failed: {}", e)))?;
        }

        Ok(())
    }
}

impl ZeroApp for ClockApp {
    fn manifest() -> &'static AppManifest {
        &CLOCK_MANIFEST
    }

    fn init(&mut self, _ctx: &AppContext) -> Result<(), AppError> {
        self.update_interval = Self::UPDATE_INTERVAL_NS;
        self.cached_time = String::from("00:00:00");
        self.cached_date = String::from("Loading...");
        Ok(())
    }

    fn update(&mut self, ctx: &AppContext) -> ControlFlow {
        // Check if it's time to update
        if ctx.uptime_ns - self.last_update_ns >= self.update_interval {
            self.last_update_ns = ctx.uptime_ns;

            // Format current time
            let (time, date) = Self::format_time(ctx.wallclock_ms);
            self.cached_time = time;
            self.cached_date = date;

            // Send state to UI
            if let Err(e) = self.send_state(ctx) {
                syscall::debug(&format!("Clock: failed to send state: {}", e));
            }
        }

        ControlFlow::Yield
    }

    fn on_message(&mut self, _ctx: &AppContext, _msg: Message) -> Result<(), AppError> {
        // Clock doesn't process input messages
        Ok(())
    }

    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("Clock: shutting down");
    }
}

// Entry point
app_main!(ClockApp);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("Clock app is meant to run as WASM in Zero OS");
}
