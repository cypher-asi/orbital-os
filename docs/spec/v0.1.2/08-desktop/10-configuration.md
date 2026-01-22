# Configuration

**Component:** 08-desktop/10-configuration  
**Status:** Specification

---

## Overview

This document specifies configuration options, feature flags, and runtime settings for the compositor.

---

## Compositor Configuration

```rust
/// Main compositor configuration
pub struct CompositorConfig {
    /// Preview settings for Void mode
    pub preview: PreviewConfig,
    
    /// Window chrome settings
    pub chrome: ChromeConfig,
    
    /// Zoom behavior
    pub zoom: ZoomConfig,
    
    /// Culling settings
    pub culling: CullingConfig,
    
    /// DOM mount settings
    pub mounts: MountConfig,
    
    /// DPI handling
    pub dpi: DpiConfig,
    
    /// Performance settings
    pub performance: PerformanceConfig,
}

impl Default for CompositorConfig {
    fn default() -> Self {
        Self {
            preview: PreviewConfig::default(),
            chrome: ChromeConfig::default(),
            zoom: ZoomConfig::default(),
            culling: CullingConfig::default(),
            mounts: MountConfig::default(),
            dpi: DpiConfig::default(),
            performance: PerformanceConfig::default(),
        }
    }
}
```

---

## Preview Configuration

```rust
/// Desktop preview settings (for Void mode)
pub struct PreviewConfig {
    /// Preview texture size (width, height)
    pub size: (u32, u32),
    
    /// Maximum preview renders per frame
    pub budget_per_frame: u32,
    
    /// Minimum time between preview updates (ms)
    pub update_interval_ms: u32,
    
    /// Enable preview rendering
    pub enabled: bool,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            size: (320, 180),
            budget_per_frame: 2,
            update_interval_ms: 100,
            enabled: true,
        }
    }
}
```

---

## Chrome Configuration

```rust
/// Window chrome (frame) configuration
pub struct ChromeConfig {
    /// Title bar height in logical pixels
    pub title_bar_height: f32,
    
    /// Corner radius
    pub corner_radius: f32,
    
    /// Border width
    pub border_width: f32,
    
    /// Shadow radius
    pub shadow_radius: f32,
    
    /// Shadow opacity (0.0 - 1.0)
    pub shadow_opacity: f32,
    
    /// Resize handle size
    pub resize_handle_size: f32,
    
    /// Title bar colors
    pub colors: ChromeColors,
}

pub struct ChromeColors {
    /// Active window title bar
    pub active_title_bar: (f32, f32, f32, f32),
    
    /// Inactive window title bar
    pub inactive_title_bar: (f32, f32, f32, f32),
    
    /// Title text color
    pub title_text: (f32, f32, f32, f32),
    
    /// Close button hover
    pub close_button_hover: (f32, f32, f32, f32),
    
    /// Border color
    pub border: (f32, f32, f32, f32),
    
    /// Shadow color
    pub shadow: (f32, f32, f32, f32),
}

impl Default for ChromeConfig {
    fn default() -> Self {
        Self {
            title_bar_height: 32.0,
            corner_radius: 8.0,
            border_width: 1.0,
            shadow_radius: 16.0,
            shadow_opacity: 0.3,
            resize_handle_size: 8.0,
            colors: ChromeColors::default(),
        }
    }
}

impl Default for ChromeColors {
    fn default() -> Self {
        Self {
            active_title_bar: (0.15, 0.15, 0.15, 1.0),
            inactive_title_bar: (0.12, 0.12, 0.12, 1.0),
            title_text: (1.0, 1.0, 1.0, 1.0),
            close_button_hover: (0.8, 0.2, 0.2, 1.0),
            border: (0.2, 0.2, 0.2, 1.0),
            shadow: (0.0, 0.0, 0.0, 0.3),
        }
    }
}
```

---

## Zoom Configuration

```rust
/// Zoom behavior configuration
pub struct ZoomConfig {
    /// Minimum zoom level
    pub min: f32,
    
    /// Maximum zoom level
    pub max: f32,
    
    /// Zoom speed multiplier
    pub speed: f32,
    
    /// Enable smooth zoom animation
    pub smooth: bool,
    
    /// Smooth zoom duration (ms)
    pub smooth_duration_ms: f32,
}

impl Default for ZoomConfig {
    fn default() -> Self {
        Self {
            min: 0.1,
            max: 10.0,
            speed: 0.001,
            smooth: true,
            smooth_duration_ms: 150.0,
        }
    }
}
```

---

## Culling Configuration

```rust
/// Culling settings
pub struct CullingConfig {
    /// Margin around viewport for culling (logical pixels)
    pub margin: f32,
    
    /// Enable window culling
    pub enabled: bool,
}

impl Default for CullingConfig {
    fn default() -> Self {
        Self {
            margin: 100.0,
            enabled: true,
        }
    }
}
```

---

## Mount Configuration

```rust
/// DOM mount settings
pub struct MountConfig {
    /// Hide mounts when in Void mode
    pub hide_in_void: bool,
    
    /// Update frequency
    pub update_frequency: MountUpdateFrequency,
    
    /// Chrome capture mode
    pub chrome_capture: ChromeCaptureMode,
}

/// Mount update frequency
#[derive(Clone, Copy, Debug)]
pub enum MountUpdateFrequency {
    /// Update every frame
    EveryFrame,
    
    /// Update only when window moves/resizes
    OnChange,
}

/// Chrome capture strategy
#[derive(Clone, Copy, Debug)]
pub enum ChromeCaptureMode {
    /// Chrome on GPU canvas only
    GpuOnly,
    
    /// DOM overlay for chrome interaction
    DomOverlay,
}

impl Default for MountConfig {
    fn default() -> Self {
        Self {
            hide_in_void: true,
            update_frequency: MountUpdateFrequency::EveryFrame,
            chrome_capture: ChromeCaptureMode::GpuOnly,
        }
    }
}
```

---

## DPI Configuration

```rust
/// DPI handling configuration
pub struct DpiConfig {
    /// DPI policy
    pub policy: DpiPolicy,
    
    /// Scale factor override (if Fixed)
    pub scale_override: Option<f32>,
}

/// DPI policy
#[derive(Clone, Copy, Debug)]
pub enum DpiPolicy {
    /// Automatically detect from devicePixelRatio
    Auto,
    
    /// Fixed scale factor
    Fixed,
}

impl Default for DpiConfig {
    fn default() -> Self {
        Self {
            policy: DpiPolicy::Auto,
            scale_override: None,
        }
    }
}
```

---

## Performance Configuration

```rust
/// Performance settings
pub struct PerformanceConfig {
    /// Target frame rate
    pub target_fps: u32,
    
    /// Enable instanced rendering
    pub instanced_rendering: bool,
    
    /// Maximum windows to render per frame
    pub max_windows_per_frame: Option<u32>,
    
    /// Enable tracing spans
    pub tracing_enabled: bool,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            target_fps: 60,
            instanced_rendering: true,
            max_windows_per_frame: None,
            tracing_enabled: cfg!(debug_assertions),
        }
    }
}
```

---

## Feature Flags

Features are enabled via Cargo features:

```toml
[features]
default = ["react-dom"]

# React DOM mount support
react-dom = []

# Post-processing effects (blur, glass)
postfx = []

# Touch/gesture support
touch = []

# State persistence
persistence = ["serde", "serde_json"]

# Performance tracing
tracing = ["tracing"]
```

### Feature Usage

```rust
// React DOM support
#[cfg(feature = "react-dom")]
pub mod ui;

#[cfg(feature = "react-dom")]
pub use ui::{UiBridge, WindowMount};

// Post-processing effects
#[cfg(feature = "postfx")]
pub mod effects;

// Touch gestures
#[cfg(feature = "touch")]
pub use input::GestureEvent;

// Persistence
#[cfg(feature = "persistence")]
pub use persistence::{Snapshot, PersistenceBackend};
```

---

## Runtime Configuration Changes

Some settings can be changed at runtime:

```rust
impl Compositor {
    /// Update chrome configuration
    pub fn set_chrome_config(&mut self, config: ChromeConfig) {
        self.config.chrome = config;
        // Mark all windows for redraw
        self.renderer.invalidate_all();
    }
    
    /// Update zoom configuration
    pub fn set_zoom_config(&mut self, config: ZoomConfig) {
        self.config.zoom = config;
    }
    
    /// Update preview configuration
    pub fn set_preview_config(&mut self, config: PreviewConfig) {
        self.config.preview = config;
        // Invalidate all previews
        for desktop in self.state.desktops.values_mut() {
            desktop.preview_dirty = true;
        }
    }
    
    /// Update mount configuration
    pub fn set_mount_config(&mut self, config: MountConfig) {
        self.config.mounts = config;
        // Update all mounts
        self.ui.reconfigure(&self.state, &config);
    }
}
```

---

## Configuration Validation

```rust
impl CompositorConfig {
    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Zoom limits
        if self.zoom.min <= 0.0 {
            return Err(Error::InvalidConfig("zoom.min must be positive".into()));
        }
        if self.zoom.max < self.zoom.min {
            return Err(Error::InvalidConfig("zoom.max must be >= zoom.min".into()));
        }
        
        // Preview size
        if self.preview.size.0 == 0 || self.preview.size.1 == 0 {
            return Err(Error::InvalidConfig("preview.size must be non-zero".into()));
        }
        
        // Chrome dimensions
        if self.chrome.title_bar_height < 0.0 {
            return Err(Error::InvalidConfig("title_bar_height must be non-negative".into()));
        }
        if self.chrome.corner_radius < 0.0 {
            return Err(Error::InvalidConfig("corner_radius must be non-negative".into()));
        }
        
        Ok(())
    }
}
```

---

## Builder Pattern

```rust
/// Configuration builder
pub struct CompositorConfigBuilder {
    config: CompositorConfig,
}

impl CompositorConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: CompositorConfig::default(),
        }
    }
    
    pub fn preview_size(mut self, width: u32, height: u32) -> Self {
        self.config.preview.size = (width, height);
        self
    }
    
    pub fn corner_radius(mut self, radius: f32) -> Self {
        self.config.chrome.corner_radius = radius;
        self
    }
    
    pub fn zoom_limits(mut self, min: f32, max: f32) -> Self {
        self.config.zoom.min = min;
        self.config.zoom.max = max;
        self
    }
    
    pub fn hide_mounts_in_void(mut self, hide: bool) -> Self {
        self.config.mounts.hide_in_void = hide;
        self
    }
    
    pub fn build(self) -> Result<CompositorConfig> {
        self.config.validate()?;
        Ok(self.config)
    }
}
```

---

## Module Structure

```
config/
├── mod.rs          # CompositorConfig, Default impl
├── preview.rs      # PreviewConfig
├── chrome.rs       # ChromeConfig, ChromeColors
├── zoom.rs         # ZoomConfig
├── mounts.rs       # MountConfig
└── builder.rs      # CompositorConfigBuilder
```

---

*[Back to Desktop](README.md) | [Previous: Persistence](09-persistence.md) | [Next: Engineering](11-engineering.md)*
