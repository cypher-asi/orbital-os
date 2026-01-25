//! Core geometry types for the desktop environment
//!
//! These types provide basic 2D math operations for positioning,
//! sizing, and camera transformations.

mod camera;
mod rect;
mod size;
mod style;
mod vec2;

pub use camera::Camera;
pub use rect::Rect;
pub use size::Size;
pub use style::{FrameStyle, FRAME_STYLE};
pub use vec2::Vec2;
