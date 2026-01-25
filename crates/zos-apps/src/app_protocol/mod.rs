//! App Protocol
//!
//! Versioned, platform-agnostic IPC protocol for communication between
//! app backends (WASM) and UI surfaces (React).
//!
//! # Wire Format
//!
//! All messages use this envelope:
//!
//! ```text
//! ┌─────────┬──────────┬─────────────┬─────────────────────┐
//! │ version │ type_tag │ payload_len │       payload       │
//! │  (u8)   │   (u8)   │    (u16)    │      (bytes)        │
//! └─────────┴──────────┴─────────────┴─────────────────────┘
//!    1 byte    1 byte     2 bytes      0-65535 bytes
//! ```

mod calculator;
mod clock;
mod input;
mod settings;
mod terminal;
mod wire;

pub use calculator::CalculatorState;
pub use clock::ClockState;
pub use input::InputEvent;
pub use settings::{SettingsArea, SettingsState};
pub use terminal::{
    InputAction, TerminalInput, TerminalState, MSG_CONSOLE_INPUT, TYPE_TERMINAL_INPUT,
    TYPE_TERMINAL_STATE,
};
pub use wire::{decode_envelope, encode_envelope, Envelope, PROTOCOL_VERSION};

/// Message tags for Backend ↔ UI communication
pub mod tags {
    /// App → UI: State update
    pub const MSG_APP_STATE: u32 = 0x2000;

    /// UI → App: User input event
    pub const MSG_APP_INPUT: u32 = 0x2001;

    /// UI → App: UI surface ready notification
    pub const MSG_UI_READY: u32 = 0x2002;

    /// App → UI: Request focus
    pub const MSG_APP_FOCUS: u32 = 0x2003;

    /// App → UI: Error notification
    pub const MSG_APP_ERROR: u32 = 0x2004;
}

/// Type tags for payload identification
pub mod type_tags {
    // State type tags
    pub const TYPE_CLOCK_STATE: u8 = 0x01;
    pub const TYPE_CALCULATOR_STATE: u8 = 0x02;
    pub const TYPE_SETTINGS_STATE: u8 = 0x03;

    // Input type tags
    pub const TYPE_BUTTON_PRESS: u8 = 0x10;
    pub const TYPE_TEXT_INPUT: u8 = 0x11;
    pub const TYPE_KEY_PRESS: u8 = 0x12;
    pub const TYPE_FOCUS_CHANGE: u8 = 0x13;
}
