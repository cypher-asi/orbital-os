//! Window management module
//!
//! Provides window lifecycle, focus management, and hit testing.

mod config;
mod manager;
mod region;
#[allow(clippy::module_inception)]
mod window;

pub use config::WindowConfig;
pub use manager::WindowManager;
pub use region::WindowRegion;
pub use window::{Window, WindowState, WindowType};

/// Unique window identifier
pub type WindowId = u64;
