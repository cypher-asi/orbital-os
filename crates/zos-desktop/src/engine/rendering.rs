//! Window rendering and screen coordinate calculations

use super::DesktopEngine;
use crate::math::Rect;
use crate::window::{WindowId, WindowState, WindowType};

/// Window with screen-space coordinates for rendering
#[derive(Clone, Debug)]
pub struct WindowScreenRect {
    pub id: WindowId,
    pub title: String,
    pub app_id: String,
    /// Associated process ID (if any)
    pub process_id: Option<u64>,
    pub state: WindowState,
    pub window_type: WindowType,
    pub focused: bool,
    pub screen_rect: Rect,
    /// Opacity for fade transitions (0.0 = invisible, 1.0 = fully visible)
    pub opacity: f32,
    /// Whether the window content area handles its own mouse events
    pub content_interactive: bool,
}

impl DesktopEngine {
    /// Get window screen rects for rendering
    pub fn get_window_screen_rects(&self, now_ms: f64) -> Vec<WindowScreenRect> {
        let workspace_index = self.get_visual_active_workspace_at(now_ms);
        let workspace = match self.desktops.desktops().get(workspace_index) {
            Some(ws) => ws,
            None => return Vec::new(),
        };

        let focused_id = self.windows.focused();
        let opacity = self.calculate_window_opacity(now_ms);

        self.windows
            .windows_by_z()
            .into_iter()
            .filter(|w| workspace.contains_window(w.id) && w.state != WindowState::Minimized)
            .map(|w| self.window_to_screen_rect(w, focused_id, opacity))
            .collect()
    }

    /// Convert a window to its screen rect representation
    fn window_to_screen_rect(
        &self,
        w: &crate::window::Window,
        focused_id: Option<WindowId>,
        opacity: f32,
    ) -> WindowScreenRect {
        let screen_pos = self.viewport.canvas_to_screen(w.position);
        let screen_size = w.size.scale(self.viewport.zoom);

        WindowScreenRect {
            id: w.id,
            title: w.title.clone(),
            app_id: w.app_id.clone(),
            process_id: w.process_id,
            state: w.state,
            window_type: w.window_type,
            focused: focused_id == Some(w.id),
            screen_rect: Rect::new(
                screen_pos.x,
                screen_pos.y,
                screen_size.width,
                screen_size.height,
            ),
            opacity,
            content_interactive: w.content_interactive,
        }
    }

    /// Calculate window opacity based on transition state
    fn calculate_window_opacity(&self, now_ms: f64) -> f32 {
        match &self.crossfade {
            Some(crossfade) => {
                // Use the crossfade's computed opacity for smooth transitions
                let (desktop_opacity, _void_opacity) = crossfade.opacities(now_ms);
                desktop_opacity
            }
            None => 1.0,
        }
    }
}
