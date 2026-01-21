//! Window management module
//!
//! Provides window lifecycle, focus management, and hit testing.

#[allow(clippy::module_inception)]
mod window;
mod config;
mod region;
mod manager;

pub use window::{Window, WindowState};
pub use config::WindowConfig;
pub use region::WindowRegion;
pub use manager::WindowManager;

/// Unique window identifier
pub type WindowId = u64;
