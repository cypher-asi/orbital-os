/**
 * PendingRequestQueue - Shared IPC request management
 *
 * This module provides a reusable queue for managing pending IPC requests
 * waiting for responses. Uses FIFO queues per response tag to handle
 * concurrent requests of the same type.
 *
 * Used by:
 * - IdentityServiceClient
 * - TimeServiceClient
 * - Any future service clients
 */

import type { MinimalSupervisor } from '../types/supervisor';

// =============================================================================
// Types
// =============================================================================

/**
 * A pending request waiting for its response.
 */
export interface PendingRequest<T> {
  resolve: (data: T) => void;
  reject: (error: Error) => void;
  timeoutId: ReturnType<typeof setTimeout>;
  uniqueId: string;
}

/**
 * Options for creating a PendingRequestQueue.
 */
export interface PendingRequestQueueOptions {
  /** Name for logging purposes (e.g., "IdentityServiceClient") */
  name: string;
}

// =============================================================================
// PendingRequestQueue Class
// =============================================================================

/**
 * Manages pending IPC requests waiting for responses.
 *
 * Features:
 * - FIFO queue per response tag for concurrent request handling
 * - Unique request IDs for timeout cleanup
 * - Event-based callback registration with supervisor
 *
 * Usage:
 * ```ts
 * const queue = new PendingRequestQueue({ name: 'MyClient' });
 * queue.register(supervisor);
 *
 * // In request method:
 * const tagHex = supervisor.send_service_ipc('service', tag, data);
 * return queue.addRequest<ResponseType>(tagHex, timeoutMs);
 * ```
 */
export class PendingRequestQueue {
  private name: string;
  private requestCounter = 0;
  private pendingRequestsByTag = new Map<string, PendingRequest<unknown>[]>();
  private pendingRequestsById = new Map<string, { tagHex: string; request: PendingRequest<unknown> }>();
  private callbackRegistered = false;
  private registeredSupervisor: MinimalSupervisor | null = null;

  constructor(options: PendingRequestQueueOptions) {
    this.name = options.name;
  }

  /**
   * Register the IPC response callback with the supervisor.
   * This should be called once when creating a service client.
   */
  register(supervisor: MinimalSupervisor): void {
    if (this.callbackRegistered && this.registeredSupervisor === supervisor) {
      return;
    }

    supervisor.set_ipc_response_callback((requestId: string, data: string) => {
      // IMPORTANT: Use queueMicrotask to defer processing and prevent wasm-bindgen reentrancy.
      // The Rust Supervisor is borrowed when this callback is invoked via callback.call2().
      // If we process synchronously, promise resolution could trigger React re-renders that
      // call back into the Supervisor (e.g., get_wallclock_ms), causing "recursive use of an
      // object detected" errors. Deferring to a microtask releases the Rust borrow first.
      queueMicrotask(() => {
        try {
          const parsed = JSON.parse(data);
          if (this.resolveOldestPendingRequest(requestId, parsed)) {
            // Successfully resolved a pending request
          } else {
            console.log(
              `[${this.name}] Received response for tag ${requestId} with no pending requests`
            );
          }
        } catch (e) {
          // Try to reject the oldest pending request for this tag
          if (!this.rejectOldestRequest(requestId, new Error(`Invalid response JSON: ${e}`))) {
            console.log(
              `[${this.name}] Received invalid JSON for tag ${requestId} with no pending requests`
            );
          }
        }
      });
    });

    this.callbackRegistered = true;
    this.registeredSupervisor = supervisor;
    console.log(`[${this.name}] IPC response callback registered`);
  }

  /**
   * Add a pending request and return a promise that resolves when response arrives.
   *
   * @param tagHex - The response tag hex string from send_service_ipc
   * @param timeoutMs - Timeout in milliseconds
   * @returns Promise that resolves with the parsed response
   */
  addRequest<T>(tagHex: string, timeoutMs: number): Promise<T> {
    const uniqueId = this.generateUniqueRequestId(tagHex);

    return new Promise<T>((resolve, reject) => {
      const timeoutId = setTimeout(() => {
        if (this.removePendingRequestById(uniqueId)) {
          reject(new Error(`Request timed out after ${timeoutMs}ms`));
        }
      }, timeoutMs);

      const pendingRequest: PendingRequest<T> = {
        resolve: resolve as (data: unknown) => void,
        reject,
        timeoutId,
        uniqueId,
      };

      this.addPendingRequestToQueue(tagHex, pendingRequest);
    });
  }

  // ===========================================================================
  // Internal Methods
  // ===========================================================================

  private generateUniqueRequestId(tagHex: string): string {
    return `${this.name}-${++this.requestCounter}-${tagHex}`;
  }

  private addPendingRequestToQueue<T>(tagHex: string, request: PendingRequest<T>): void {
    let queue = this.pendingRequestsByTag.get(tagHex);
    if (!queue) {
      queue = [];
      this.pendingRequestsByTag.set(tagHex, queue);
    }
    queue.push(request as PendingRequest<unknown>);
    this.pendingRequestsById.set(request.uniqueId, {
      tagHex,
      request: request as PendingRequest<unknown>,
    });
  }

  private removePendingRequestById(uniqueId: string): boolean {
    const entry = this.pendingRequestsById.get(uniqueId);
    if (!entry) return false;

    const { tagHex, request } = entry;
    const queue = this.pendingRequestsByTag.get(tagHex);
    if (queue) {
      const index = queue.indexOf(request);
      if (index !== -1) {
        queue.splice(index, 1);
        if (queue.length === 0) {
          this.pendingRequestsByTag.delete(tagHex);
        }
      }
    }
    this.pendingRequestsById.delete(uniqueId);
    return true;
  }

  private resolveOldestPendingRequest(tagHex: string, data: unknown): boolean {
    const queue = this.pendingRequestsByTag.get(tagHex);
    if (!queue || queue.length === 0) {
      return false;
    }

    const request = queue.shift();
    if (!request) return false;
    clearTimeout(request.timeoutId);
    this.pendingRequestsById.delete(request.uniqueId);

    if (queue.length === 0) {
      this.pendingRequestsByTag.delete(tagHex);
    }

    request.resolve(data);
    return true;
  }

  private rejectOldestRequest(tagHex: string, error: Error): boolean {
    const queue = this.pendingRequestsByTag.get(tagHex);
    if (!queue || queue.length === 0) return false;

    const request = queue.shift();
    if (!request) return false;

    clearTimeout(request.timeoutId);
    this.pendingRequestsById.delete(request.uniqueId);

    if (queue.length === 0) {
      this.pendingRequestsByTag.delete(tagHex);
    }

    request.reject(error);
    return true;
  }
}
