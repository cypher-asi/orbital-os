# Factory Reference Apps

> Clock and Calculator demonstrate the application framework as bundled reference implementations.

## Overview

Factory apps are bundled applications that:

- Demonstrate the `ZeroApp` trait and framework
- Ship with the system (trusted, auto-granted basic capabilities)
- Serve as reference implementations for third-party developers
- Are intentionally simple to illustrate core concepts

## Clock App

**Purpose**: Demonstrate time syscalls, periodic updates, one-way IPC (state output)

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Clock App                                 │
│                                                                 │
│  ┌──────────────────────┐         ┌──────────────────────────┐ │
│  │   WASM Backend       │   IPC   │      React UI            │ │
│  │   (clock.wasm)       │────────►│   (ClockApp.tsx)         │ │
│  │                      │         │                          │ │
│  │  - SYS_GET_WALLCLOCK │         │  - Receives ClockState   │ │
│  │  - Format time/date  │         │  - Renders time display  │ │
│  │  - Send ClockState   │         │  - No input needed       │ │
│  └──────────────────────┘         └──────────────────────────┘ │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Manifest

```rust
pub static CLOCK_MANIFEST: AppManifest = AppManifest {
    id: "com.Zero.clock",
    name: "Clock",
    version: "1.0.0",
    description: "Displays current time and date",
    capabilities: &[
        CapabilityRequest {
            object_type: ObjectType::Endpoint,
            permissions: Permissions::READ_WRITE,
            reason: "Send time updates to display",
            required: true,
        },
    ],
};
```

### Backend Implementation

```rust
// crates/zos-apps/src/bin/clock.rs

#![no_std]
#![no_main]

extern crate alloc;
use alloc::string::String;
use Zero_apps::*;

#[derive(Default)]
pub struct ClockApp {
    /// Last time we sent an update (nanoseconds)
    last_update_ns: u64,
    
    /// Cached formatted time
    cached_time: String,
    
    /// Cached formatted date
    cached_date: String,
    
    /// Update interval (1 second in nanos)
    update_interval: u64,
}

impl ClockApp {
    const UPDATE_INTERVAL_NS: u64 = 1_000_000_000; // 1 second
    
    fn format_time(wallclock_ms: u64) -> (String, String) {
        // Convert milliseconds since epoch to components
        // Using a simple algorithm (no timezone support initially)
        
        let total_seconds = wallclock_ms / 1000;
        let seconds = (total_seconds % 60) as u8;
        let minutes = ((total_seconds / 60) % 60) as u8;
        let hours = ((total_seconds / 3600) % 24) as u8;
        
        // Format time as "HH:MM:SS"
        let time = format!("{:02}:{:02}:{:02}", hours, minutes, seconds);
        
        // Calculate date (simplified - days since epoch)
        let days_since_epoch = total_seconds / 86400;
        
        // Simple day-of-week calculation (Jan 1, 1970 was Thursday)
        let day_names = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
        let day_of_week = day_names[(days_since_epoch % 7) as usize];
        
        // For now, just show days since epoch (full date calculation is complex)
        let date = format!("{}, Day {}", day_of_week, days_since_epoch);
        
        (time, date)
    }
    
    fn send_state(&self, ctx: &AppContext) -> Result<(), AppError> {
        let state = ClockState {
            time_display: self.cached_time.clone(),
            date_display: self.cached_date.clone(),
            is_24_hour: true,
            timezone: String::from("UTC"),
        };
        
        let bytes = state.to_bytes();
        
        if let Some(slot) = ctx.ui_endpoint {
            syscall::send(slot, tags::MSG_APP_STATE, &bytes)
                .map_err(|e| AppError::IpcError(format!("{:?}", e)))?;
        }
        
        Ok(())
    }
}

impl ZeroApp for ClockApp {
    fn manifest() -> &'static AppManifest {
        &CLOCK_MANIFEST
    }
    
    fn init(&mut self, _ctx: &AppContext) -> Result<(), AppError> {
        self.update_interval = Self::UPDATE_INTERVAL_NS;
        Ok(())
    }
    
    fn update(&mut self, ctx: &AppContext) -> ControlFlow {
        // Check if it's time to update
        if ctx.uptime_ns - self.last_update_ns >= self.update_interval {
            self.last_update_ns = ctx.uptime_ns;
            
            // Format current time
            let (time, date) = Self::format_time(ctx.wallclock_ms);
            self.cached_time = time;
            self.cached_date = date;
            
            // Send state to UI
            if let Err(e) = self.send_state(ctx) {
                syscall::debug(&format!("Clock: failed to send state: {:?}", e));
            }
        }
        
        ControlFlow::Yield
    }
    
    fn on_message(&mut self, _ctx: &AppContext, _msg: Message) -> Result<(), AppError> {
        // Clock doesn't process input messages
        Ok(())
    }
    
    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("Clock: shutting down");
    }
}

app_main!(ClockApp);
```

### React UI

```tsx
// web/apps/ClockApp/ClockApp.tsx

import React from 'react';
import { Card, Typography } from '@/components/zui';
import { useSupervisor } from '@/hooks/useSupervisor';
import { ClockState, decodeClockState } from './protocol';
import styles from './ClockApp.module.css';

interface ClockAppProps {
  windowId: string;
  processId: number;
}

export function ClockApp({ windowId, processId }: ClockAppProps) {
  const [state, setState] = React.useState<ClockState | null>(null);
  const { registerAppCallback } = useSupervisor();
  
  React.useEffect(() => {
    // Register to receive state updates from backend
    const unregister = registerAppCallback(processId, (data: Uint8Array) => {
      try {
        const clockState = decodeClockState(data);
        setState(clockState);
      } catch (e) {
        console.error('Failed to decode clock state:', e);
      }
    });
    
    return unregister;
  }, [processId, registerAppCallback]);
  
  if (!state) {
    return (
      <Card className={styles.clock}>
        <Typography variant="body">Loading...</Typography>
      </Card>
    );
  }
  
  return (
    <Card className={styles.clock}>
      <Typography variant="h1" className={styles.time}>
        {state.timeDisplay}
      </Typography>
      <Typography variant="body" className={styles.date}>
        {state.dateDisplay}
      </Typography>
      <Typography variant="caption" className={styles.timezone}>
        {state.timezone}
      </Typography>
    </Card>
  );
}
```

```css
/* web/apps/ClockApp/ClockApp.module.css */

.clock {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 24px;
  min-width: 200px;
  min-height: 120px;
}

.time {
  font-family: 'SF Mono', 'Consolas', monospace;
  font-size: 48px;
  font-weight: 300;
  letter-spacing: 2px;
}

.date {
  margin-top: 8px;
  opacity: 0.8;
}

.timezone {
  margin-top: 4px;
  opacity: 0.5;
}
```

---

## Calculator App

**Purpose**: Demonstrate bidirectional IPC, state management, user input handling

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      Calculator App                              │
│                                                                 │
│  ┌──────────────────────┐         ┌──────────────────────────┐ │
│  │   WASM Backend       │◄───────►│      React UI            │ │
│  │   (calculator.wasm)  │   IPC   │   (CalculatorApp.tsx)    │ │
│  │                      │         │                          │ │
│  │  - Receive input     │         │  - Render keypad         │ │
│  │  - Calculate result  │         │  - Send button presses   │ │
│  │  - Send state        │         │  - Display result        │ │
│  └──────────────────────┘         └──────────────────────────┘ │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Manifest

```rust
pub static CALCULATOR_MANIFEST: AppManifest = AppManifest {
    id: "com.Zero.calculator",
    name: "Calculator",
    version: "1.0.0",
    description: "Basic arithmetic calculator",
    capabilities: &[
        CapabilityRequest {
            object_type: ObjectType::Endpoint,
            permissions: Permissions::READ_WRITE,
            reason: "Receive input and send results",
            required: true,
        },
    ],
};
```

### Backend Implementation

```rust
// crates/zos-apps/src/bin/calculator.rs

#![no_std]
#![no_main]

extern crate alloc;
use alloc::string::String;
use Zero_apps::*;

#[derive(Default)]
pub struct CalculatorApp {
    /// Current display value
    display: String,
    
    /// Accumulator for calculations
    accumulator: f64,
    
    /// Pending operation
    pending_op: Option<char>,
    
    /// Whether we just completed an operation (next digit clears display)
    just_computed: bool,
    
    /// Error state
    has_error: bool,
    
    /// Memory register
    memory: f64,
}

impl CalculatorApp {
    fn handle_digit(&mut self, digit: char) {
        if self.has_error {
            return;
        }
        
        if self.just_computed {
            self.display.clear();
            self.just_computed = false;
        }
        
        // Limit display length
        if self.display.len() < 15 {
            self.display.push(digit);
        }
    }
    
    fn handle_operation(&mut self, op: char) {
        if self.has_error {
            return;
        }
        
        // Complete pending operation first
        if self.pending_op.is_some() {
            self.handle_equals();
        }
        
        // Parse current display as accumulator
        if let Ok(value) = self.display.parse::<f64>() {
            self.accumulator = value;
            self.pending_op = Some(op);
            self.just_computed = true;
        }
    }
    
    fn handle_equals(&mut self) {
        if self.has_error || self.pending_op.is_none() {
            return;
        }
        
        let current = match self.display.parse::<f64>() {
            Ok(v) => v,
            Err(_) => {
                self.has_error = true;
                self.display = String::from("Error");
                return;
            }
        };
        
        let result = match self.pending_op {
            Some('+') => self.accumulator + current,
            Some('-') => self.accumulator - current,
            Some('×') | Some('*') => self.accumulator * current,
            Some('÷') | Some('/') => {
                if current == 0.0 {
                    self.has_error = true;
                    self.display = String::from("Error");
                    self.pending_op = None;
                    return;
                }
                self.accumulator / current
            }
            _ => current,
        };
        
        // Format result
        self.display = if result.fract() == 0.0 && result.abs() < 1e15 {
            format!("{}", result as i64)
        } else {
            format!("{:.8}", result).trim_end_matches('0').trim_end_matches('.').to_string()
        };
        
        self.accumulator = result;
        self.pending_op = None;
        self.just_computed = true;
    }
    
    fn handle_clear(&mut self) {
        self.display = String::from("0");
        self.accumulator = 0.0;
        self.pending_op = None;
        self.just_computed = false;
        self.has_error = false;
    }
    
    fn handle_button(&mut self, name: &str) {
        match name {
            // Digits
            "digit_0" => self.handle_digit('0'),
            "digit_1" => self.handle_digit('1'),
            "digit_2" => self.handle_digit('2'),
            "digit_3" => self.handle_digit('3'),
            "digit_4" => self.handle_digit('4'),
            "digit_5" => self.handle_digit('5'),
            "digit_6" => self.handle_digit('6'),
            "digit_7" => self.handle_digit('7'),
            "digit_8" => self.handle_digit('8'),
            "digit_9" => self.handle_digit('9'),
            "decimal" => {
                if !self.display.contains('.') {
                    self.handle_digit('.');
                }
            }
            
            // Operations
            "op_add" => self.handle_operation('+'),
            "op_sub" => self.handle_operation('-'),
            "op_mul" => self.handle_operation('×'),
            "op_div" => self.handle_operation('÷'),
            "op_equals" => self.handle_equals(),
            
            // Control
            "clear" => self.handle_clear(),
            "clear_entry" => {
                self.display = String::from("0");
                self.has_error = false;
            }
            "backspace" => {
                if !self.has_error && !self.just_computed && self.display.len() > 1 {
                    self.display.pop();
                } else {
                    self.display = String::from("0");
                }
            }
            "negate" => {
                if !self.has_error {
                    if self.display.starts_with('-') {
                        self.display = self.display[1..].to_string();
                    } else if self.display != "0" {
                        self.display = format!("-{}", self.display);
                    }
                }
            }
            
            _ => {}
        }
    }
    
    fn send_state(&self, ctx: &AppContext) -> Result<(), AppError> {
        let state = CalculatorState {
            display: self.display.clone(),
            pending_op: self.pending_op,
            has_error: self.has_error,
            memory_indicator: self.memory != 0.0,
        };
        
        let bytes = state.to_bytes();
        
        if let Some(slot) = ctx.ui_endpoint {
            syscall::send(slot, tags::MSG_APP_STATE, &bytes)
                .map_err(|e| AppError::IpcError(format!("{:?}", e)))?;
        }
        
        Ok(())
    }
}

impl ZeroApp for CalculatorApp {
    fn manifest() -> &'static AppManifest {
        &CALCULATOR_MANIFEST
    }
    
    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError> {
        self.display = String::from("0");
        self.send_state(ctx)
    }
    
    fn update(&mut self, _ctx: &AppContext) -> ControlFlow {
        ControlFlow::Yield
    }
    
    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError> {
        if msg.tag == tags::MSG_APP_INPUT {
            // Decode input event
            let event = InputEvent::from_bytes(&msg.data)
                .map_err(|e| AppError::ProtocolError(e))?;
            
            // Handle button press
            if let InputEvent::ButtonPress { name } = event {
                self.handle_button(&name);
                self.send_state(ctx)?;
            }
        }
        
        Ok(())
    }
    
    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("Calculator: shutting down");
    }
}

app_main!(CalculatorApp);
```

### React UI

```tsx
// web/apps/CalculatorApp/CalculatorApp.tsx

import React from 'react';
import { Card, Button, Typography } from '@/components/zui';
import { useSupervisor } from '@/hooks/useSupervisor';
import { CalculatorState, decodeCalculatorState, encodeInputEvent } from './protocol';
import styles from './CalculatorApp.module.css';

interface CalculatorAppProps {
  windowId: string;
  processId: number;
}

const BUTTONS = [
  ['clear', 'clear_entry', 'backspace', 'op_div'],
  ['digit_7', 'digit_8', 'digit_9', 'op_mul'],
  ['digit_4', 'digit_5', 'digit_6', 'op_sub'],
  ['digit_1', 'digit_2', 'digit_3', 'op_add'],
  ['negate', 'digit_0', 'decimal', 'op_equals'],
];

const BUTTON_LABELS: Record<string, string> = {
  digit_0: '0', digit_1: '1', digit_2: '2', digit_3: '3', digit_4: '4',
  digit_5: '5', digit_6: '6', digit_7: '7', digit_8: '8', digit_9: '9',
  decimal: '.', clear: 'C', clear_entry: 'CE', backspace: '⌫',
  op_add: '+', op_sub: '−', op_mul: '×', op_div: '÷', op_equals: '=',
  negate: '±',
};

export function CalculatorApp({ windowId, processId }: CalculatorAppProps) {
  const [state, setState] = React.useState<CalculatorState | null>(null);
  const { registerAppCallback, sendToApp } = useSupervisor();
  
  React.useEffect(() => {
    const unregister = registerAppCallback(processId, (data: Uint8Array) => {
      try {
        const calcState = decodeCalculatorState(data);
        setState(calcState);
      } catch (e) {
        console.error('Failed to decode calculator state:', e);
      }
    });
    
    return unregister;
  }, [processId, registerAppCallback]);
  
  const handleButton = React.useCallback((name: string) => {
    const event = encodeInputEvent({ type: 'buttonPress', name });
    sendToApp(processId, event);
  }, [processId, sendToApp]);
  
  // Keyboard support
  React.useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const keyMap: Record<string, string> = {
        '0': 'digit_0', '1': 'digit_1', '2': 'digit_2', '3': 'digit_3',
        '4': 'digit_4', '5': 'digit_5', '6': 'digit_6', '7': 'digit_7',
        '8': 'digit_8', '9': 'digit_9', '.': 'decimal', '+': 'op_add',
        '-': 'op_sub', '*': 'op_mul', '/': 'op_div', 'Enter': 'op_equals',
        '=': 'op_equals', 'Escape': 'clear', 'Backspace': 'backspace',
      };
      
      if (keyMap[e.key]) {
        e.preventDefault();
        handleButton(keyMap[e.key]);
      }
    };
    
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleButton]);
  
  return (
    <Card className={styles.calculator}>
      <div className={styles.display}>
        <Typography variant="caption" className={styles.pendingOp}>
          {state?.pendingOp || '\u00A0'}
        </Typography>
        <Typography 
          variant="h2" 
          className={`${styles.result} ${state?.hasError ? styles.error : ''}`}
        >
          {state?.display || '0'}
        </Typography>
      </div>
      
      <div className={styles.keypad}>
        {BUTTONS.map((row, rowIndex) => (
          <div key={rowIndex} className={styles.row}>
            {row.map((btn) => (
              <Button
                key={btn}
                className={`${styles.button} ${btn.startsWith('op_') ? styles.operator : ''} ${btn === 'op_equals' ? styles.equals : ''}`}
                onClick={() => handleButton(btn)}
              >
                {BUTTON_LABELS[btn]}
              </Button>
            ))}
          </div>
        ))}
      </div>
      
      {state?.memoryIndicator && (
        <Typography variant="caption" className={styles.memory}>M</Typography>
      )}
    </Card>
  );
}
```

```css
/* web/apps/CalculatorApp/CalculatorApp.module.css */

.calculator {
  display: flex;
  flex-direction: column;
  padding: 16px;
  min-width: 280px;
  gap: 12px;
}

.display {
  background: var(--color-surface-secondary);
  border-radius: 8px;
  padding: 12px 16px;
  text-align: right;
}

.pendingOp {
  height: 16px;
  opacity: 0.6;
}

.result {
  font-family: 'SF Mono', 'Consolas', monospace;
  font-size: 32px;
  font-weight: 400;
  overflow: hidden;
  text-overflow: ellipsis;
}

.result.error {
  color: var(--color-error);
}

.keypad {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.row {
  display: flex;
  gap: 8px;
}

.button {
  flex: 1;
  height: 48px;
  font-size: 20px;
  border-radius: 8px;
}

.operator {
  background: var(--color-primary-light);
  color: var(--color-primary);
}

.equals {
  background: var(--color-primary);
  color: var(--color-on-primary);
}

.memory {
  position: absolute;
  top: 8px;
  left: 8px;
  opacity: 0.6;
}
```

---

## AppRouter Integration

```tsx
// web/apps/AppRouter/AppRouter.tsx

import React from 'react';
import { ClockApp } from '../ClockApp/ClockApp';
import { CalculatorApp } from '../CalculatorApp/CalculatorApp';

interface AppRouterProps {
  appId: string;
  windowId: string;
  processId: number;
}

export function AppRouter({ appId, windowId, processId }: AppRouterProps) {
  switch (appId) {
    case 'com.Zero.clock':
      return <ClockApp windowId={windowId} processId={processId} />;
    
    case 'com.Zero.calculator':
      return <CalculatorApp windowId={windowId} processId={processId} />;
    
    default:
      return (
        <div style={{ padding: 16 }}>
          Unknown app: {appId}
        </div>
      );
  }
}
```

---

## Testing

### Unit Tests (Rust)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn calculator_basic_addition() {
        let mut calc = CalculatorApp::default();
        calc.handle_button("digit_2");
        calc.handle_button("op_add");
        calc.handle_button("digit_3");
        calc.handle_button("op_equals");
        assert_eq!(calc.display, "5");
    }
    
    #[test]
    fn calculator_division_by_zero() {
        let mut calc = CalculatorApp::default();
        calc.handle_button("digit_5");
        calc.handle_button("op_div");
        calc.handle_button("digit_0");
        calc.handle_button("op_equals");
        assert!(calc.has_error);
    }
    
    #[test]
    fn calculator_clear() {
        let mut calc = CalculatorApp::default();
        calc.handle_button("digit_1");
        calc.handle_button("digit_2");
        calc.handle_button("digit_3");
        calc.handle_button("clear");
        assert_eq!(calc.display, "0");
    }
}
```

### End-to-End Tests

1. **Clock App**
   - Launch from Begin menu
   - Verify time display updates every second
   - Verify date format is correct

2. **Calculator App**
   - Launch from Begin menu
   - Test: "2 + 3 =" → "5"
   - Test: "10 / 0 =" → "Error"
   - Test keyboard input
   - Test clear functions
