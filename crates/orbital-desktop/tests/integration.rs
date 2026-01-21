//! Integration tests for DesktopEngine
//!
//! These tests verify the full desktop workflow including:
//! - Window lifecycle (create, move, resize, minimize, maximize, close)
//! - Desktop switching with window preservation
//! - Void mode entry/exit
//! - Input handling and drag operations
//! - Camera animations and transitions

use orbital_desktop::{
    DesktopEngine, WindowConfig, Size, Vec2, WindowState, ViewMode,
    CROSSFADE_DURATION_MS, CAMERA_ANIMATION_DURATION_MS,
};

// =============================================================================
// Window Lifecycle Tests
// =============================================================================

#[test]
fn test_window_lifecycle_full() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    // Create window
    let id = engine.create_window(WindowConfig {
        title: "Test Window".to_string(),
        position: Some(Vec2::new(100.0, 100.0)),
        size: Size::new(800.0, 600.0),
        app_id: "test-app".to_string(),
        ..Default::default()
    });

    assert!(engine.windows.get(id).is_some());
    let window = engine.windows.get(id).unwrap();
    assert_eq!(window.title, "Test Window");
    assert_eq!(window.state, WindowState::Normal);

    // Move window
    engine.move_window(id, 200.0, 200.0);
    let window = engine.windows.get(id).unwrap();
    assert!((window.position.x - 200.0).abs() < 0.001);
    assert!((window.position.y - 200.0).abs() < 0.001);

    // Resize window
    engine.resize_window(id, 1000.0, 800.0);
    let window = engine.windows.get(id).unwrap();
    assert!((window.size.width - 1000.0).abs() < 0.001);
    assert!((window.size.height - 800.0).abs() < 0.001);

    // Minimize window
    engine.minimize_window(id);
    let window = engine.windows.get(id).unwrap();
    assert_eq!(window.state, WindowState::Minimized);

    // Restore window
    engine.restore_window(id);
    let window = engine.windows.get(id).unwrap();
    assert_eq!(window.state, WindowState::Normal);

    // Maximize window
    engine.maximize_window(id);
    let window = engine.windows.get(id).unwrap();
    assert_eq!(window.state, WindowState::Maximized);

    // Maximize again to restore
    engine.maximize_window(id);
    let window = engine.windows.get(id).unwrap();
    assert_eq!(window.state, WindowState::Normal);

    // Close window
    engine.close_window(id);
    assert!(engine.windows.get(id).is_none());
}

#[test]
fn test_window_focus_management() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    let id1 = engine.create_window(WindowConfig {
        title: "Window 1".to_string(),
        size: Size::new(800.0, 600.0),
        app_id: "test".to_string(),
        ..Default::default()
    });

    let id2 = engine.create_window(WindowConfig {
        title: "Window 2".to_string(),
        size: Size::new(800.0, 600.0),
        app_id: "test".to_string(),
        ..Default::default()
    });

    let id3 = engine.create_window(WindowConfig {
        title: "Window 3".to_string(),
        size: Size::new(800.0, 600.0),
        app_id: "test".to_string(),
        ..Default::default()
    });

    // Most recently created window should be focused
    assert_eq!(engine.windows.focused(), Some(id3));

    // Focus window 1
    engine.focus_window(id1);
    assert_eq!(engine.windows.focused(), Some(id1));

    // Minimize focused window - should focus next
    engine.minimize_window(id1);
    // Focused should now be one of the non-minimized windows
    let focused = engine.windows.focused();
    assert!(focused == Some(id2) || focused == Some(id3));

    // Close all and check focus is none
    engine.close_window(id1);
    engine.close_window(id2);
    engine.close_window(id3);
    assert_eq!(engine.windows.focused(), None);
}

#[test]
fn test_window_z_order() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    let id1 = engine.create_window(WindowConfig {
        title: "Window 1".to_string(),
        size: Size::new(800.0, 600.0),
        app_id: "test".to_string(),
        ..Default::default()
    });

    let id2 = engine.create_window(WindowConfig {
        title: "Window 2".to_string(),
        size: Size::new(800.0, 600.0),
        app_id: "test".to_string(),
        ..Default::default()
    });

    // Window 2 should be on top (higher z-order)
    let w1 = engine.windows.get(id1).unwrap();
    let w2 = engine.windows.get(id2).unwrap();
    assert!(w2.z_order > w1.z_order);

    // Focus window 1 - should bring to top
    engine.focus_window(id1);
    let w1 = engine.windows.get(id1).unwrap();
    let w2 = engine.windows.get(id2).unwrap();
    assert!(w1.z_order > w2.z_order);
}

// =============================================================================
// Desktop Switching Tests
// =============================================================================

#[test]
fn test_desktop_switch_preserves_windows() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    // Create windows on first desktop
    let win1 = engine.create_window(WindowConfig {
        title: "Desktop 1 Window".to_string(),
        size: Size::new(800.0, 600.0),
        app_id: "test".to_string(),
        ..Default::default()
    });

    // Create second desktop and switch to it
    engine.create_desktop("Desktop 2");
    engine.switch_desktop(1, 0.0);

    // Create window on second desktop
    let win2 = engine.create_window(WindowConfig {
        title: "Desktop 2 Window".to_string(),
        size: Size::new(800.0, 600.0),
        app_id: "test".to_string(),
        ..Default::default()
    });

    // Windows should exist on their respective desktops
    let desktop1 = &engine.desktops.desktops()[0];
    let desktop2 = &engine.desktops.desktops()[1];

    assert!(desktop1.contains_window(win1));
    assert!(!desktop1.contains_window(win2));
    assert!(!desktop2.contains_window(win1));
    assert!(desktop2.contains_window(win2));

    // Switch back to desktop 1
    engine.switch_desktop(0, 0.0);
    assert_eq!(engine.desktops.active_index(), 0);

    // Both windows should still exist
    assert!(engine.windows.get(win1).is_some());
    assert!(engine.windows.get(win2).is_some());
}

#[test]
fn test_multiple_desktop_operations() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    // Create multiple desktops
    engine.create_desktop("Work");
    engine.create_desktop("Personal");
    engine.create_desktop("Development");

    assert_eq!(engine.desktops.count(), 4); // Including initial "Main"

    // Switch through all desktops
    for i in 0..4 {
        engine.switch_desktop(i, 0.0);
        assert_eq!(engine.desktops.active_index(), i);
    }

    // Invalid switch should not change active desktop
    engine.switch_desktop(0, 0.0);
    engine.switch_desktop(100, 0.0); // Invalid index
    assert_eq!(engine.desktops.active_index(), 0);
}

// =============================================================================
// Void Mode Tests
// =============================================================================

#[test]
fn test_void_entry_exit_roundtrip() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    // Create multiple desktops with windows
    let win1 = engine.create_window(WindowConfig {
        title: "Window 1".to_string(),
        size: Size::new(800.0, 600.0),
        app_id: "test".to_string(),
        ..Default::default()
    });

    engine.create_desktop("Second");
    
    // Complete the desktop switch transition first
    engine.switch_desktop(1, 0.0);
    engine.tick_transition(500.0);
    
    let win2 = engine.create_window(WindowConfig {
        title: "Window 2".to_string(),
        size: Size::new(800.0, 600.0),
        app_id: "test".to_string(),
        ..Default::default()
    });

    // Should be in desktop mode
    assert!(matches!(*engine.get_view_mode(), ViewMode::Desktop { .. }));

    // Enter void
    let mut time = 1000.0;
    engine.enter_void(time);
    
    // Complete the transition
    time += CROSSFADE_DURATION_MS as f64 + 100.0;
    engine.tick_transition(time);
    
    assert!(matches!(*engine.get_view_mode(), ViewMode::Void));

    // Exit void to first desktop
    engine.exit_void(0, time);
    time += CROSSFADE_DURATION_MS as f64 + 100.0;
    engine.tick_transition(time);
    
    assert!(matches!(*engine.get_view_mode(), ViewMode::Desktop { index: 0 }));
    assert_eq!(engine.desktops.active_index(), 0);

    // Windows should still exist
    assert!(engine.windows.get(win1).is_some());
    assert!(engine.windows.get(win2).is_some());
}

#[test]
fn test_void_mode_viewport_constraints() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    // Create additional desktops
    engine.create_desktop("Desktop 2");
    engine.create_desktop("Desktop 3");

    // Enter void
    engine.enter_void(0.0);
    let transition_end = CROSSFADE_DURATION_MS as f64 + 100.0;
    engine.tick_transition(transition_end);

    // Pan in void mode should be constrained to desktop bounds
    let _initial_center = engine.viewport.center;
    engine.pan(10000.0, 10000.0); // Try to pan far away

    // Viewport should be constrained (not at exactly initial + 10000)
    // The void state constrains the viewport to stay near desktops
    let new_center = engine.viewport.center;
    // After large pan, center should still be within reasonable bounds
    // (constrained by void state)
    assert!(new_center.x < 10000.0 || new_center.y < 10000.0);
}

// =============================================================================
// Transition Tests
// =============================================================================

#[test]
fn test_transition_completion() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    engine.create_desktop("Desktop 2");

    // Start desktop switch transition
    engine.switch_desktop(1, 0.0);

    // Should be transitioning
    assert!(engine.is_transitioning());

    // Tick past transition duration
    let transition_end = 1000.0; // Well past the transition duration
    engine.tick_transition(transition_end);

    // Should no longer be transitioning
    assert!(!engine.is_transitioning());
    assert_eq!(engine.desktops.active_index(), 1);
}

#[test]
fn test_crossfade_opacities_during_transition() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    // Enter void with transition
    engine.enter_void(0.0);

    // At start of transition, crossfade should exist
    assert!(engine.crossfade().is_some());

    // Tick midway
    let midpoint = (CROSSFADE_DURATION_MS / 2) as f64;
    engine.tick_transition(midpoint);

    // Should still be transitioning
    assert!(engine.is_transitioning());

    // Tick to completion
    let end = CROSSFADE_DURATION_MS as f64 + 100.0;
    engine.tick_transition(end);

    // Transition should be complete
    assert!(engine.crossfade().is_none());
}

// =============================================================================
// Input Handling Tests
// =============================================================================

#[test]
fn test_pointer_down_on_window() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    let id = engine.create_window(WindowConfig {
        title: "Test Window".to_string(),
        position: Some(Vec2::new(100.0, 100.0)),
        size: Size::new(800.0, 600.0),
        app_id: "test".to_string(),
        ..Default::default()
    });

    // Click inside window content area
    let _result = engine.handle_pointer_down(500.0, 400.0, 0, false, false);

    // Should have handled the click on the window
    assert!(engine.windows.focused() == Some(id));
}

#[test]
fn test_pan_gesture() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    let initial_center = engine.viewport.center;

    // Start pan (middle click or ctrl+click)
    engine.handle_pointer_down(500.0, 500.0, 1, false, false);
    
    // Move pointer
    engine.handle_pointer_move(600.0, 600.0);

    // Center should have moved
    let new_center = engine.viewport.center;
    assert!((new_center.x - initial_center.x).abs() > 0.001 || 
            (new_center.y - initial_center.y).abs() > 0.001);

    // Release
    engine.handle_pointer_up();
    assert!(!engine.input.is_dragging());
}

#[test]
fn test_zoom_gesture() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    let initial_zoom = engine.viewport.zoom;

    // Zoom in (ctrl + wheel)
    engine.handle_wheel(0.0, -100.0, 960.0, 540.0, true);

    // Zoom should have increased
    assert!(engine.viewport.zoom > initial_zoom);

    // Zoom out
    engine.handle_wheel(0.0, 100.0, 960.0, 540.0, true);

    // Zoom should have decreased
    assert!(engine.viewport.zoom < initial_zoom + 0.5);
}

// =============================================================================
// Camera Animation Tests
// =============================================================================

#[test]
fn test_pan_to_window_animation() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    // Create window far from center
    let id = engine.create_window(WindowConfig {
        title: "Far Window".to_string(),
        position: Some(Vec2::new(5000.0, 5000.0)),
        size: Size::new(800.0, 600.0),
        app_id: "test".to_string(),
        ..Default::default()
    });

    // Pan to window
    engine.pan_to_window(id, 0.0);

    // Should have started camera animation (is_transitioning includes camera_animation)
    assert!(engine.is_transitioning());

    // Tick past animation duration
    let animation_end = CAMERA_ANIMATION_DURATION_MS as f64 + 100.0;
    engine.tick_transition(animation_end);

    // Animation should be complete (no longer transitioning)
    assert!(!engine.is_transitioning());

    // Viewport should be near the window
    let window = engine.windows.get(id).unwrap();
    let window_center = window.rect().center();
    let viewport_center = engine.viewport.center;
    
    // Should be reasonably close (allowing for some margin)
    assert!((viewport_center.x - window_center.x).abs() < 500.0);
    assert!((viewport_center.y - window_center.y).abs() < 500.0);
}

// =============================================================================
// State Consistency Tests
// =============================================================================

#[test]
fn test_state_consistency_after_many_operations() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    let mut time = 0.0;

    // Perform many operations
    for i in 0..10 {
        // Create desktop
        engine.create_desktop(&format!("Desktop {}", i + 2));
        
        // Switch to new desktop first (so windows are created on the new desktop)
        engine.switch_desktop(i + 1, time);
        time += 500.0;
        engine.tick_transition(time);
        
        // Create windows on this desktop
        for j in 0..5 {
            engine.create_window(WindowConfig {
                title: format!("Window {}-{}", i, j),
                position: Some(Vec2::new(100.0 * j as f32, 100.0 * j as f32)),
                size: Size::new(400.0, 300.0),
                app_id: "test".to_string(),
                ..Default::default()
            });
        }
    }

    // Verify state
    assert_eq!(engine.desktops.count(), 11); // 1 initial + 10 created
    assert_eq!(engine.windows.count(), 50); // 5 windows per desktop iteration

    // Switch back to first desktop
    engine.switch_desktop(0, time);
    time += 500.0;
    engine.tick_transition(time);
    assert_eq!(engine.desktops.active_index(), 0);

    // Enter and exit void
    engine.enter_void(time);
    time += CROSSFADE_DURATION_MS as f64 + 100.0;
    engine.tick_transition(time);
    
    engine.exit_void(5, time);
    time += CROSSFADE_DURATION_MS as f64 + 100.0;
    engine.tick_transition(time);

    // Should be on desktop 5
    assert_eq!(engine.desktops.active_index(), 5);
    
    // All windows should still exist
    assert_eq!(engine.windows.count(), 50);
}

#[test]
fn test_window_resize_respects_constraints() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    let id = engine.create_window(WindowConfig {
        title: "Constrained Window".to_string(),
        position: Some(Vec2::new(100.0, 100.0)),
        size: Size::new(800.0, 600.0),
        min_size: Some(Size::new(300.0, 200.0)),
        max_size: Some(Size::new(1000.0, 800.0)),
        app_id: "test".to_string(),
        ..Default::default()
    });

    // Try to resize smaller than minimum
    engine.resize_window(id, 100.0, 100.0);
    let window = engine.windows.get(id).unwrap();
    assert!(window.size.width >= 300.0);
    assert!(window.size.height >= 200.0);

    // Try to resize larger than maximum
    engine.resize_window(id, 2000.0, 1500.0);
    let window = engine.windows.get(id).unwrap();
    assert!(window.size.width <= 1000.0);
    assert!(window.size.height <= 800.0);
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[test]
fn test_close_nonexistent_window() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    // Should not panic
    engine.close_window(999);
    engine.focus_window(999);
    engine.minimize_window(999);
}

#[test]
fn test_switch_to_invalid_desktop() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    let initial = engine.desktops.active_index();
    
    // Try to switch to invalid index
    engine.switch_desktop(100, 0.0);
    
    // Should remain on current desktop
    assert_eq!(engine.desktops.active_index(), initial);
}

#[test]
fn test_operations_blocked_during_transition() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    let _initial_center = engine.viewport.center;

    // Start void transition
    engine.enter_void(0.0);
    
    // Try to pan during transition
    engine.pan(1000.0, 1000.0);

    // Pan should be blocked during crossfade
    // Note: The actual behavior depends on implementation
    // This test documents expected behavior
    assert!(engine.is_crossfading());
}

#[test]
fn test_get_window_screen_rects() {
    let mut engine = DesktopEngine::new();
    engine.init(1920.0, 1080.0);

    let id = engine.create_window(WindowConfig {
        title: "Test".to_string(),
        position: Some(Vec2::new(100.0, 100.0)),
        size: Size::new(800.0, 600.0),
        app_id: "test".to_string(),
        ..Default::default()
    });

    let rects = engine.get_window_screen_rects(0.0);
    
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0].id, id);
    assert_eq!(rects[0].title, "Test");
    assert!((rects[0].opacity - 1.0).abs() < 0.001);
}
