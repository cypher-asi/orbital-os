/**
 * DateTime Component
 *
 * Displays the current time and date in the taskbar.
 * Uses Date.now() for time (to avoid concurrent WASM access issues),
 * and settings store for format preferences.
 */

import { useState, useEffect } from 'react';
import {
  useSettingsStore,
  selectTimeFormat24h,
  selectTimezone,
  formatTime,
  formatShortDate,
} from '@/stores';
import styles from './DateTime.module.css';

/**
 * DateTime component for the taskbar.
 *
 * Architecture:
 * - Time source: Date.now() (avoids concurrent WASM borrow issues with supervisor)
 * - Format settings: settingsStore (synced with time_service)
 * - Updates every second
 *
 * Note: We intentionally use Date.now() instead of supervisor.get_wallclock_ms()
 * because calling into WASM from a setInterval can cause "recursive use of an object"
 * errors when it overlaps with the main poll_syscalls() interval. For UI display
 * purposes, Date.now() is sufficient.
 */
export function DateTime() {
  const timeFormat24h = useSettingsStore(selectTimeFormat24h);
  const timezone = useSettingsStore(selectTimezone);

  // Current time state - use Date.now() directly
  const [time, setTime] = useState<number>(() => Date.now());

  // Update time every second using Date.now() (no WASM calls)
  useEffect(() => {
    setTime(Date.now());

    const interval = setInterval(() => {
      setTime(Date.now());
    }, 1000);

    return () => clearInterval(interval);
  }, []);

  // Format time and date using settings
  const formattedTime = formatTime(time, timezone, timeFormat24h);
  const formattedDate = formatShortDate(time, timezone);

  return (
    <div className={styles.dateTime} title={`${formattedTime} • ${formattedDate}`}>
      <span className={styles.time}>{formattedTime}</span>
      <span className={styles.date}>{formattedDate}</span>
    </div>
  );
}
