# Public API

**Component:** 08-desktop/01-api  
**Status:** Specification

---

## Overview

This document specifies the public API surface of `orbital-desktop`. The API is designed to be minimal, stable, and ergonomic for embedding.

---

## Top-Level Types

The crate must expose these types:

```rust
// Core compositor
pub struct Compositor;
pub struct CompositorConfig;

// Identifiers
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DesktopId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

// Specifications
pub struct DesktopSpec;
pub struct WindowSpec;
pub struct TransitionSpec;

// Input
pub enum InputEvent;
pub struct InputOutcome;

// Surface types
pub enum SurfaceKind;

// Events
pub enum Event;

// Persistence
pub struct Snapshot;
```

---

## Initialization

### `Compositor::new`

```rust
impl Compositor {
    /// Create a new compositor instance.
    ///
    /// # Arguments
    /// * `config` - Compositor configuration
    /// * `canvas` - WebGPU render target
    /// * `ui_root` - DOM element for React mount overlays
    ///
    /// # Errors
    /// Returns error if WebGPU initialization fails.
    pub async fn new(
        config: CompositorConfig,
        canvas: web_sys::HtmlCanvasElement,
        ui_root: web_sys::HtmlElement,
    ) -> Result<Self>;
}
```

### `CompositorConfig`

```rust
pub struct CompositorConfig {
    /// Preview texture size for Void mode portals
    pub preview_size: (u32, u32),
    
    /// Max preview renders per frame (budget)
    pub preview_budget: u32,
    
    /// Window chrome settings
    pub chrome: ChromeConfig,
    
    /// Zoom limits and speed
    pub zoom: ZoomConfig,
    
    /// Culling thresholds
    pub culling: CullingConfig,
    
    /// DOM mount strategy
    pub mounts: MountConfig,
    
    /// DPI handling policy
    pub dpi_policy: DpiPolicy,
}

pub struct ChromeConfig {
    pub corner_radius: f32,
    pub title_bar_height: f32,
    pub shadow_radius: f32,
    pub shadow_opacity: f32,
}

pub struct ZoomConfig {
    pub min: f32,
    pub max: f32,
    pub speed: f32,
}

pub struct CullingConfig {
    pub margin: f32,
}

pub struct MountConfig {
    pub hide_in_void: bool,
    pub update_frequency: MountUpdateFrequency,
}

pub enum MountUpdateFrequency {
    EveryFrame,
    OnChange,
}

pub enum DpiPolicy {
    Auto,
    Fixed(f32),
}
```

---

## Frame Lifecycle

### Update and Render

```rust
impl Compositor {
    /// Advance simulation by delta time.
    ///
    /// Processes animations, transitions, and inertia.
    pub fn update(&mut self, dt_ms: f32);
    
    /// Render current scene to canvas.
    pub fn render(&mut self);
    
    /// Combined update + render convenience method.
    pub fn frame(&mut self, dt_ms: f32) {
        self.update(dt_ms);
        self.render();
    }
}
```

---

## Input Routing

### `handle_input`

```rust
impl Compositor {
    /// Process an input event.
    ///
    /// Returns outcome indicating how the event was handled.
    pub fn handle_input(&mut self, event: InputEvent) -> InputOutcome;
}

/// Input event types
pub enum InputEvent {
    Pointer(PointerEvent),
    Wheel(WheelEvent),
    Keyboard(KeyboardEvent),
}

pub struct PointerEvent {
    pub kind: PointerKind,
    pub position: (f32, f32),
    pub button: Option<PointerButton>,
    pub modifiers: Modifiers,
}

pub enum PointerKind {
    Down,
    Move,
    Up,
    Cancel,
}

pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

pub struct WheelEvent {
    pub delta: (f32, f32),
    pub position: (f32, f32),
    pub modifiers: Modifiers,
}

pub struct KeyboardEvent {
    pub kind: KeyKind,
    pub key: String,
    pub code: String,
    pub modifiers: Modifiers,
}

pub enum KeyKind {
    Down,
    Up,
}

pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

/// Result of input handling
pub struct InputOutcome {
    /// Whether the event was consumed
    pub consumed: bool,
    
    /// Window under pointer (if any)
    pub hovered_window: Option<WindowId>,
    
    /// Focus change (if any)
    pub focus_changed: Option<Option<WindowId>>,
}
```

---

## Desktop Management

```rust
impl Compositor {
    /// Create a new desktop.
    pub fn desktop_create(&mut self, spec: DesktopSpec) -> DesktopId;
    
    /// Remove a desktop and all its windows.
    ///
    /// # Errors
    /// Returns error if desktop doesn't exist or is the last desktop.
    pub fn desktop_remove(&mut self, id: DesktopId) -> Result<()>;
    
    /// List all desktop IDs.
    pub fn desktop_list(&self) -> Vec<DesktopId>;
    
    /// Get active desktop ID.
    pub fn desktop_active(&self) -> DesktopId;
    
    /// Set active desktop (with optional transition).
    pub fn desktop_set_active(&mut self, id: DesktopId);
    
    /// Enter Void overview mode.
    pub fn enter_void(&mut self);
    
    /// Exit Void mode to target desktop.
    pub fn exit_void(&mut self, target: DesktopId);
    
    /// Set Void layout configuration.
    pub fn void_layout_set(&mut self, layout: VoidLayout);
}

pub struct DesktopSpec {
    pub name: String,
    pub background: Option<BackgroundSpec>,
}

pub enum BackgroundSpec {
    Solid { color: (f32, f32, f32, f32) },
    Gradient { stops: Vec<GradientStop> },
    Procedural(ProceduralBackground),
}

/// Procedural animated backgrounds (see [13-backgrounds.md](13-backgrounds.md))
pub enum ProceduralBackground {
    /// Subtle animated film grain on near-black (default)
    Grain,
    /// Animated misty smoke with glass overlay effect
    Mist,
}

pub struct GradientStop {
    pub position: f32,
    pub color: (f32, f32, f32, f32),
}

pub enum VoidLayout {
    StripHorizontal { gap: f32, portal_size: (f32, f32) },
    Grid { cols: u32, gap: f32, portal_size: (f32, f32) },
}
```

---

## Window Management

```rust
impl Compositor {
    /// Create a new window on a desktop.
    pub fn window_create(
        &mut self,
        desktop: DesktopId,
        spec: WindowSpec,
    ) -> WindowId;
    
    /// Close a window.
    pub fn window_close(&mut self, id: WindowId);
    
    /// Move window to new position (world coordinates).
    pub fn window_move(&mut self, id: WindowId, x: f64, y: f64);
    
    /// Resize window.
    pub fn window_resize(&mut self, id: WindowId, width: f32, height: f32);
    
    /// Set window z-order.
    pub fn window_set_z(&mut self, id: WindowId, z: u32);
    
    /// Focus a window (brings to front by default).
    pub fn window_focus(&mut self, id: WindowId);
    
    /// Set window surface kind.
    pub fn window_set_surface(&mut self, id: WindowId, kind: SurfaceKind);
    
    /// Get window info.
    pub fn window_get(&self, id: WindowId) -> Option<WindowInfo>;
}

pub struct WindowSpec {
    pub title: String,
    pub position: Option<(f64, f64)>,
    pub size: (f32, f32),
    pub min_size: Option<(f32, f32)>,
    pub max_size: Option<(f32, f32)>,
    pub surface: SurfaceKind,
}

pub enum SurfaceKind {
    /// Content rendered via WebGPU texture
    Gpu,
    
    /// Content rendered via React DOM mount
    ReactDom,
    
    /// Both GPU underlay and React overlay
    Hybrid,
}

pub struct WindowInfo {
    pub id: WindowId,
    pub desktop: DesktopId,
    pub title: String,
    pub world_rect: WorldRect,
    pub screen_rect: ScreenRect,
    pub z_order: u32,
    pub focused: bool,
    pub surface: SurfaceKind,
}

pub struct WorldRect {
    pub x: f64,
    pub y: f64,
    pub width: f32,
    pub height: f32,
}

pub struct ScreenRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
```

---

## React Surface Integration

```rust
impl Compositor {
    /// Get DOM mount element for a window.
    ///
    /// Returns `None` if window doesn't exist or isn't `ReactDom`/`Hybrid`.
    pub fn mount_element(&self, id: WindowId) -> Option<web_sys::HtmlElement>;
    
    /// Set custom attributes on mount element.
    pub fn set_mount_attributes(
        &mut self,
        id: WindowId,
        attrs: MountAttributes,
    );
    
    /// Set mount clipping mode.
    pub fn set_mount_clip(&mut self, id: WindowId, clip: ClipMode);
}

pub struct MountAttributes {
    pub class_name: Option<String>,
    pub data_attrs: Vec<(String, String)>,
}

pub enum ClipMode {
    /// Hard clip to content rect
    Clip,
    
    /// Allow overflow
    Visible,
}
```

---

## Events

```rust
impl Compositor {
    /// Drain pending events (polling model).
    pub fn drain_events(&mut self) -> Vec<Event>;
}

pub enum Event {
    FocusChanged {
        window: Option<WindowId>,
    },
    
    DesktopChanged {
        desktop: DesktopId,
    },
    
    ModeChanged {
        mode: Mode,
    },
    
    WindowRequestedClose {
        window: WindowId,
    },
    
    WindowResized {
        window: WindowId,
        rect: WorldRect,
    },
    
    WindowMoved {
        window: WindowId,
        rect: WorldRect,
    },
    
    TransitionFinished {
        kind: TransitionKind,
    },
}

pub enum Mode {
    Desktop,
    Void,
}

pub enum TransitionKind {
    EnterVoid,
    ExitVoid,
    DesktopSwitch,
}
```

---

## State Persistence

```rust
impl Compositor {
    /// Create serializable snapshot of current state.
    pub fn snapshot(&self) -> Snapshot;
    
    /// Restore state from snapshot.
    ///
    /// # Errors
    /// Returns error if snapshot is invalid or incompatible.
    pub fn restore(&mut self, snapshot: Snapshot) -> Result<()>;
}

/// Serializable compositor state
#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub desktops: Vec<DesktopSnapshot>,
    pub active_desktop: DesktopId,
    pub void_layout: VoidLayout,
}

#[derive(Serialize, Deserialize)]
pub struct DesktopSnapshot {
    pub id: DesktopId,
    pub name: String,
    pub camera: CameraSnapshot,
    pub windows: Vec<WindowSnapshot>,
}

#[derive(Serialize, Deserialize)]
pub struct CameraSnapshot {
    pub center_x: f64,
    pub center_y: f64,
    pub zoom: f32,
}

#[derive(Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub id: WindowId,
    pub title: String,
    pub x: f64,
    pub y: f64,
    pub width: f32,
    pub height: f32,
    pub z_order: u32,
    pub surface: SurfaceKind,
}
```

---

## Persistence Backend Trait

```rust
/// Optional trait for persistence backends.
///
/// Not implemented by the crate; host provides implementation.
pub trait PersistenceBackend {
    fn save(&self, snapshot: &Snapshot) -> Result<()>;
    fn load(&self) -> Result<Option<Snapshot>>;
}
```

---

## Error Type

```rust
/// Crate-local error type
#[derive(Debug)]
pub enum Error {
    WebGpuInit(String),
    InvalidDesktop(DesktopId),
    InvalidWindow(WindowId),
    LastDesktop,
    InvalidSnapshot(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

---

*[Back to Desktop](README.md) | [Next: Compositor](02-compositor.md)*
