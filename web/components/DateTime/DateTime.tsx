/**
 * DateTime Component
 *
 * Displays the current time and date in the taskbar.
 * Uses HAL wallclock via supervisor for time, and settings store for format preferences.
 */

import { useState, useEffect } from 'react';
import { useSupervisor } from '../../desktop/hooks/useSupervisor';
import {
  useSettingsStore,
  selectTimeFormat24h,
  selectTimezone,
  formatTime,
  formatShortDate,
} from '../../stores';
import styles from './DateTime.module.css';

/**
 * DateTime component for the taskbar.
 *
 * Architecture:
 * - Time source: HAL wallclock via supervisor.get_wallclock_ms()
 * - Format settings: settingsStore (synced with time_service)
 * - Updates every second
 */
export function DateTime() {
  const supervisor = useSupervisor();
  const timeFormat24h = useSettingsStore(selectTimeFormat24h);
  const timezone = useSettingsStore(selectTimezone);

  // Current time state - initialize with Date.now() as fallback
  const [time, setTime] = useState<number>(() => Date.now());

  // Update time every second
  useEffect(() => {
    // Helper to get time from supervisor or fallback to Date.now()
    const getTime = (): number => {
      // Check if supervisor and get_wallclock_ms are available
      if (supervisor && typeof supervisor.get_wallclock_ms === 'function') {
        return supervisor.get_wallclock_ms();
      }
      // Fallback to JS Date when supervisor or method not available
      return Date.now();
    };

    // Get initial time
    setTime(getTime());

    const interval = setInterval(() => {
      setTime(getTime());
    }, 1000);

    return () => clearInterval(interval);
  }, [supervisor]);

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
