//! Viewport Animation and Transition Management
//!
//! Provides smooth animated transitions by interpolating both center AND zoom together.
//! This eliminates the jumping bug that occurred when animating them separately.
//!
//! The TransitionManager handles workspace-switching animations with a three-phase
//! state machine: ZoomOut -> Panning -> ZoomIn.

use super::types::Vec2;

/// Interpolated viewport state from animation
#[derive(Clone, Copy, Debug)]
pub struct ViewportState {
    pub center: Vec2,
    pub zoom: f32,
}

/// Transition phase during workspace switching
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionPhase {
    /// Zooming out from source workspace
    ZoomingOut,
    /// Panning between workspaces (at overview zoom)
    Panning,
    /// Zooming into target workspace
    ZoomingIn,
}

/// Type of transition being performed
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransitionType {
    /// Switching from one workspace to another
    SwitchWorkspace {
        from_workspace: usize,
        to_workspace: usize,
    },
    /// Entering the void (zooming out to see all workspaces)
    EnterVoid {
        from_workspace: usize,
    },
    /// Exiting the void into a workspace
    ExitVoid {
        to_workspace: usize,
    },
    /// Panning to a specific position (e.g., to center on a window)
    PanToPosition,
}

/// Simple viewport animation - interpolates from current state to target
#[derive(Clone, Copy, Debug)]
pub struct ViewportAnimation {
    pub from_center: Vec2,
    pub to_center: Vec2,
    pub from_zoom: f32,
    pub to_zoom: f32,
    pub start_time: f64,
    pub duration_ms: f32,
}

impl ViewportAnimation {
    /// Create a new animation
    pub fn new(
        from_center: Vec2,
        to_center: Vec2,
        from_zoom: f32,
        to_zoom: f32,
        start_time: f64,
        duration_ms: f32,
    ) -> Self {
        Self {
            from_center,
            to_center,
            from_zoom,
            to_zoom,
            start_time,
            duration_ms,
        }
    }

    /// Tick the animation, returns interpolated state.
    /// Returns None when animation is complete.
    pub fn tick(&self, now: f64) -> Option<ViewportState> {
        let elapsed = (now - self.start_time) as f32;
        let t = (elapsed / self.duration_ms).clamp(0.0, 1.0);

        if t >= 1.0 {
            return None; // Animation complete
        }

        let eased = ease_out_cubic(t);
        Some(ViewportState {
            center: Vec2::new(
                self.from_center.x + (self.to_center.x - self.from_center.x) * eased,
                self.from_center.y + (self.to_center.y - self.from_center.y) * eased,
            ),
            zoom: self.from_zoom + (self.to_zoom - self.from_zoom) * eased,
        })
    }

    /// Get the final state (target values)
    pub fn final_state(&self) -> ViewportState {
        ViewportState {
            center: self.to_center,
            zoom: self.to_zoom,
        }
    }

    /// Get progress (0.0 to 1.0)
    pub fn progress(&self, now: f64) -> f32 {
        let elapsed = (now - self.start_time) as f32;
        (elapsed / self.duration_ms).clamp(0.0, 1.0)
    }
}

/// Ease-out cubic - smooth deceleration
fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

// =============================================================================
// Transition Manager - Handles workspace switch animations
// =============================================================================

/// Animation duration constants (in milliseconds)
const ZOOM_OUT_DURATION: f32 = 200.0;
const PAN_DURATION: f32 = 250.0;
const ZOOM_IN_DURATION: f32 = 200.0;
const PAN_TO_WINDOW_DURATION: f32 = 300.0;
const VOID_TRANSITION_DURATION: f32 = 300.0;

/// Overview zoom level (how far to zoom out during transitions)
const OVERVIEW_ZOOM: f32 = 0.4;

/// Manager for viewport transitions (workspace switching, void enter/exit)
///
/// This handles the three-phase animation for switching workspaces:
/// 1. ZoomOut - Zoom out from current workspace to overview
/// 2. Panning - Pan across to target workspace
/// 3. ZoomIn - Zoom back into target workspace
pub struct TransitionManager {
    /// Current animation (if any)
    animation: Option<ViewportAnimation>,
    /// Current phase of the transition
    phase: Option<TransitionPhase>,
    /// Type of transition being performed
    transition_type: Option<TransitionType>,
    /// Source workspace index
    from_workspace: usize,
    /// Target workspace index  
    to_workspace: usize,
    /// Center position of source workspace
    from_center: Vec2,
    /// Center position of target workspace
    to_center: Vec2,
    /// Total width of all workspaces (for calculating overview zoom)
    total_width: f32,
    /// Screen width (for calculating overview zoom)
    screen_width: f32,
    /// Source workspace viewport (zoom 1.0, source center).
    /// Used for visibility filtering during ZoomOut/Panning phases.
    /// Windows visible in this viewport are shown when displaying source workspace.
    source_viewport: Option<ViewportState>,
    /// Target workspace viewport (zoom 1.0, target center).
    /// Used for visibility filtering during ZoomIn phase.
    /// Windows visible in this viewport are shown when displaying target workspace.
    target_viewport: Option<ViewportState>,
}

impl Default for TransitionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TransitionManager {
    /// Create a new transition manager
    pub fn new() -> Self {
        Self {
            animation: None,
            phase: None,
            transition_type: None,
            from_workspace: 0,
            to_workspace: 0,
            from_center: Vec2::ZERO,
            to_center: Vec2::ZERO,
            total_width: 0.0,
            screen_width: 0.0,
            source_viewport: None,
            target_viewport: None,
        }
    }

    /// Start a workspace switch transition
    /// 
    /// Captures both source and target viewports for phase-aware visibility filtering:
    /// - Source viewport: the actual viewport state where user was looking (preserves their pan/zoom)
    /// - Target viewport: where user will end up (target workspace center at zoom 1.0)
    /// 
    /// The source viewport must reflect where the user was ACTUALLY looking, not the workspace
    /// center, otherwise windows they had panned away from will incorrectly appear.
    pub fn start(
        &mut self,
        from_workspace: usize,
        to_workspace: usize,
        from_center: Vec2,
        to_center: Vec2,
        total_width: f32,
        screen_width: f32,
        now: f64,
        current_viewport: ViewportState,
    ) {
        self.from_workspace = from_workspace;
        self.to_workspace = to_workspace;
        self.from_center = from_center;
        self.to_center = to_center;
        self.total_width = total_width;
        self.screen_width = screen_width;
        
        // Capture source viewport: where user was ACTUALLY looking
        // This is critical - using workspace center would include off-screen windows!
        self.source_viewport = Some(current_viewport);
        
        // Capture target viewport: where we're going (zoom 1.0, target center)
        self.target_viewport = Some(ViewportState {
            center: to_center,
            zoom: 1.0,
        });
        
        self.transition_type = Some(TransitionType::SwitchWorkspace {
            from_workspace,
            to_workspace,
        });

        // Start with zoom-out phase
        self.start_zoom_out(now);
    }

    /// Get the viewport to use for visibility filtering.
    /// 
    /// Returns the stable viewport for the DESTINATION of the current animation phase.
    /// This ensures windows are filtered based on where they would be visible when
    /// the current phase completes:
    /// 
    /// - ZoomingOut/Panning: Use source viewport (we're showing source workspace windows)
    /// - ZoomingIn: Use target viewport (we're showing target workspace windows)
    /// - PanToPosition: None (use current viewport, handled by caller)
    /// 
    /// The key insight is that during ZoomingIn, we show target workspace windows,
    /// so we need to filter based on the target workspace's coordinate space.
    pub fn get_visibility_viewport(&self) -> Option<ViewportState> {
        if !self.is_active() {
            return None;
        }
        
        // PanToPosition uses current viewport for visibility (windows slide into view naturally)
        if matches!(self.transition_type, Some(TransitionType::PanToPosition)) {
            return None;
        }
        
        match self.phase {
            Some(TransitionPhase::ZoomingOut) | Some(TransitionPhase::Panning) => {
                // During zoom-out and panning, we show source workspace windows
                // Filter using source workspace viewport at zoom 1.0
                self.source_viewport
            }
            Some(TransitionPhase::ZoomingIn) => {
                // During zoom-in, we show target workspace windows
                // Filter using target workspace viewport at zoom 1.0
                self.target_viewport
            }
            None => None,
        }
    }

    /// Enter the void (zoom out to overview showing all workspaces)
    /// 
    /// Captures source viewport for visibility filtering during zoom-out.
    /// This ensures off-screen windows don't suddenly appear during the transition.
    /// 
    /// `current_viewport` must be the actual viewport state where user was looking.
    pub fn enter_void(
        &mut self,
        from_workspace: usize,
        from_center: Vec2,
        void_center: Vec2,
        total_width: f32,
        screen_width: f32,
        now: f64,
        current_viewport: ViewportState,
    ) {
        self.from_workspace = from_workspace;
        self.to_workspace = from_workspace; // Stay on same logical workspace
        self.from_center = from_center;
        self.to_center = void_center;
        self.total_width = total_width;
        self.screen_width = screen_width;
        
        // Capture source viewport: where user was ACTUALLY looking
        self.source_viewport = Some(current_viewport);
        
        // No target viewport needed for enter_void (we're going to overview, not a workspace)
        self.target_viewport = None;
        
        self.transition_type = Some(TransitionType::EnterVoid { from_workspace });

        // Calculate target zoom to see all workspaces
        let target_zoom = (screen_width / total_width).min(0.5).max(0.1);

        // Single animation: zoom out and pan to void center
        self.animation = Some(ViewportAnimation::new(
            current_viewport.center,
            void_center,
            current_viewport.zoom,
            target_zoom,
            now,
            VOID_TRANSITION_DURATION,
        ));
        self.phase = Some(TransitionPhase::ZoomingOut);
    }

    /// Exit the void into a workspace
    /// 
    /// Captures target viewport for visibility filtering during zoom-in.
    /// This ensures only windows that will be visible at zoom 1.0 are shown.
    pub fn exit_void(
        &mut self,
        to_workspace: usize,
        from_center: Vec2,
        to_center: Vec2,
        current_zoom: f32,
        now: f64,
    ) {
        self.from_workspace = to_workspace;
        self.to_workspace = to_workspace;
        self.from_center = from_center;
        self.to_center = to_center;
        
        // No source viewport needed for exit_void (we're coming from overview, not a workspace)
        self.source_viewport = None;
        
        // Capture target viewport: where we're going (zoom 1.0, target center)
        self.target_viewport = Some(ViewportState {
            center: to_center,
            zoom: 1.0,
        });
        
        self.transition_type = Some(TransitionType::ExitVoid { to_workspace });

        // Single animation: pan to workspace and zoom in
        self.animation = Some(ViewportAnimation::new(
            from_center,
            to_center,
            current_zoom,
            1.0,
            now,
            VOID_TRANSITION_DURATION,
        ));
        self.phase = Some(TransitionPhase::ZoomingIn);
    }

    /// Pan to a specific position (for centering on windows)
    pub fn pan_to(
        &mut self,
        from_center: Vec2,
        to_center: Vec2,
        current_zoom: f32,
        now: f64,
    ) {
        self.from_center = from_center;
        self.to_center = to_center;
        self.transition_type = Some(TransitionType::PanToPosition);

        // Simple pan animation
        self.animation = Some(ViewportAnimation::new(
            from_center,
            to_center,
            current_zoom,
            current_zoom, // Keep same zoom
            now,
            PAN_TO_WINDOW_DURATION,
        ));
        self.phase = Some(TransitionPhase::Panning);
    }

    /// Get the pan animation bounds (from_center to to_center) for PanToPosition transitions.
    /// Returns None if not a pan transition.
    /// Used to expand visibility rect to include windows at both start and end of pan.
    pub fn get_pan_bounds(&self) -> Option<(Vec2, Vec2)> {
        if matches!(self.transition_type, Some(TransitionType::PanToPosition)) {
            Some((self.from_center, self.to_center))
        } else {
            None
        }
    }

    /// Handle rapid navigation to a new workspace during an active transition
    /// Returns true if navigation was handled
    pub fn navigate_to(&mut self, index: usize, to_center: Vec2, now: f64) -> bool {
        if !self.is_active() {
            return false;
        }

        // Update target
        self.to_workspace = index;
        self.to_center = to_center;

        // If we're in zoom-out or panning, just update the target
        // The animation will naturally transition to the new target
        if let Some(TransitionType::SwitchWorkspace { from_workspace, .. }) = self.transition_type {
            self.transition_type = Some(TransitionType::SwitchWorkspace {
                from_workspace,
                to_workspace: index,
            });
        }

        // If we're panning or zooming in, restart from current position
        if matches!(self.phase, Some(TransitionPhase::Panning) | Some(TransitionPhase::ZoomingIn)) {
            if let Some(anim) = &self.animation {
                let current = anim.tick(now).unwrap_or_else(|| anim.final_state());
                self.start_pan_phase(current.center, now);
            }
        }

        true
    }

    /// Tick the transition, advancing the animation.
    /// Returns the current viewport state, or None if no transition is active.
    pub fn tick(&mut self, now: f64) -> Option<ViewportState> {
        let animation = self.animation.as_ref()?;

        // Check if current animation phase is complete
        if let Some(state) = animation.tick(now) {
            // Animation still in progress
            Some(state)
        } else {
            // Current phase complete, advance to next
            let final_state = animation.final_state();
            self.advance_phase(now);

            if self.animation.is_some() {
                // New phase started, return its initial state
                self.tick(now)
            } else {
                // Transition complete - clear both viewports
                self.source_viewport = None;
                self.target_viewport = None;
                Some(final_state)
            }
        }
    }

    /// Get current viewport state without advancing
    pub fn current_viewport_state(&self, now: f64) -> Option<ViewportState> {
        self.animation.as_ref().and_then(|a| {
            a.tick(now).or_else(|| Some(a.final_state()))
        })
    }

    /// Check if a transition is currently active
    pub fn is_active(&self) -> bool {
        self.animation.is_some()
    }

    /// Get the current phase
    pub fn phase(&self) -> Option<TransitionPhase> {
        self.phase
    }

    /// Get the transition type
    pub fn transition_type(&self) -> Option<TransitionType> {
        self.transition_type
    }

    /// Get the visual workspace index (which workspace to render)
    /// During zoom-out/pan: source workspace
    /// During zoom-in: target workspace
    /// For PanToPosition: None (use current workspace, not transition workspace)
    pub fn visual_workspace(&self) -> Option<usize> {
        // PanToPosition doesn't change which workspace is visible - it just pans within the current one
        if matches!(self.transition_type, Some(TransitionType::PanToPosition)) {
            return None;
        }
        
        match self.phase {
            Some(TransitionPhase::ZoomingOut) | Some(TransitionPhase::Panning) => {
                Some(self.from_workspace)
            }
            Some(TransitionPhase::ZoomingIn) => Some(self.to_workspace),
            None => None,
        }
    }

    /// Cancel the current transition
    pub fn cancel(&mut self) {
        self.animation = None;
        self.phase = None;
        self.transition_type = None;
        self.source_viewport = None;
        self.target_viewport = None;
    }

    // =========================================================================
    // Private methods
    // =========================================================================

    fn start_zoom_out(&mut self, now: f64) {
        let target_zoom = self.calculate_overview_zoom();

        self.animation = Some(ViewportAnimation::new(
            self.from_center,
            self.from_center, // Stay centered on source during zoom-out
            1.0,
            target_zoom,
            now,
            ZOOM_OUT_DURATION,
        ));
        self.phase = Some(TransitionPhase::ZoomingOut);
    }

    fn start_pan_phase(&mut self, from_center: Vec2, now: f64) {
        let zoom = self.calculate_overview_zoom();

        self.animation = Some(ViewportAnimation::new(
            from_center,
            self.to_center,
            zoom,
            zoom,
            now,
            PAN_DURATION,
        ));
        self.phase = Some(TransitionPhase::Panning);
    }

    fn start_zoom_in(&mut self, now: f64) {
        let from_zoom = self.calculate_overview_zoom();

        self.animation = Some(ViewportAnimation::new(
            self.to_center,
            self.to_center, // Stay centered on target during zoom-in
            from_zoom,
            1.0,
            now,
            ZOOM_IN_DURATION,
        ));
        self.phase = Some(TransitionPhase::ZoomingIn);
    }

    fn advance_phase(&mut self, now: f64) {
        match self.phase {
            Some(TransitionPhase::ZoomingOut) => {
                // After zoom-out, start panning (unless it's an EnterVoid)
                if matches!(self.transition_type, Some(TransitionType::EnterVoid { .. })) {
                    // EnterVoid complete
                    self.animation = None;
                    self.phase = None;
                    self.transition_type = None;
                } else {
                    self.start_pan_phase(self.from_center, now);
                }
            }
            Some(TransitionPhase::Panning) => {
                // After panning, start zoom-in (unless it's PanToPosition)
                if matches!(self.transition_type, Some(TransitionType::PanToPosition)) {
                    // Pan complete
                    self.animation = None;
                    self.phase = None;
                    self.transition_type = None;
                } else {
                    self.start_zoom_in(now);
                }
            }
            Some(TransitionPhase::ZoomingIn) | None => {
                // Transition complete
                self.animation = None;
                self.phase = None;
                self.transition_type = None;
            }
        }
    }

    fn calculate_overview_zoom(&self) -> f32 {
        if self.total_width <= 0.0 {
            return OVERVIEW_ZOOM;
        }

        // Calculate zoom to fit all workspaces
        let fit_zoom = self.screen_width / self.total_width;

        // Use the smaller of fit_zoom and OVERVIEW_ZOOM, but not too small
        fit_zoom.min(OVERVIEW_ZOOM).max(0.15)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_animation_interpolates_both() {
        let anim = ViewportAnimation::new(
            Vec2::new(0.0, 0.0),
            Vec2::new(1000.0, 0.0),
            0.4,
            1.0,
            0.0,
            300.0,
        );

        // At midpoint, both center AND zoom should be interpolated
        let state = anim.tick(150.0).unwrap();
        
        assert!(state.center.x > 0.0, "Center should have moved");
        assert!(state.center.x < 1000.0, "Center should not have jumped to target");
        assert!(state.zoom > 0.4, "Zoom should have increased");
        assert!(state.zoom < 1.0, "Zoom should not have jumped to target");
    }

    #[test]
    fn test_animation_completes() {
        let anim = ViewportAnimation::new(
            Vec2::new(0.0, 0.0),
            Vec2::new(100.0, 0.0),
            0.5,
            1.0,
            0.0,
            300.0,
        );

        // Before completion, should return Some
        assert!(anim.tick(299.0).is_some());
        
        // At/after completion, should return None
        assert!(anim.tick(300.0).is_none());
        assert!(anim.tick(500.0).is_none());
    }

    #[test]
    fn test_final_state() {
        let anim = ViewportAnimation::new(
            Vec2::new(0.0, 0.0),
            Vec2::new(100.0, 200.0),
            0.5,
            1.0,
            0.0,
            300.0,
        );

        let final_state = anim.final_state();
        assert!((final_state.center.x - 100.0).abs() < 0.001);
        assert!((final_state.center.y - 200.0).abs() < 0.001);
        assert!((final_state.zoom - 1.0).abs() < 0.001);
    }
}
