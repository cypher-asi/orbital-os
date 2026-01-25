/**
 * ZosNetwork - Network HAL for Zero OS
 *
 * This HAL provides HTTP request capabilities for WASM processes via the
 * Network Service. It uses the browser's fetch API to make HTTP requests.
 *
 * ## Architecture
 *
 * ZosNetwork is called by the supervisor when a WASM process makes a
 * network_fetch_async syscall. The flow is:
 *
 * 1. Process calls syscall::network_fetch_async()
 * 2. Supervisor intercepts and calls ZosNetwork.startFetch()
 * 3. ZosNetwork performs fetch() and waits for response
 * 4. ZosNetwork calls supervisor.onNetworkResult() with the result
 * 5. Supervisor delivers MSG_NET_RESULT to the process via IPC
 *
 * ## Security
 *
 * The Network Service enforces URL allowlists before calling the HAL.
 * This HAL is a pass-through and does not perform policy checks.
 */

console.log('[ZosNetwork] Script loading...');

const ZosNetwork = {
  // === Supervisor Reference ===
  /** @type {object|null} Reference to the WASM supervisor for callbacks */
  supervisor: null,

  // === Pending Requests ===
  /** @type {Map<number, AbortController>} Map of request_id -> AbortController for cancellation */
  pendingRequests: new Map(),

  // ==========================================================================
  // Initialization
  // ==========================================================================

  /**
   * Initialize ZosNetwork with the supervisor reference.
   * Must be called before any network operations.
   * @param {object} supervisor - The WASM supervisor instance
   */
  initSupervisor(supervisor) {
    this.supervisor = supervisor;
    console.log('[ZosNetwork] Supervisor reference set');
  },

  // ==========================================================================
  // Network Operations
  // ==========================================================================

  /**
   * Start an async HTTP fetch operation.
   * Called by the supervisor when WASM issues network_fetch_async.
   *
   * @param {number} requestId - Unique request ID for tracking
   * @param {number} pid - Process ID making the request
   * @param {object} request - HTTP request object with:
   *   - method: string (GET, POST, etc.)
   *   - url: string
   *   - headers: Array<[string, string]>
   *   - body: Uint8Array | null
   *   - timeout_ms: number
   */
  async startFetch(requestId, pid, request) {
    console.log(
      `[ZosNetwork] startFetch: request_id=${requestId}, pid=${pid}, method=${request.method}, url=${request.url}`
    );

    if (!this.supervisor) {
      console.error('[ZosNetwork] startFetch: supervisor not initialized');
      return;
    }

    // Capture supervisor reference for deferred callback
    const supervisor = this.supervisor;

    // Create abort controller for timeout/cancellation
    const abortController = new AbortController();
    this.pendingRequests.set(requestId, abortController);

    // Set up timeout
    const timeoutId = setTimeout(() => {
      abortController.abort();
    }, request.timeout_ms || 30000);

    try {
      // Build fetch options
      const fetchOptions = {
        method: request.method || 'GET',
        signal: abortController.signal,
      };

      // Add headers
      if (request.headers && request.headers.length > 0) {
        fetchOptions.headers = new Headers();
        for (const [key, value] of request.headers) {
          fetchOptions.headers.append(key, value);
        }
      }

      // Add body for non-GET requests
      if (request.body && request.method !== 'GET' && request.method !== 'HEAD') {
        fetchOptions.body = new Uint8Array(request.body);
      }

      // Perform fetch
      const response = await fetch(request.url, fetchOptions);

      // Clear timeout
      clearTimeout(timeoutId);

      // Read response body
      const bodyBuffer = await response.arrayBuffer();
      const body = new Uint8Array(bodyBuffer);

      // Collect response headers
      const headers = [];
      response.headers.forEach((value, key) => {
        headers.push([key, value]);
      });

      // Build success result
      const result = {
        result: {
          Ok: {
            status: response.status,
            headers: headers,
            body: Array.from(body),
          },
        },
      };

      console.log(
        `[ZosNetwork] Fetch complete: request_id=${requestId}, status=${response.status}, body_len=${body.length}`
      );

      // Defer callback to avoid re-entrancy with wasm-bindgen's RefCell borrow
      // (pid must be BigInt for WASM u64)
      setTimeout(() => supervisor.onNetworkResult(requestId, BigInt(pid), result), 0);
    } catch (error) {
      // Clear timeout
      clearTimeout(timeoutId);

      // Determine error type
      let errorResult;
      if (error.name === 'AbortError') {
        // Timeout or explicit abort
        errorResult = { result: { Err: 'Timeout' } };
      } else if (error.name === 'TypeError') {
        // Network error (CORS, DNS, connection refused, etc.)
        if (error.message.includes('Failed to fetch') || error.message.includes('NetworkError')) {
          errorResult = { result: { Err: 'ConnectionFailed' } };
        } else if (error.message.includes('URL')) {
          errorResult = { result: { Err: 'InvalidUrl' } };
        } else {
          errorResult = { result: { Err: { Other: error.message } } };
        }
      } else {
        errorResult = { result: { Err: { Other: error.message || 'Unknown error' } } };
      }

      console.log(`[ZosNetwork] Fetch error: request_id=${requestId}, error=${error.message}`);

      // Defer callback to avoid re-entrancy with wasm-bindgen's RefCell borrow
      setTimeout(() => supervisor.onNetworkResult(requestId, BigInt(pid), errorResult), 0);
    } finally {
      // Clean up pending request
      this.pendingRequests.delete(requestId);
    }
  },

  /**
   * Cancel a pending request.
   * @param {number} requestId - The request ID to cancel
   */
  cancelRequest(requestId) {
    const controller = this.pendingRequests.get(requestId);
    if (controller) {
      controller.abort();
      this.pendingRequests.delete(requestId);
      console.log(`[ZosNetwork] Cancelled request: ${requestId}`);
    }
  },

  /**
   * Get the number of pending requests.
   * @returns {number} Number of pending requests
   */
  getPendingCount() {
    return this.pendingRequests.size;
  },
};

// Make ZosNetwork available globally
if (typeof window !== 'undefined') {
  window.ZosNetwork = ZosNetwork;
  console.log('[ZosNetwork] Attached to window.ZosNetwork');
} else {
  console.log('[ZosNetwork] No window object available');
}
