/**
 * Pending Request Management
 *
 * Manages the queue of pending IPC requests waiting for responses.
 * Uses FIFO queues per response tag to handle concurrent requests.
 */

import type { Supervisor } from './types';

// =============================================================================
// Types
// =============================================================================

export interface PendingRequest<T> {
  resolve: (data: T) => void;
  reject: (error: Error) => void;
  timeoutId: ReturnType<typeof setTimeout>;
  uniqueId: string;
}

// =============================================================================
// Module State
// =============================================================================

/**
 * Counter for generating unique request IDs.
 * Combined with the response tag to create truly unique identifiers.
 */
let requestCounter = 0;

/**
 * Map of pending requests by response tag (hex).
 * Each tag has a FIFO queue of pending requests to handle concurrent requests
 * of the same message type.
 */
const pendingRequestsByTag = new Map<string, PendingRequest<unknown>[]>();

/**
 * Map of pending requests by unique ID (for timeout cleanup).
 * This allows us to find and remove a specific request from its tag's queue.
 */
const pendingRequestsById = new Map<string, { tagHex: string; request: PendingRequest<unknown> }>();

/** Track whether callback has been registered */
let callbackRegistered = false;

/** Track the supervisor we've registered with */
let registeredSupervisor: Supervisor | null = null;

// =============================================================================
// Request Management Functions
// =============================================================================

/**
 * Generate a unique request ID.
 * Format: {counter}-{tag_hex}
 */
export function generateUniqueRequestId(tagHex: string): string {
  return `${++requestCounter}-${tagHex}`;
}

/**
 * Add a pending request to the queue for its tag.
 */
export function addPendingRequest<T>(tagHex: string, request: PendingRequest<T>): void {
  let queue = pendingRequestsByTag.get(tagHex);
  if (!queue) {
    queue = [];
    pendingRequestsByTag.set(tagHex, queue);
  }
  queue.push(request as PendingRequest<unknown>);
  pendingRequestsById.set(request.uniqueId, {
    tagHex,
    request: request as PendingRequest<unknown>,
  });
}

/**
 * Remove a pending request by its unique ID (used for timeout cleanup).
 */
export function removePendingRequestById(uniqueId: string): boolean {
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
export function resolveOldestPendingRequest(tagHex: string, data: unknown): boolean {
  const queue = pendingRequestsByTag.get(tagHex);
  if (!queue || queue.length === 0) {
    return false;
  }

  // FIFO: resolve the oldest request (first in queue)
  const request = queue.shift();
  if (!request) return false;
  clearTimeout(request.timeoutId);
  pendingRequestsById.delete(request.uniqueId);

  if (queue.length === 0) {
    pendingRequestsByTag.delete(tagHex);
  }

  request.resolve(data);
  return true;
}

/**
 * Get the queue for a specific tag.
 */
export function getQueueForTag(tagHex: string): PendingRequest<unknown>[] | undefined {
  return pendingRequestsByTag.get(tagHex);
}

/**
 * Reject and remove the oldest request from a queue.
 */
export function rejectOldestRequest(tagHex: string, error: Error): boolean {
  const queue = pendingRequestsByTag.get(tagHex);
  if (!queue || queue.length === 0) return false;

  const request = queue.shift();
  if (!request) return false;

  clearTimeout(request.timeoutId);
  pendingRequestsById.delete(request.uniqueId);

  if (queue.length === 0) {
    pendingRequestsByTag.delete(tagHex);
  }

  request.reject(error);
  return true;
}

// =============================================================================
// Callback Registration
// =============================================================================

/**
 * Ensure the IPC response callback is registered with the supervisor.
 * This is called once per supervisor instance.
 */
export function ensureCallbackRegistered(supervisor: Supervisor): void {
  // Only register once per supervisor
  if (callbackRegistered && registeredSupervisor === supervisor) {
    return;
  }

  // Register the callback for ALL IPC responses (event-based, no polling)
  supervisor.set_ipc_response_callback((requestId: string, data: string) => {
    // requestId is the response tag hex (e.g., "00007055")
    // We use this to find the queue of pending requests for that tag
    try {
      const parsed = JSON.parse(data);
      if (resolveOldestPendingRequest(requestId, parsed)) {
        // Successfully resolved a pending request
      } else {
        console.log(
          `[IdentityServiceClient] Received response for tag ${requestId} with no pending requests`
        );
      }
    } catch (e) {
      // Try to reject the oldest pending request for this tag
      if (!rejectOldestRequest(requestId, new Error(`Invalid response JSON: ${e}`))) {
        console.log(
          `[IdentityServiceClient] Received invalid JSON for tag ${requestId} with no pending requests`
        );
      }
    }
  });

  callbackRegistered = true;
  registeredSupervisor = supervisor;
  console.log('[IdentityServiceClient] IPC response callback registered');
}
