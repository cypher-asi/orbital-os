//! Window management module
//!
//! Provides window lifecycle, focus management, and hit testing.

mod config;
mod manager;
mod region;
mod types;

pub use config::WindowConfig;
pub use manager::WindowManager;
pub use region::WindowRegion;
pub use types::{Window, WindowState, WindowType};

// Re-export WindowId from crate types module for backward compatibility
pub use crate::types::WindowId;
