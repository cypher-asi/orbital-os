/**
 * Time Service IPC Client
 *
 * This TypeScript client provides a clean API for interacting with the
 * time_service WASM process to manage time display settings.
 *
 * Architecture:
 * - Client constructs JSON IPC messages with proper message tags
 * - Supervisor provides generic send_service_ipc() and callback registration
 * - Settings are persisted to /system/settings/time.json via the service
 */

// =============================================================================
// Message Tags (mirrors time_service.rs time_msg module)
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

// =============================================================================
// Supervisor interface (minimal subset needed by this client)
// =============================================================================

export interface Supervisor {
  /** Register callback for IPC responses (event-based) */
  set_ipc_response_callback(callback: (requestId: string, data: string) => void): void;
  /** Send IPC to a named service, returns request_id */
  send_service_ipc(serviceName: string, tag: number, data: string): string;
  /** Process pending syscalls (needed to let service run) */
  poll_syscalls(): number;
}

// =============================================================================
// Pending request management
// =============================================================================

interface PendingRequest<T> {
  resolve: (data: T) => void;
  reject: (error: Error) => void;
  timeoutId: ReturnType<typeof setTimeout>;
  uniqueId: string;
}

/** Counter for generating unique request IDs */
let requestCounter = 0;

/** Map of pending requests by response tag (hex) */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const pendingRequestsByTag = new Map<string, PendingRequest<any>[]>();

/** Map of pending requests by unique ID (for timeout cleanup) */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const pendingRequestsById = new Map<string, { tagHex: string; request: PendingRequest<any> }>();

/** Track whether callback has been registered */
let callbackRegistered = false;

/** Track the supervisor we've registered with */
let registeredSupervisor: Supervisor | null = null;

/**
 * Generate a unique request ID.
 */
function generateUniqueRequestId(tagHex: string): string {
  return `time-${++requestCounter}-${tagHex}`;
}

/**
 * Add a pending request to the queue for its tag.
 */
function addPendingRequest<T>(tagHex: string, request: PendingRequest<T>): void {
  let queue = pendingRequestsByTag.get(tagHex);
  if (!queue) {
    queue = [];
    pendingRequestsByTag.set(tagHex, queue);
  }
  queue.push(request);
  pendingRequestsById.set(request.uniqueId, { tagHex, request });
}

/**
 * Remove a pending request by its unique ID (used for timeout cleanup).
 */
function removePendingRequestById(uniqueId: string): boolean {
  const entry = pendingRequestsById.get(uniqueId);
  if (!entry) return false;

  const { tagHex, request } = entry;
  const queue = pendingRequestsByTag.get(tagHex);
  if (queue) {
    const index = queue.indexOf(request);
    if (index !== -1) {
      queue.splice(index, 1);
      if (queue.length === 0) {
        pendingRequestsByTag.delete(tagHex);
      }
    }
  }
  pendingRequestsById.delete(uniqueId);
  return true;
}

/**
 * Resolve the oldest pending request for a given tag (FIFO).
 */
function resolveOldestPendingRequest(tagHex: string, data: unknown): boolean {
  const queue = pendingRequestsByTag.get(tagHex);
  if (!queue || queue.length === 0) {
    return false;
  }

  const request = queue.shift()!;
  clearTimeout(request.timeoutId);
  pendingRequestsById.delete(request.uniqueId);

  if (queue.length === 0) {
    pendingRequestsByTag.delete(tagHex);
  }

  request.resolve(data);
  return true;
}

/**
 * Ensure the IPC response callback is registered with the supervisor.
 */
function ensureCallbackRegistered(supervisor: Supervisor): void {
  if (callbackRegistered && registeredSupervisor === supervisor) {
    return;
  }

  supervisor.set_ipc_response_callback((requestId: string, data: string) => {
    try {
      const parsed = JSON.parse(data);
      if (resolveOldestPendingRequest(requestId, parsed)) {
        // Successfully resolved
      } else {
        console.log(`[TimeServiceClient] Received response for tag ${requestId} with no pending requests`);
      }
    } catch (e) {
      const queue = pendingRequestsByTag.get(requestId);
      if (queue && queue.length > 0) {
        const request = queue.shift()!;
        clearTimeout(request.timeoutId);
        pendingRequestsById.delete(request.uniqueId);
        if (queue.length === 0) {
          pendingRequestsByTag.delete(requestId);
        }
        request.reject(new Error(`Invalid response JSON: ${e}`));
      }
    }
  });

  callbackRegistered = true;
  registeredSupervisor = supervisor;
  console.log('[TimeServiceClient] IPC response callback registered');
}

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
  private supervisor: Supervisor;
  private timeoutMs: number;

  constructor(supervisor: Supervisor, timeoutMs = 5000) {
    this.supervisor = supervisor;
    this.timeoutMs = timeoutMs;
    ensureCallbackRegistered(supervisor);
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

    const uniqueId = generateUniqueRequestId(tagHex);
    const timeoutMs = this.timeoutMs;

    return new Promise<T>((resolve, reject) => {
      const timeoutId = setTimeout(() => {
        if (removePendingRequestById(uniqueId)) {
          reject(new TimeRequestTimeoutError(timeoutMs));
        }
      }, timeoutMs);

      const pendingRequest: PendingRequest<T> = {
        resolve: resolve as (data: unknown) => void,
        reject,
        timeoutId,
        uniqueId,
      };

      addPendingRequest(tagHex, pendingRequest);
      // Note: We rely on the global polling loop in main.tsx (setInterval calling poll_syscalls)
      // to process syscalls. The IPC response callback will resolve this promise when
      // the response arrives. Having our own polling loop causes race conditions with
      // the global loop, leading to "recursive use of an object" errors in Rust WASM.
    });
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
      const response = await this.request<TimeSettings>(
        TIME_MSG.GET_TIME_SETTINGS,
        {}
      );
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
    await this.request<TimeSettings>(
      TIME_MSG.SET_TIME_SETTINGS,
      settings
    );
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
