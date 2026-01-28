//! Serial port driver for x86_64
//!
//! Uses the standard COM1 port (0x3F8) for debug output.
//! This is the primary output mechanism for QEMU debugging.

use core::fmt::{self, Write};
use spin::Mutex;
use uart_16550::SerialPort;

/// COM1 serial port base address
const COM1_PORT: u16 = 0x3F8;

/// Global serial port writer
static SERIAL: Mutex<Option<SerialPort>> = Mutex::new(None);

/// Initialize the serial port
///
/// # Safety
/// Must be called only once during early kernel initialization.
pub fn init() {
    let mut serial = unsafe { SerialPort::new(COM1_PORT) };
    serial.init();
    *SERIAL.lock() = Some(serial);
}

/// Serial port writer for formatting
pub struct SerialWriter;

impl Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if let Some(ref mut serial) = *SERIAL.lock() {
            for byte in s.bytes() {
                serial.send(byte);
            }
        }
        Ok(())
    }
}

/// Write a string to serial output
pub fn write_str(s: &str) {
    if let Some(ref mut serial) = *SERIAL.lock() {
        for byte in s.bytes() {
            serial.send(byte);
        }
    }
}

/// Write a formatted string to serial output
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let _ = write!($crate::x86_64::serial::SerialWriter, $($arg)*);
    }};
}

/// Write a formatted string with newline to serial output
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => {{
        $crate::serial_print!($($arg)*);
        $crate::serial_print!("\n");
    }};
}

/// Write raw bytes to serial port
pub fn write_bytes(bytes: &[u8]) {
    if let Some(ref mut serial) = *SERIAL.lock() {
        for &byte in bytes {
            serial.send(byte);
        }
    }
}

/// Write a single byte to serial port
pub fn write_byte(byte: u8) {
    if let Some(ref mut serial) = *SERIAL.lock() {
        serial.send(byte);
    }
}
