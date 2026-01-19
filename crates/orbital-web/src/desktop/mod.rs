//! Desktop Environment for Orbital OS
//!
//! This module implements the desktop environment with:
//! - Infinite canvas viewport with pan/zoom
//! - Window management with z-order and focus
//! - Multiple desktops (isolated infinite canvases)
//! - Input routing for window interactions
//! - Animated desktop transitions with crossfade effect
//!
//! ## Architecture
//!
//! The desktop engine runs in Rust/WASM and manages all window state.
//! React handles only window content rendering as positioned overlays.
//! Window state is ephemeral (not logged to Axiom).
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    DesktopEngine                        │
//! │  ┌──────────┐ ┌──────────────┐ ┌───────────────────┐   │
//! │  │ Viewport │ │WindowManager │ │  DesktopManager   │   │
//! │  │ (state)  │ │   (CRUD)     │ │   (canvases)      │   │
//! │  └──────────┘ └──────────────┘ └───────────────────┘   │
//! │  ┌─────────────┐ ┌────────────────────────────────┐    │
//! │  │ InputRouter │ │     Crossfade / CameraAnimation│    │
//! │  │  (drag)     │ │  (desktop switch animations)   │    │
//! │  └─────────────┘ └────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Components
//!
//! - [`DesktopEngine`]: Main engine coordinating all components
//! - [`Viewport`]: Simple state holder for position and zoom
//! - [`WindowManager`]: CRUD operations for windows, z-order, focus stack
//! - [`DesktopManager`]: Manages multiple desktops (infinite canvases)
//! - [`InputRouter`]: Pan/zoom, window drag/resize, event forwarding
//! - [`Crossfade`]: Opacity-based crossfade transitions between layers
//! - [`CameraAnimation`]: Smooth camera pan/zoom animations
//!
//! ## Desktop Transitions
//!
//! When switching desktops, transitions use opacity crossfade between layers:
//! - Both desktop and void layers render simultaneously during transitions
//! - Smooth visual effect without complex zoom/pan animations
//!
//! Call [`DesktopEngine::tick_transition`] each frame to update animations.

mod input;
mod transition;
mod types;
mod windows;
pub mod desktops;

pub use input::{DragState, InputResult, InputRouter};
pub use transition::{
    CameraAnimation, Crossfade, CrossfadeDirection, CAMERA_ANIMATION_DURATION_MS,
    CROSSFADE_DURATION_MS,
};
pub use types::{Camera, Rect, Size, Vec2, FRAME_STYLE};
pub use windows::{Window, WindowConfig, WindowId, WindowManager, WindowRegion, WindowState};
pub use desktops::{
    Desktop, DesktopId, DesktopManager, PersistedDesktop, VoidState,
};

// Backward compatibility re-exports (deprecated)
#[allow(deprecated)]
pub use desktops::{
    PersistedWorkspace, Workspace, WorkspaceId, WorkspaceManager, WorkspaceViewport,
};

// =============================================================================
// View Mode - Controls what the user is viewing
// =============================================================================

/// The current viewing mode of the desktop
///
/// The desktop can be in one of two states:
/// - **Desktop**: Viewing a single desktop with infinite zoom/pan capability
/// - **Void**: Zoomed out to see all desktops (the meta-layer)
///
/// Transitions between modes are handled separately via opacity crossfade.
/// Both layers render simultaneously during transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// Viewing a single desktop - infinite zoom/pan within it
    Desktop {
        /// Index of the desktop being viewed
        index: usize,
    },
    /// In the Void - can see all desktops as tiles
    Void,
}

impl Default for ViewMode {
    fn default() -> Self {
        ViewMode::Desktop { index: 0 }
    }
}

impl ViewMode {
    /// Check if currently in a desktop view
    pub fn is_desktop(&self) -> bool {
        matches!(self, ViewMode::Desktop { .. })
    }

    /// Check if currently in a workspace view (alias for is_desktop for compatibility)
    pub fn is_workspace(&self) -> bool {
        self.is_desktop()
    }

    /// Check if currently in the void view
    pub fn is_void(&self) -> bool {
        matches!(self, ViewMode::Void)
    }

    /// Get the desktop index if in desktop mode
    pub fn desktop_index(&self) -> Option<usize> {
        match self {
            ViewMode::Desktop { index } => Some(*index),
            ViewMode::Void => None,
        }
    }

    /// Get the workspace index (alias for desktop_index for compatibility)
    pub fn workspace_index(&self) -> Option<usize> {
        self.desktop_index()
    }
}

/// Desktop engine coordinating all desktop components
///
/// This is the main entry point for desktop operations, managing:
/// - View mode (desktop or void)
/// - Layer cameras (each desktop has a camera, void has its own camera)
/// - Window manager (window CRUD, focus, z-order)
/// - Desktop manager (separate infinite canvases)
/// - Input router (drag/resize state machine)
/// - Crossfade transitions (simple opacity animations between layers)
///
/// ## Conceptual Model
///
/// The desktop has two independently-rendered layers:
///
/// - **Desktop Layer**: The current desktop with its windows (infinite zoom/pan)
/// - **Void Layer**: The meta-layer showing all desktops as tiles
///
/// Each layer has its own camera. Transitions between layers use simple
/// opacity crossfade - both layers render simultaneously during transitions.
///
/// ## Cameras
///
/// - Each desktop has its own camera (center, zoom) stored in the Desktop struct
/// - The void has its own camera managed by VoidState
/// - The current "active" camera depends on view_mode
pub struct DesktopEngine {
    /// Current view mode (desktop or void)
    pub view_mode: ViewMode,
    /// Void layer state (camera for viewing all desktops)
    pub void_state: VoidState,
    /// Legacy viewport (for backward compatibility during migration)
    pub viewport: Viewport,
    /// Window manager
    pub windows: WindowManager,
    /// Desktop manager
    pub desktops: DesktopManager,
    /// Input router
    pub input: InputRouter,
    /// Current crossfade transition (if any)
    crossfade: Option<Crossfade>,
    /// Camera animation within current layer (if any)
    camera_animation: Option<CameraAnimation>,
    /// Last viewport activity time (ms) for animation detection
    last_activity_ms: f64,
}

/// Viewport for infinite canvas navigation
///
/// Simple state holder for the current viewport position and zoom.
/// Animation is handled by [`CameraAnimation`] which updates viewport state each frame.
#[derive(Clone, Debug)]
pub struct Viewport {
    /// Center position on infinite canvas
    pub center: Vec2,
    /// Zoom level (1.0 = 100%, 0.5 = zoomed out, 2.0 = zoomed in)
    pub zoom: f32,
    /// Screen size in pixels
    pub screen_size: Size,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
            screen_size: Size::new(1920.0, 1080.0),
        }
    }
}

impl Viewport {
    /// Create a new viewport with the given screen size
    pub fn new(screen_width: f32, screen_height: f32) -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
            screen_size: Size::new(screen_width, screen_height),
        }
    }

    /// Set the zoom level directly (no clamping - caller should clamp if needed)
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom;
    }

    /// Set the zoom level with custom min/max clamping
    pub fn set_zoom_clamped(&mut self, zoom: f32, min: f32, max: f32) {
        self.zoom = zoom.clamp(min, max);
    }

    /// Convert screen coordinates to canvas coordinates
    pub fn screen_to_canvas(&self, screen: Vec2) -> Vec2 {
        let half_screen = self.screen_size.as_vec2() * 0.5;
        let offset = screen - half_screen;
        self.center + offset / self.zoom
    }

    /// Convert canvas coordinates to screen coordinates
    pub fn canvas_to_screen(&self, canvas: Vec2) -> Vec2 {
        let offset = canvas - self.center;
        let half_screen = self.screen_size.as_vec2() * 0.5;
        offset * self.zoom + half_screen
    }

    /// Pan the viewport by the given delta (in screen pixels)
    /// This is raw panning with no bounds checking - caller should apply constraints
    pub fn pan(&mut self, dx: f32, dy: f32) {
        // Panning moves the viewport center in the opposite direction
        self.center.x -= dx / self.zoom;
        self.center.y -= dy / self.zoom;
    }

    /// Zoom the viewport around an anchor point (in screen coordinates)
    /// No clamping is applied - use for infinite zoom within workspaces
    pub fn zoom_at(&mut self, factor: f32, anchor_x: f32, anchor_y: f32) {
        // Convert anchor to canvas coords before zoom
        let anchor_screen = Vec2::new(anchor_x, anchor_y);
        let anchor_canvas = self.screen_to_canvas(anchor_screen);

        // Apply zoom (no clamping for infinite zoom)
        self.zoom *= factor;

        // Adjust center so anchor point stays at same screen position
        let half_screen = self.screen_size.as_vec2() * 0.5;
        let anchor_offset = anchor_screen - half_screen;
        self.center = anchor_canvas - anchor_offset / self.zoom;
    }

    /// Zoom the viewport with min/max clamping (for void mode)
    pub fn zoom_at_clamped(
        &mut self,
        factor: f32,
        anchor_x: f32,
        anchor_y: f32,
        min_zoom: f32,
        max_zoom: f32,
    ) {
        let anchor_screen = Vec2::new(anchor_x, anchor_y);
        let anchor_canvas = self.screen_to_canvas(anchor_screen);

        // Apply zoom with clamping
        self.zoom = (self.zoom * factor).clamp(min_zoom, max_zoom);

        let half_screen = self.screen_size.as_vec2() * 0.5;
        let anchor_offset = anchor_screen - half_screen;
        self.center = anchor_canvas - anchor_offset / self.zoom;
    }

    /// Get the visible rectangle on the canvas
    pub fn visible_rect(&self) -> Rect {
        let half_size = self.screen_size.as_vec2() / self.zoom * 0.5;
        Rect::new(
            self.center.x - half_size.x,
            self.center.y - half_size.y,
            self.screen_size.width / self.zoom,
            self.screen_size.height / self.zoom,
        )
    }

    /// Apply a camera state (from CameraAnimation)
    pub fn apply_camera(&mut self, camera: Camera) {
        self.center = camera.center;
        self.zoom = camera.zoom;
    }

    /// Get the current state as a Camera
    pub fn to_camera(&self) -> Camera {
        Camera::at(self.center, self.zoom)
    }
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
        }
    }

    // =========================================================================
    // New Crossfade Transition Methods
    // =========================================================================

    /// Get the current crossfade transition (if any)
    pub fn crossfade(&self) -> Option<&Crossfade> {
        self.crossfade.as_ref()
    }

    /// Check if a crossfade transition is active
    pub fn is_crossfading(&self) -> bool {
        self.crossfade.is_some()
    }

    /// Get layer opacities for rendering
    ///
    /// Returns (desktop_opacity, void_opacity)
    /// - During normal desktop view: (1.0, 0.0)
    /// - During normal void view: (0.0, 1.0)
    /// - During transition: interpolated values
    pub fn layer_opacities(&self) -> (f32, f32) {
        if let Some(ref crossfade) = self.crossfade {
            let now = js_sys::Date::now();
            crossfade.opacities(now)
        } else {
            match self.view_mode {
                ViewMode::Desktop { .. } => (1.0, 0.0),
                ViewMode::Void => (0.0, 1.0),
            }
        }
    }

    /// Get the desktop camera for a specific desktop index
    pub fn desktop_camera(&self, index: usize) -> Option<Camera> {
        self.desktops
            .desktops()
            .get(index)
            .map(|ws| ws.camera())
    }

    /// Get the current active camera based on view mode
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

    /// Start a crossfade transition to void
    pub fn start_crossfade_to_void(&mut self) {
        if !self.view_mode.is_desktop() {
            return;
        }

        let from_desktop = match self.view_mode {
            ViewMode::Desktop { index } => index,
            _ => return,
        };

        let now = js_sys::Date::now();

        // Save current desktop's camera state
        let current_camera = self.viewport.to_camera();
        self.desktops.save_desktop_camera(
            from_desktop,
            current_camera.center,
            current_camera.zoom,
        );

        // Initialize void camera at the center of all desktops with appropriate zoom
        let desktop_bounds: Vec<Rect> = self
            .desktops
            .desktops()
            .iter()
            .map(|d| d.bounds)
            .collect();
        let void_center = VoidState::calculate_void_center(&desktop_bounds);
        let void_zoom = VoidState::calculate_fit_zoom(&desktop_bounds, self.viewport.screen_size);

        self.void_state
            .set_camera(Camera::at(void_center, void_zoom));

        // Start crossfade
        self.crossfade = Some(Crossfade::to_void(now, from_desktop));
    }

    /// Start a crossfade transition to desktop
    pub fn start_crossfade_to_desktop(&mut self, desktop_index: usize) {
        if !self.view_mode.is_void() {
            return;
        }

        let now = js_sys::Date::now();

        // Update workspace manager's active index
        self.desktops.switch_to(desktop_index);

        // Start crossfade
        self.crossfade = Some(Crossfade::to_desktop(now, desktop_index));
    }

    /// Tick crossfade transition
    /// Returns true if transition completed this frame
    fn tick_crossfade(&mut self) -> bool {
        let crossfade = match &self.crossfade {
            Some(c) => c,
            None => return false,
        };

        let now = js_sys::Date::now();

        if crossfade.is_complete(now) {
            // Transition complete - update view mode
            match crossfade.direction {
                CrossfadeDirection::ToVoid => {
                    self.view_mode = ViewMode::Void;
                    // Update viewport to match void camera
                    self.viewport.center = self.void_state.camera().center;
                    self.viewport.zoom = self.void_state.camera().zoom;
                }
                CrossfadeDirection::ToDesktop => {
                    let index = crossfade.target_desktop.unwrap_or(0);
                    self.view_mode = ViewMode::Desktop { index };
                    // Restore desktop's saved camera
                    if let Some(saved_camera) = self.desktops.get_desktop_camera(index) {
                        self.viewport.center = saved_camera.center;
                        self.viewport.zoom = saved_camera.zoom;
                    }
                    // Focus top window on the desktop
                    self.focus_top_window_on_desktop(index);
                }
                CrossfadeDirection::SwitchDesktop => {
                    let index = crossfade.target_desktop.unwrap_or(0);
                    self.view_mode = ViewMode::Desktop { index };
                    // Camera already restored at start of switch_desktop()
                    // Focus top window on the new desktop
                    self.focus_top_window_on_desktop(index);
                }
            }
            self.crossfade = None;
            return true;
        }

        false
    }

    /// Commit current viewport state to the active desktop.
    ///
    /// Called on every pan/zoom to keep desktop viewport in sync.
    /// Only commits when in Desktop mode (not during transitions or void).
    fn commit_viewport_to_workspace(&mut self) {
        if let ViewMode::Desktop { index } = self.view_mode {
            self.desktops.save_desktop_camera(
                index,
                self.viewport.center,
                self.viewport.zoom,
            );
        }
    }

    /// Get the current view mode
    pub fn get_view_mode(&self) -> &ViewMode {
        &self.view_mode
    }

    /// Check if currently in void mode (can see all workspaces)
    pub fn is_in_void(&self) -> bool {
        self.view_mode.is_void()
    }

    /// Check if viewing a specific workspace
    pub fn is_in_workspace(&self) -> bool {
        self.view_mode.is_workspace()
    }

    /// Initialize the desktop with screen dimensions
    pub fn init(&mut self, width: f32, height: f32) {
        let screen_size = Size::new(width, height);
        self.viewport.screen_size = screen_size;

        // Initialize void state with screen size
        self.void_state.set_screen_size(screen_size);

        // Ensure workspace size is at least as large as screen size
        // This prevents forced zoom > 1.0 on larger screens
        self.desktops
            .set_desktop_size(Size::new(width.max(1920.0), height.max(1080.0)));

        // Create default workspace centered at origin
        let workspace_id = self.desktops.create("Main");

        // Center viewport on the first workspace
        if let Some(workspace) = self.desktops.get(workspace_id) {
            self.viewport.center = workspace.bounds.center();
        }
    }

    /// Resize the viewport
    pub fn resize(&mut self, width: f32, height: f32) {
        let screen_size = Size::new(width, height);
        self.viewport.screen_size = screen_size;

        // Update void state screen size
        self.void_state.set_screen_size(screen_size);

        // Update workspace sizes to accommodate new screen size
        self.update_desktop_sizes_for_screen();

        self.clamp_viewport_to_workspace();
    }

    /// Ensure all desktop bounds are at least as large as the screen
    ///
    /// This updates both the desktop size setting AND recalculates all
    /// existing desktop bounds. This ensures the Rust layout matches
    /// what the background shader expects based on desktop_size.
    fn update_desktop_sizes_for_screen(&mut self) {
        let min_width = self.viewport.screen_size.width.max(1920.0);
        let min_height = self.viewport.screen_size.height.max(1080.0);

        // Update workspace size - this also recalculates all existing workspace bounds
        self.desktops
            .set_desktop_size(Size::new(min_width, min_height));
    }

    /// Pan the viewport - behavior depends on view mode
    ///
    /// - **Desktop mode**: Infinite panning allowed, auto-saves to desktop
    /// - **Void mode**: Pan is constrained to keep desktops visible
    /// - **Transitioning**: Pan is ignored (animation controls viewport)
    pub fn pan(&mut self, dx: f32, dy: f32) {
        // Ignore pan during crossfade transitions
        if self.is_crossfading() {
            return;
        }

        match &self.view_mode {
            ViewMode::Desktop { .. } => {
                // Infinite pan within desktop - no constraints
                self.viewport.pan(dx, dy);
                // Auto-save viewport state to desktop for persistence
                self.commit_viewport_to_workspace();
                #[cfg(target_arch = "wasm32")]
                {
                    self.last_activity_ms = js_sys::Date::now();
                }
            }
            ViewMode::Void => {
                // In void, constrain pan to keep desktops visible
                self.viewport.pan(dx, dy);
                self.clamp_viewport_in_void();
                #[cfg(target_arch = "wasm32")]
                {
                    self.last_activity_ms = js_sys::Date::now();
                }
            }
        }
    }

    /// Zoom the viewport at anchor point - behavior depends on view mode
    ///
    /// - **Desktop mode**: Infinite zoom allowed (zoom in forever), auto-saves to desktop
    /// - **Void mode**: Zoom is constrained (0.1 to 1.0)
    /// - **Transitioning**: Zoom is ignored (animation controls viewport)
    pub fn zoom_at(&mut self, factor: f32, anchor_x: f32, anchor_y: f32) {
        // Ignore zoom during crossfade transitions
        if self.is_crossfading() {
            return;
        }

        match &self.view_mode {
            ViewMode::Desktop { .. } => {
                // Infinite zoom in desktops - only clamp to prevent zoom <= 0
                self.viewport.zoom_at(factor, anchor_x, anchor_y);
                // Ensure zoom doesn't go below a minimum (for numerical stability)
                if self.viewport.zoom < 0.001 {
                    self.viewport.zoom = 0.001;
                }
                // Auto-save viewport state to desktop for persistence
                self.commit_viewport_to_workspace();
                #[cfg(target_arch = "wasm32")]
                {
                    self.last_activity_ms = js_sys::Date::now();
                }
            }
            ViewMode::Void => {
                // In void, constrain zoom to see desktops (0.1 to 1.0)
                self.viewport
                    .zoom_at_clamped(factor, anchor_x, anchor_y, 0.1, 1.0);
                self.clamp_viewport_in_void();
                #[cfg(target_arch = "wasm32")]
                {
                    self.last_activity_ms = js_sys::Date::now();
                }
            }
        }
    }

    /// Clamp viewport when in void mode to keep workspaces visible.
    /// This only applies to void mode - workspaces have infinite pan/zoom.
    fn clamp_viewport_in_void(&mut self) {
        // Calculate the bounding box of all workspaces
        let workspaces = self.desktops.desktops();
        if workspaces.is_empty() {
            return;
        }

        let min_x = workspaces
            .iter()
            .map(|ws| ws.bounds.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = workspaces
            .iter()
            .map(|ws| ws.bounds.x + ws.bounds.width)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = workspaces
            .iter()
            .map(|ws| ws.bounds.y)
            .fold(f32::INFINITY, f32::min);
        let max_y = workspaces
            .iter()
            .map(|ws| ws.bounds.y + ws.bounds.height)
            .fold(f32::NEG_INFINITY, f32::max);

        // Add padding around the workspace area
        let padding = 200.0;
        let bounds = Rect::new(
            min_x - padding,
            min_y - padding,
            (max_x - min_x) + padding * 2.0,
            (max_y - min_y) + padding * 2.0,
        );

        // Calculate visible area
        let half_visible_w = self.viewport.screen_size.width / self.viewport.zoom / 2.0;
        let half_visible_h = self.viewport.screen_size.height / self.viewport.zoom / 2.0;

        // Clamp center to keep some workspaces visible
        let center_min_x = bounds.x + half_visible_w;
        let center_max_x = bounds.x + bounds.width - half_visible_w;
        let center_min_y = bounds.y + half_visible_h;
        let center_max_y = bounds.y + bounds.height - half_visible_h;

        if center_min_x <= center_max_x {
            self.viewport.center.x = self.viewport.center.x.clamp(center_min_x, center_max_x);
        } else {
            self.viewport.center.x = bounds.x + bounds.width / 2.0;
        }

        if center_min_y <= center_max_y {
            self.viewport.center.y = self.viewport.center.y.clamp(center_min_y, center_max_y);
        } else {
            self.viewport.center.y = bounds.y + bounds.height / 2.0;
        }
    }

    /// Legacy method - kept for compatibility but now a no-op in desktop mode
    /// Called during resize to apply appropriate constraints
    fn clamp_viewport_to_workspace(&mut self) {
        // Don't clamp during transitions
        if self.is_crossfading() {
            return;
        }

        match &self.view_mode {
            ViewMode::Desktop { .. } => {
                // No clamping in desktop mode - infinite zoom/pan
            }
            ViewMode::Void => {
                self.clamp_viewport_in_void();
            }
        }
    }

    /// Tick the transition state machine and update viewport.
    /// Call this each frame during rendering.
    /// Returns true if a transition is active.
    ///
    /// This method handles:
    /// - Crossfade transitions (opacity-based layer switching)
    /// - Camera animations (smooth pan/zoom within a layer)
    ///
    /// This method also updates `view_mode` when transitions complete:
    /// - ToVoid completion -> ViewMode::Void
    /// - ToDesktop/SwitchDesktop completion -> ViewMode::Desktop { index }
    pub fn tick_transition(&mut self) -> bool {
        let now = js_sys::Date::now();

        // Tick crossfade transitions
        if self.tick_crossfade() {
            // Crossfade completed this frame
            return self.camera_animation.is_some();
        }

        // Tick camera animation (for pan_to_window)
        if let Some(ref animation) = self.camera_animation {
            if animation.is_complete(now) {
                // Animation complete - apply final state
                let final_camera = animation.final_camera();
                self.viewport.center = final_camera.center;
                self.viewport.zoom = final_camera.zoom;
                self.camera_animation = None;
                return self.is_crossfading();
            } else {
                // Animation in progress - apply interpolated state
                let current = animation.current(now);
                self.viewport.center = current.center;
                self.viewport.zoom = current.zoom;
                return true;
            }
        }

        // Return true if any transition is active
        self.is_crossfading()
    }

    /// Get all windows on the current workspace with their screen-space rectangles
    /// Returns JSON-serializable data for React positioning
    ///
    /// ## Visibility Behavior
    ///
    /// - **Normal mode (no transition)**: All windows on the workspace are included.
    ///   No filtering prevents flicker during manual pan/zoom.
    ///
    /// - **Workspace transitions**: Windows are culled based on phase-appropriate viewport.
    ///   This prevents off-screen windows from appearing during transitions.
    ///   - ZoomOut/Pan: Cull using source viewport (where user was looking)
    ///   - ZoomIn: Cull using target viewport (where user will end up)
    ///
    /// - **Pan-to-window**: Visibility expanded to include windows at both start and
    ///   end positions, so windows slide naturally into view.
    pub fn get_window_screen_rects(&self) -> Vec<WindowScreenRect> {
        // Determine which workspace's windows to show
        let workspace_index = self.visible_workspace_index();

        let workspace = match self.desktops.desktops().get(workspace_index) {
            Some(ws) => ws,
            None => return Vec::new(),
        };

        // Get cull rect for transitions (None if not transitioning or pan-to)
        let cull_rect = self.get_transition_cull_rect();

        // Calculate window opacity based on transition state
        // Windows immediately fade out during workspace transitions for a clean visual
        let opacity = self.calculate_window_opacity();

        // Debug logging for transition culling (only during transitions)
        #[cfg(target_arch = "wasm32")]
        if self.is_crossfading() {
            static mut LAST_CULL_LOG: f64 = 0.0;
            let now = js_sys::Date::now();
            unsafe {
                if now - LAST_CULL_LOG > 200.0 {
                    let direction = self.crossfade.as_ref().map(|c| c.direction);
                    web_sys::console::log_1(
                        &format!(
                            "[cull] crossfade={:?} ws_idx={} cull_rect={:?} viewport=({:.0},{:.0}) zoom={:.2} opacity={:.2}",
                            direction, workspace_index, 
                            cull_rect.map(|r| format!("({:.0},{:.0},{:.0},{:.0})", r.x, r.y, r.width, r.height)),
                            self.viewport.center.x, self.viewport.center.y, self.viewport.zoom, opacity
                        )
                        .into(),
                    );
                    LAST_CULL_LOG = now;
                }
            }
        }

        let mut rects = Vec::new();

        for window in self.windows.windows_by_z() {
            // Check if window belongs to the workspace we're showing
            if !workspace.contains_window(window.id) {
                continue;
            }

            // Skip minimized windows
            if window.state == WindowState::Minimized {
                continue;
            }

            // During workspace transitions, cull windows outside the phase-appropriate viewport
            // This prevents off-screen windows from suddenly appearing
            if let Some(ref cull) = cull_rect {
                let window_rect = window.rect();
                if !cull.intersects(&window_rect) {
                    continue;
                }
            }

            // Convert to screen coordinates using CURRENT viewport (for rendering position)
            let screen_pos = self.viewport.canvas_to_screen(window.position);
            let screen_size = Size::new(
                window.size.width * self.viewport.zoom,
                window.size.height * self.viewport.zoom,
            );

            rects.push(WindowScreenRect {
                id: window.id,
                title: window.title.clone(),
                app_id: window.app_id.clone(),
                state: window.state,
                focused: self.windows.focused() == Some(window.id),
                screen_rect: Rect::new(
                    screen_pos.x,
                    screen_pos.y,
                    screen_size.width,
                    screen_size.height,
                ),
                opacity,
            });
        }

        rects
    }

    /// Calculate window opacity based on transition state.
    ///
    /// Windows fade during crossfade transitions following the animation curve.
    /// Windows remain fully visible during camera animations (pan to window).
    fn calculate_window_opacity(&self) -> f32 {
        // Crossfade transitions control window opacity
        if let Some(ref crossfade) = self.crossfade {
            let now = js_sys::Date::now();
            let (desktop_opacity, _void_opacity) = crossfade.opacities(now);
            return desktop_opacity;
        }

        // Camera animations (pan to window) keep windows visible
        // No transition = fully visible
        1.0
    }

    /// Get the cull rect for visibility filtering during transitions.
    ///
    /// Returns None - the crossfade system handles visibility via opacity,
    /// so we don't need to cull windows during transitions.
    fn get_transition_cull_rect(&self) -> Option<Rect> {
        // Crossfade transitions use opacity to hide windows, not culling
        // Camera animations (pan to window) should show all windows in both
        // start and end positions, so no culling needed
        None
    }

    /// Get the workspace index whose windows should be visible.
    /// During desktop switch crossfades, shows the target desktop's windows.
    fn visible_workspace_index(&self) -> usize {
        // During crossfade, show target desktop's windows
        if let Some(ref crossfade) = self.crossfade {
            if let Some(target) = crossfade.target_desktop {
                return target;
            }
        }
        self.desktops.active_index()
    }

    /// Create a window and return its ID
    pub fn create_window(&mut self, config: WindowConfig) -> WindowId {
        let id = self.windows.create(config);

        // Add to current workspace
        let active = self.desktops.active_index();
        self.desktops.add_window_to_desktop(active, id);

        id
    }

    /// Close a window
    pub fn close_window(&mut self, id: WindowId) {
        self.desktops.remove_window(id);
        self.windows.close(id);
    }

    /// Focus a window (brings it to front in z-order)
    ///
    /// Note: This method only handles focus. For panning to the window,
    /// call `pan_to_window` separately. This separation allows the caller
    /// to choose between instant focus (clicking visible window) vs
    /// animated pan + focus (clicking taskbar item for off-screen window).
    pub fn focus_window(&mut self, id: WindowId) {
        self.windows.focus(id);
    }

    /// Focus the top (highest z-order) non-minimized window on a workspace.
    /// Used when switching workspaces to ensure a window on the new workspace is focused.
    fn focus_top_window_on_desktop(&mut self, workspace_index: usize) {
        let workspace = match self.desktops.desktops().get(workspace_index) {
            Some(ws) => ws,
            None => return,
        };

        // Find the top non-minimized window on this workspace
        // windows_by_z returns lowest z-order first, so we use rfind to get the last matching element
        let top_window = self
            .windows
            .windows_by_z()
            .into_iter()
            .rfind(|w| workspace.contains_window(w.id) && w.state != WindowState::Minimized);

        if let Some(window) = top_window {
            self.windows.focus(window.id);
        }
    }

    /// Pan the viewport to center on a window with smooth animation
    ///
    /// This is the preferred way to navigate to an off-screen window.
    /// It will smoothly animate the camera to center on the window.
    pub fn pan_to_window(&mut self, id: WindowId) {
        // Don't start pan while user is actively dragging a window
        if self.input.is_dragging() {
            return;
        }

        // Don't start new pan during crossfade transitions
        // But allow interrupting an existing camera animation
        if self.is_crossfading() {
            return;
        }

        if let Some(window) = self.windows.get(id) {
            // Skip minimized windows (they're not visible anyway)
            if window.state == WindowState::Minimized {
                return;
            }

            let window_rect = window.rect();
            let target_center = window_rect.center();
            let now = js_sys::Date::now();

            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&format!(
                "[pan_to_window] id={} pos=({:.0},{:.0}) size=({:.0}x{:.0}) center=({:.0},{:.0}) viewport=({:.0},{:.0})",
                id, window.position.x, window.position.y,
                window.size.width, window.size.height,
                target_center.x, target_center.y,
                self.viewport.center.x, self.viewport.center.y
            ).into());

            // Start camera animation to pan to window
            let from_camera = Camera::at(self.viewport.center, self.viewport.zoom);
            let to_camera = Camera::at(target_center, self.viewport.zoom);
            self.camera_animation = Some(CameraAnimation::new(from_camera, to_camera, now));
        }
    }

    /// Move a window
    pub fn move_window(&mut self, id: WindowId, x: f32, y: f32) {
        self.windows.move_window(id, Vec2::new(x, y));
    }

    /// Resize a window
    pub fn resize_window(&mut self, id: WindowId, width: f32, height: f32) {
        self.windows.resize(id, Size::new(width, height));
    }

    /// Minimize a window
    pub fn minimize_window(&mut self, id: WindowId) {
        self.windows.minimize(id);
    }

    /// Maximize a window
    pub fn maximize_window(&mut self, id: WindowId) {
        // Use actual screen size, not workspace bounds
        // This ensures maximize fills the visible viewport regardless of zoom level
        let taskbar_height = 48.0;
        let maximize_bounds = Rect::new(
            0.0,
            0.0,
            self.viewport.screen_size.width,
            self.viewport.screen_size.height - taskbar_height,
        );
        self.windows.maximize(id, Some(maximize_bounds));
    }

    /// Restore a minimized window
    pub fn restore_window(&mut self, id: WindowId) {
        self.windows.restore(id);
    }

    /// Start a resize drag operation from a specific direction
    /// Called directly by React resize handles to bypass hit testing
    pub fn start_resize_drag(
        &mut self,
        id: WindowId,
        direction: &str,
        screen_x: f32,
        screen_y: f32,
    ) {
        // Cancel any camera animation to prevent viewport drift during resize
        self.camera_animation = None;

        let handle = match direction {
            "n" => WindowRegion::ResizeN,
            "s" => WindowRegion::ResizeS,
            "e" => WindowRegion::ResizeE,
            "w" => WindowRegion::ResizeW,
            "ne" => WindowRegion::ResizeNE,
            "nw" => WindowRegion::ResizeNW,
            "se" => WindowRegion::ResizeSE,
            "sw" => WindowRegion::ResizeSW,
            _ => return,
        };

        if let Some(window) = self.windows.get(id) {
            let canvas_pos = self
                .viewport
                .screen_to_canvas(Vec2::new(screen_x, screen_y));
            self.input
                .start_window_resize(id, handle, window.position, window.size, canvas_pos);
        }
    }

    /// Start a move drag operation from a window's title bar
    /// Called directly by React title bar to bypass hit testing
    pub fn start_move_drag(&mut self, id: WindowId, screen_x: f32, screen_y: f32) {
        // Cancel any camera animation to prevent viewport drift during drag
        // The drag offset is calculated relative to current viewport, so the
        // viewport must stay fixed during the entire drag operation
        self.camera_animation = None;

        // Extract position first to avoid borrow conflict with focus()
        let window_position = match self.windows.get(id) {
            Some(window) => window.position,
            None => {
                #[cfg(target_arch = "wasm32")]
                web_sys::console::log_1(
                    &format!("[start_move_drag] window {} not found!", id).into(),
                );
                return;
            }
        };

        let canvas_pos = self
            .viewport
            .screen_to_canvas(Vec2::new(screen_x, screen_y));
        let offset = canvas_pos - window_position;
        self.windows.focus(id);
        self.input.start_window_move(id, offset);
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(
            &format!(
            "[start_move_drag] id={} window_pos=({:.0},{:.0}) offset=({:.0},{:.0}) is_dragging={}",
            id, window_position.x, window_position.y, offset.x, offset.y, self.input.is_dragging()
        )
            .into(),
        );
    }

    /// Switch to a desktop by index using crossfade transition
    ///
    /// Uses simple opacity crossfade: current desktop fades out while target fades in.
    /// Both desktops render simultaneously during the transition.
    pub fn switch_desktop(&mut self, index: usize) {
        // Don't switch while user is actively dragging a window
        if self.input.is_dragging() {
            return;
        }

        // Don't switch during an active transition
        if self.is_crossfading() {
            return;
        }

        let current_index = self.desktops.active_index();

        // Don't switch if already on the target desktop
        if current_index == index {
            return;
        }

        let now = js_sys::Date::now();

        // Save current desktop's camera state before switching
        self.desktops
            .save_desktop_camera(current_index, self.viewport.center, self.viewport.zoom);

        // Switch to target desktop
        if self.desktops.switch_to(index) {
            // Focus top window on new desktop immediately
            self.focus_top_window_on_desktop(index);

            // Start crossfade transition
            self.crossfade = Some(Crossfade::switch_desktop(now, current_index, index));

            // Restore target desktop's saved camera state
            if let Some(saved_camera) = self.desktops.get_desktop_camera(index) {
                self.viewport.center = saved_camera.center;
                self.viewport.zoom = saved_camera.zoom;
            }
        }
    }

    /// Switch to a workspace by index (backward compatibility alias)
    #[deprecated(note = "Use switch_desktop() instead")]
    pub fn switch_workspace(&mut self, index: usize) {
        self.switch_desktop(index);
    }

    /// Enter the void (zoomed out view showing all desktops)
    ///
    /// From the void, users can see all desktops and select one to enter.
    /// Uses simple opacity crossfade - desktop layer fades out, void layer fades in.
    pub fn enter_void(&mut self) {
        // Don't enter void while user is actively dragging a window
        if self.input.is_dragging() {
            return;
        }

        // Can't enter void if already in void or transitioning
        if !self.view_mode.is_desktop() || self.is_crossfading() {
            return;
        }

        // Use the new crossfade system
        self.start_crossfade_to_void();
    }

    /// Exit the void into a specific desktop
    ///
    /// Uses simple opacity crossfade - void layer fades out, desktop layer fades in.
    ///
    /// # Arguments
    /// * `desktop_index` - The desktop to enter
    pub fn exit_void(&mut self, desktop_index: usize) {
        // Don't exit void while user is actively dragging
        if self.input.is_dragging() {
            return;
        }

        // Can't exit void if not in void or already transitioning
        if !self.view_mode.is_void() || self.is_crossfading() {
            return;
        }

        // Use the new crossfade system
        self.start_crossfade_to_desktop(desktop_index);
    }

    /// Calculate the center point of all workspaces (for void view)
    fn calculate_void_center(&self) -> Vec2 {
        let workspaces = self.desktops.desktops();
        if workspaces.is_empty() {
            return Vec2::ZERO;
        }

        let min_x = workspaces
            .iter()
            .map(|ws| ws.bounds.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = workspaces
            .iter()
            .map(|ws| ws.bounds.x + ws.bounds.width)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = workspaces
            .iter()
            .map(|ws| ws.bounds.y)
            .fold(f32::INFINITY, f32::min);
        let max_y = workspaces
            .iter()
            .map(|ws| ws.bounds.y + ws.bounds.height)
            .fold(f32::NEG_INFINITY, f32::max);

        Vec2::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0)
    }

    /// Get the workspace index that should be rendered visually.
    /// During crossfade transitions, returns the target desktop.
    pub fn get_visual_active_workspace(&self) -> usize {
        // During crossfade, show target desktop
        if let Some(ref crossfade) = self.crossfade {
            if let Some(target) = crossfade.target_desktop {
                return target;
            }
        }
        self.desktops.active_index()
    }

    /// Calculate total width of all workspaces on the canvas
    fn calculate_total_workspaces_width(&self) -> f32 {
        let workspaces = self.desktops.desktops();
        if workspaces.is_empty() {
            return self.viewport.screen_size.width;
        }

        let min_x = workspaces
            .iter()
            .map(|ws| ws.bounds.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = workspaces
            .iter()
            .map(|ws| ws.bounds.x + ws.bounds.width)
            .fold(f32::NEG_INFINITY, f32::max);

        max_x - min_x
    }

    /// Check if a transition is in progress (crossfade or camera animation)
    pub fn is_transitioning(&self) -> bool {
        self.is_crossfading() || self.camera_animation.is_some()
    }

    /// Check if any animation/activity is happening (transitions OR recent pan/zoom)
    /// Used by frontend to determine render framerate
    pub fn is_animating(&self) -> bool {
        // Active crossfade transition always means animating
        if self.is_crossfading() {
            return true;
        }
        // Camera animation (pan to window)
        if self.camera_animation.is_some() {
            return true;
        }
        // Check for recent manual pan/zoom activity (within 200ms)
        let now = js_sys::Date::now();
        let activity_threshold_ms = 200.0;
        now - self.last_activity_ms < activity_threshold_ms
    }

    /// Check if a viewport animation is in progress (desktop transitions, void enter/exit)
    /// This is distinct from is_animating() which also includes recent pan/zoom activity
    pub fn is_animating_viewport(&self) -> bool {
        // Crossfade transitions are viewport animations
        self.is_crossfading()
    }

    /// Get the current crossfade direction (if any)
    pub fn get_crossfade_direction(&self) -> Option<CrossfadeDirection> {
        self.crossfade.as_ref().map(|c| c.direction)
    }

    /// Create a new desktop
    pub fn create_desktop(&mut self, name: &str) -> DesktopId {
        self.desktops.create(name)
    }

    /// Create a new workspace (backward compatibility alias)
    #[deprecated(note = "Use create_desktop() instead")]
    pub fn create_workspace(&mut self, name: &str) -> DesktopId {
        self.create_desktop(name)
    }

    /// Launch an application (creates window with app_id)
    pub fn launch_app(&mut self, app_id: &str) -> WindowId {
        // Size window to fit within the visible viewport
        let screen_w = self.viewport.screen_size.width;
        let screen_h = self.viewport.screen_size.height;

        // Window size: use preferred size but constrain to viewport (with padding for taskbar)
        let taskbar_height = 48.0;
        let padding = 20.0;
        let max_w = (screen_w - padding * 2.0).max(400.0);
        let max_h = (screen_h - taskbar_height - padding * 2.0).max(300.0);

        let win_w = 900.0_f32.min(max_w);
        let win_h = 600.0_f32.min(max_h);

        // Cascade windows: offset based on window count for visual separation
        let window_count = self.windows.count() as f32;
        let cascade_offset = (window_count % 8.0) * 30.0; // Cycle every 8 windows

        // Position window centered in the visible viewport, with cascade offset
        let pos_x = self.viewport.center.x - win_w / 2.0 + cascade_offset;
        let pos_y = self.viewport.center.y - win_h / 2.0 + cascade_offset;

        let config = WindowConfig {
            title: app_id.to_string(),
            position: Some(Vec2::new(pos_x, pos_y)),
            size: Size::new(win_w, win_h),
            min_size: Some(Size::new(400.0, 300.0)),
            max_size: None,
            app_id: app_id.to_string(),
            process_id: None,
        };

        self.create_window(config)
    }

    // =========================================================================
    // Input Handling - delegates to InputRouter but manages borrowing
    // =========================================================================

    /// Handle pointer down event
    pub fn handle_pointer_down(
        &mut self,
        x: f32,
        y: f32,
        button: u8,
        ctrl: bool,
        shift: bool,
    ) -> InputResult {
        let screen_pos = Vec2::new(x, y);
        let canvas_pos = self.viewport.screen_to_canvas(screen_pos);

        // Middle mouse button starts canvas pan
        if button == 1 {
            self.camera_animation = None; // Cancel any camera animation
            self.input.start_pan(screen_pos, self.viewport.center);
            return InputResult::Handled;
        }

        // Ctrl or Shift + primary button also pans (even over windows)
        if button == 0 && (ctrl || shift) {
            self.camera_animation = None; // Cancel any camera animation
            self.input.start_pan(screen_pos, self.viewport.center);
            return InputResult::Handled;
        }

        // Primary button - check for window interactions (only in active workspace)
        if button == 0 {
            let active_windows = &self.desktops.active_desktop().windows;
            let zoom = self.viewport.zoom;

            if let Some((window_id, region)) =
                self.windows
                    .region_at_filtered(canvas_pos, Some(active_windows), zoom)
            {
                match region {
                    WindowRegion::CloseButton => {
                        self.close_window(window_id);
                        return InputResult::Handled;
                    }
                    WindowRegion::MinimizeButton => {
                        self.minimize_window(window_id);
                        return InputResult::Handled;
                    }
                    WindowRegion::MaximizeButton => {
                        self.maximize_window(window_id);
                        return InputResult::Handled;
                    }
                    WindowRegion::TitleBar => {
                        // Cancel any camera animation to prevent viewport drift during drag
                        self.camera_animation = None;
                        self.focus_window(window_id);
                        if let Some(window) = self.windows.get(window_id) {
                            self.input
                                .start_window_move(window_id, canvas_pos - window.position);
                        }
                        return InputResult::Handled;
                    }
                    WindowRegion::Content => {
                        self.focus_window(window_id);
                        if let Some(window) = self.windows.get(window_id) {
                            let local = canvas_pos - window.position;
                            return InputResult::Forward {
                                window_id,
                                local_x: local.x,
                                local_y: local.y,
                            };
                        }
                    }
                    // Resize handles
                    handle @ (WindowRegion::ResizeN
                    | WindowRegion::ResizeS
                    | WindowRegion::ResizeE
                    | WindowRegion::ResizeW
                    | WindowRegion::ResizeNE
                    | WindowRegion::ResizeNW
                    | WindowRegion::ResizeSE
                    | WindowRegion::ResizeSW) => {
                        // Cancel any camera animation to prevent viewport drift during resize
                        self.camera_animation = None;
                        self.focus_window(window_id);
                        if let Some(window) = self.windows.get(window_id) {
                            self.input.start_window_resize(
                                window_id,
                                handle,
                                window.position,
                                window.size,
                                canvas_pos,
                            );
                        }
                        return InputResult::Handled;
                    }
                }
            }
        }

        InputResult::Unhandled
    }

    /// Handle pointer move event
    pub fn handle_pointer_move(&mut self, x: f32, y: f32) -> InputResult {
        let screen_pos = Vec2::new(x, y);
        let canvas_pos = self.viewport.screen_to_canvas(screen_pos);

        if let Some(drag_state) = self.input.drag_state() {
            match drag_state {
                DragState::PanCanvas {
                    start,
                    start_center,
                } => {
                    let delta = screen_pos - *start;
                    self.viewport.center = *start_center - delta / self.viewport.zoom;
                    self.clamp_viewport_to_workspace();
                    return InputResult::Handled;
                }
                DragState::MoveWindow { window_id, offset } => {
                    let new_pos = canvas_pos - *offset;
                    let wid = *window_id;
                    self.move_window(wid, new_pos.x, new_pos.y);
                    return InputResult::Handled;
                }
                DragState::ResizeWindow {
                    window_id,
                    handle,
                    start_pos,
                    start_size,
                    start_mouse,
                } => {
                    let delta = canvas_pos - *start_mouse;
                    let (new_pos, new_size) =
                        input::calculate_resize(*handle, *start_pos, *start_size, delta);
                    let wid = *window_id;
                    self.move_window(wid, new_pos.x, new_pos.y);
                    self.resize_window(wid, new_size.width, new_size.height);
                    return InputResult::Handled;
                }
            }
        }

        InputResult::Unhandled
    }

    /// Handle pointer up event
    pub fn handle_pointer_up(&mut self) -> InputResult {
        if self.input.is_dragging() {
            // Check drag type before ending to handle state persistence
            let was_canvas_pan =
                matches!(self.input.drag_state(), Some(DragState::PanCanvas { .. }));

            // Log final position when window drag ends
            #[cfg(target_arch = "wasm32")]
            if let Some(DragState::MoveWindow { window_id, .. }) = self.input.drag_state() {
                if let Some(window) = self.windows.get(*window_id) {
                    web_sys::console::log_1(
                        &format!(
                            "[drag_end] id={} final_pos=({:.0},{:.0})",
                            window_id, window.position.x, window.position.y
                        )
                        .into(),
                    );
                }
            }
            self.input.end_drag();

            // Commit viewport state after canvas pan ends
            // (PanCanvas bypasses pan() method, so auto-save doesn't happen during drag)
            if was_canvas_pan {
                self.commit_viewport_to_workspace();
            }

            return InputResult::Handled;
        }
        InputResult::Unhandled
    }

    /// Handle wheel event - Ctrl+scroll zooms the desktop
    pub fn handle_wheel(&mut self, _dx: f32, dy: f32, x: f32, y: f32, ctrl: bool) -> InputResult {
        if ctrl {
            // Ctrl+scroll = zoom
            let factor = if dy < 0.0 { 1.1 } else { 0.9 };
            self.zoom_at(factor, x, y);
            InputResult::Handled
        } else {
            InputResult::Unhandled
        }
    }
}

/// Window information with screen-space rectangle for React positioning
#[derive(Clone, Debug)]
pub struct WindowScreenRect {
    pub id: WindowId,
    pub title: String,
    pub app_id: String,
    pub state: WindowState,
    pub focused: bool,
    pub screen_rect: Rect,
    /// Opacity for fade transitions (0.0 = invisible, 1.0 = fully visible)
    /// Windows fade out immediately at the start of workspace transitions
    pub opacity: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_screen_to_canvas() {
        let viewport = Viewport::new(1920.0, 1080.0);

        // Center of screen should map to viewport center
        let center = viewport.screen_to_canvas(Vec2::new(960.0, 540.0));
        assert!((center.x - 0.0).abs() < 0.001);
        assert!((center.y - 0.0).abs() < 0.001);

        // Top-left of screen
        let top_left = viewport.screen_to_canvas(Vec2::new(0.0, 0.0));
        assert!((top_left.x - (-960.0)).abs() < 0.001);
        assert!((top_left.y - (-540.0)).abs() < 0.001);
    }

    #[test]
    fn test_viewport_canvas_to_screen() {
        let viewport = Viewport::new(1920.0, 1080.0);

        // Canvas origin should map to screen center
        let screen = viewport.canvas_to_screen(Vec2::ZERO);
        assert!((screen.x - 960.0).abs() < 0.001);
        assert!((screen.y - 540.0).abs() < 0.001);
    }

    #[test]
    fn test_viewport_zoom() {
        let mut viewport = Viewport::new(1920.0, 1080.0);

        // Zoom in at center
        viewport.zoom_at(2.0, 960.0, 540.0);
        assert!((viewport.zoom - 2.0).abs() < 0.001);
        // Center should not move when zooming at center
        assert!((viewport.center.x - 0.0).abs() < 0.001);
        assert!((viewport.center.y - 0.0).abs() < 0.001);
    }

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
            min_size: None,
            max_size: None,
            app_id: "test".to_string(),
            process_id: None,
        });

        assert!(engine.windows.get(id).is_some());
        assert_eq!(engine.desktops.active_desktop().windows.len(), 1);
    }

    // Note: switch_workspace uses js_sys::Date::now() which only works in WASM
    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_desktop_engine_workspace_transition() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        // Create a second workspace
        engine.create_workspace("Second");
        assert_eq!(engine.desktops.desktops().len(), 2);

        // Switch workspace
        engine.switch_workspace(1);
        assert!(engine.is_transitioning());
        assert_eq!(engine.desktops.active_index(), 1);
    }

    #[test]
    fn test_desktop_engine_create_workspace() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        // Create additional workspaces
        engine.create_workspace("Second");
        engine.create_workspace("Third");

        assert_eq!(engine.desktops.desktops().len(), 3);
        assert_eq!(engine.desktops.desktops()[1].name, "Second");
        assert_eq!(engine.desktops.desktops()[2].name, "Third");
    }

    #[test]
    fn test_viewport_pan() {
        let mut viewport = Viewport::new(1920.0, 1080.0);

        // Pan right 100 screen pixels
        viewport.pan(-100.0, 0.0);

        // At zoom 1.0, this should move center by 100
        assert!((viewport.center.x - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_viewport_visible_rect() {
        let viewport = Viewport::new(1920.0, 1080.0);

        let rect = viewport.visible_rect();

        // At center (0,0) and zoom 1.0, visible rect should be:
        // x: -960, y: -540, w: 1920, h: 1080
        assert!((rect.x - (-960.0)).abs() < 0.001);
        assert!((rect.y - (-540.0)).abs() < 0.001);
        assert!((rect.width - 1920.0).abs() < 0.001);
        assert!((rect.height - 1080.0).abs() < 0.001);
    }

    #[test]
    fn test_visible_workspace_index_no_transition() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);
        engine.create_workspace("Second");

        // Without transition, should return active workspace
        assert_eq!(engine.visible_workspace_index(), 0);

        // Manually switch (without animation) and check
        engine.desktops.switch_to(1);
        assert_eq!(engine.visible_workspace_index(), 1);
    }

    #[test]
    fn test_get_window_screen_rects_empty() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        // No windows, should return empty
        let rects = engine.get_window_screen_rects();
        assert!(rects.is_empty());
    }

    #[test]
    fn test_get_window_screen_rects_with_window() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        // Create a window at center
        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(-400.0, -300.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        let rects = engine.get_window_screen_rects();
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].id, id);
        assert_eq!(rects[0].title, "Test");
    }

    #[test]
    fn test_get_window_screen_rects_minimized_not_shown() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(-400.0, -300.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        // Minimize the window
        engine.minimize_window(id);

        // Minimized windows should not appear in screen rects
        let rects = engine.get_window_screen_rects();
        assert!(rects.is_empty());
    }

    #[test]
    fn test_focus_window_brings_to_front() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        let id1 = engine.create_window(WindowConfig {
            title: "Window 1".to_string(),
            position: Some(Vec2::new(0.0, 0.0)),
            size: Size::new(400.0, 300.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        let id2 = engine.create_window(WindowConfig {
            title: "Window 2".to_string(),
            position: Some(Vec2::new(100.0, 100.0)),
            size: Size::new(400.0, 300.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        // Window 2 should be focused (most recently created)
        assert_eq!(engine.windows.focused(), Some(id2));

        // Focus window 1
        engine.focus_window(id1);
        assert_eq!(engine.windows.focused(), Some(id1));

        // Window 1 should now have higher z-order
        let w1 = engine.windows.get(id1).unwrap();
        let w2 = engine.windows.get(id2).unwrap();
        assert!(w1.z_order > w2.z_order);
    }

    #[test]
    fn test_close_window_removes_from_workspace() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(0.0, 0.0)),
            size: Size::new(400.0, 300.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        assert_eq!(engine.desktops.active_desktop().windows.len(), 1);

        engine.close_window(id);

        assert!(engine.windows.get(id).is_none());
        assert_eq!(engine.desktops.active_desktop().windows.len(), 0);
    }

    #[test]
    fn test_maximize_restores_correctly() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(100.0, 100.0)),
            size: Size::new(400.0, 300.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        let original_pos = engine.windows.get(id).unwrap().position;
        let original_size = engine.windows.get(id).unwrap().size;

        // Maximize
        engine.maximize_window(id);
        assert_eq!(
            engine.windows.get(id).unwrap().state,
            WindowState::Maximized
        );

        // Maximize again should restore
        engine.maximize_window(id);
        assert_eq!(engine.windows.get(id).unwrap().state, WindowState::Normal);

        let restored_pos = engine.windows.get(id).unwrap().position;
        let restored_size = engine.windows.get(id).unwrap().size;

        assert!((restored_pos.x - original_pos.x).abs() < 0.001);
        assert!((restored_pos.y - original_pos.y).abs() < 0.001);
        assert!((restored_size.width - original_size.width).abs() < 0.001);
        assert!((restored_size.height - original_size.height).abs() < 0.001);
    }

    #[test]
    fn test_crossfade_transition() {
        // Test crossfade opacities
        let crossfade = Crossfade::switch_desktop(0.0, 0, 1);

        // At start (t=0), desktop opacity should be high
        let (desktop_opacity, void_opacity) = crossfade.opacities(0.0);
        assert!(desktop_opacity > 0.9, "Desktop should be visible at start");
        assert!(void_opacity < 0.1, "Void should not be visible for desktop switch");

        // At midpoint, opacity should dip
        let midpoint = CROSSFADE_DURATION_MS as f64 / 2.0;
        let (mid_opacity, _) = crossfade.opacities(midpoint);
        assert!(mid_opacity < 0.9, "Opacity should dip at midpoint");

        // At end (t=duration), desktop opacity should be back to high
        let (end_opacity, _) = crossfade.opacities(CROSSFADE_DURATION_MS as f64);
        assert!(end_opacity > 0.9, "Desktop should be visible at end");
    }

    #[test]
    fn test_crossfade_to_void() {
        let crossfade = Crossfade::to_void(0.0, 0);

        // At start, desktop visible, void hidden
        let (desktop_opacity, void_opacity) = crossfade.opacities(0.0);
        assert!(desktop_opacity > 0.9, "Desktop should be visible at start");
        assert!(void_opacity < 0.1, "Void should be hidden at start");

        // At end, desktop hidden, void visible
        let (desktop_opacity, void_opacity) = crossfade.opacities(CROSSFADE_DURATION_MS as f64);
        assert!(desktop_opacity < 0.1, "Desktop should be hidden at end");
        assert!(void_opacity > 0.9, "Void should be visible at end");
    }

    #[test]
    fn test_crossfade_to_desktop() {
        let crossfade = Crossfade::to_desktop(0.0, 1);

        // At start, void visible, desktop hidden
        let (desktop_opacity, void_opacity) = crossfade.opacities(0.0);
        assert!(desktop_opacity < 0.1, "Desktop should be hidden at start");
        assert!(void_opacity > 0.9, "Void should be visible at start");

        // At end, desktop visible, void hidden
        let (desktop_opacity, void_opacity) = crossfade.opacities(CROSSFADE_DURATION_MS as f64);
        assert!(desktop_opacity > 0.9, "Desktop should be visible at end");
        assert!(void_opacity < 0.1, "Void should be hidden at end");
    }

    #[test]
    fn test_camera_animation() {
        let from = Camera::at(Vec2::new(0.0, 0.0), 1.0);
        let to = Camera::at(Vec2::new(100.0, 50.0), 2.0);
        let anim = CameraAnimation::new(from, to, 0.0);

        // At start
        let current = anim.current(0.0);
        assert!((current.center.x - 0.0).abs() < 0.001);
        assert!((current.zoom - 1.0).abs() < 0.001);

        // At end
        let final_cam = anim.final_camera();
        assert!((final_cam.center.x - 100.0).abs() < 0.001);
        assert!((final_cam.center.y - 50.0).abs() < 0.001);
        assert!((final_cam.zoom - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_total_workspaces_width() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        // With one workspace, width should be screen width
        let width1 = engine.calculate_total_workspaces_width();
        assert!((width1 - 1920.0).abs() < 0.001);

        // Add another workspace
        engine.create_workspace("Second");
        let width2 = engine.calculate_total_workspaces_width();

        // With two workspaces, total should be larger
        assert!(width2 > width1);
    }

    #[test]
    fn test_viewport_apply_camera() {
        let mut viewport = Viewport::new(1920.0, 1080.0);

        let camera = Camera::at(Vec2::new(100.0, 200.0), 0.5);
        viewport.apply_camera(camera);

        assert!((viewport.center.x - 100.0).abs() < 0.001);
        assert!((viewport.center.y - 200.0).abs() < 0.001);
        assert!((viewport.zoom - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_viewport_infinite_pan_in_workspace() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        // Workspace mode has infinite pan - panning should move the viewport freely

        // Try to pan far to the right
        engine.pan(-5000.0, 0.0);

        // With infinite pan, center should have moved
        assert!(
            (engine.viewport.center.x - 5000.0).abs() < 1.0,
            "Center X should move with infinite pan, got {}",
            engine.viewport.center.x
        );
    }

    #[test]
    fn test_viewport_infinite_pan_when_zoomed() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        // Zoom in first
        engine.zoom_at(2.0, 960.0, 540.0);

        // At zoom 2.0, panning delta is scaled by zoom
        // Pan right
        engine.pan(-200.0, 0.0);
        assert!(
            engine.viewport.center.x > 50.0,
            "Should be able to pan right when zoomed in, got {}",
            engine.viewport.center.x
        );

        // With infinite pan, we can pan beyond workspace bounds
        engine.pan(-10000.0, 0.0);
        assert!(
            engine.viewport.center.x > 480.0,
            "Infinite pan should allow panning beyond workspace bounds, got {}",
            engine.viewport.center.x
        );
    }

    #[test]
    fn test_viewport_infinite_zoom_in_workspace() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);

        // Workspace mode has infinite zoom - zooming out should work
        engine.zoom_at(0.5, 960.0, 540.0);

        // Infinite zoom allows zooming out
        assert!(
            (engine.viewport.zoom - 0.5).abs() < 0.01,
            "Infinite zoom should allow zooming out, got {}",
            engine.viewport.zoom
        );

        // Zooming in should also work
        engine.zoom_at(4.0, 960.0, 540.0);
        assert!(
            (engine.viewport.zoom - 2.0).abs() < 0.01,
            "Should be able to zoom in further, got {}",
            engine.viewport.zoom
        );
    }
}
