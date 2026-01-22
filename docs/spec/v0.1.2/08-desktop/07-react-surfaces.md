# React Surfaces

**Component:** 08-desktop/07-react-surfaces  
**Status:** Specification

---

## Overview

React surfaces provide DOM mount elements aligned to window content rects. This enables React applications to render UI inside compositor-managed windows while the compositor handles chrome, positioning, and z-ordering.

---

## Surface Kinds

```rust
/// Surface rendering kind
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    /// Content rendered via WebGPU texture only
    Gpu,
    
    /// Content rendered via React DOM mount
    ReactDom,
    
    /// GPU underlay with React overlay
    Hybrid,
}
```

| Kind | Chrome | Content | Use Case |
|------|--------|---------|----------|
| `Gpu` | WebGPU | WebGPU texture | Games, visualizations |
| `ReactDom` | WebGPU | DOM element | Standard apps |
| `Hybrid` | WebGPU | Both | Apps needing GPU underlays |

---

## UI Bridge

The UI Bridge manages DOM mount lifecycle and alignment:

```rust
/// UI Bridge for DOM mount management
pub struct UiBridge {
    /// Root element for mounts
    ui_root: web_sys::HtmlElement,
    
    /// Active mounts by window ID
    mounts: HashMap<WindowId, WindowMount>,
    
    /// Current DPI scale
    dpi: f32,
}

/// A DOM mount for a window
pub struct WindowMount {
    /// The mount element
    element: web_sys::HtmlElement,
    
    /// Current screen rect
    screen_rect: ScreenRect,
    
    /// Current z-index
    z_index: i32,
    
    /// Whether currently visible
    visible: bool,
    
    /// Custom attributes
    attrs: MountAttributes,
    
    /// Clip mode
    clip: ClipMode,
}

pub struct MountAttributes {
    pub class_name: Option<String>,
    pub data_attrs: Vec<(String, String)>,
}

pub enum ClipMode {
    /// Clip content to window bounds
    Clip,
    
    /// Allow content overflow
    Visible,
}
```

---

## Mount Lifecycle

### Creation

```rust
impl UiBridge {
    /// Create a mount for a window
    pub fn create_mount(&mut self, window: &Window) -> Result<()> {
        if !matches!(window.surface, SurfaceKind::ReactDom | SurfaceKind::Hybrid) {
            return Ok(());
        }
        
        if self.mounts.contains_key(&window.id) {
            return Ok(());
        }
        
        // Create element
        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or(Error::NoDom)?;
        
        let element = document
            .create_element("div")
            .map_err(|_| Error::DomError)?
            .dyn_into::<web_sys::HtmlElement>()
            .map_err(|_| Error::DomError)?;
        
        // Set initial styles
        self.apply_base_styles(&element)?;
        
        // Append to root
        self.ui_root
            .append_child(&element)
            .map_err(|_| Error::DomError)?;
        
        let mount = WindowMount {
            element,
            screen_rect: ScreenRect::default(),
            z_index: 0,
            visible: true,
            attrs: MountAttributes::default(),
            clip: ClipMode::Clip,
        };
        
        self.mounts.insert(window.id, mount);
        Ok(())
    }
    
    fn apply_base_styles(&self, element: &web_sys::HtmlElement) -> Result<()> {
        let style = element.style();
        style.set_property("position", "absolute")?;
        style.set_property("overflow", "hidden")?;
        style.set_property("pointer-events", "auto")?;
        style.set_property("box-sizing", "border-box")?;
        Ok(())
    }
}
```

### Destruction

```rust
impl UiBridge {
    /// Remove a window's mount
    pub fn destroy_mount(&mut self, window_id: WindowId) {
        if let Some(mount) = self.mounts.remove(&window_id) {
            let _ = mount.element.remove();
        }
    }
    
    /// Remove all mounts
    pub fn destroy_all_mounts(&mut self) {
        for (_, mount) in self.mounts.drain() {
            let _ = mount.element.remove();
        }
    }
}
```

---

## Mount Updates

### Per-Frame Update

```rust
impl UiBridge {
    /// Update all mounts based on current state
    pub fn update(
        &mut self,
        state: &CompositorState,
        windows: &WindowManager,
        config: &MountConfig,
    ) {
        match state.mode {
            Mode::Desktop => self.update_desktop_mode(state, windows, config),
            Mode::Void => self.update_void_mode(config),
        }
    }
    
    fn update_desktop_mode(
        &mut self,
        state: &CompositorState,
        windows: &WindowManager,
        config: &MountConfig,
    ) {
        let desktop = state.active_desktop();
        
        for (&window_id, mount) in &mut self.mounts {
            let window = match windows.get(window_id) {
                Some(w) if w.desktop == desktop.id => w,
                _ => {
                    // Window not on active desktop - hide
                    self.set_mount_visible(mount, false);
                    continue;
                }
            };
            
            // Calculate screen rect for content area
            let content_rect = self.calculate_content_screen_rect(
                window,
                &desktop.camera,
                state.screen_size,
                &config.chrome,
            );
            
            // Check if visible
            if !self.is_rect_visible(&content_rect, state.screen_size) {
                self.set_mount_visible(mount, false);
                continue;
            }
            
            // Update mount
            self.set_mount_visible(mount, true);
            self.update_mount_rect(mount, &content_rect);
            self.update_mount_z(mount, window.z_order);
        }
    }
    
    fn update_void_mode(&mut self, config: &MountConfig) {
        if config.hide_in_void {
            for mount in self.mounts.values_mut() {
                self.set_mount_visible(mount, false);
            }
        }
    }
}
```

### Rect Calculation

```rust
impl UiBridge {
    fn calculate_content_screen_rect(
        &self,
        window: &Window,
        camera: &Camera,
        screen_size: (f32, f32),
        chrome: &ChromeConfig,
    ) -> ScreenRect {
        // Content rect in world space (below title bar)
        let content_world = (
            window.position.0,
            window.position.1 + chrome.title_bar_height as f64,
        );
        let content_size = (
            window.size.0,
            window.size.1 - chrome.title_bar_height,
        );
        
        // Convert to screen space
        let screen_pos = camera.world_to_screen(content_world, screen_size);
        let screen_size = (
            content_size.0 * camera.zoom,
            content_size.1 * camera.zoom,
        );
        
        ScreenRect {
            x: screen_pos.0,
            y: screen_pos.1,
            width: screen_size.0,
            height: screen_size.1,
        }
    }
}
```

### Style Application

```rust
impl UiBridge {
    fn set_mount_visible(&self, mount: &mut WindowMount, visible: bool) {
        if mount.visible == visible {
            return;
        }
        
        mount.visible = visible;
        let style = mount.element.style();
        let _ = style.set_property(
            "display",
            if visible { "block" } else { "none" },
        );
    }
    
    fn update_mount_rect(&self, mount: &mut WindowMount, rect: &ScreenRect) {
        // Skip if unchanged (optimization)
        if mount.screen_rect.approx_eq(rect) {
            return;
        }
        
        mount.screen_rect = rect.clone();
        
        let style = mount.element.style();
        let dpi = self.dpi;
        
        // Apply CSS transform for sub-pixel positioning
        let _ = style.set_property("left", &format!("{}px", rect.x / dpi));
        let _ = style.set_property("top", &format!("{}px", rect.y / dpi));
        let _ = style.set_property("width", &format!("{}px", rect.width / dpi));
        let _ = style.set_property("height", &format!("{}px", rect.height / dpi));
    }
    
    fn update_mount_z(&self, mount: &mut WindowMount, z_order: u32) {
        let z_index = z_order as i32 + 1000; // Base offset for z-index
        
        if mount.z_index == z_index {
            return;
        }
        
        mount.z_index = z_index;
        let _ = mount.element.style().set_property(
            "z-index",
            &z_index.to_string(),
        );
    }
}
```

---

## Mount Attributes

```rust
impl UiBridge {
    /// Set custom attributes on a mount
    pub fn set_mount_attributes(
        &mut self,
        window_id: WindowId,
        attrs: MountAttributes,
    ) {
        if let Some(mount) = self.mounts.get_mut(&window_id) {
            // Update class name
            if let Some(class) = &attrs.class_name {
                let _ = mount.element.set_class_name(class);
            }
            
            // Update data attributes
            for (key, value) in &attrs.data_attrs {
                let _ = mount.element.set_attribute(
                    &format!("data-{}", key),
                    value,
                );
            }
            
            mount.attrs = attrs;
        }
    }
    
    /// Set mount clip mode
    pub fn set_mount_clip(&mut self, window_id: WindowId, clip: ClipMode) {
        if let Some(mount) = self.mounts.get_mut(&window_id) {
            mount.clip = clip;
            let overflow = match clip {
                ClipMode::Clip => "hidden",
                ClipMode::Visible => "visible",
            };
            let _ = mount.element.style().set_property("overflow", overflow);
        }
    }
}
```

---

## Public API

```rust
impl Compositor {
    /// Get DOM mount element for a window
    pub fn mount_element(&self, id: WindowId) -> Option<web_sys::HtmlElement> {
        self.ui.mounts.get(&id).map(|m| m.element.clone())
    }
    
    /// Set custom attributes on mount element
    pub fn set_mount_attributes(&mut self, id: WindowId, attrs: MountAttributes) {
        self.ui.set_mount_attributes(id, attrs);
    }
    
    /// Set mount clipping mode
    pub fn set_mount_clip(&mut self, id: WindowId, clip: ClipMode) {
        self.ui.set_mount_clip(id, clip);
    }
}
```

---

## Chrome Capture Modes

For ReactDom windows, input on chrome areas must be handled:

```rust
/// Chrome capture strategy
pub enum ChromeCaptureMode {
    /// Chrome rendered on canvas; mount positioned below title bar
    GpuOnly,
    
    /// Transparent overlay div for drag/resize handles
    DomOverlay,
}
```

### GpuOnly (Default)

- Title bar and resize handles rendered by WebGPU
- React mount positioned at content rect (below title bar)
- Pointer events on canvas pass through transparent areas to mounts
- Chrome hit-testing done in compositor

### DomOverlay (Optional)

- Transparent overlay div created for each window
- Overlay covers title bar and resize handles
- Overlay captures pointer events for drag/resize
- More complex but allows CSS-styled chrome

---

## React Integration Example

```typescript
// TypeScript side - rendering into mount

function mountWindowContent(windowId: number, content: ReactNode) {
  const mount = compositor.mount_element(windowId);
  if (!mount) return;
  
  // Set attributes for styling
  compositor.set_mount_attributes(windowId, {
    class_name: "window-content",
    data_attrs: [["window-id", windowId.toString()]],
  });
  
  // Render React into mount
  const root = createRoot(mount);
  root.render(
    <WindowContext.Provider value={{ windowId }}>
      {content}
    </WindowContext.Provider>
  );
  
  return () => root.unmount();
}
```

---

## Module Structure

```
ui/
├── bridge.rs       # UiBridge, update logic
├── mount.rs        # WindowMount, lifecycle
└── styles.rs       # Style application helpers
```

---

*[Back to Desktop](README.md) | [Previous: Input](06-input.md) | [Next: Transitions](08-transitions.md)*
