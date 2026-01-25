//! Transition and animation module
//!
//! Provides crossfade transitions and camera animations.

mod camera;
mod crossfade;
mod easing;

pub use camera::CameraAnimation;
pub use crossfade::{Crossfade, CrossfadeDirection};
pub use easing::{ease_in_out, ease_out_cubic};

/// Duration of crossfade transitions in milliseconds (void enter/exit)
pub const CROSSFADE_DURATION_MS: u32 = 750;

/// Duration of desktop switch transitions in milliseconds (faster for responsiveness)
pub const DESKTOP_SWITCH_DURATION_MS: u32 = 400;

/// Duration of camera animations in milliseconds
pub const CAMERA_ANIMATION_DURATION_MS: u32 = 300;
