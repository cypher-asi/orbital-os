# Backend ↔ UI Protocol

> A versioned, platform-agnostic IPC protocol for communication between app backends and UI surfaces.

## Overview

This document specifies the protocol used for communication between:
- **App Backend**: WASM process running app logic (via `ZeroApp` trait)
- **UI Surface**: Platform-specific renderer (React component on WASM, framebuffer on native)

The protocol is:
- **Versioned**: Includes version byte for forward compatibility
- **Binary**: Efficient serialization with explicit lengths
- **State-based**: Apps send typed state objects, not DOM manipulations
- **Bounded**: All parsing includes length validation

## Wire Format

All messages use this envelope:

```
┌─────────┬──────────┬─────────────┬─────────────────────┐
│ version │ type_tag │ payload_len │       payload       │
│  (u8)   │   (u8)   │    (u16)    │      (bytes)        │
└─────────┴──────────┴─────────────┴─────────────────────┘
   1 byte    1 byte     2 bytes      0-65535 bytes
```

| Field | Size | Description |
|-------|------|-------------|
| `version` | 1 byte | Protocol version (currently `0x01`) |
| `type_tag` | 1 byte | Message type identifier |
| `payload_len` | 2 bytes | Length of payload in little-endian |
| `payload` | variable | Type-specific payload data |

## Message Tags

```rust
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
```

## Design Decision: State-Based Protocol

Apps send **typed state** rather than element IDs or DOM commands. This provides:

| Approach | State-Based (chosen) | Element-Based |
|----------|---------------------|---------------|
| Coupling | Loose - UI interprets state | Tight - Backend knows UI structure |
| Platform | Same state works on all platforms | Tied to specific UI framework |
| Testing | Easy to test state logic | Requires UI mocking |
| Evolution | UI can change independently | Backend must update with UI |

### Clock App State

```rust
/// Clock app state - sent via MSG_APP_STATE
#[derive(Clone, Debug)]
pub struct ClockState {
    /// Formatted time string, e.g., "14:32:05"
    pub time_display: String,
    
    /// Formatted date string, e.g., "Wednesday, Jan 21"
    pub date_display: String,
    
    /// Whether 24-hour format is enabled
    pub is_24_hour: bool,
    
    /// Timezone name, e.g., "UTC" or "America/New_York"
    pub timezone: String,
}
```

### Calculator App State

```rust
/// Calculator app state - sent via MSG_APP_STATE
#[derive(Clone, Debug)]
pub struct CalculatorState {
    /// Current display value
    pub display: String,
    
    /// Pending operation indicator (e.g., '+', '-', '×', '÷')
    pub pending_op: Option<char>,
    
    /// Whether an error occurred (e.g., division by zero)
    pub has_error: bool,
    
    /// Memory indicator ('M' if memory is set)
    pub memory_indicator: bool,
}
```

### Input Events

```rust
/// Abstract input event (UI converts platform events to these)
#[derive(Clone, Debug)]
pub enum InputEvent {
    /// A named button was pressed
    ButtonPress { 
        /// Button identifier (e.g., "digit_5", "op_add", "clear")
        name: String 
    },
    
    /// Text was entered (for text input fields)
    TextInput { 
        text: String 
    },
    
    /// A key was pressed
    KeyPress { 
        /// Standard key code
        key_code: u32,
        /// Modifier flags (shift, ctrl, alt)
        modifiers: u8,
    },
    
    /// Focus gained/lost
    FocusChange {
        gained: bool,
    },
}
```

## Serialization

### Protocol Version

```rust
pub const PROTOCOL_VERSION: u8 = 0x01;
```

### Type Tags (within payload)

```rust
// State type tags
pub const TYPE_CLOCK_STATE: u8 = 0x01;
pub const TYPE_CALCULATOR_STATE: u8 = 0x02;

// Input type tags  
pub const TYPE_BUTTON_PRESS: u8 = 0x10;
pub const TYPE_TEXT_INPUT: u8 = 0x11;
pub const TYPE_KEY_PRESS: u8 = 0x12;
pub const TYPE_FOCUS_CHANGE: u8 = 0x13;
```

### String Encoding

Strings are encoded as length-prefixed UTF-8:

```
┌──────────┬─────────────────────┐
│  length  │       bytes         │
│  (u16)   │      (UTF-8)        │
└──────────┴─────────────────────┘
  2 bytes      0-65535 bytes
```

### ClockState Wire Format

```
┌──────┬─────────────────┬─────────────────┬──────────┬────────────────┐
│ 0x01 │ time_display    │ date_display    │ is_24h   │ timezone       │
│(type)│ (len + UTF-8)   │ (len + UTF-8)   │ (u8)     │ (len + UTF-8)  │
└──────┴─────────────────┴─────────────────┴──────────┴────────────────┘
```

### CalculatorState Wire Format

```
┌──────┬─────────────────┬────────────┬───────────┬─────────────────┐
│ 0x02 │ display         │ pending_op │ has_error │ memory_indicator│
│(type)│ (len + UTF-8)   │ (opt char) │ (u8)      │ (u8)            │
└──────┴─────────────────┴────────────┴───────────┴─────────────────┘
```

Optional char encoding:
- `0x00` = None
- `0x01 <char>` = Some(char as u32, 4 bytes little-endian)

### InputEvent Wire Format

```
ButtonPress:
┌──────┬─────────────────┐
│ 0x10 │ name            │
│(type)│ (len + UTF-8)   │
└──────┴─────────────────┘

KeyPress:
┌──────┬──────────┬───────────┐
│ 0x12 │ key_code │ modifiers │
│(type)│ (u32 LE) │ (u8)      │
└──────┴──────────┴───────────┘
```

## Bounds Checking

All deserialization includes proper bounds checking to prevent buffer overflows:

```rust
impl ClockState {
    pub fn from_bytes(data: &[u8]) -> Result<Self, ProtocolError> {
        // Check minimum header size
        if data.len() < 4 {
            return Err(ProtocolError::TooShort);
        }
        
        // Check version
        if data[0] != PROTOCOL_VERSION {
            return Err(ProtocolError::UnknownVersion(data[0]));
        }
        
        // Check payload length
        let payload_len = u16::from_le_bytes([data[2], data[3]]) as usize;
        if data.len() < 4 + payload_len {
            return Err(ProtocolError::PayloadOverflow { 
                declared: payload_len, 
                available: data.len() - 4 
            });
        }
        
        let payload = &data[4..4 + payload_len];
        
        // Parse type tag
        if payload.is_empty() {
            return Err(ProtocolError::EmptyPayload);
        }
        
        if payload[0] != TYPE_CLOCK_STATE {
            return Err(ProtocolError::UnexpectedType { 
                expected: TYPE_CLOCK_STATE, 
                got: payload[0] 
            });
        }
        
        // Parse strings with bounds checking
        let mut cursor = 1;
        let time_display = Self::read_string(payload, &mut cursor)?;
        let date_display = Self::read_string(payload, &mut cursor)?;
        
        // ... continue parsing
        
        Ok(ClockState { time_display, date_display, /* ... */ })
    }
    
    fn read_string(data: &[u8], cursor: &mut usize) -> Result<String, ProtocolError> {
        if *cursor + 2 > data.len() {
            return Err(ProtocolError::TooShort);
        }
        
        let len = u16::from_le_bytes([data[*cursor], data[*cursor + 1]]) as usize;
        *cursor += 2;
        
        if *cursor + len > data.len() {
            return Err(ProtocolError::StringOverflow { 
                declared: len, 
                available: data.len() - *cursor 
            });
        }
        
        let bytes = &data[*cursor..*cursor + len];
        *cursor += len;
        
        String::from_utf8(bytes.to_vec())
            .map_err(|_| ProtocolError::InvalidUtf8)
    }
}
```

## Error Types

```rust
#[derive(Debug, Clone)]
pub enum ProtocolError {
    /// Message is too short to contain required fields
    TooShort,
    
    /// Unknown protocol version
    UnknownVersion(u8),
    
    /// Payload length exceeds available data
    PayloadOverflow { declared: usize, available: usize },
    
    /// String length exceeds available data
    StringOverflow { declared: usize, available: usize },
    
    /// Invalid UTF-8 in string
    InvalidUtf8,
    
    /// Empty payload
    EmptyPayload,
    
    /// Unexpected type tag
    UnexpectedType { expected: u8, got: u8 },
    
    /// Unknown message type
    UnknownMessageType(u8),
}
```

## TypeScript Bindings

For the React UI, TypeScript types mirror the Rust definitions:

```typescript
// web/apps/shared/protocol.ts

export const PROTOCOL_VERSION = 0x01;

export interface ClockState {
  timeDisplay: string;
  dateDisplay: string;
  is24Hour: boolean;
  timezone: string;
}

export interface CalculatorState {
  display: string;
  pendingOp: string | null;
  hasError: boolean;
  memoryIndicator: boolean;
}

export type InputEvent = 
  | { type: 'buttonPress'; name: string }
  | { type: 'textInput'; text: string }
  | { type: 'keyPress'; keyCode: number; modifiers: number }
  | { type: 'focusChange'; gained: boolean };

export function decodeClockState(data: Uint8Array): ClockState { /* ... */ }
export function decodeCalculatorState(data: Uint8Array): CalculatorState { /* ... */ }
export function encodeInputEvent(event: InputEvent): Uint8Array { /* ... */ }
```

## Message Flow Examples

### Clock Update Flow

```
1. Clock backend (WASM):
   - Calls SYS_GET_WALLCLOCK syscall
   - Formats time string
   - Creates ClockState { time_display: "14:32:05", ... }
   - Serializes to bytes
   - Sends via MSG_APP_STATE to UI endpoint

2. Kernel:
   - Routes message to React UI process

3. React UI:
   - Receives bytes via useSupervisor hook
   - Calls decodeClockState(data)
   - Renders <ClockApp state={clockState} />
```

### Calculator Input Flow

```
1. React UI:
   - User clicks "5" button
   - Creates InputEvent { type: 'buttonPress', name: 'digit_5' }
   - Calls encodeInputEvent(event)
   - Sends via MSG_APP_INPUT to app endpoint

2. Kernel:
   - Routes message to Calculator WASM process

3. Calculator backend:
   - Receives bytes in on_message()
   - Deserializes to InputEvent::ButtonPress { name: "digit_5" }
   - Updates internal state
   - Sends new CalculatorState via MSG_APP_STATE
```

## Versioning Strategy

When the protocol evolves:

1. **Minor changes**: Add new optional fields at end of payload
2. **Breaking changes**: Increment `PROTOCOL_VERSION`
3. **Negotiation**: UI sends its supported version in `MSG_UI_READY`

```rust
pub struct UiReady {
    /// Maximum protocol version supported by this UI
    pub max_version: u8,
    /// UI type identifier
    pub ui_type: UiType,
}

pub enum UiType {
    React = 0x01,
    Terminal = 0x02,
    Framebuffer = 0x03,
}
```

The app backend should use the minimum of its version and the UI's `max_version`.
