# Acceptance Tests

**Component:** 08-desktop/12-acceptance  
**Status:** Specification

---

## Overview

This document defines the acceptance criteria and test cases for `zos-desktop`. All tests must pass before the crate is considered complete.

---

## Test Categories

| Category | Coverage |
|----------|----------|
| Unit Tests | Core math, easing, layout algorithms |
| Integration Tests | Component interactions |
| Visual Tests | Rendering output verification |
| Performance Tests | FPS targets, memory usage |
| Code Quality | Size limits, lint compliance |

---

## Acceptance Criteria

### 1. Desktop Management

**AC-1.1: Create and remove desktops**

```rust
#[test]
fn desktop_create_remove() {
    let mut compositor = create_test_compositor();
    
    // Create desktops
    let d1 = compositor.desktop_create(DesktopSpec {
        name: "Desktop 1".into(),
        ..Default::default()
    });
    let d2 = compositor.desktop_create(DesktopSpec {
        name: "Desktop 2".into(),
        ..Default::default()
    });
    
    assert_eq!(compositor.desktop_list().len(), 3); // Initial + 2 new
    
    // Remove desktop
    compositor.desktop_remove(d1).unwrap();
    assert_eq!(compositor.desktop_list().len(), 2);
    assert!(!compositor.desktop_list().contains(&d1));
    
    // Cannot remove last desktop
    compositor.desktop_remove(d2).unwrap();
    let remaining = compositor.desktop_list()[0];
    assert!(compositor.desktop_remove(remaining).is_err());
}
```

**AC-1.2: Active desktop persists in snapshot**

```rust
#[test]
fn active_desktop_persists() {
    let mut compositor = create_test_compositor();
    
    let d1 = compositor.desktop_create(DesktopSpec::default());
    let d2 = compositor.desktop_create(DesktopSpec::default());
    
    compositor.desktop_set_active(d2);
    assert_eq!(compositor.desktop_active(), d2);
    
    // Snapshot and restore
    let snapshot = compositor.snapshot();
    compositor.desktop_set_active(d1);
    compositor.restore(snapshot).unwrap();
    
    assert_eq!(compositor.desktop_active(), d2);
}
```

---

### 2. Infinite Canvas

**AC-2.1: Zoom/pan without precision jitter at large coordinates**

```rust
#[test]
fn infinite_canvas_precision() {
    let mut compositor = create_test_compositor();
    
    // Move camera to extreme coordinates
    let extreme = 1_000_000_000.0; // 1 billion
    compositor.pan((-extreme as f32, -extreme as f32));
    
    // Create window at that location
    let window = compositor.window_create(
        compositor.desktop_active(),
        WindowSpec {
            position: Some((extreme, extreme)),
            size: (200.0, 150.0),
            ..Default::default()
        },
    );
    
    // Render multiple frames
    for _ in 0..100 {
        compositor.frame(16.0);
    }
    
    // Window position should remain stable (within f32 precision)
    let info = compositor.window_get(window).unwrap();
    let delta_x = (info.world_rect.x - extreme).abs();
    let delta_y = (info.world_rect.y - extreme).abs();
    
    assert!(delta_x < 1.0, "X drift: {}", delta_x);
    assert!(delta_y < 1.0, "Y drift: {}", delta_y);
}
```

**AC-2.2: Zoom anchored correctly**

```rust
#[test]
fn zoom_anchor_stability() {
    let mut compositor = create_test_compositor();
    
    // Place cursor at specific screen position
    let anchor = (400.0, 300.0);
    
    // Get world position under anchor
    let world_before = compositor.screen_to_world(anchor);
    
    // Zoom in
    compositor.handle_input(InputEvent::Wheel(WheelEvent {
        delta: (0.0, -100.0), // Zoom in
        position: anchor,
        modifiers: Modifiers { ctrl: true, ..Default::default() },
    }));
    
    // World position under anchor should be same
    let world_after = compositor.screen_to_world(anchor);
    
    assert!((world_before.0 - world_after.0).abs() < 0.01);
    assert!((world_before.1 - world_after.1).abs() < 0.01);
}
```

---

### 3. Window Management

**AC-3.1: Window CRUD operations**

```rust
#[test]
fn window_lifecycle() {
    let mut compositor = create_test_compositor();
    let desktop = compositor.desktop_active();
    
    // Create
    let id = compositor.window_create(desktop, WindowSpec {
        title: "Test".into(),
        position: Some((100.0, 100.0)),
        size: (400.0, 300.0),
        ..Default::default()
    });
    
    // Read
    let info = compositor.window_get(id).unwrap();
    assert_eq!(info.title, "Test");
    assert_eq!(info.world_rect.x, 100.0);
    
    // Move
    compositor.window_move(id, 200.0, 200.0);
    let info = compositor.window_get(id).unwrap();
    assert_eq!(info.world_rect.x, 200.0);
    
    // Resize
    compositor.window_resize(id, 500.0, 400.0);
    let info = compositor.window_get(id).unwrap();
    assert_eq!(info.world_rect.width, 500.0);
    
    // Close
    compositor.window_close(id);
    assert!(compositor.window_get(id).is_none());
}
```

**AC-3.2: Z-order follows focus policy**

```rust
#[test]
fn z_order_focus_policy() {
    let mut compositor = create_test_compositor();
    let desktop = compositor.desktop_active();
    
    let w1 = compositor.window_create(desktop, WindowSpec::default());
    let w2 = compositor.window_create(desktop, WindowSpec::default());
    let w3 = compositor.window_create(desktop, WindowSpec::default());
    
    // w3 should be on top (created last)
    let info3 = compositor.window_get(w3).unwrap();
    let info2 = compositor.window_get(w2).unwrap();
    let info1 = compositor.window_get(w1).unwrap();
    
    assert!(info3.z_order > info2.z_order);
    assert!(info2.z_order > info1.z_order);
    
    // Focus w1 - should come to top
    compositor.window_focus(w1);
    
    let info1 = compositor.window_get(w1).unwrap();
    let info3 = compositor.window_get(w3).unwrap();
    
    assert!(info1.z_order > info3.z_order);
}
```

---

### 4. Void Mode

**AC-4.1: Enter/exit void with transitions**

```rust
#[test]
fn void_mode_transitions() {
    let mut compositor = create_test_compositor();
    
    // Start in desktop mode
    assert_eq!(compositor.mode(), Mode::Desktop);
    
    // Enter void
    compositor.enter_void();
    assert_eq!(compositor.mode(), Mode::Void);
    
    // Check transition started
    assert!(compositor.is_transitioning());
    
    // Advance through transition
    for _ in 0..30 {
        compositor.frame(16.0);
    }
    
    // Transition complete
    assert!(!compositor.is_transitioning());
    
    // Exit void
    let desktop = compositor.desktop_active();
    compositor.exit_void(desktop);
    
    assert_eq!(compositor.mode(), Mode::Desktop);
}
```

**AC-4.2: Portal selection and navigation**

```rust
#[test]
fn void_portal_selection() {
    let mut compositor = create_test_compositor();
    
    // Create multiple desktops
    let d1 = compositor.desktop_create(DesktopSpec::default());
    let d2 = compositor.desktop_create(DesktopSpec::default());
    
    compositor.enter_void();
    advance_past_transition(&mut compositor);
    
    // Click on d1's portal
    let portal_pos = compositor.void_portal_center(d1);
    compositor.handle_input(InputEvent::Pointer(PointerEvent {
        kind: PointerKind::Down,
        position: portal_pos,
        button: Some(PointerButton::Primary),
        modifiers: Default::default(),
    }));
    
    // d1 should be selected
    assert_eq!(compositor.void_selection(), Some(d1));
    
    // Double-click enters desktop
    compositor.handle_input(InputEvent::Pointer(PointerEvent {
        kind: PointerKind::Down,
        position: portal_pos,
        button: Some(PointerButton::Primary),
        modifiers: Default::default(),
    }));
    
    advance_past_transition(&mut compositor);
    
    assert_eq!(compositor.mode(), Mode::Desktop);
    assert_eq!(compositor.desktop_active(), d1);
}
```

---

### 5. React DOM Integration

**AC-5.1: Mount element exists and aligns**

```rust
#[test]
fn react_mount_alignment() {
    let mut compositor = create_test_compositor();
    let desktop = compositor.desktop_active();
    
    let id = compositor.window_create(desktop, WindowSpec {
        surface: SurfaceKind::ReactDom,
        position: Some((100.0, 100.0)),
        size: (400.0, 300.0),
        ..Default::default()
    });
    
    // Mount should exist
    let mount = compositor.mount_element(id);
    assert!(mount.is_some());
    
    // Update frame
    compositor.frame(16.0);
    
    // Mount rect should match window content rect
    let mount = mount.unwrap();
    let style = mount.style();
    
    // Get computed position (accounting for title bar)
    let expected = compositor.window_content_screen_rect(id).unwrap();
    
    // Compare (with tolerance for rounding)
    // ...
}
```

**AC-5.2: Mounts hidden in Void**

```rust
#[test]
fn mounts_hidden_in_void() {
    let mut compositor = create_test_compositor();
    let desktop = compositor.desktop_active();
    
    let id = compositor.window_create(desktop, WindowSpec {
        surface: SurfaceKind::ReactDom,
        ..Default::default()
    });
    
    // Mount visible in desktop mode
    compositor.frame(16.0);
    let mount = compositor.mount_element(id).unwrap();
    assert_ne!(mount.style().get_property_value("display").unwrap(), "none");
    
    // Enter void
    compositor.enter_void();
    advance_past_transition(&mut compositor);
    
    // Mount hidden
    assert_eq!(mount.style().get_property_value("display").unwrap(), "none");
    
    // Exit void
    compositor.exit_void(desktop);
    advance_past_transition(&mut compositor);
    
    // Mount visible again
    assert_ne!(mount.style().get_property_value("display").unwrap(), "none");
}
```

**AC-5.3: Mount z-index matches window**

```rust
#[test]
fn mount_z_index_ordering() {
    let mut compositor = create_test_compositor();
    let desktop = compositor.desktop_active();
    
    let w1 = compositor.window_create(desktop, WindowSpec {
        surface: SurfaceKind::ReactDom,
        ..Default::default()
    });
    let w2 = compositor.window_create(desktop, WindowSpec {
        surface: SurfaceKind::ReactDom,
        ..Default::default()
    });
    
    compositor.frame(16.0);
    
    let m1 = compositor.mount_element(w1).unwrap();
    let m2 = compositor.mount_element(w2).unwrap();
    
    let z1: i32 = m1.style().get_property_value("z-index").unwrap().parse().unwrap();
    let z2: i32 = m2.style().get_property_value("z-index").unwrap().parse().unwrap();
    
    // w2 created later, should be higher z
    assert!(z2 > z1);
    
    // Focus w1
    compositor.window_focus(w1);
    compositor.frame(16.0);
    
    let z1: i32 = m1.style().get_property_value("z-index").unwrap().parse().unwrap();
    let z2: i32 = m2.style().get_property_value("z-index").unwrap().parse().unwrap();
    
    // w1 now higher
    assert!(z1 > z2);
}
```

---

### 6. Browser Resize

**AC-6.1: Canvas resize updates correctly**

```rust
#[test]
fn canvas_resize_dpi_aware() {
    let mut compositor = create_test_compositor();
    
    // Initial size
    let initial_size = compositor.screen_size();
    
    // Simulate resize
    compositor.resize(1920, 1080);
    
    let new_size = compositor.screen_size();
    assert_eq!(new_size, (1920, 1080));
    
    // Render should succeed
    compositor.frame(16.0);
}
```

---

### 7. Resource Cleanup

**AC-7.1: No leaked DOM nodes**

```rust
#[test]
fn no_dom_leaks_on_close() {
    let mut compositor = create_test_compositor();
    let desktop = compositor.desktop_active();
    
    let mount_count_before = count_mount_elements();
    
    // Create and close windows
    for _ in 0..100 {
        let id = compositor.window_create(desktop, WindowSpec {
            surface: SurfaceKind::ReactDom,
            ..Default::default()
        });
        compositor.frame(16.0);
        compositor.window_close(id);
        compositor.frame(16.0);
    }
    
    let mount_count_after = count_mount_elements();
    
    assert_eq!(mount_count_before, mount_count_after);
}
```

**AC-7.2: No leaks on desktop remove**

```rust
#[test]
fn no_leaks_on_desktop_remove() {
    let mut compositor = create_test_compositor();
    
    let desktop = compositor.desktop_create(DesktopSpec::default());
    
    // Create windows with mounts
    for _ in 0..10 {
        compositor.window_create(desktop, WindowSpec {
            surface: SurfaceKind::ReactDom,
            ..Default::default()
        });
    }
    
    compositor.frame(16.0);
    let mount_count_before = count_mount_elements();
    
    // Remove desktop
    compositor.desktop_remove(desktop).unwrap();
    compositor.frame(16.0);
    
    let mount_count_after = count_mount_elements();
    
    // 10 fewer mounts
    assert_eq!(mount_count_after, mount_count_before - 10);
}
```

---

### 8. Code Quality

**AC-8.1: 60-line function limit**

```bash
# CI script to verify
#!/bin/bash
for file in $(find crates/zos-desktop/src -name "*.rs"); do
    awk '
        /^[[:space:]]*(pub )?fn / { start=NR; in_func=1; name=$0 }
        in_func && /^[[:space:]]*}$/ {
            lines = NR - start
            if (lines > 60) {
                print FILENAME ":" start ": function exceeds 60 lines (" lines ")"
                exit 1
            }
            in_func=0
        }
    ' "$file"
done
```

**AC-8.2: 500-line file limit**

```bash
#!/bin/bash
for file in $(find crates/zos-desktop/src -name "*.rs"); do
    lines=$(wc -l < "$file")
    if [ "$lines" -gt 500 ]; then
        echo "$file: exceeds 500 lines ($lines)"
        exit 1
    fi
done
```

**AC-8.3: rustfmt + clippy clean**

```bash
# CI verification
cargo fmt --check
cargo clippy -- -D warnings -D clippy::all -D clippy::pedantic \
    -A clippy::module_name_repetitions \
    -A clippy::too_many_arguments
```

---

## Performance Targets

| Scenario | Metric | Target |
|----------|--------|--------|
| 50 windows visible | FPS | ≥ 60 |
| 4 desktops in Void | FPS | ≥ 60 |
| Zoom/pan transition | FPS | ≥ 60 |
| Window create | Latency | < 1ms |
| Void enter | Transition | < 300ms |

---

## Test Helpers

```rust
fn create_test_compositor() -> Compositor {
    // Creates compositor with mock WebGPU context
}

fn advance_past_transition(compositor: &mut Compositor) {
    for _ in 0..30 {
        compositor.frame(16.0);
    }
}

fn count_mount_elements() -> usize {
    // Count elements with class "Zero-mount" in DOM
}
```

---

*[Back to Desktop](README.md) | [Previous: Engineering](11-engineering.md)*
