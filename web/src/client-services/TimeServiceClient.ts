/**
 * Time Service IPC Client
 *
 * This TypeScript client provides a clean API for interacting with the
 * time WASM process to manage time display settings.
 *
 * Architecture:
 * - Client constructs JSON IPC messages with proper message tags
 * - Supervisor provides generic send_service_ipc() and callback registration
 * - Settings are persisted to /system/settings/time.json via the service
 */

import { PendingRequestQueue } from '../shared/ipc';
import type { MinimalSupervisor } from '../shared/types';

// =============================================================================
// Message Tags (mirrors time.rs time_msg module)
// =============================================================================

/** IPC message tags for time service requests/responses */
export const TIME_MSG = {
  /** Request current time settings */
  GET_TIME_SETTINGS: 0x8001,
  /** Response with time settings */
  GET_TIME_SETTINGS_RESPONSE: 0x8002,
  /** Set time settings */
  SET_TIME_SETTINGS: 0x8003,
  /** Response confirming settings update */
  SET_TIME_SETTINGS_RESPONSE: 0x8004,
} as const;

// =============================================================================
// Types
// =============================================================================

/** Time settings that can be persisted */
export interface TimeSettings {
  /** Use 24-hour time format (false = 12-hour with AM/PM) */
  time_format_24h: boolean;
  /** Timezone identifier (e.g., "America/New_York", "UTC") */
  timezone: string;
}

/** Default time settings */
export const DEFAULT_TIME_SETTINGS: TimeSettings = {
  time_format_24h: false,
  timezone: 'UTC',
};

// =============================================================================
// Error Classes
// =============================================================================

/**
 * Base class for Time Service errors.
 */
export class TimeServiceError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'TimeServiceError';
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, this.constructor);
    }
  }
}

/**
 * Service was not found or is not running.
 */
export class TimeServiceNotFoundError extends TimeServiceError {
  constructor() {
    super('Time service not found');
    this.name = 'TimeServiceNotFoundError';
  }
}

/**
 * Request timed out waiting for response.
 */
export class TimeRequestTimeoutError extends TimeServiceError {
  public readonly timeoutMs: number;

  constructor(timeoutMs: number) {
    super(`Request timed out after ${timeoutMs}ms`);
    this.name = 'TimeRequestTimeoutError';
    this.timeoutMs = timeoutMs;
  }
}

// Re-export Supervisor type for backward compatibility
export type { MinimalSupervisor as Supervisor } from '../shared/types';

// =============================================================================
// Shared request queue for all TimeServiceClient instances
// =============================================================================

const requestQueue = new PendingRequestQueue({ name: 'TimeServiceClient' });

// =============================================================================
// TimeServiceClient
// =============================================================================

/**
 * Client for Time Service IPC communication.
 *
 * Uses the supervisor's generic IPC APIs to communicate with the time
 * service for managing time display settings.
 */
export class TimeServiceClient {
  private supervisor: MinimalSupervisor;
  private timeoutMs: number;

  constructor(supervisor: MinimalSupervisor, timeoutMs = 5000) {
    this.supervisor = supervisor;
    this.timeoutMs = timeoutMs;
    requestQueue.register(supervisor);
  }

  /**
   * Send a request to the time service and wait for response.
   */
  private async request<T>(tag: number, data: object): Promise<T> {
    const requestJson = JSON.stringify(data);

    const tagHex = this.supervisor.send_service_ipc('time', tag, requestJson);

    // Check for immediate errors
    if (tagHex.startsWith('error:service_not_found:')) {
      throw new TimeServiceNotFoundError();
    }
    if (tagHex.startsWith('error:')) {
      throw new TimeServiceError(tagHex);
    }

    // Use shared request queue to wait for response
    return requestQueue.addRequest<T>(tagHex, this.timeoutMs);
  }

  // ===========================================================================
  // Public API
  // ===========================================================================

  /**
   * Get current time settings from the service.
   *
   * @returns TimeSettings object
   */
  async getTimeSettings(): Promise<TimeSettings> {
    try {
      const response = await this.request<TimeSettings>(TIME_MSG.GET_TIME_SETTINGS, {});
      return response;
    } catch (error) {
      // On error, return defaults
      console.warn('[TimeServiceClient] Failed to get settings, using defaults:', error);
      return DEFAULT_TIME_SETTINGS;
    }
  }

  /**
   * Update time settings.
   *
   * @param settings - New settings to save
   */
  async setTimeSettings(settings: TimeSettings): Promise<void> {
    await this.request<TimeSettings>(TIME_MSG.SET_TIME_SETTINGS, settings);
  }

  /**
   * Update just the time format preference.
   *
   * @param timeFormat24h - True for 24-hour format, false for 12-hour
   */
  async setTimeFormat24h(timeFormat24h: boolean): Promise<void> {
    const current = await this.getTimeSettings();
    await this.setTimeSettings({
      ...current,
      time_format_24h: timeFormat24h,
    });
  }

  /**
   * Update just the timezone.
   *
   * @param timezone - Timezone identifier (e.g., "America/New_York")
   */
  async setTimezone(timezone: string): Promise<void> {
    const current = await this.getTimeSettings();
    await this.setTimeSettings({
      ...current,
      timezone,
    });
  }
}
