# Engineering Standards

**Component:** 08-desktop/11-engineering  
**Status:** Specification

---

## Overview

This document specifies mandatory code quality constraints, architectural patterns, and engineering standards for the `orbital-desktop` crate.

---

## Hard Limits

### Function Size Limit

**No function may exceed 60 lines** (excluding doc comments and blank lines).

```rust
// GOOD: Function under 60 lines
fn handle_pointer_down(&mut self, event: PointerEvent) -> InputOutcome {
    // 15-20 lines of focused logic
    match self.determine_target(&event) {
        Target::Window(id, region) => self.handle_window_hit(event, id, region),
        Target::Canvas => self.start_canvas_pan(event.position),
        Target::Void(portal) => self.handle_void_portal_click(portal),
    }
}

// BAD: Monolithic function exceeding 60 lines
fn handle_all_input(&mut self, event: InputEvent) -> InputOutcome {
    // 150+ lines handling all cases inline
    // ...
}
```

### File Size Limit

**No file may exceed 500 lines** (excluding license header if present).

```
// GOOD: Well-factored module structure
window/
├── manager.rs      (180 lines)
├── window.rs       (120 lines)
├── chrome.rs       (200 lines)
└── layout.rs       (150 lines)

// BAD: Monolithic file
window.rs           (1200 lines)
```

---

## Enforcement Strategy

These limits are **enforced by design**, not by "hero refactors":

1. **Split proactively** when approaching 350-400 LOC in a file
2. **Extract helpers** when a function approaches 40 lines
3. **Use command-style patterns** to delegate logic
4. **Design for small modules** from the start

---

## Architectural Decomposition

### Required Components

To satisfy size limits, the crate must include:

| Component | Purpose | Max File Size |
|-----------|---------|---------------|
| `InputRouter` | Hit testing, intent extraction | 400 lines |
| `LayoutEngine` | World→screen projection, culling | 400 lines |
| `UiBridge` | DOM mount lifecycle, style updates | 400 lines |
| `TransitionSystem` | Tweening, easing, mode triggers | 400 lines |
| `Renderer` | Pipeline management, render passes | Split across files |

### Recommended Module Structure

```
crates/orbital-desktop/
├── src/
│   ├── lib.rs                 # Public API facade (< 200 lines)
│   │
│   ├── compositor/
│   │   ├── mod.rs             # Compositor struct, coordination
│   │   ├── state.rs           # CompositorState
│   │   └── lifecycle.rs       # update(), render()
│   │
│   ├── scene/
│   │   ├── desktop.rs         # Desktop, camera ops
│   │   ├── void.rs            # Void mode, portals
│   │   └── preview.rs         # Preview textures
│   │
│   ├── window/
│   │   ├── manager.rs         # WindowManager
│   │   ├── window.rs          # Window struct
│   │   ├── chrome.rs          # Chrome regions, hit testing
│   │   └── layout.rs          # Constraints, cascading
│   │
│   ├── input/
│   │   ├── router.rs          # InputRouter
│   │   ├── events.rs          # Event types
│   │   ├── drag.rs            # Drag state
│   │   └── gestures.rs        # Touch gestures (feature-gated)
│   │
│   ├── render/
│   │   ├── mod.rs             # Renderer facade
│   │   ├── pipeline.rs        # Pipeline creation
│   │   ├── buffers.rs         # Buffer management
│   │   └── passes/
│   │       ├── background.rs
│   │       ├── window.rs
│   │       └── void.rs
│   │
│   ├── ui/
│   │   ├── bridge.rs          # UiBridge
│   │   ├── mount.rs           # WindowMount
│   │   └── styles.rs          # Style helpers
│   │
│   ├── transition/
│   │   ├── system.rs          # TransitionSystem
│   │   ├── easing.rs          # Easing functions
│   │   └── types.rs           # Transition definitions
│   │
│   ├── math/
│   │   ├── camera.rs          # Camera, projection
│   │   ├── geom.rs            # Rect, Vec2, Size
│   │   └── precision.rs       # Origin rebasing
│   │
│   ├── persistence/
│   │   ├── snapshot.rs        # Snapshot types
│   │   ├── save.rs            # Creating snapshots
│   │   └── restore.rs         # Restoring state
│   │
│   └── config/
│       ├── mod.rs             # CompositorConfig
│       └── builder.rs         # Config builder
```

---

## Rust Conventions

### Code Style

```rust
// REQUIRED: rustfmt clean
// Run: cargo fmt --check

// REQUIRED: clippy clean with strict profile
// Run: cargo clippy -- -D warnings -D clippy::all

// Recommended clippy.toml:
[lints.clippy]
pedantic = "warn"
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
```

### Module Boundaries

```rust
// GOOD: Clear module boundary with minimal pub surface
mod window {
    mod manager;  // Internal
    mod chrome;   // Internal
    
    pub use manager::{WindowManager, WindowId};
    pub use chrome::WindowRegion;
    // Internal types not re-exported
}

// BAD: Exposing everything
pub mod window {
    pub mod manager;
    pub mod chrome;
    pub mod internal_helpers;  // Should be private
}
```

### Error Handling

```rust
// REQUIRED: Crate-local error type
#[derive(Debug)]
pub enum Error {
    WebGpuInit(String),
    InvalidDesktop(DesktopId),
    InvalidWindow(WindowId),
    InvalidConfig(String),
    InvalidSnapshot(String),
    NoDom,
    DomError,
    StorageError,
}

pub type Result<T> = std::result::Result<T, Error>;

// FORBIDDEN in library code:
fn bad_function() {
    let x = some_option.unwrap();       // NO
    let y = some_result.expect("msg");  // NO
    panic!("error");                    // NO
}

// GOOD:
fn good_function() -> Result<()> {
    let x = some_option.ok_or(Error::InvalidConfig("missing value".into()))?;
    Ok(())
}
```

---

## Global State Policy

```rust
// FORBIDDEN: Global mutable state
static mut COMPOSITOR: Option<Compositor> = None;  // NO

lazy_static! {
    static ref STATE: Mutex<State> = Mutex::new(State::new());  // NO
}

// GOOD: All state owned by Compositor instance
pub struct Compositor {
    state: CompositorState,
    // ...
}
```

---

## Reuse and Duplication Policy

### No Copy-Paste Logic

```rust
// BAD: Duplicated coordinate conversion in multiple places
impl Desktop {
    fn world_to_screen(&self, world: (f64, f64)) -> (f32, f32) {
        // ... 10 lines of math
    }
}

impl Void {
    fn world_to_screen(&self, world: (f64, f64)) -> (f32, f32) {
        // ... same 10 lines of math
    }
}

// GOOD: Shared math in dedicated module
mod math {
    pub mod camera {
        pub fn world_to_screen(
            camera: &Camera,
            world: (f64, f64),
            screen_size: (f32, f32),
        ) -> (f32, f32) {
            // Single implementation
        }
    }
}
```

### Parameterized Rendering

```rust
// GOOD: Single renderer with scene parameter
impl Renderer {
    pub fn render(&mut self, scene: &dyn Scene, camera: &Camera) {
        // Shared rendering logic
    }
}

trait Scene {
    fn background(&self) -> &Background;
    fn windows(&self) -> &[&Window];
}

impl Scene for Desktop { /* ... */ }
impl Scene for VoidScene { /* ... */ }
```

---

## Testability Requirements

### Core Logic Must Be Testable

```rust
// GOOD: Math testable without WebGPU
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn camera_world_to_screen() {
        let camera = Camera {
            center: (100.0, 100.0),
            zoom: 2.0,
        };
        let screen_size = (800.0, 600.0);
        
        let screen = camera.world_to_screen((100.0, 100.0), screen_size);
        assert_eq!(screen, (400.0, 300.0)); // Center of screen
    }
    
    #[test]
    fn easing_linear() {
        assert_eq!(Easing::Linear.apply(0.0), 0.0);
        assert_eq!(Easing::Linear.apply(0.5), 0.5);
        assert_eq!(Easing::Linear.apply(1.0), 1.0);
    }
}
```

### Trait Boundaries for Mocking

```rust
/// Time source (mockable for tests)
pub trait TimeSource {
    fn now_ms(&self) -> f32;
}

/// UI backend (mockable for tests)
pub trait UiBackend {
    fn create_mount(&mut self, id: WindowId) -> Result<()>;
    fn destroy_mount(&mut self, id: WindowId);
    fn update_mount_rect(&mut self, id: WindowId, rect: &ScreenRect);
}

#[cfg(test)]
mod tests {
    struct MockUiBackend {
        mounts: Vec<WindowId>,
        rects: HashMap<WindowId, ScreenRect>,
    }
    
    impl UiBackend for MockUiBackend {
        // Test implementation
    }
}
```

---

## Observability

### Tracing Spans

```rust
use tracing::{instrument, span, Level};

impl Compositor {
    #[instrument(skip(self))]
    pub fn update(&mut self, dt_ms: f32) {
        let _span = span!(Level::DEBUG, "compositor_update").entered();
        
        self.transitions.update(dt_ms);
        self.update_culling();
        self.ui.update(&self.state, &self.config);
    }
    
    #[instrument(skip(self))]
    pub fn render(&mut self) {
        let _span = span!(Level::DEBUG, "compositor_render").entered();
        
        // ...
    }
}
```

### Performance Counters

```rust
/// Performance counters (optional)
#[derive(Default)]
pub struct PerformanceCounters {
    /// Windows visible this frame
    pub visible_windows: u32,
    
    /// Windows culled this frame
    pub culled_windows: u32,
    
    /// Preview renders this frame
    pub preview_renders: u32,
    
    /// DOM mounts updated this frame
    pub mount_updates: u32,
    
    /// Frame time (ms)
    pub frame_time_ms: f32,
}

impl Compositor {
    /// Get performance counters for last frame
    pub fn counters(&self) -> &PerformanceCounters {
        &self.counters
    }
}
```

---

## Documentation Standards

### Required Documentation

```rust
/// Window identifier.
///
/// Uniquely identifies a window within the compositor.
/// IDs are never reused within a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

/// Create a new window on a desktop.
///
/// # Arguments
///
/// * `desktop` - The desktop to create the window on
/// * `spec` - Window specification
///
/// # Returns
///
/// The ID of the newly created window.
///
/// # Example
///
/// ```
/// let id = compositor.window_create(desktop_id, WindowSpec {
///     title: "My Window".into(),
///     size: (800.0, 600.0),
///     ..Default::default()
/// });
/// ```
pub fn window_create(&mut self, desktop: DesktopId, spec: WindowSpec) -> WindowId {
    // ...
}
```

---

*[Back to Desktop](README.md) | [Previous: Configuration](10-configuration.md) | [Next: Acceptance](12-acceptance.md)*
