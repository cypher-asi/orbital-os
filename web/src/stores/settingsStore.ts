/**
 * Settings Store - Centralized state for system settings.
 *
 * Manages time format, timezone, and other system preferences.
 * Syncs with TimeServiceClient for persistence via the time WASM process.
 * Falls back to localStorage when service is unavailable.
 */

import { create } from 'zustand';
import { persist, subscribeWithSelector } from 'zustand/middleware';
import {
  TimeServiceClient,
  DEFAULT_TIME_SETTINGS,
  IdentityServiceClient,
  type Supervisor,
  type KeyScheme,
} from '@/client-services';

// =============================================================================
// Constants
// =============================================================================

/** Default RPC endpoint for Zero-ID service */
export const DEFAULT_RPC_ENDPOINT = '127.0.0.1:9999';

// =============================================================================
// Store Types
// =============================================================================

/** Settings navigation areas */
export type SettingsArea = 'general' | 'identity' | 'network' | 'permissions' | 'theme';

/** Settings sub-panel identifiers for deep-linking */
export type SettingsSubPanel = 'neural-key' | 'machine-keys' | 'linked-accounts';

/** Pending navigation state - supports deep-linking to sub-panels */
export interface PendingNavigation {
  area: SettingsArea;
  subPanel?: SettingsSubPanel;
}

interface SettingsStoreState {
  // Time settings
  timeFormat24h: boolean;
  timezone: string;

  // Network settings
  rpcEndpoint: string;

  // Identity preferences
  defaultKeyScheme: KeyScheme;
  /** Default machine key ID for authentication (hex string) */
  defaultMachineId: string | null;
  isLoadingPreferences: boolean;

  // Navigation state (replaces module-level pendingNavigation)
  pendingNavigation: PendingNavigation | null;

  // Loading state
  isLoading: boolean;
  isSynced: boolean;
  error: string | null;

  // Internal: Service client references
  _serviceClient: TimeServiceClient | null;
  _identityClient: IdentityServiceClient | null;

  // Actions
  setTimeFormat24h: (value: boolean) => Promise<void>;
  setTimezone: (value: string) => Promise<void>;
  setRpcEndpoint: (value: string) => void;

  // Identity preferences actions
  loadIdentityPreferences: (userId: bigint) => Promise<void>;
  setDefaultKeyScheme: (userId: bigint, scheme: KeyScheme) => Promise<void>;
  setDefaultMachineKey: (userId: bigint, machineId: string) => Promise<void>;

  // Navigation actions
  setPendingNavigation: (navigation: PendingNavigation) => void;
  clearPendingNavigation: () => void;

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
        rpcEndpoint: DEFAULT_RPC_ENDPOINT,
        defaultKeyScheme: 'classical',
        defaultMachineId: null,
        isLoadingPreferences: false,

        // Navigation state
        pendingNavigation: null,

        isLoading: false,
        isSynced: false,
        error: null,

        _serviceClient: null,
        _identityClient: null,

        // Navigation actions
        setPendingNavigation: (navigation: PendingNavigation) => {
          set({ pendingNavigation: navigation });
        },

        clearPendingNavigation: () => {
          set({ pendingNavigation: null });
        },

        // Initialize with supervisor reference
        initializeService: (supervisor: Supervisor) => {
          const timeClient = new TimeServiceClient(supervisor);
          const identityClient = new IdentityServiceClient(supervisor);
          set({ _serviceClient: timeClient, _identityClient: identityClient });

          // Sync from service on initialization
          get().syncFromService();
        },

        // Load identity preferences from VFS
        loadIdentityPreferences: async (userId: bigint) => {
          const client = get()._identityClient;
          if (!client) {
            console.log('[SettingsStore] No identity client available');
            return;
          }

          set({ isLoadingPreferences: true });
          try {
            const prefs = await client.getIdentityPreferences(userId);
            set({
              defaultKeyScheme: prefs.default_key_scheme,
              defaultMachineId: prefs.default_machine_id ?? null,
              isLoadingPreferences: false,
            });
            console.log('[SettingsStore] Loaded identity preferences:', prefs);
          } catch (err) {
            console.error('[SettingsStore] Failed to load preferences:', err);
            set({ isLoadingPreferences: false });
          }
        },

        // Set default key scheme in VFS
        setDefaultKeyScheme: async (userId: bigint, scheme: KeyScheme) => {
          const prevScheme = get().defaultKeyScheme;
          // Optimistic update - always set in store state for UI
          set({ defaultKeyScheme: scheme });

          const client = get()._identityClient;
          if (!client) {
            // No client to persist, but UI state is updated
            console.log('[SettingsStore] No identity client available, default key scheme set in memory only:', scheme);
            return;
          }

          try {
            await client.setDefaultKeyScheme(userId, scheme);
            console.log('[SettingsStore] Updated default key scheme:', scheme);
          } catch (err) {
            console.error('[SettingsStore] Failed to set default key scheme:', err);
            // Revert on error
            set({ defaultKeyScheme: prevScheme });
            throw err;
          }
        },

        // Set default machine key in VFS
        setDefaultMachineKey: async (userId: bigint, machineId: string) => {
          const prevMachineId = get().defaultMachineId;
          // Optimistic update - always set in store state for UI
          set({ defaultMachineId: machineId });

          const client = get()._identityClient;
          if (!client) {
            // No client to persist, but UI state is updated
            console.log('[SettingsStore] No identity client available, default machine key set in memory only:', machineId);
            return;
          }

          try {
            await client.setDefaultMachineKey(userId, machineId);
            console.log('[SettingsStore] Updated default machine key:', machineId);
          } catch (err) {
            console.error('[SettingsStore] Failed to set default machine key:', err);
            // Revert on error
            set({ defaultMachineId: prevMachineId });
            throw err;
          }
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

        // Set RPC endpoint
        // TODO: Persist via identity service when wired up
        setRpcEndpoint: (value: string) => {
          set({ rpcEndpoint: value });
          console.log('[SettingsStore] RPC endpoint saved:', value);
        },
      }),
      {
        name: 'zero-settings-store',
        // Persist settings to localStorage as fallback cache
        partialize: (state) => ({
          timeFormat24h: state.timeFormat24h,
          timezone: state.timezone,
          rpcEndpoint: state.rpcEndpoint,
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

/** Select RPC endpoint */
export const selectRpcEndpoint = (state: SettingsStoreState) => state.rpcEndpoint;

/** Select loading state */
export const selectSettingsIsLoading = (state: SettingsStoreState) => state.isLoading;

/** Select sync status */
export const selectSettingsIsSynced = (state: SettingsStoreState) => state.isSynced;

/** Select error state */
export const selectSettingsError = (state: SettingsStoreState) => state.error;

/** Select pending navigation */
export const selectPendingNavigation = (state: SettingsStoreState) => state.pendingNavigation;

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
export function formatTime(timestamp: number, timezone: string, timeFormat24h: boolean): string {
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
