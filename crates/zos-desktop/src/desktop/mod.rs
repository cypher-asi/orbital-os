//! Desktop management module
//!
//! Provides desktop (workspace) management with multiple infinite canvases.

mod manager;
mod types;
mod view_mode;
mod void;

pub use manager::DesktopManager;
pub use types::{Desktop, PersistedDesktop};
pub use view_mode::ViewMode;
pub use void::VoidState;

// Re-export DesktopId from crate types module for backward compatibility
pub use crate::types::DesktopId;
