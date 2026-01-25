//! Desktop Compositor for Zero OS
//!
//! This crate provides the core desktop environment functionality:
//! - Window management (create, close, focus, z-order)
//! - Desktop management (multiple infinite canvases)
//! - Viewport/camera transformations
//! - Input routing and hit testing
//! - Animated transitions between desktops
//!
//! ## Architecture
//!
//! The crate is organized into focused modules:
//!
//! - [`math`]: Core geometry types (`Vec2`, `Rect`, `Size`, `Camera`)
//! - [`window`]: Window lifecycle and management
//! - [`desktop`]: Desktop (workspace) management
//! - [`input`]: Input routing and drag state machine
//! - [`transition`]: Animation and transition systems
//! - [`persistence`]: State serialization for storage
//!
//! ## Example
//!
//! ```rust
//! use zos_desktop::{DesktopEngine, WindowConfig, Size, Vec2};
//!
//! let mut engine = DesktopEngine::new();
//! engine.init(1920.0, 1080.0);
//!
//! let window_id = engine.create_window(WindowConfig {
//!     title: "My Window".to_string(),
//!     position: Some(Vec2::new(100.0, 100.0)),
//!     size: Size::new(800.0, 600.0),
//!     app_id: "my-app".to_string(),
//!     ..Default::default()
//! });
//! ```
//!
//! ## Design Principles
//!
//! 1. **Pure Rust Core**: All state management is pure Rust, testable without browser
//! 2. **Time Abstraction**: Animations use injectable time sources for deterministic testing
//! 3. **Small Modules**: Each file stays under 300 lines for maintainability
//! 4. **Minimal Dependencies**: Core types have no browser dependencies

pub mod desktop;
pub mod input;
pub mod math;
pub mod persistence;
pub mod transition;
pub mod window;

mod engine;
mod view_mode;
mod viewport;

// WASM exports (only available with "wasm" feature)
#[cfg(feature = "wasm")]
mod wasm;
#[cfg(feature = "wasm")]
pub use wasm::*;

// Background renderer (only available with "wasm" feature)
#[cfg(feature = "wasm")]
pub mod background;

// Re-export core types for convenience
pub use desktop::{Desktop, DesktopId, DesktopManager, PersistedDesktop, VoidState};
pub use input::{DragState, InputResult, InputRouter};
pub use math::{Camera, FrameStyle, Rect, Size, Vec2, FRAME_STYLE};
pub use persistence::Snapshot;
pub use transition::{CameraAnimation, Crossfade, CrossfadeDirection};
pub use window::{
    Window, WindowConfig, WindowId, WindowManager, WindowRegion, WindowState, WindowType,
};

pub use engine::{DesktopEngine, WindowScreenRect};
pub use view_mode::ViewMode;
pub use viewport::Viewport;

/// Duration of crossfade transitions in milliseconds
pub use transition::CROSSFADE_DURATION_MS;

/// Duration of camera animations in milliseconds
pub use transition::CAMERA_ANIMATION_DURATION_MS;
