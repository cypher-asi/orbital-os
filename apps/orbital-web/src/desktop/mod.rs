//! Desktop Environment for Orbital OS
//!
//! This module implements the desktop environment with:
//! - Infinite canvas viewport with pan/zoom
//! - Window management with z-order and focus
//! - Workspace regions on the infinite canvas
//! - Input routing for window interactions
//! - Animated workspace transitions with film-strip zoom effect
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
//! │  │ Viewport │ │WindowManager │ │ WorkspaceManager  │   │
//! │  │ (state)  │ │   (CRUD)     │ │    (regions)      │   │
//! │  └──────────┘ └──────────────┘ └───────────────────┘   │
//! │  ┌─────────────┐ ┌────────────────────────────────┐    │
//! │  │ InputRouter │ │     TransitionManager          │    │
//! │  │  (drag)     │ │ (workspace switch animations)  │    │
//! │  └─────────────┘ └────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Components
//!
//! - [`DesktopEngine`]: Main engine coordinating all components
//! - [`Viewport`]: Simple state holder for position and zoom
//! - [`WindowManager`]: CRUD operations for windows, z-order, focus stack
//! - [`WorkspaceManager`]: Infinite canvas regions, workspace switching
//! - [`InputRouter`]: Pan/zoom, window drag/resize, event forwarding
//! - [`TransitionManager`]: Animated workspace transitions with state machine
//!
//! ## Workspace Transitions
//!
//! When switching workspaces, the transition follows three phases:
//! 1. **ZoomOut**: Zoom out from current workspace to overview
//! 2. **Overview**: User can navigate between workspaces (with settle delay)
//! 3. **ZoomIn**: Zoom into target workspace
//!
//! Call [`DesktopEngine::tick_transition`] each frame to update animations.

mod input;
mod transition;
mod types;
mod windows;
pub mod workspaces;

pub use input::{DragState, InputResult, InputRouter};
pub use transition::{TransitionManager, TransitionPhase, TransitionType, ViewportState};
pub use types::{Rect, Size, Vec2, FRAME_STYLE};
pub use windows::{Window, WindowConfig, WindowId, WindowManager, WindowRegion, WindowState};
pub use workspaces::{PersistedWorkspace, Workspace, WorkspaceId, WorkspaceManager, WorkspaceViewport};

// =============================================================================
// View Mode - Controls what the user is viewing
// =============================================================================

/// The current viewing mode of the desktop
///
/// The desktop can be in one of three states:
/// - **Workspace**: Viewing a single workspace with infinite zoom/pan capability
/// - **Void**: Zoomed out to see all workspaces (the meta-layer)
/// - **Transitioning**: Animating between workspace and void modes
#[derive(Clone, Debug, PartialEq)]
pub enum ViewMode {
    /// Viewing a single workspace - infinite zoom/pan within it
    Workspace {
        /// Index of the workspace being viewed
        index: usize,
    },
    /// In the Void - can see all workspaces
    Void,
    /// Transitioning between view modes
    Transitioning {
        /// The transition type and state
        transition_type: TransitionType,
        /// Progress from 0.0 to 1.0
        progress: f32,
    },
}

impl Default for ViewMode {
    fn default() -> Self {
        ViewMode::Workspace { index: 0 }
    }
}

impl ViewMode {
    /// Check if currently in a workspace view
    pub fn is_workspace(&self) -> bool {
        matches!(self, ViewMode::Workspace { .. })
    }

    /// Check if currently in the void view
    pub fn is_void(&self) -> bool {
        matches!(self, ViewMode::Void)
    }

    /// Check if currently transitioning
    pub fn is_transitioning(&self) -> bool {
        matches!(self, ViewMode::Transitioning { .. })
    }

    /// Get the workspace index if in workspace mode
    pub fn workspace_index(&self) -> Option<usize> {
        match self {
            ViewMode::Workspace { index } => Some(*index),
            _ => None,
        }
    }

    /// Get the visual workspace index (what should be rendered)
    /// During transitions, returns the appropriate workspace based on transition state
    pub fn visual_workspace_index(&self) -> Option<usize> {
        match self {
            ViewMode::Workspace { index } => Some(*index),
            ViewMode::Void => None,
            ViewMode::Transitioning { transition_type, progress } => {
                match transition_type {
                    TransitionType::EnterVoid { from_workspace } => {
                        // Show source workspace until mostly zoomed out
                        if *progress < 0.7 {
                            Some(*from_workspace)
                        } else {
                            None
                        }
                    }
                    TransitionType::ExitVoid { to_workspace } => {
                        // Show target workspace once zoom starts
                        if *progress > 0.3 {
                            Some(*to_workspace)
                        } else {
                            None
                        }
                    }
                    TransitionType::SwitchWorkspace { from_workspace, to_workspace } => {
                        // Show source during first half, target during second half
                        if *progress < 0.5 {
                            Some(*from_workspace)
                        } else {
                            Some(*to_workspace)
                        }
                    }
                    TransitionType::PanToPosition => {
                        // Pan doesn't change workspace view
                        None
                    }
                }
            }
        }
    }
}

/// Desktop engine coordinating all desktop components
///
/// This is the main entry point for desktop operations, managing:
/// - View mode (workspace, void, or transitioning)
/// - Viewport (pan/zoom state - the camera)
/// - Window manager (window CRUD, focus, z-order)
/// - Workspace manager (separate infinite canvases)
/// - Input router (drag/resize state machine)
/// - Transition manager (view mode change animations)
///
/// ## Conceptual Model
///
/// - **Workspace**: An isolated infinite canvas with its own windows and background
/// - **Void**: The meta-layer where you can see all workspaces
/// - **Viewport**: The camera pointing at either a workspace or the void
///
/// Within a workspace, users have infinite zoom/pan. The void is only accessible
/// via explicit transitions (keyboard shortcut, gesture, or workspace switch).
pub struct DesktopEngine {
    /// Current view mode (workspace, void, or transitioning)
    pub view_mode: ViewMode,
    /// Viewport (the camera)
    pub viewport: Viewport,
    /// Window manager
    pub windows: WindowManager,
    /// Workspace manager
    pub workspaces: WorkspaceManager,
    /// Input router
    pub input: InputRouter,
    /// View mode transition manager
    transitions: TransitionManager,
    /// Last viewport activity time (ms) for animation detection
    last_activity_ms: f64,
}

/// Viewport for infinite canvas navigation
///
/// Simple state holder for the current viewport position and zoom.
/// Animation is handled by [`TransitionManager`] which updates viewport state each frame.
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
    pub fn zoom_at_clamped(&mut self, factor: f32, anchor_x: f32, anchor_y: f32, min_zoom: f32, max_zoom: f32) {
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

    /// Apply a viewport state (from TransitionManager animation)
    pub fn apply_state(&mut self, state: ViewportState) {
        self.center = state.center;
        self.zoom = state.zoom;
    }
    
    /// Get the current state as a ViewportState
    pub fn to_state(&self) -> ViewportState {
        ViewportState {
            center: self.center,
            zoom: self.zoom,
        }
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
            viewport: Viewport::default(),
            windows: WindowManager::new(),
            workspaces: WorkspaceManager::new(),
            input: InputRouter::new(),
            transitions: TransitionManager::new(),
            last_activity_ms: 0.0,
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
        self.viewport.screen_size = Size::new(width, height);

        // Ensure workspace size is at least as large as screen size
        // This prevents forced zoom > 1.0 on larger screens
        self.workspaces.set_workspace_size(Size::new(
            width.max(1920.0),
            height.max(1080.0),
        ));

        // Create default workspace centered at origin
        let workspace_id = self.workspaces.create("Main");

        // Center viewport on the first workspace
        if let Some(workspace) = self.workspaces.get(workspace_id) {
            self.viewport.center = workspace.bounds.center();
        }
    }

    /// Resize the viewport
    pub fn resize(&mut self, width: f32, height: f32) {
        self.viewport.screen_size = Size::new(width, height);
        
        // Update workspace sizes to accommodate new screen size
        self.update_workspace_sizes_for_screen();
        
        self.clamp_viewport_to_workspace();
    }
    
    /// Ensure all workspace bounds are at least as large as the screen
    /// 
    /// This updates both the workspace size setting AND recalculates all
    /// existing workspace bounds. This ensures the Rust layout matches
    /// what the background shader expects based on workspace_size.
    fn update_workspace_sizes_for_screen(&mut self) {
        let min_width = self.viewport.screen_size.width.max(1920.0);
        let min_height = self.viewport.screen_size.height.max(1080.0);
        
        // Update workspace size - this also recalculates all existing workspace bounds
        self.workspaces.set_workspace_size(Size::new(min_width, min_height));
    }

    /// Pan the viewport - behavior depends on view mode
    ///
    /// - **Workspace mode**: Infinite panning allowed
    /// - **Void mode**: Pan is constrained to keep workspaces visible
    /// - **Transitioning**: Pan is ignored (animation controls viewport)
    pub fn pan(&mut self, dx: f32, dy: f32) {
        match &self.view_mode {
            ViewMode::Workspace { .. } => {
                // Infinite pan within workspace - no constraints
                self.viewport.pan(dx, dy);
                self.last_activity_ms = js_sys::Date::now();
            }
            ViewMode::Void => {
                // In void, constrain pan to keep workspaces visible
                self.viewport.pan(dx, dy);
                self.clamp_viewport_in_void();
                self.last_activity_ms = js_sys::Date::now();
            }
            ViewMode::Transitioning { .. } => {
                // Ignore manual pan during transitions
            }
        }
    }

    /// Zoom the viewport at anchor point - behavior depends on view mode
    ///
    /// - **Workspace mode**: Infinite zoom allowed (zoom in forever)
    /// - **Void mode**: Zoom is constrained (0.1 to 1.0)
    /// - **Transitioning**: Zoom is ignored (animation controls viewport)
    pub fn zoom_at(&mut self, factor: f32, anchor_x: f32, anchor_y: f32) {
        match &self.view_mode {
            ViewMode::Workspace { .. } => {
                // Infinite zoom in workspaces - only clamp to prevent zoom <= 0
                self.viewport.zoom_at(factor, anchor_x, anchor_y);
                // Ensure zoom doesn't go below a minimum (for numerical stability)
                if self.viewport.zoom < 0.001 {
                    self.viewport.zoom = 0.001;
                }
                self.last_activity_ms = js_sys::Date::now();
            }
            ViewMode::Void => {
                // In void, constrain zoom to see workspaces (0.1 to 1.0)
                self.viewport.zoom_at_clamped(factor, anchor_x, anchor_y, 0.1, 1.0);
                self.clamp_viewport_in_void();
                self.last_activity_ms = js_sys::Date::now();
            }
            ViewMode::Transitioning { .. } => {
                // Ignore manual zoom during transitions
            }
        }
    }

    /// Clamp viewport when in void mode to keep workspaces visible.
    /// This only applies to void mode - workspaces have infinite pan/zoom.
    fn clamp_viewport_in_void(&mut self) {
        // Calculate the bounding box of all workspaces
        let workspaces = self.workspaces.workspaces();
        if workspaces.is_empty() {
            return;
        }

        let min_x = workspaces.iter().map(|ws| ws.bounds.x).fold(f32::INFINITY, f32::min);
        let max_x = workspaces.iter().map(|ws| ws.bounds.x + ws.bounds.width).fold(f32::NEG_INFINITY, f32::max);
        let min_y = workspaces.iter().map(|ws| ws.bounds.y).fold(f32::INFINITY, f32::min);
        let max_y = workspaces.iter().map(|ws| ws.bounds.y + ws.bounds.height).fold(f32::NEG_INFINITY, f32::max);

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
    
    /// Legacy method - kept for compatibility but now a no-op in workspace mode
    /// Called during resize to apply appropriate constraints
    fn clamp_viewport_to_workspace(&mut self) {
        match &self.view_mode {
            ViewMode::Workspace { .. } => {
                // No clamping in workspace mode - infinite zoom/pan
            }
            ViewMode::Void => {
                self.clamp_viewport_in_void();
            }
            ViewMode::Transitioning { .. } => {
                // TransitionManager controls viewport during transitions
            }
        }
    }

    /// Tick the transition state machine and update viewport.
    /// Call this each frame during rendering.
    /// Returns true if a transition is active.
    ///
    /// This method also updates `view_mode` when transitions complete:
    /// - EnterVoid completion -> ViewMode::Void
    /// - ExitVoid completion -> ViewMode::Workspace { index }
    /// - SwitchWorkspace completion -> ViewMode::Workspace { index }
    pub fn tick_transition(&mut self) -> bool {
        let now = js_sys::Date::now();
        
        // Get current transition type before ticking (it might complete)
        let transition_type = self.transitions.transition_type();
        
        if let Some(viewport_state) = self.transitions.tick(now) {
            self.viewport.apply_state(viewport_state);
            
            // Check if transition just completed (state was cleared)
            if !self.transitions.is_active() {
                // Update view_mode based on what transition completed
                match transition_type {
                    Some(TransitionType::EnterVoid { .. }) => {
                        self.view_mode = ViewMode::Void;
                        web_sys::console::log_1(&"[desktop] Entered void".into());
                    }
                    Some(TransitionType::ExitVoid { to_workspace }) => {
                        self.view_mode = ViewMode::Workspace { index: to_workspace };
                        // Focus the top window on the target workspace
                        self.focus_top_window_on_workspace(to_workspace);
                        web_sys::console::log_1(&format!(
                            "[desktop] Exited void to workspace {}", to_workspace
                        ).into());
                    }
                    Some(TransitionType::SwitchWorkspace { to_workspace, .. }) => {
                        self.view_mode = ViewMode::Workspace { index: to_workspace };
                        // Focus the top window on the target workspace
                        self.focus_top_window_on_workspace(to_workspace);
                        web_sys::console::log_1(&format!(
                            "[desktop] Switched to workspace {}", to_workspace
                        ).into());
                    }
                    Some(TransitionType::PanToPosition) => {
                        // Pan completed - view_mode unchanged, just updated viewport position
                    }
                    None => {}
                }
                return false; // Transition completed
            }
            
            // Update transition_type in view_mode if transitioning
            if let ViewMode::Transitioning { transition_type: tt, .. } = &mut self.view_mode {
                if let Some(new_tt) = transition_type {
                    *tt = new_tt;
                }
            }
            
            true
        } else {
            // No transition active
            false
        }
    }

    /// Get all windows on the current workspace with their screen-space rectangles
    /// Returns JSON-serializable data for React positioning
    /// 
    /// NOTE: We intentionally do NOT filter by visibility. All windows on the workspace
    /// are always included. This prevents visual glitches (flicker, jumping) when windows
    /// enter/leave the viewport during panning or zooming. The performance impact of
    /// rendering off-screen windows is minimal for typical window counts.
    pub fn get_window_screen_rects(&self) -> Vec<WindowScreenRect> {
        // Determine which workspace's windows to show
        let workspace_index = self.visible_workspace_index();
        
        let workspace = match self.workspaces.workspaces().get(workspace_index) {
            Some(ws) => ws,
            None => return Vec::new(),
        };
        
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
                screen_rect: Rect::new(screen_pos.x, screen_pos.y, screen_size.width, screen_size.height),
            });
        }

        rects
    }

    /// Get the workspace index whose windows should be visible.
    /// During transitions, uses the visual workspace from TransitionManager.
    fn visible_workspace_index(&self) -> usize {
        self.transitions.visual_workspace()
            .unwrap_or_else(|| self.workspaces.active_index())
    }

    /// Create a window and return its ID
    pub fn create_window(&mut self, config: WindowConfig) -> WindowId {
        let id = self.windows.create(config);

        // Add to current workspace
        let active = self.workspaces.active_index();
        self.workspaces.add_window_to_workspace(active, id);

        id
    }

    /// Close a window
    pub fn close_window(&mut self, id: WindowId) {
        self.workspaces.remove_window(id);
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
    fn focus_top_window_on_workspace(&mut self, workspace_index: usize) {
        let workspace = match self.workspaces.workspaces().get(workspace_index) {
            Some(ws) => ws,
            None => return,
        };
        
        // Find the top non-minimized window on this workspace
        let top_window = self.windows.windows_by_z()
            .into_iter()
            .filter(|w| workspace.contains_window(w.id))
            .filter(|w| w.state != WindowState::Minimized)
            .last(); // windows_by_z returns lowest z-order first, so last is top
        
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
        
        // Don't start new pan during other transitions (workspace switches, etc.)
        // But allow interrupting an existing pan with a new one
        if self.transitions.is_active() {
            // Only allow interrupting if it's another pan
            if !matches!(self.transitions.transition_type(), Some(crate::desktop::transition::TransitionType::PanToPosition)) {
                return;
            }
        }
        
        if let Some(window) = self.windows.get(id) {
            // Skip minimized windows (they're not visible anyway)
            if window.state == WindowState::Minimized {
                return;
            }
            
            let window_rect = window.rect();
            let target_center = window_rect.center();
            let now = js_sys::Date::now();
            
            web_sys::console::log_1(&format!(
                "[pan_to_window] id={} pos=({:.0},{:.0}) size=({:.0}x{:.0}) center=({:.0},{:.0}) viewport=({:.0},{:.0})",
                id, window.position.x, window.position.y, 
                window.size.width, window.size.height,
                target_center.x, target_center.y,
                self.viewport.center.x, self.viewport.center.y
            ).into());
            
            // Start animated pan
            self.transitions.pan_to(
                self.viewport.center,
                target_center,
                self.viewport.zoom,
                now,
            );
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
    pub fn start_resize_drag(&mut self, id: WindowId, direction: &str, screen_x: f32, screen_y: f32) {
        // Cancel any pan animation to prevent viewport drift during resize
        self.transitions.cancel();
        
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
            let canvas_pos = self.viewport.screen_to_canvas(Vec2::new(screen_x, screen_y));
            self.input.start_window_resize(
                id,
                handle,
                window.position,
                window.size,
                canvas_pos,
            );
        }
    }

    /// Start a move drag operation from a window's title bar
    /// Called directly by React title bar to bypass hit testing
    pub fn start_move_drag(&mut self, id: WindowId, screen_x: f32, screen_y: f32) {
        // Cancel any pan animation to prevent viewport drift during drag
        // The drag offset is calculated relative to current viewport, so the
        // viewport must stay fixed during the entire drag operation
        self.transitions.cancel();
        
        if let Some(window) = self.windows.get(id) {
            let canvas_pos = self.viewport.screen_to_canvas(Vec2::new(screen_x, screen_y));
            let offset = canvas_pos - window.position;
            self.windows.focus(id);
            self.input.start_window_move(id, offset);
            web_sys::console::log_1(&format!(
                "[start_move_drag] id={} window_pos=({:.0},{:.0}) offset=({:.0},{:.0}) is_dragging={}",
                id, window.position.x, window.position.y, offset.x, offset.y, self.input.is_dragging()
            ).into());
        } else {
            web_sys::console::log_1(&format!("[start_move_drag] window {} not found!", id).into());
        }
    }

    /// Switch to a workspace by index with film-strip zoom animation
    /// Phase 1: Zoom out to show all workspaces
    /// Phase 2: Pan across to target workspace  
    /// Phase 3: Zoom back in
    pub fn switch_workspace(&mut self, index: usize) {
        // Don't switch while user is actively dragging a window
        if self.input.is_dragging() {
            return;
        }
        
        let current_index = self.workspaces.active_index();
        let now = js_sys::Date::now();
        
        // Handle rapid navigation during active transition
        if self.transitions.is_active() {
            // Use saved viewport center if workspace was visited, otherwise use bounds center
            let to_center = self.workspaces.workspaces()
                .get(index)
                .map(|ws| {
                    let saved = ws.viewport.center;
                    // If saved center is non-zero, workspace was visited - use saved position
                    // If zero, either never visited or left at default - use bounds center
                    if saved != Vec2::ZERO {
                        saved
                    } else {
                        ws.bounds.center()
                    }
                })
                .unwrap_or(Vec2::ZERO);
            
            if self.transitions.navigate_to(index, to_center, now) {
                web_sys::console::log_1(&format!(
                    "[workspace] Rapid nav to {}", index
                ).into());
                self.workspaces.switch_to(index);
                // Focus top window on new workspace immediately
                self.focus_top_window_on_workspace(index);
            }
            return;
        }
        
        // Don't switch if already on the target workspace
        if current_index == index {
            return;
        }
        
        // Save current workspace's viewport state before switching
        self.workspaces.save_active_viewport(self.viewport.center, self.viewport.zoom);
        
        // Capture actual viewport state BEFORE starting transition
        // This is where the user is actually looking - critical for correct window filtering
        let current_viewport = self.viewport.to_state();
        
        // Start new transition
        if self.workspaces.switch_to(index) {
            // Focus top window on new workspace immediately so taskbar shows correct focus
            self.focus_top_window_on_workspace(index);
            
            // from_center: use current viewport position (where we actually are)
            let from_center = self.viewport.center;
            
            // to_center: restore saved viewport if workspace was visited, otherwise use bounds center
            let to_center = self.workspaces.workspaces()
                .get(index)
                .map(|ws| {
                    let saved = ws.viewport.center;
                    // If saved center is non-zero, workspace was visited - restore to saved position
                    // If zero, either never visited or left at default - use bounds center
                    if saved != Vec2::ZERO {
                        saved
                    } else {
                        ws.bounds.center()
                    }
                })
                .unwrap_or(Vec2::ZERO);
            
            let total_width = self.calculate_total_workspaces_width();
            
            web_sys::console::log_1(&format!(
                "[workspace] Switching {} -> {}", current_index, index
            ).into());
            
            // Update view_mode to transitioning
            self.view_mode = ViewMode::Transitioning {
                transition_type: TransitionType::SwitchWorkspace {
                    from_workspace: current_index,
                    to_workspace: index,
                },
                progress: 0.0,
            };
            
            self.transitions.start(
                current_index,
                index,
                from_center,
                to_center,
                total_width,
                self.viewport.screen_size.width,
                now,
                current_viewport,
            );
        }
    }
    
    /// Enter the void (zoomed out view showing all workspaces)
    ///
    /// From the void, users can see all workspaces and select one to enter.
    /// This is the "meta-layer" above individual workspaces.
    pub fn enter_void(&mut self) {
        // Don't enter void while user is actively dragging a window
        if self.input.is_dragging() {
            return;
        }
        
        // Can't enter void if already in void or transitioning
        if !self.view_mode.is_workspace() {
            return;
        }
        
        let current_index = match self.view_mode {
            ViewMode::Workspace { index } => index,
            _ => return,
        };
        
        let now = js_sys::Date::now();
        
        // Save current workspace's viewport state
        self.workspaces.save_active_viewport(self.viewport.center, self.viewport.zoom);
        
        // Capture actual viewport state BEFORE starting transition
        let current_viewport = self.viewport.to_state();
        
        // Calculate void center (center of all workspaces)
        let void_center = self.calculate_void_center();
        let total_width = self.calculate_total_workspaces_width();
        
        web_sys::console::log_1(&"[desktop] Entering void".into());
        
        // Update view_mode
        self.view_mode = ViewMode::Transitioning {
            transition_type: TransitionType::EnterVoid { from_workspace: current_index },
            progress: 0.0,
        };
        
        self.transitions.enter_void(
            current_index,
            self.viewport.center,
            void_center,
            total_width,
            self.viewport.screen_size.width,
            now,
            current_viewport,
        );
    }
    
    /// Exit the void into a specific workspace
    ///
    /// # Arguments
    /// * `workspace_index` - The workspace to enter
    pub fn exit_void(&mut self, workspace_index: usize) {
        // Don't exit void while user is actively dragging
        if self.input.is_dragging() {
            return;
        }
        
        // Can't exit void if not in void
        if !self.view_mode.is_void() {
            return;
        }
        
        let now = js_sys::Date::now();
        
        // Restore saved viewport if workspace was visited, otherwise use bounds center
        let to_center = self.workspaces.workspaces()
            .get(workspace_index)
            .map(|ws| {
                let saved = ws.viewport.center;
                // If saved center is non-zero, workspace was visited - restore to saved position
                // If zero, either never visited or left at default - use bounds center
                if saved != Vec2::ZERO {
                    saved
                } else {
                    ws.bounds.center()
                }
            })
            .unwrap_or(Vec2::ZERO);
        
        web_sys::console::log_1(&format!(
            "[desktop] Exiting void to workspace {} | from_center=({:.0}, {:.0}) to_center=({:.0}, {:.0}) zoom={:.3}",
            workspace_index, 
            self.viewport.center.x, self.viewport.center.y,
            to_center.x, to_center.y,
            self.viewport.zoom
        ).into());
        
        // Switch to the target workspace and focus top window immediately
        self.workspaces.switch_to(workspace_index);
        self.focus_top_window_on_workspace(workspace_index);
        
        // Update view_mode
        self.view_mode = ViewMode::Transitioning {
            transition_type: TransitionType::ExitVoid { to_workspace: workspace_index },
            progress: 0.0,
        };
        
        self.transitions.exit_void(
            workspace_index,
            self.viewport.center,
            to_center,
            self.viewport.zoom,
            now,
        );
    }
    
    /// Calculate the center point of all workspaces (for void view)
    fn calculate_void_center(&self) -> Vec2 {
        let workspaces = self.workspaces.workspaces();
        if workspaces.is_empty() {
            return Vec2::ZERO;
        }
        
        let min_x = workspaces.iter().map(|ws| ws.bounds.x).fold(f32::INFINITY, f32::min);
        let max_x = workspaces.iter().map(|ws| ws.bounds.x + ws.bounds.width).fold(f32::NEG_INFINITY, f32::max);
        let min_y = workspaces.iter().map(|ws| ws.bounds.y).fold(f32::INFINITY, f32::min);
        let max_y = workspaces.iter().map(|ws| ws.bounds.y + ws.bounds.height).fold(f32::NEG_INFINITY, f32::max);
        
        Vec2::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0)
    }
    
    /// Get the workspace index that should be rendered visually when zoomed in.
    /// During transitions, this returns the source workspace during ZoomOut/Overview,
    /// and the destination workspace during ZoomIn.
    /// Returns None when in void mode (no single workspace is active).
    pub fn get_visual_active_workspace(&self) -> usize {
        self.transitions.visual_workspace()
            .unwrap_or_else(|| self.workspaces.active_index())
    }
    
    /// Calculate total width of all workspaces on the canvas
    fn calculate_total_workspaces_width(&self) -> f32 {
        let workspaces = self.workspaces.workspaces();
        if workspaces.is_empty() {
            return self.viewport.screen_size.width;
        }
        
        let min_x = workspaces.iter().map(|ws| ws.bounds.x).fold(f32::INFINITY, f32::min);
        let max_x = workspaces.iter().map(|ws| ws.bounds.x + ws.bounds.width).fold(f32::NEG_INFINITY, f32::max);
        
        max_x - min_x
    }
    
    /// Check if a workspace transition is in progress
    pub fn is_transitioning(&self) -> bool {
        self.transitions.is_active()
    }
    
    /// Check if any animation/activity is happening (transitions OR recent pan/zoom)
    /// Used by frontend to determine render framerate
    pub fn is_animating(&self) -> bool {
        // Active transition always means animating
        if self.transitions.is_active() {
            return true;
        }
        // Check for recent manual pan/zoom activity (within 200ms)
        let now = js_sys::Date::now();
        let activity_threshold_ms = 200.0;
        now - self.last_activity_ms < activity_threshold_ms
    }

    /// Check if a viewport animation is in progress (workspace transitions, void enter/exit)
    /// This is distinct from is_animating() which also includes recent pan/zoom activity
    /// Note: PanToPosition animations are excluded - they should not show other workspaces
    pub fn is_animating_viewport(&self) -> bool {
        if !self.transitions.is_active() {
            return false;
        }
        // Only consider workspace-related transitions, not simple pan animations
        matches!(
            self.transitions.transition_type(),
            Some(TransitionType::SwitchWorkspace { .. })
            | Some(TransitionType::EnterVoid { .. })
            | Some(TransitionType::ExitVoid { .. })
        )
    }
    
    /// Get the current transition phase (if any)
    pub fn get_transition_phase(&self) -> Option<TransitionPhase> {
        self.transitions.phase()
    }

    /// Create a new workspace
    pub fn create_workspace(&mut self, name: &str) -> WorkspaceId {
        self.workspaces.create(name)
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
    pub fn handle_pointer_down(&mut self, x: f32, y: f32, button: u8, ctrl: bool, shift: bool) -> InputResult {
        let screen_pos = Vec2::new(x, y);
        let canvas_pos = self.viewport.screen_to_canvas(screen_pos);

        // Middle mouse button starts canvas pan
        if button == 1 {
            self.transitions.cancel(); // Cancel any transition
            self.input.start_pan(screen_pos, self.viewport.center);
            return InputResult::Handled;
        }

        // Ctrl or Shift + primary button also pans (even over windows)
        if button == 0 && (ctrl || shift) {
            self.transitions.cancel(); // Cancel any transition
            self.input.start_pan(screen_pos, self.viewport.center);
            return InputResult::Handled;
        }

        // Primary button - check for window interactions (only in active workspace)
        if button == 0 {
            let active_windows = &self.workspaces.active_workspace().windows;
            let zoom = self.viewport.zoom;
            
            if let Some((window_id, region)) = self.windows.region_at_filtered(canvas_pos, Some(active_windows), zoom) {
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
                        // Cancel any pan animation to prevent viewport drift during drag
                        self.transitions.cancel();
                        self.focus_window(window_id);
                        if let Some(window) = self.windows.get(window_id) {
                            self.input.start_window_move(window_id, canvas_pos - window.position);
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
                        // Cancel any pan animation to prevent viewport drift during resize
                        self.transitions.cancel();
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
                DragState::PanCanvas { start, start_center } => {
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
                    let (new_pos, new_size) = input::calculate_resize(*handle, *start_pos, *start_size, delta);
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
            // Log final position when drag ends
            if let Some(drag_state) = self.input.drag_state() {
                if let DragState::MoveWindow { window_id, .. } = drag_state {
                    if let Some(window) = self.windows.get(*window_id) {
                        web_sys::console::log_1(&format!(
                            "[drag_end] id={} final_pos=({:.0},{:.0})",
                            window_id, window.position.x, window.position.y
                        ).into());
                    }
                }
            }
            self.input.end_drag();
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
        assert_eq!(engine.workspaces.workspaces().len(), 1);
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
        assert_eq!(engine.workspaces.active_workspace().windows.len(), 1);
    }

    // Note: switch_workspace uses js_sys::Date::now() which only works in WASM
    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_desktop_engine_workspace_transition() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);
        
        // Create a second workspace
        engine.create_workspace("Second");
        assert_eq!(engine.workspaces.workspaces().len(), 2);
        
        // Switch workspace
        engine.switch_workspace(1);
        assert!(engine.is_transitioning());
        assert_eq!(engine.workspaces.active_index(), 1);
    }

    #[test]
    fn test_desktop_engine_create_workspace() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);
        
        // Create additional workspaces
        engine.create_workspace("Second");
        engine.create_workspace("Third");
        
        assert_eq!(engine.workspaces.workspaces().len(), 3);
        assert_eq!(engine.workspaces.workspaces()[1].name, "Second");
        assert_eq!(engine.workspaces.workspaces()[2].name, "Third");
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
        engine.workspaces.switch_to(1);
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
        
        assert_eq!(engine.workspaces.active_workspace().windows.len(), 1);
        
        engine.close_window(id);
        
        assert!(engine.windows.get(id).is_none());
        assert_eq!(engine.workspaces.active_workspace().windows.len(), 0);
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
        assert_eq!(engine.windows.get(id).unwrap().state, WindowState::Maximized);
        
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
    fn test_transition_manager_integration() {
        // Test TransitionManager methods directly (no js_sys dependency)
        let mut tm = TransitionManager::new();
        
        assert!(!tm.is_active());
        assert!(tm.visual_workspace().is_none());
        assert!(tm.phase().is_none());
        
        // Start transition
        let from_center = Vec2::new(0.0, 0.0);
        let to_center = Vec2::new(2020.0, 0.0);
        let current_viewport = ViewportState { center: from_center, zoom: 1.0 };
        tm.start(0, 1, from_center, to_center, 4040.0, 1920.0, 0.0, current_viewport);
        
        assert!(tm.is_active());
        assert_eq!(tm.visual_workspace(), Some(0)); // ZoomingOut shows source
        assert_eq!(tm.phase(), Some(TransitionPhase::ZoomingOut));
        
        // Get viewport state without advancing (uses passed time)
        let state = tm.current_viewport_state(0.0).unwrap();
        assert!((state.zoom - 1.0).abs() < 0.01); // Should start at zoom 1.0
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
    fn test_viewport_apply_state() {
        let mut viewport = Viewport::new(1920.0, 1080.0);
        
        let state = ViewportState {
            center: Vec2::new(100.0, 200.0),
            zoom: 0.5,
        };
        
        viewport.apply_state(state);
        
        assert!((viewport.center.x - 100.0).abs() < 0.001);
        assert!((viewport.center.y - 200.0).abs() < 0.001);
        assert!((viewport.zoom - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_viewport_clamping_prevents_leaving_workspace() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);
        
        // At zoom 1.0 with screen == workspace, visible area fills workspace exactly
        // So center should stay at workspace center (0, 0)
        
        // Try to pan far to the right
        engine.pan(-5000.0, 0.0);
        
        // Center should stay at workspace center since visible area == workspace
        assert!((engine.viewport.center.x - 0.0).abs() < 1.0, 
            "Center X should stay at 0 when visible area fills workspace, got {}", engine.viewport.center.x);
    }

    #[test]
    fn test_viewport_clamping_allows_pan_when_zoomed_in() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);
        
        // Zoom in first - now we can see less and should be able to pan
        engine.zoom_at(2.0, 960.0, 540.0);
        
        // At zoom 2.0, visible area is 960x540
        // Workspace bounds: x: -960 to 960, y: -540 to 540
        // Center can range from: x: -960+480 to 960-480 = -480 to 480
        
        // Pan right
        engine.pan(-200.0, 0.0);
        assert!(engine.viewport.center.x > 50.0, 
            "Should be able to pan right when zoomed in, got {}", engine.viewport.center.x);
        
        // Pan should stop at calculated edge
        engine.pan(-10000.0, 0.0);
        assert!(engine.viewport.center.x <= 480.0 + 1.0,
            "Should be clamped so visible area stays in workspace, got {}", engine.viewport.center.x);
    }

    #[test]
    fn test_viewport_zoom_clamped_to_workspace() {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);
        
        // Try to zoom out - should be prevented since screen == workspace at 1920x1080
        engine.zoom_at(0.5, 960.0, 540.0);
        
        // Zoom should be clamped to 1.0 (min zoom = screen_size / workspace_size = 1.0)
        assert!((engine.viewport.zoom - 1.0).abs() < 0.01,
            "Zoom should be clamped to 1.0 to prevent seeing outside workspace, got {}", engine.viewport.zoom);
        
        // Zooming in should still work
        engine.zoom_at(2.0, 960.0, 540.0);
        assert!((engine.viewport.zoom - 2.0).abs() < 0.01,
            "Should be able to zoom in, got {}", engine.viewport.zoom);
    }
}
