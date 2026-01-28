//! CMOS Real-Time Clock (RTC) driver for x86_64
//!
//! Reads the current time from the CMOS RTC. The RTC provides:
//! - Real-time clock (seconds, minutes, hours, day, month, year)
//! - Alarm functionality (not used here)
//!
//! # I/O Ports
//!
//! - 0x70: CMOS address/index port
//! - 0x71: CMOS data port
//!
//! # Registers
//!
//! | Register | Description |
//! |----------|-------------|
//! | 0x00 | Seconds (0-59) |
//! | 0x02 | Minutes (0-59) |
//! | 0x04 | Hours (0-23 or 1-12 + AM/PM) |
//! | 0x06 | Day of week (1-7, Sunday=1) |
//! | 0x07 | Day of month (1-31) |
//! | 0x08 | Month (1-12) |
//! | 0x09 | Year (0-99) |
//! | 0x32 | Century (optional, not always present) |
//! | 0x0A | Status Register A (update in progress) |
//! | 0x0B | Status Register B (format flags) |

use x86_64::instructions::port::Port;

/// CMOS address port
const CMOS_ADDR: u16 = 0x70;
/// CMOS data port
const CMOS_DATA: u16 = 0x71;

/// RTC register offsets
mod reg {
    pub const SECONDS: u8 = 0x00;
    pub const MINUTES: u8 = 0x02;
    pub const HOURS: u8 = 0x04;
    pub const DAY_OF_MONTH: u8 = 0x07;
    pub const MONTH: u8 = 0x08;
    pub const YEAR: u8 = 0x09;
    pub const CENTURY: u8 = 0x32; // Not always present
    pub const STATUS_A: u8 = 0x0A;
    pub const STATUS_B: u8 = 0x0B;
}

/// Status Register B flags
mod status_b {
    /// Data is in binary format (if set) or BCD (if clear)
    pub const BINARY: u8 = 0x04;
    /// 24-hour mode (if set) or 12-hour mode (if clear)
    pub const HOUR_24: u8 = 0x02;
}

/// Read a CMOS register
///
/// # Safety
/// This function performs raw I/O port access.
unsafe fn read_cmos(reg: u8) -> u8 {
    let mut addr_port: Port<u8> = Port::new(CMOS_ADDR);
    let mut data_port: Port<u8> = Port::new(CMOS_DATA);
    
    // Disable NMI (bit 7) and select register
    addr_port.write(0x80 | reg);
    data_port.read()
}

/// Wait for RTC update to complete
///
/// The RTC sets bit 7 of Status Register A while updating.
/// We should wait for it to clear before reading.
unsafe fn wait_for_update() {
    // Wait for any update to finish
    while read_cmos(reg::STATUS_A) & 0x80 != 0 {
        core::hint::spin_loop();
    }
}

/// Convert BCD to binary
fn bcd_to_binary(bcd: u8) -> u8 {
    ((bcd & 0xF0) >> 4) * 10 + (bcd & 0x0F)
}

/// Read the current time from the RTC
///
/// Returns (year, month, day, hour, minute, second)
pub fn read_rtc() -> (u16, u8, u8, u8, u8, u8) {
    unsafe {
        // Wait for RTC to be stable
        wait_for_update();
        
        // Read status register B to check format
        let status_b = read_cmos(reg::STATUS_B);
        let is_binary = status_b & status_b::BINARY != 0;
        let is_24h = status_b & status_b::HOUR_24 != 0;
        
        // Read time values
        let mut second = read_cmos(reg::SECONDS);
        let mut minute = read_cmos(reg::MINUTES);
        let mut hour = read_cmos(reg::HOURS);
        let mut day = read_cmos(reg::DAY_OF_MONTH);
        let mut month = read_cmos(reg::MONTH);
        let mut year = read_cmos(reg::YEAR);
        
        // Try to read century (may not be present on all systems)
        // If it reads as 0, assume 20xx
        let century = read_cmos(reg::CENTURY);
        
        // Convert from BCD if needed
        if !is_binary {
            second = bcd_to_binary(second);
            minute = bcd_to_binary(minute);
            hour = bcd_to_binary(hour & 0x7F) | (hour & 0x80); // Preserve AM/PM bit
            day = bcd_to_binary(day);
            month = bcd_to_binary(month);
            year = bcd_to_binary(year);
        }
        
        // Convert 12-hour to 24-hour if needed
        if !is_24h {
            let pm = hour & 0x80 != 0;
            hour &= 0x7F;
            if pm && hour != 12 {
                hour += 12;
            } else if !pm && hour == 12 {
                hour = 0;
            }
        }
        
        // Calculate full year
        let full_year = if century != 0 && century < 100 {
            let century_val = if !is_binary {
                bcd_to_binary(century)
            } else {
                century
            };
            (century_val as u16) * 100 + (year as u16)
        } else {
            // Assume 20xx for year < 80, 19xx for year >= 80
            if year < 80 {
                2000 + (year as u16)
            } else {
                1900 + (year as u16)
            }
        };
        
        (full_year, month, day, hour, minute, second)
    }
}

/// Calculate milliseconds since Unix epoch (January 1, 1970 00:00:00 UTC)
///
/// This is a simplified calculation that doesn't account for leap seconds.
pub fn unix_timestamp_ms() -> u64 {
    let (year, month, day, hour, minute, second) = read_rtc();
    
    // Days from year 1970 to start of this year
    let mut days: i64 = 0;
    for y in 1970..year {
        days += days_in_year(y) as i64;
    }
    
    // Days from start of year to start of this month
    for m in 1..month {
        days += days_in_month(year, m) as i64;
    }
    
    // Days in this month
    days += (day as i64) - 1;
    
    // Convert to milliseconds
    let hours_total = days * 24 + (hour as i64);
    let minutes_total = hours_total * 60 + (minute as i64);
    let seconds_total = minutes_total * 60 + (second as i64);
    
    (seconds_total * 1000) as u64
}

/// Check if a year is a leap year
fn is_leap_year(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Get the number of days in a year
fn days_in_year(year: u16) -> u16 {
    if is_leap_year(year) { 366 } else { 365 }
}

/// Get the number of days in a month
fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 => 31,  // January
        2 => if is_leap_year(year) { 29 } else { 28 }, // February
        3 => 31,  // March
        4 => 30,  // April
        5 => 31,  // May
        6 => 30,  // June
        7 => 31,  // July
        8 => 31,  // August
        9 => 30,  // September
        10 => 31, // October
        11 => 30, // November
        12 => 31, // December
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_bcd_conversion() {
        assert_eq!(bcd_to_binary(0x00), 0);
        assert_eq!(bcd_to_binary(0x09), 9);
        assert_eq!(bcd_to_binary(0x10), 10);
        assert_eq!(bcd_to_binary(0x59), 59);
        assert_eq!(bcd_to_binary(0x99), 99);
    }
    
    #[test]
    fn test_leap_year() {
        assert!(!is_leap_year(1900)); // Divisible by 100 but not 400
        assert!(is_leap_year(2000));  // Divisible by 400
        assert!(is_leap_year(2024));  // Divisible by 4
        assert!(!is_leap_year(2023)); // Not divisible by 4
    }
    
    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2024, 2), 29); // Leap year
        assert_eq!(days_in_month(2023, 2), 28); // Non-leap year
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 4), 30);
    }
}
