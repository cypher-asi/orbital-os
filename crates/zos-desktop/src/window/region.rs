//! Window region for hit testing

/// Region of a window for hit testing
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowRegion {
    /// Title bar area (for dragging)
    TitleBar,
    /// Content area (for interaction)
    Content,
    /// Close button
    CloseButton,
    /// Minimize button
    MinimizeButton,
    /// Maximize button
    MaximizeButton,
    /// North (top) resize edge
    ResizeN,
    /// South (bottom) resize edge
    ResizeS,
    /// East (right) resize edge
    ResizeE,
    /// West (left) resize edge
    ResizeW,
    /// Northeast corner
    ResizeNE,
    /// Northwest corner
    ResizeNW,
    /// Southeast corner
    ResizeSE,
    /// Southwest corner
    ResizeSW,
}

impl WindowRegion {
    /// Check if this is a resize region
    #[inline]
    pub fn is_resize(&self) -> bool {
        matches!(
            self,
            WindowRegion::ResizeN
                | WindowRegion::ResizeS
                | WindowRegion::ResizeE
                | WindowRegion::ResizeW
                | WindowRegion::ResizeNE
                | WindowRegion::ResizeNW
                | WindowRegion::ResizeSE
                | WindowRegion::ResizeSW
        )
    }

    /// Check if this is a corner resize region
    #[inline]
    pub fn is_corner(&self) -> bool {
        matches!(
            self,
            WindowRegion::ResizeNE
                | WindowRegion::ResizeNW
                | WindowRegion::ResizeSE
                | WindowRegion::ResizeSW
        )
    }

    /// Get CSS cursor style for this region
    pub fn cursor(&self) -> &'static str {
        match self {
            WindowRegion::TitleBar => "move",
            WindowRegion::Content => "default",
            WindowRegion::CloseButton
            | WindowRegion::MinimizeButton
            | WindowRegion::MaximizeButton => "pointer",
            WindowRegion::ResizeN | WindowRegion::ResizeS => "ns-resize",
            WindowRegion::ResizeE | WindowRegion::ResizeW => "ew-resize",
            WindowRegion::ResizeNE | WindowRegion::ResizeSW => "nesw-resize",
            WindowRegion::ResizeNW | WindowRegion::ResizeSE => "nwse-resize",
        }
    }
}
