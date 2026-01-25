import { useState, useEffect } from 'react';
import { Text, Label } from '@cypher-asi/zui';
import { Clock } from 'lucide-react';
import { decodeClockState, ClockState } from '../_wire-format/app-protocol';
import styles from './ClockApp.module.css';

/**
 * Clock App - Displays current time and date
 *
 * Uses ZUI components: Panel, Text, Label
 */
export function ClockApp() {
  const [state, setState] = useState<ClockState>({
    timeDisplay: '00:00:00',
    dateDisplay: 'Loading...',
    is24Hour: true,
    timezone: 'UTC',
  });

  useEffect(() => {
    const updateTime = () => {
      const now = new Date();

      const hours = String(now.getUTCHours()).padStart(2, '0');
      const minutes = String(now.getUTCMinutes()).padStart(2, '0');
      const seconds = String(now.getUTCSeconds()).padStart(2, '0');
      const timeDisplay = `${hours}:${minutes}:${seconds}`;

      const days = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
      const months = [
        'Jan',
        'Feb',
        'Mar',
        'Apr',
        'May',
        'Jun',
        'Jul',
        'Aug',
        'Sep',
        'Oct',
        'Nov',
        'Dec',
      ];
      const dayName = days[now.getUTCDay()];
      const monthName = months[now.getUTCMonth()];
      const dayOfMonth = now.getUTCDate();
      const year = now.getUTCFullYear();
      const dateDisplay = `${dayName}, ${monthName} ${dayOfMonth}, ${year}`;

      setState({ timeDisplay, dateDisplay, is24Hour: true, timezone: 'UTC' });
    };

    updateTime();
    const interval = setInterval(updateTime, 1000);
    return () => clearInterval(interval);
  }, []);

  const handleMessage = (data: Uint8Array) => {
    const decoded = decodeClockState(data);
    if (decoded) setState(decoded);
  };

  (window as unknown as { clockAppHandler?: (data: Uint8Array) => void }).clockAppHandler =
    handleMessage;

  return (
    <div className={styles.container}>
      <div className={styles.clockPanel}>
        {/* Icon */}
        <div className={styles.iconPanel}>
          <Clock size={32} className={styles.icon} />
        </div>

        {/* Time Display */}
        <Text as="div" size="lg" className={styles.time}>
          {state.timeDisplay}
        </Text>

        {/* Date Display */}
        <Text as="div" size="sm" variant="muted">
          {state.dateDisplay}
        </Text>

        {/* Timezone Info */}
        <div className={styles.infoRow}>
          <Label size="xs">{state.timezone}</Label>
          {state.is24Hour && (
            <Label size="xs" variant="success">
              24h
            </Label>
          )}
        </div>
      </div>
    </div>
  );
}
