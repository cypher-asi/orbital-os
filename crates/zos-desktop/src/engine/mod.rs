//! Desktop engine coordinating all components
//!
//! The `DesktopEngine` struct has its `impl` blocks split across focused submodules
//! for maintainability. Each file contains related method groups:
//!
//! ## Method Organization
//!
//! | File                | Methods                                                    |
//! |---------------------|-----------------------------------------------------------|
//! | `mod.rs`            | Core: `new`, `init`, `resize`, `pan`, `zoom_at`, `active_camera`, accessors |
//! | `windows.rs`        | Window lifecycle: `create_window`, `close_window`, `focus_window`, `move_window`, `resize_window`, `launch_app` |
//! | `pointer_events.rs` | Input handling: `handle_pointer_down`, `handle_pointer_move`, `handle_pointer_up`, `handle_wheel` |
//! | `void_mode.rs`      | Void transitions: `enter_void`, `exit_void`               |
//! | `transitions.rs`    | Animation ticking: `tick_transition`, `layer_opacities`, `is_crossfading`, `is_animating` |
//! | `animation.rs`      | Camera animation: `pan_to_window`                         |
//! | `rendering.rs`      | Screen calculations: `get_window_screen_rects`            |
//!
//! ## Invariants
//!
//! - `view_mode` accurately reflects whether we're viewing a desktop or the void
//! - During crossfades, `view_mode` represents the *destination* state
//! - Only one crossfade or camera animation can be active at a time
//! - The active desktop index is always valid
//!
//! ## State Transitions
//!
//! ```text
//! Desktop(n) --enter_void--> [CrossfadeToVoid] --> Void
//! Void --exit_void(n)--> [CrossfadeToDesktop] --> Desktop(n)
//! Desktop(n) --switch_desktop(m)--> [CrossfadeSwitchDesktop] --> Desktop(m)
//! ```
//!
//! ## Failure Modes
//!
//! - Transitions are blocked during drag operations
//! - Void/desktop transitions are blocked during active crossfades
//! - Desktop switches can interrupt other desktop switches (for responsiveness)

mod animation;
mod pointer_events;
mod rendering;
mod transitions;
mod void_mode;
mod windows;

use crate::desktop::{DesktopManager, VoidState};
use crate::input::InputRouter;
use crate::math::{Camera, Rect, Size};
use crate::transition::{CameraAnimation, Crossfade};
use crate::desktop::ViewMode;
use crate::viewport::Viewport;
use crate::window::{WindowId, WindowManager, WindowState};
use std::collections::HashMap;

pub use rendering::WindowScreenRect;

/// Desktop engine coordinating all desktop components
///
/// This is the main entry point for desktop operations, managing:
/// - View mode (desktop or void)
/// - Layer cameras (each desktop has a camera, void has its own)
/// - Window manager (window CRUD, focus, z-order)
/// - Desktop manager (separate infinite canvases)
/// - Input router (drag/resize state machine)
/// - Crossfade transitions (opacity animations between layers)
///
/// ## Design
///
/// Fields are `pub(crate)` to maintain invariants through methods.
/// External code should use the provided accessor and mutator methods.
pub struct DesktopEngine {
    /// Current view mode (desktop or void)
    pub(crate) view_mode: ViewMode,
    /// Void layer state
    pub(crate) void_state: VoidState,
    /// Legacy viewport (for backward compatibility)
    pub(crate) viewport: Viewport,
    /// Window manager
    pub(crate) windows: WindowManager,
    /// Desktop manager
    pub(crate) desktops: DesktopManager,
    /// Input router
    pub(crate) input: InputRouter,
    /// Current crossfade transition
    pub(crate) crossfade: Option<Crossfade>,
    /// Camera animation
    pub(crate) camera_animation: Option<CameraAnimation>,
    /// Last viewport activity time (ms) for animation detection
    pub(crate) last_activity_ms: f64,
    /// Per-window camera memory (remembers camera position for each window)
    pub(crate) window_cameras: HashMap<WindowId, Camera>,
}

impl Default for DesktopEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopEngine {
    /// Create a new desktop engine
    pub fn new() -> Self {
        Self {
            view_mode: ViewMode::default(),
            void_state: VoidState::default(),
            viewport: Viewport::default(),
            windows: WindowManager::new(),
            desktops: DesktopManager::new(),
            input: InputRouter::new(),
            crossfade: None,
            camera_animation: None,
            last_activity_ms: 0.0,
            window_cameras: HashMap::new(),
        }
    }

    /// Initialize the desktop with screen dimensions
    pub fn init(&mut self, width: f32, height: f32) {
        let screen_size = Size::new(width, height);
        self.viewport.screen_size = screen_size;
        self.void_state.set_screen_size(screen_size);
        self.desktops
            .set_desktop_size(Size::new(width.max(1920.0), height.max(1080.0)));

        // Only create first desktop if none exists yet (idempotent for React StrictMode)
        if self.desktops.desktops().is_empty() {
            let desktop_id = self.desktops.create("Main");
            if let Some(desktop) = self.desktops.get(desktop_id) {
                self.viewport.center = desktop.bounds.center();
            }
        }
    }

    /// Resize the viewport
    pub fn resize(&mut self, width: f32, height: f32) {
        let screen_size = Size::new(width, height);
        self.viewport.screen_size = screen_size;
        self.void_state.set_screen_size(screen_size);

        let min_width = width.max(1920.0);
        let min_height = height.max(1080.0);
        self.desktops
            .set_desktop_size(Size::new(min_width, min_height));
    }

    /// Pan the viewport
    pub fn pan(&mut self, dx: f32, dy: f32) {
        if self.is_crossfading() {
            return;
        }

        match &self.view_mode {
            ViewMode::Desktop { .. } => {
                self.viewport.pan(dx, dy);
                self.commit_viewport_to_desktop();
            }
            ViewMode::Void => {
                self.viewport.pan(dx, dy);
                let bounds: Vec<Rect> = self.desktops.desktops().iter().map(|d| d.bounds).collect();
                self.void_state.constrain_to_desktops(&bounds, 200.0);
            }
        }
    }

    /// Zoom at anchor point
    pub fn zoom_at(&mut self, factor: f32, anchor_x: f32, anchor_y: f32) {
        if self.is_crossfading() {
            return;
        }

        match &self.view_mode {
            ViewMode::Desktop { .. } => {
                self.viewport.zoom_at(factor, anchor_x, anchor_y);
                if self.viewport.zoom < 0.001 {
                    self.viewport.zoom = 0.001;
                }
                self.commit_viewport_to_desktop();
            }
            ViewMode::Void => {
                self.viewport
                    .zoom_at_clamped(factor, anchor_x, anchor_y, 0.1, 1.0);
            }
        }
    }

    /// Commit viewport state to active desktop
    pub(crate) fn commit_viewport_to_desktop(&mut self) {
        if let ViewMode::Desktop { index } = self.view_mode {
            self.desktops
                .save_desktop_camera(index, self.viewport.center, self.viewport.zoom);
        }
    }

    /// Get active camera
    pub fn active_camera(&self) -> Camera {
        match self.view_mode {
            ViewMode::Desktop { index } => self
                .desktops
                .desktops()
                .get(index)
                .map(|d| d.camera())
                .unwrap_or_default(),
            ViewMode::Void => *self.void_state.camera(),
        }
    }

    // =========================================================================
    // Accessor methods for internal state
    // =========================================================================

    /// Get a reference to the viewport
    #[inline]
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// Get a mutable reference to the viewport
    #[inline]
    pub fn viewport_mut(&mut self) -> &mut Viewport {
        &mut self.viewport
    }

    /// Get a reference to the window manager
    #[inline]
    pub fn windows(&self) -> &WindowManager {
        &self.windows
    }

    /// Get a mutable reference to the window manager
    #[inline]
    pub fn windows_mut(&mut self) -> &mut WindowManager {
        &mut self.windows
    }

    /// Get a reference to the desktop manager
    #[inline]
    pub fn desktops(&self) -> &DesktopManager {
        &self.desktops
    }

    /// Get a reference to the void state
    #[inline]
    pub fn void_state(&self) -> &VoidState {
        &self.void_state
    }

    /// Get a reference to the input router
    #[inline]
    pub fn input(&self) -> &InputRouter {
        &self.input
    }

    /// Focus top window on a desktop
    pub(crate) fn focus_top_window_on_desktop(&mut self, desktop_index: usize) {
        let desktop = match self.desktops.desktops().get(desktop_index) {
            Some(d) => d,
            None => return,
        };

        let top_window = self
            .windows
            .windows_by_z()
            .into_iter()
            .rfind(|w| desktop.contains_window(w.id) && w.state != WindowState::Minimized);

        if let Some(window) = top_window {
            self.windows.focus(window.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec2;
    use crate::window::WindowConfig;

    #[test]
    fn test_desktop_engine_init() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        assert!((engine.viewport.screen_size.width - 1920.0).abs() < 0.001);
        assert_eq!(engine.desktops.desktops().len(), 1);
    }

    #[test]
    fn test_desktop_engine_window_creation() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        let id = engine.create_window(WindowConfig {
            title: "Test Window".to_string(),
            position: Some(Vec2::new(100.0, 100.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        assert!(engine.windows.get(id).is_some());
        assert_eq!(engine.desktops.active_desktop().windows.len(), 1);
    }

    #[test]
    fn test_desktop_engine_create_desktop() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        engine.create_desktop("Second");
        engine.create_desktop("Third");

        assert_eq!(engine.desktops.desktops().len(), 3);
    }

    #[test]
    fn test_viewport_pan() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        engine.pan(-100.0, 0.0);

        assert!((engine.viewport.center.x - 100.0).abs() < 1.0);
    }
}
