# Input Handling

**Component:** 08-desktop/06-input  
**Status:** Specification

---

## Overview

Input handling routes pointer, wheel, and keyboard events to appropriate targets: compositor (pan/zoom/chrome), windows (content), or Void mode interactions.

---

## Input Events

```rust
/// Input event (from host)
pub enum InputEvent {
    Pointer(PointerEvent),
    Wheel(WheelEvent),
    Keyboard(KeyboardEvent),
}

/// Pointer event
pub struct PointerEvent {
    pub kind: PointerKind,
    pub position: (f32, f32),
    pub button: Option<PointerButton>,
    pub modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

/// Wheel event
pub struct WheelEvent {
    pub delta: (f32, f32),
    pub position: (f32, f32),
    pub modifiers: Modifiers,
}

/// Keyboard event
pub struct KeyboardEvent {
    pub kind: KeyKind,
    pub key: String,
    pub code: String,
    pub modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Down,
    Up,
}

/// Modifier keys
#[derive(Clone, Copy, Debug, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}
```

---

## Input Outcome

```rust
/// Result of input handling
pub struct InputOutcome {
    /// Whether the event was consumed by compositor
    pub consumed: bool,
    
    /// Window under pointer (for hover states)
    pub hovered_window: Option<WindowId>,
    
    /// Focus changed (Some(None) = unfocused, Some(Some(id)) = focused)
    pub focus_changed: Option<Option<WindowId>>,
}

impl InputOutcome {
    pub fn consumed() -> Self {
        Self {
            consumed: true,
            hovered_window: None,
            focus_changed: None,
        }
    }
    
    pub fn unconsumed() -> Self {
        Self {
            consumed: false,
            hovered_window: None,
            focus_changed: None,
        }
    }
}
```

---

## Input Router

```rust
/// Input router
pub struct InputRouter {
    /// Current drag operation
    drag: Option<DragState>,
    
    /// Last pointer position (for delta calculation)
    last_pointer: Option<(f32, f32)>,
    
    /// Double-click detection
    click_tracker: ClickTracker,
}

impl InputRouter {
    pub fn new() -> Self {
        Self {
            drag: None,
            last_pointer: None,
            click_tracker: ClickTracker::new(),
        }
    }
}

/// Active drag operation
struct DragState {
    kind: DragKind,
    start_position: (f32, f32),
    last_position: (f32, f32),
}

/// Drag operation type
enum DragKind {
    /// Panning the canvas
    PanCanvas,
    
    /// Moving a window
    MoveWindow(WindowId),
    
    /// Resizing a window
    ResizeWindow {
        id: WindowId,
        edge: ResizeEdge,
        initial_rect: WorldRect,
    },
}

#[derive(Clone, Copy)]
enum ResizeEdge {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}
```

---

## Event Handling

### Pointer Events

```rust
impl InputRouter {
    pub fn handle_pointer(
        &mut self,
        event: PointerEvent,
        state: &mut CompositorState,
        windows: &mut WindowManager,
        config: &CompositorConfig,
    ) -> InputOutcome {
        match event.kind {
            PointerKind::Down => self.handle_pointer_down(event, state, windows, config),
            PointerKind::Move => self.handle_pointer_move(event, state, windows, config),
            PointerKind::Up => self.handle_pointer_up(event, state, windows),
            PointerKind::Cancel => self.handle_pointer_cancel(),
        }
    }
    
    fn handle_pointer_down(
        &mut self,
        event: PointerEvent,
        state: &mut CompositorState,
        windows: &mut WindowManager,
        config: &CompositorConfig,
    ) -> InputOutcome {
        self.last_pointer = Some(event.position);
        
        match state.mode {
            Mode::Desktop => self.desktop_pointer_down(event, state, windows, config),
            Mode::Void => self.void_pointer_down(event, state),
        }
    }
    
    fn desktop_pointer_down(
        &mut self,
        event: PointerEvent,
        state: &mut CompositorState,
        windows: &mut WindowManager,
        config: &CompositorConfig,
    ) -> InputOutcome {
        let desktop = state.active_desktop();
        let world = desktop.camera.screen_to_world(
            event.position,
            state.screen_size,
        );
        
        // Hit test windows
        if let Some((window_id, region)) = windows.window_at(
            desktop.id,
            world,
            &config.chrome,
        ) {
            return self.handle_window_hit(
                event,
                window_id,
                region,
                windows,
                config,
            );
        }
        
        // Click on empty canvas - start pan
        self.start_drag(DragKind::PanCanvas, event.position);
        InputOutcome::consumed()
    }
    
    fn handle_window_hit(
        &mut self,
        event: PointerEvent,
        window_id: WindowId,
        region: WindowRegion,
        windows: &mut WindowManager,
        config: &CompositorConfig,
    ) -> InputOutcome {
        match region {
            WindowRegion::TitleBar => {
                // Start window drag
                self.start_drag(DragKind::MoveWindow(window_id), event.position);
                windows.focus(window_id, true);
                InputOutcome {
                    consumed: true,
                    hovered_window: Some(window_id),
                    focus_changed: Some(Some(window_id)),
                }
            }
            
            WindowRegion::Content => {
                // Focus window, but don't consume (forward to React)
                let focus_changed = if windows.focused() != Some(window_id) {
                    windows.focus(window_id, true);
                    Some(Some(window_id))
                } else {
                    None
                };
                InputOutcome {
                    consumed: false,
                    hovered_window: Some(window_id),
                    focus_changed,
                }
            }
            
            WindowRegion::CloseButton => {
                // Will emit WindowRequestedClose event
                InputOutcome {
                    consumed: true,
                    hovered_window: Some(window_id),
                    focus_changed: None,
                }
            }
            
            WindowRegion::ResizeBottomRight => {
                let window = windows.get(window_id).expect("window exists");
                self.start_drag(
                    DragKind::ResizeWindow {
                        id: window_id,
                        edge: ResizeEdge::BottomRight,
                        initial_rect: window.world_rect(),
                    },
                    event.position,
                );
                InputOutcome::consumed()
            }
            
            _ => InputOutcome::consumed(),
        }
    }
}
```

### Pointer Move

```rust
impl InputRouter {
    fn handle_pointer_move(
        &mut self,
        event: PointerEvent,
        state: &mut CompositorState,
        windows: &mut WindowManager,
        config: &CompositorConfig,
    ) -> InputOutcome {
        let last = self.last_pointer.unwrap_or(event.position);
        let delta = (event.position.0 - last.0, event.position.1 - last.1);
        self.last_pointer = Some(event.position);
        
        // Handle active drag
        if let Some(drag) = &mut self.drag {
            drag.last_position = event.position;
            
            match &drag.kind {
                DragKind::PanCanvas => {
                    let desktop = state.active_desktop_mut();
                    let scale = 1.0 / desktop.camera.zoom;
                    desktop.camera.center.0 -= delta.0 as f64 * scale as f64;
                    desktop.camera.center.1 -= delta.1 as f64 * scale as f64;
                    desktop.preview_dirty = true;
                    return InputOutcome::consumed();
                }
                
                DragKind::MoveWindow(id) => {
                    let desktop = state.active_desktop();
                    let scale = 1.0 / desktop.camera.zoom;
                    let canvas_delta = (
                        delta.0 as f64 * scale as f64,
                        delta.1 as f64 * scale as f64,
                    );
                    
                    if let Some(window) = windows.get_mut(*id) {
                        window.position.0 += canvas_delta.0;
                        window.position.1 += canvas_delta.1;
                    }
                    
                    state.active_desktop_mut().preview_dirty = true;
                    return InputOutcome::consumed();
                }
                
                DragKind::ResizeWindow { id, edge, initial_rect } => {
                    self.apply_resize(*id, *edge, initial_rect, event.position, state, windows);
                    return InputOutcome::consumed();
                }
            }
        }
        
        // No drag - check hover
        let desktop = state.active_desktop();
        let world = desktop.camera.screen_to_world(event.position, state.screen_size);
        
        let hovered = windows.window_at(desktop.id, world, &config.chrome)
            .map(|(id, _)| id);
        
        InputOutcome {
            consumed: false,
            hovered_window: hovered,
            focus_changed: None,
        }
    }
}
```

### Wheel Events

```rust
impl InputRouter {
    pub fn handle_wheel(
        &mut self,
        event: WheelEvent,
        state: &mut CompositorState,
        config: &CompositorConfig,
    ) -> InputOutcome {
        match state.mode {
            Mode::Desktop => {
                if event.modifiers.ctrl {
                    // Ctrl+wheel = zoom
                    let factor = 1.0 - event.delta.1 * config.zoom.speed;
                    let desktop = state.active_desktop_mut();
                    
                    // Get world position under pointer before zoom
                    let world = desktop.camera.screen_to_world(
                        event.position,
                        state.screen_size,
                    );
                    
                    // Apply zoom
                    let new_zoom = (desktop.camera.zoom * factor)
                        .clamp(config.zoom.min, config.zoom.max);
                    desktop.camera.zoom = new_zoom;
                    
                    // Adjust to keep pointer position stable
                    let new_screen = desktop.camera.world_to_screen(world, state.screen_size);
                    let correction = (
                        (event.position.0 - new_screen.0) / new_zoom,
                        (event.position.1 - new_screen.1) / new_zoom,
                    );
                    desktop.camera.center.0 -= correction.0 as f64;
                    desktop.camera.center.1 -= correction.1 as f64;
                    
                    desktop.preview_dirty = true;
                } else {
                    // Regular scroll = pan
                    let desktop = state.active_desktop_mut();
                    let scale = 1.0 / desktop.camera.zoom;
                    desktop.camera.center.0 += event.delta.0 as f64 * scale as f64;
                    desktop.camera.center.1 += event.delta.1 as f64 * scale as f64;
                    desktop.preview_dirty = true;
                }
            }
            
            Mode::Void => {
                // Pan/zoom in void
                if event.modifiers.ctrl {
                    let factor = 1.0 - event.delta.1 * config.zoom.speed;
                    state.void_camera.zoom = (state.void_camera.zoom * factor)
                        .clamp(0.1, 2.0);
                } else {
                    let scale = 1.0 / state.void_camera.zoom;
                    state.void_camera.center.0 += event.delta.0 as f64 * scale as f64;
                    state.void_camera.center.1 += event.delta.1 as f64 * scale as f64;
                }
            }
        }
        
        InputOutcome::consumed()
    }
}
```

### Keyboard Events

```rust
impl InputRouter {
    pub fn handle_keyboard(
        &mut self,
        event: KeyboardEvent,
        state: &mut CompositorState,
        windows: &WindowManager,
    ) -> InputOutcome {
        if event.kind != KeyKind::Down {
            return InputOutcome::unconsumed();
        }
        
        // Global shortcuts
        match (event.modifiers.ctrl, event.modifiers.alt, event.key.as_str()) {
            // Ctrl+` = Enter void
            (true, false, "`") => {
                return InputOutcome::consumed();
                // Compositor will handle void toggle
            }
            
            // Escape in void = exit
            (false, false, "Escape") if state.mode == Mode::Void => {
                return InputOutcome::consumed();
            }
            
            _ => {}
        }
        
        // Forward to focused window
        InputOutcome::unconsumed()
    }
}
```

---

## Touch Gesture Support (Optional)

```rust
#[cfg(feature = "touch")]
pub struct GestureEvent {
    pub kind: GestureKind,
    pub center: (f32, f32),
    pub delta: (f32, f32),
    pub scale: f32,
    pub rotation: f32,
}

#[cfg(feature = "touch")]
pub enum GestureKind {
    Pan,
    Pinch,
    Rotate,
}

#[cfg(feature = "touch")]
impl InputRouter {
    pub fn handle_gesture(
        &mut self,
        event: GestureEvent,
        state: &mut CompositorState,
    ) -> InputOutcome {
        match event.kind {
            GestureKind::Pan => {
                let desktop = state.active_desktop_mut();
                let scale = 1.0 / desktop.camera.zoom;
                desktop.camera.center.0 -= event.delta.0 as f64 * scale as f64;
                desktop.camera.center.1 -= event.delta.1 as f64 * scale as f64;
            }
            GestureKind::Pinch => {
                let desktop = state.active_desktop_mut();
                desktop.camera.zoom *= event.scale;
                desktop.camera.zoom = desktop.camera.zoom.clamp(0.1, 10.0);
            }
            GestureKind::Rotate => {
                // Not used currently
            }
        }
        
        InputOutcome::consumed()
    }
}
```

---

## Module Structure

```
input/
├── router.rs       # InputRouter, main handling
├── events.rs       # Event types (InputEvent, etc.)
├── drag.rs         # Drag state, resize logic
└── gestures.rs     # Touch gesture support (feature-gated)
```

---

*[Back to Desktop](README.md) | [Previous: Rendering](05-rendering.md) | [Next: React Surfaces](07-react-surfaces.md)*
