//! View mode for desktop/void navigation

/// The current viewing mode of the desktop
///
/// The desktop can be in one of two states:
/// - **Desktop**: Viewing a single desktop with infinite zoom/pan
/// - **Void**: Zoomed out to see all desktops (the meta-layer)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// Viewing a single desktop
    Desktop {
        /// Index of the desktop being viewed
        index: usize,
    },
    /// In the Void - can see all desktops as tiles
    Void,
}

impl Default for ViewMode {
    fn default() -> Self {
        ViewMode::Desktop { index: 0 }
    }
}

impl ViewMode {
    /// Check if currently in a desktop view
    #[inline]
    pub fn is_desktop(&self) -> bool {
        matches!(self, ViewMode::Desktop { .. })
    }

    /// Check if currently in the void view
    #[inline]
    pub fn is_void(&self) -> bool {
        matches!(self, ViewMode::Void)
    }

    /// Get the desktop index if in desktop mode
    pub fn desktop_index(&self) -> Option<usize> {
        match self {
            ViewMode::Desktop { index } => Some(*index),
            ViewMode::Void => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_mode_default() {
        let mode = ViewMode::default();
        assert!(mode.is_desktop());
        assert!(!mode.is_void());
        assert_eq!(mode.desktop_index(), Some(0));
    }

    #[test]
    fn test_view_mode_void() {
        let mode = ViewMode::Void;
        assert!(!mode.is_desktop());
        assert!(mode.is_void());
        assert_eq!(mode.desktop_index(), None);
    }

    #[test]
    fn test_view_mode_desktop() {
        let mode = ViewMode::Desktop { index: 2 };
        assert!(mode.is_desktop());
        assert_eq!(mode.desktop_index(), Some(2));
    }
}
