//! Serial port driver for x86_64
//!
//! Uses the standard COM1 port (0x3F8) for debug output and input.
//! This is the primary I/O mechanism for QEMU debugging.

use alloc::collections::VecDeque;
use core::fmt::{self, Write};
use spin::Mutex;
use uart_16550::SerialPort;
use x86_64::instructions::port::Port;

/// COM1 serial port base address
const COM1_PORT: u16 = 0x3F8;

/// Global serial port writer
static SERIAL: Mutex<Option<SerialPort>> = Mutex::new(None);

/// Input buffer for received serial data
static INPUT_BUFFER: Mutex<VecDeque<u8>> = Mutex::new(VecDeque::new());

/// Maximum input buffer size
const MAX_INPUT_BUFFER: usize = 256;

/// Initialize the serial port
///
/// # Safety
/// Must be called only once during early kernel initialization.
pub fn init() {
    let mut serial = unsafe { SerialPort::new(COM1_PORT) };
    serial.init();
    *SERIAL.lock() = Some(serial);
    
    // Enable receive interrupts (IER bit 0)
    enable_receive_interrupt();
}

/// Enable serial port receive interrupt
fn enable_receive_interrupt() {
    // COM1 + 1 = IER (Interrupt Enable Register)
    // Bit 0: Received Data Available Interrupt
    let mut ier_port: Port<u8> = Port::new(COM1_PORT + 1);
    unsafe {
        let current = ier_port.read();
        ier_port.write(current | 0x01);
    }
}

/// Read a byte from serial input (non-blocking)
///
/// First checks the interrupt buffer, then polls the hardware directly.
/// This ensures input works even if serial interrupts aren't routed via IOAPIC.
///
/// Returns `Some(byte)` if data is available, `None` otherwise.
pub fn read_byte() -> Option<u8> {
    // First check interrupt buffer
    if let Some(byte) = INPUT_BUFFER.lock().pop_front() {
        return Some(byte);
    }
    // Fall back to direct polling if buffer is empty
    receive_byte_raw()
}

/// Check if the serial port has data available to read
pub fn has_data_available() -> bool {
    // COM1 + 5 = LSR (Line Status Register)
    // Bit 0: Data Ready
    let mut lsr_port: Port<u8> = Port::new(COM1_PORT + 5);
    unsafe { lsr_port.read() & 0x01 != 0 }
}

/// Receive a byte directly from the serial port hardware (for interrupt handler)
///
/// Returns `Some(byte)` if data is available, `None` otherwise.
pub fn receive_byte_raw() -> Option<u8> {
    if has_data_available() {
        let mut data_port: Port<u8> = Port::new(COM1_PORT);
        Some(unsafe { data_port.read() })
    } else {
        None
    }
}

/// Queue a received byte into the input buffer (called by interrupt handler)
pub fn queue_input_byte(byte: u8) {
    let mut buffer = INPUT_BUFFER.lock();
    if buffer.len() < MAX_INPUT_BUFFER {
        buffer.push_back(byte);
    }
    // If buffer is full, drop the byte (oldest data is preserved)
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
