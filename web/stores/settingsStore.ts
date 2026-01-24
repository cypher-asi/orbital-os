/**
 * Settings Store - Centralized state for system settings.
 *
 * Manages time format, timezone, and other system preferences.
 * Syncs with TimeServiceClient for persistence via the time_service WASM process.
 * Falls back to localStorage when service is unavailable.
 */

import { create } from 'zustand';
import { persist, subscribeWithSelector } from 'zustand/middleware';
import {
  TimeServiceClient,
  DEFAULT_TIME_SETTINGS,
  type TimeSettings,
  type Supervisor,
} from '../services';

// =============================================================================
// Store Types
// =============================================================================

interface SettingsStoreState {
  // Time settings
  timeFormat24h: boolean;
  timezone: string;

  // Loading state
  isLoading: boolean;
  isSynced: boolean;
  error: string | null;

  // Internal: Service client reference
  _serviceClient: TimeServiceClient | null;

  // Actions
  setTimeFormat24h: (value: boolean) => Promise<void>;
  setTimezone: (value: string) => Promise<void>;

  // Service sync
  initializeService: (supervisor: Supervisor) => void;
  syncFromService: () => Promise<void>;
}

// =============================================================================
// Store Implementation
// =============================================================================

export const useSettingsStore = create<SettingsStoreState>()(
  subscribeWithSelector(
    persist(
      (set, get) => ({
        // Default values
        timeFormat24h: DEFAULT_TIME_SETTINGS.time_format_24h,
        timezone: DEFAULT_TIME_SETTINGS.timezone,

        isLoading: false,
        isSynced: false,
        error: null,

        _serviceClient: null,

        // Initialize with supervisor reference
        initializeService: (supervisor: Supervisor) => {
          const client = new TimeServiceClient(supervisor);
          set({ _serviceClient: client });

          // Sync from service on initialization
          get().syncFromService();
        },

        // Sync settings from service
        syncFromService: async () => {
          const client = get()._serviceClient;
          if (!client) {
            console.log('[SettingsStore] No service client, using cached values');
            return;
          }

          set({ isLoading: true, error: null });

          try {
            const settings = await client.getTimeSettings();
            set({
              timeFormat24h: settings.time_format_24h,
              timezone: settings.timezone,
              isLoading: false,
              isSynced: true,
            });
            console.log('[SettingsStore] Synced from service:', settings);
          } catch (error) {
            console.warn('[SettingsStore] Failed to sync from service:', error);
            set({
              isLoading: false,
              error: error instanceof Error ? error.message : 'Failed to sync settings',
            });
          }
        },

        // Set time format (12h/24h)
        setTimeFormat24h: async (value: boolean) => {
          const prevValue = get().timeFormat24h;
          const client = get()._serviceClient;

          // Optimistic update
          set({ timeFormat24h: value, error: null });

          if (client) {
            try {
              await client.setTimeSettings({
                time_format_24h: value,
                timezone: get().timezone,
              });
              console.log('[SettingsStore] Time format saved:', value);
            } catch (error) {
              console.warn('[SettingsStore] Failed to save time format:', error);
              // Revert on error
              set({
                timeFormat24h: prevValue,
                error: error instanceof Error ? error.message : 'Failed to save setting',
              });
            }
          }
        },

        // Set timezone
        setTimezone: async (value: string) => {
          const prevValue = get().timezone;
          const client = get()._serviceClient;

          // Optimistic update
          set({ timezone: value, error: null });

          if (client) {
            try {
              await client.setTimeSettings({
                time_format_24h: get().timeFormat24h,
                timezone: value,
              });
              console.log('[SettingsStore] Timezone saved:', value);
            } catch (error) {
              console.warn('[SettingsStore] Failed to save timezone:', error);
              // Revert on error
              set({
                timezone: prevValue,
                error: error instanceof Error ? error.message : 'Failed to save setting',
              });
            }
          }
        },
      }),
      {
        name: 'zero-settings-store',
        // Persist time settings to localStorage as fallback cache
        partialize: (state) => ({
          timeFormat24h: state.timeFormat24h,
          timezone: state.timezone,
        }),
      }
    )
  )
);

// =============================================================================
// Selectors
// =============================================================================

/** Select time format preference */
export const selectTimeFormat24h = (state: SettingsStoreState) => state.timeFormat24h;

/** Select timezone */
export const selectTimezone = (state: SettingsStoreState) => state.timezone;

/** Select loading state */
export const selectSettingsIsLoading = (state: SettingsStoreState) => state.isLoading;

/** Select sync status */
export const selectSettingsIsSynced = (state: SettingsStoreState) => state.isSynced;

/** Select error state */
export const selectSettingsError = (state: SettingsStoreState) => state.error;

// =============================================================================
// Time Formatting Utilities
// =============================================================================

/**
 * Format a timestamp as time string based on settings.
 *
 * @param timestamp - Unix timestamp in milliseconds
 * @param timezone - Timezone identifier (e.g., "America/New_York")
 * @param timeFormat24h - Use 24-hour format
 * @returns Formatted time string (e.g., "2:30 PM" or "14:30")
 */
export function formatTime(
  timestamp: number,
  timezone: string,
  timeFormat24h: boolean
): string {
  try {
    const date = new Date(timestamp);
    return date.toLocaleTimeString('en-US', {
      timeZone: timezone,
      hour: 'numeric',
      minute: '2-digit',
      hour12: !timeFormat24h,
    });
  } catch {
    // Fallback if timezone is invalid
    const date = new Date(timestamp);
    return date.toLocaleTimeString('en-US', {
      hour: 'numeric',
      minute: '2-digit',
      hour12: !timeFormat24h,
    });
  }
}

/**
 * Format a timestamp as date string.
 *
 * @param timestamp - Unix timestamp in milliseconds
 * @param timezone - Timezone identifier
 * @returns Formatted date string (e.g., "Jan 23, 2026")
 */
export function formatDate(timestamp: number, timezone: string): string {
  try {
    const date = new Date(timestamp);
    return date.toLocaleDateString('en-US', {
      timeZone: timezone,
      month: 'short',
      day: 'numeric',
      year: 'numeric',
    });
  } catch {
    const date = new Date(timestamp);
    return date.toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      year: 'numeric',
    });
  }
}

/**
 * Format a timestamp as short date (day and month only).
 *
 * @param timestamp - Unix timestamp in milliseconds
 * @param timezone - Timezone identifier
 * @returns Formatted short date (e.g., "Jan 23")
 */
export function formatShortDate(timestamp: number, timezone: string): string {
  try {
    const date = new Date(timestamp);
    return date.toLocaleDateString('en-US', {
      timeZone: timezone,
      month: 'short',
      day: 'numeric',
    });
  } catch {
    const date = new Date(timestamp);
    return date.toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
    });
  }
}

/**
 * Common timezones for the settings UI.
 */
export const COMMON_TIMEZONES = [
  { value: 'UTC', label: 'UTC' },
  { value: 'America/New_York', label: 'Eastern Time (US)' },
  { value: 'America/Chicago', label: 'Central Time (US)' },
  { value: 'America/Denver', label: 'Mountain Time (US)' },
  { value: 'America/Los_Angeles', label: 'Pacific Time (US)' },
  { value: 'America/Anchorage', label: 'Alaska Time' },
  { value: 'Pacific/Honolulu', label: 'Hawaii Time' },
  { value: 'Europe/London', label: 'London (GMT/BST)' },
  { value: 'Europe/Paris', label: 'Paris (CET/CEST)' },
  { value: 'Europe/Berlin', label: 'Berlin (CET/CEST)' },
  { value: 'Asia/Tokyo', label: 'Tokyo (JST)' },
  { value: 'Asia/Shanghai', label: 'Shanghai (CST)' },
  { value: 'Asia/Singapore', label: 'Singapore (SGT)' },
  { value: 'Australia/Sydney', label: 'Sydney (AEST/AEDT)' },
] as const;
