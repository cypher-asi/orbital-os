//! Transition and crossfade state management

use super::DesktopEngine;
use crate::transition::{Crossfade, CrossfadeDirection};
use crate::view_mode::ViewMode;

impl DesktopEngine {
    /// Get the current crossfade transition
    pub fn crossfade(&self) -> Option<&Crossfade> {
        self.crossfade.as_ref()
    }

    /// Check if a crossfade is active
    pub fn is_crossfading(&self) -> bool {
        self.crossfade.is_some()
    }

    /// Check if any transition is active
    pub fn is_transitioning(&self) -> bool {
        self.is_crossfading() || self.camera_animation.is_some()
    }

    /// Check if void layer should be visible (during void mode or void transitions)
    pub fn should_show_void(&self) -> bool {
        match &self.view_mode {
            ViewMode::Void => true,
            ViewMode::Desktop { .. } => {
                // Only show void during transitions TO or FROM void, not during desktop switches
                if let Some(ref crossfade) = self.crossfade {
                    matches!(
                        crossfade.direction,
                        CrossfadeDirection::ToVoid | CrossfadeDirection::ToDesktop
                    )
                } else {
                    false
                }
            }
        }
    }

    /// Get layer opacities (desktop_opacity, void_opacity)
    pub fn layer_opacities(&self, now_ms: f64) -> (f32, f32) {
        if let Some(ref crossfade) = self.crossfade {
            crossfade.opacities(now_ms)
        } else {
            match self.view_mode {
                ViewMode::Desktop { .. } => (1.0, 0.0),
                ViewMode::Void => (0.0, 1.0),
            }
        }
    }

    /// Get the current view mode
    pub fn get_view_mode(&self) -> &ViewMode {
        &self.view_mode
    }

    /// Check if in void mode
    pub fn is_in_void(&self) -> bool {
        self.view_mode.is_void()
    }

    /// Tick transitions, returns true if any transition is active
    pub fn tick_transition(&mut self, now_ms: f64) -> bool {
        if self.tick_crossfade(now_ms) {
            return self.camera_animation.is_some() || self.is_crossfading();
        }

        self.tick_camera_animation(now_ms)
    }

    /// Tick the crossfade transition, returns true if crossfade just completed
    fn tick_crossfade(&mut self, now_ms: f64) -> bool {
        let crossfade = match &self.crossfade {
            Some(cf) => cf,
            None => return false,
        };

        // For desktop switches: sync viewport to match the currently visible desktop
        if matches!(crossfade.direction, CrossfadeDirection::SwitchDesktop) {
            let visual_workspace = self.get_visual_active_workspace_at(now_ms);
            if let Some(saved) = self.desktops.get_desktop_camera(visual_workspace) {
                self.viewport.center = saved.center;
                self.viewport.zoom = saved.zoom;
            }
        }

        if !crossfade.is_complete(now_ms) {
            return false;
        }

        // Crossfade completed - apply final state
        self.apply_crossfade_completion();
        true
    }

    /// Apply the final state when a crossfade completes
    fn apply_crossfade_completion(&mut self) {
        let crossfade = match self.crossfade.take() {
            Some(cf) => cf,
            None => return,
        };

        match crossfade.direction {
            CrossfadeDirection::ToVoid => {
                self.view_mode = ViewMode::Void;
                self.viewport.center = self.void_state.camera().center;
                self.viewport.zoom = self.void_state.camera().zoom;
            }
            CrossfadeDirection::ToDesktop | CrossfadeDirection::SwitchDesktop => {
                let index = crossfade.target_desktop.unwrap_or(0);
                self.view_mode = ViewMode::Desktop { index };
                if let Some(saved) = self.desktops.get_desktop_camera(index) {
                    self.viewport.center = saved.center;
                    self.viewport.zoom = saved.zoom;
                }
                self.focus_top_window_on_desktop(index);
            }
        }
    }

    /// Tick the camera animation, returns true if any animation is active
    fn tick_camera_animation(&mut self, now_ms: f64) -> bool {
        let animation = match &self.camera_animation {
            Some(anim) => anim,
            None => return self.is_crossfading(),
        };

        if animation.is_complete(now_ms) {
            let final_camera = animation.final_camera();
            self.viewport.center = final_camera.center;
            self.viewport.zoom = final_camera.zoom;
            self.camera_animation = None;
            self.is_crossfading()
        } else {
            let current = animation.current(now_ms);
            self.viewport.center = current.center;
            self.viewport.zoom = current.zoom;
            true
        }
    }

    /// Check if any animation/activity is happening
    pub fn is_animating(&self, now_ms: f64) -> bool {
        if self.is_crossfading() || self.camera_animation.is_some() {
            return true;
        }
        // Check for recent manual pan/zoom activity (within 200ms)
        let activity_threshold_ms = 200.0;
        now_ms - self.last_activity_ms < activity_threshold_ms
    }

    /// Check if a viewport animation is in progress
    pub fn is_animating_viewport(&self) -> bool {
        self.is_crossfading()
    }

    /// Mark viewport activity (for is_animating check)
    pub fn mark_activity(&mut self, now_ms: f64) {
        self.last_activity_ms = now_ms;
    }

    /// Get the workspace index that should be rendered visually
    pub fn get_visual_active_workspace_at(&self, now_ms: f64) -> usize {
        if let Some(ref crossfade) = self.crossfade {
            self.visual_workspace_during_crossfade(crossfade, now_ms)
        } else {
            self.desktops.active_index()
        }
    }

    /// Determine visual workspace during a crossfade
    fn visual_workspace_during_crossfade(&self, crossfade: &Crossfade, now_ms: f64) -> usize {
        match crossfade.direction {
            CrossfadeDirection::SwitchDesktop => {
                // During desktop switch: show source in first half, target in second half
                let progress = crossfade.progress(now_ms);
                if progress < 0.5 {
                    crossfade
                        .source_desktop
                        .unwrap_or_else(|| self.desktops.active_index())
                } else {
                    crossfade
                        .target_desktop
                        .unwrap_or_else(|| self.desktops.active_index())
                }
            }
            _ => {
                // For other transitions, use target if available
                crossfade
                    .target_desktop
                    .unwrap_or_else(|| self.desktops.active_index())
            }
        }
    }

    /// Get the workspace index that should be rendered visually (uses current time estimate)
    pub fn get_visual_active_workspace(&self) -> usize {
        self.get_visual_active_workspace_at(self.last_activity_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Size, Vec2};
    use crate::transition::CROSSFADE_DURATION_MS;
    use crate::window::WindowConfig;

    fn create_test_engine() -> DesktopEngine {
        let mut engine = DesktopEngine::new();
        engine.init(1920.0, 1080.0);
        engine
    }

    #[test]
    fn test_is_crossfading_false_initially() {
        let engine = create_test_engine();
        assert!(!engine.is_crossfading());
    }

    #[test]
    fn test_is_crossfading_true_during_transition() {
        let mut engine = create_test_engine();
        engine.enter_void(0.0);

        assert!(engine.is_crossfading());
    }

    #[test]
    fn test_is_transitioning_includes_camera_animation() {
        let mut engine = create_test_engine();

        // Create window far away
        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(5000.0, 5000.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        // Pan to window starts camera animation
        engine.pan_to_window(id, 0.0);

        assert!(engine.is_transitioning());
        assert!(engine.camera_animation.is_some());
    }

    #[test]
    fn test_should_show_void_in_desktop_mode() {
        let engine = create_test_engine();
        assert!(!engine.should_show_void());
    }

    #[test]
    fn test_should_show_void_during_void_transition() {
        let mut engine = create_test_engine();
        engine.enter_void(0.0);

        assert!(engine.should_show_void());
    }

    #[test]
    fn test_should_show_void_not_during_desktop_switch() {
        let mut engine = create_test_engine();
        engine.create_desktop("Second");

        // Start desktop switch
        engine.switch_desktop(1, 0.0);

        // Should NOT show void during desktop switch
        assert!(!engine.should_show_void());
        assert!(engine.is_crossfading());
    }

    #[test]
    fn test_layer_opacities_desktop_mode() {
        let engine = create_test_engine();
        let (desktop, void) = engine.layer_opacities(0.0);

        assert!((desktop - 1.0).abs() < 0.001);
        assert!((void - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_layer_opacities_void_mode() {
        let mut engine = create_test_engine();
        engine.enter_void(0.0);

        // Complete transition
        let end_time = CROSSFADE_DURATION_MS as f64 + 100.0;
        engine.tick_transition(end_time);

        let (desktop, void) = engine.layer_opacities(end_time);
        assert!((desktop - 0.0).abs() < 0.001);
        assert!((void - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_tick_transition_completes_crossfade() {
        let mut engine = create_test_engine();
        engine.enter_void(0.0);

        assert!(engine.is_crossfading());

        // Tick past duration
        let end_time = CROSSFADE_DURATION_MS as f64 + 100.0;
        engine.tick_transition(end_time);

        assert!(!engine.is_crossfading());
        assert!(matches!(engine.view_mode, ViewMode::Void));
    }

    #[test]
    fn test_tick_transition_completes_camera_animation() {
        let mut engine = create_test_engine();

        let id = engine.create_window(WindowConfig {
            title: "Test".to_string(),
            position: Some(Vec2::new(5000.0, 5000.0)),
            size: Size::new(800.0, 600.0),
            app_id: "test".to_string(),
            ..Default::default()
        });

        engine.pan_to_window(id, 0.0);
        assert!(engine.camera_animation.is_some());

        // Tick past animation duration
        use crate::transition::CAMERA_ANIMATION_DURATION_MS;
        let end_time = CAMERA_ANIMATION_DURATION_MS as f64 + 100.0;
        engine.tick_transition(end_time);

        assert!(engine.camera_animation.is_none());
    }

    #[test]
    fn test_is_animating_true_during_transition() {
        let mut engine = create_test_engine();
        engine.enter_void(0.0);

        assert!(engine.is_animating(50.0));
    }

    #[test]
    fn test_is_animating_true_after_recent_activity() {
        let mut engine = create_test_engine();
        engine.mark_activity(100.0);

        // Should still be considered animating shortly after activity
        assert!(engine.is_animating(150.0));

        // Should no longer be animating after threshold
        assert!(!engine.is_animating(500.0));
    }

    #[test]
    fn test_get_visual_active_workspace_during_switch() {
        let mut engine = create_test_engine();
        engine.create_desktop("Second");

        // Start switch from 0 to 1
        engine.switch_desktop(1, 0.0);

        // At start (< 50%), should show source desktop
        let visual = engine.get_visual_active_workspace_at(50.0);
        assert_eq!(visual, 0);

        // After midpoint (> 50%), should show target desktop
        use crate::transition::DESKTOP_SWITCH_DURATION_MS;
        let visual =
            engine.get_visual_active_workspace_at((DESKTOP_SWITCH_DURATION_MS / 2 + 50) as f64);
        assert_eq!(visual, 1);
    }

    #[test]
    fn test_crossfade_to_void_updates_viewport() {
        let mut engine = create_test_engine();

        // Set specific viewport
        engine.viewport.center = Vec2::new(100.0, 100.0);
        engine.viewport.zoom = 2.0;

        engine.enter_void(0.0);

        // Complete transition
        let end_time = CROSSFADE_DURATION_MS as f64 + 100.0;
        engine.tick_transition(end_time);

        // Viewport should be updated to void camera
        let void_camera = engine.void_state.camera();
        assert!((engine.viewport.center.x - void_camera.center.x).abs() < 0.001);
        assert!((engine.viewport.zoom - void_camera.zoom).abs() < 0.001);
    }

    #[test]
    fn test_crossfade_to_desktop_restores_camera() {
        let mut engine = create_test_engine();
        engine.create_desktop("Second");

        // Save camera for desktop 0
        engine.viewport.center = Vec2::new(500.0, 500.0);
        engine.viewport.zoom = 1.5;
        engine.switch_desktop(1, 0.0);

        // Complete desktop switch
        let mut time = 500.0;
        engine.tick_transition(time);

        // Enter and exit void
        engine.enter_void(time);
        time += CROSSFADE_DURATION_MS as f64 + 100.0;
        engine.tick_transition(time);

        engine.exit_void(0, time);
        time += CROSSFADE_DURATION_MS as f64 + 100.0;
        engine.tick_transition(time);

        // Should be back on desktop 0 with saved camera
        assert_eq!(engine.desktops.active_index(), 0);
        assert!((engine.viewport.center.x - 500.0).abs() < 1.0);
    }

    #[test]
    fn test_is_animating_viewport() {
        let mut engine = create_test_engine();

        assert!(!engine.is_animating_viewport());

        engine.enter_void(0.0);

        assert!(engine.is_animating_viewport());
    }
}
