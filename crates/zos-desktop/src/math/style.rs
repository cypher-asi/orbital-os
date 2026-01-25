//! Frame style constants

/// Frame style constants for window chrome
pub struct FrameStyle {
    pub title_bar_height: f32,
    pub border_radius: f32,
    pub border_width: f32,
    pub shadow_blur: f32,
    pub shadow_offset_y: f32,
    pub resize_handle_size: f32,
    pub button_size: f32,
    pub button_spacing: f32,
    pub button_margin: f32,
}

/// Default frame style matching the UI design
pub const FRAME_STYLE: FrameStyle = FrameStyle {
    title_bar_height: 34.0,
    border_radius: 0.0,
    border_width: 1.0,
    shadow_blur: 20.0,
    shadow_offset_y: 4.0,
    resize_handle_size: 8.0,
    button_size: 32.0, // ZUI --control-height-sm
    button_spacing: 0.0,
    button_margin: 0.0,
};
