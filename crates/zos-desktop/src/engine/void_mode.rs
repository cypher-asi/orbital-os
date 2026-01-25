//! Void mode transitions

use super::DesktopEngine;
use crate::desktop::VoidState;
use crate::math::{Camera, Rect};
use crate::transition::Crossfade;
use crate::view_mode::ViewMode;

impl DesktopEngine {
    /// Enter the void (zoom out to see all desktops)
    pub fn enter_void(&mut self, now_ms: f64) {
        if !self.can_enter_void() {
            return;
        }

        let from_desktop = match self.view_mode {
            ViewMode::Desktop { index } => index,
            _ => return,
        };

        // Save current desktop camera state
        self.desktops
            .save_desktop_camera(from_desktop, self.viewport.center, self.viewport.zoom);

        // Calculate void camera to show all desktops
        let bounds: Vec<Rect> = self.desktops.desktops().iter().map(|d| d.bounds).collect();
        let center = VoidState::calculate_void_center(&bounds);
        let zoom = VoidState::calculate_fit_zoom(&bounds, self.viewport.screen_size);
        self.void_state.set_camera(Camera::at(center, zoom));

        // Start crossfade to void
        self.crossfade = Some(Crossfade::to_void(now_ms, from_desktop));
        self.last_activity_ms = now_ms;
    }

    /// Check if we can enter void mode
    fn can_enter_void(&self) -> bool {
        !self.input.is_dragging() && self.view_mode.is_desktop() && !self.is_crossfading()
    }

    /// Exit the void into a specific desktop
    pub fn exit_void(&mut self, desktop_index: usize, now_ms: f64) {
        if !self.can_exit_void() {
            return;
        }

        // Switch to target desktop
        self.desktops.switch_to(desktop_index);

        // Start crossfade to desktop
        self.crossfade = Some(Crossfade::to_desktop(now_ms, desktop_index));
        self.last_activity_ms = now_ms;
    }

    /// Check if we can exit void mode
    fn can_exit_void(&self) -> bool {
        !self.input.is_dragging() && self.view_mode.is_void() && !self.is_crossfading()
    }
}
