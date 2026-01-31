/**
 * Supervisor Type - Kernel/Process management API
 *
 * This is the single source of truth for the Supervisor interface.
 * All service clients and hooks should import from here.
 *
 * The Supervisor is the bridge between React/TypeScript and the
 * zos-supervisor WASM module that manages OS processes.
 */

// =============================================================================
// Supervisor Interface
// =============================================================================

/**
 * Full Supervisor interface for kernel/process management.
 *
 * This interface provides access to all supervisor capabilities including:
 * - Process lifecycle (spawn, kill, monitoring)
 * - IPC message routing between processes
 * - System metrics and diagnostics
 * - Console I/O for terminal processes
 */
export interface Supervisor {
  // ===========================================================================
  // Core Kernel API
  // ===========================================================================

  /** Boot the kernel */
  boot(): void;
  /** Spawn the init process (PID 0) */
  spawn_init(): void;

  // ===========================================================================
  // Process Console I/O
  // ===========================================================================

  /** Legacy: Send input to first terminal (use send_input_to_process for isolation) */
  send_input(input: string): void;
  /** Send input to a specific process by PID */
  send_input_to_process(pid: number, input: string): void;
  /** Register a console callback for a specific process (per-process isolation) */
  register_console_callback(pid: bigint, callback: (text: string) => void): void;
  /** Unregister the console callback for a process */
  unregister_console_callback(pid: bigint): void;

  // ===========================================================================
  // Process Spawning
  // ===========================================================================

  /** Set callback for spawn requests from processes */
  set_spawn_callback(callback: (procType: string, name: string) => void): void;
  /** Complete a spawn request with the binary */
  complete_spawn(name: string, binary: Uint8Array): bigint;

  // ===========================================================================
  // Storage & Axiom
  // ===========================================================================

  /** Initialize the Axiom storage backend */
  init_axiom_storage(): Promise<boolean>;
  /** Initialize the VFS storage backend (zos-filesystem IndexedDB) */
  init_vfs_storage(): Promise<boolean>;
  /** Initialize the Keystore storage backend (zos-keystore IndexedDB) */
  init_keystore(): Promise<boolean>;
  /** Sync Axiom commit log to persistent storage */
  sync_axiom_log(): Promise<number>;

  // ===========================================================================
  // Syscall Processing
  // ===========================================================================

  /** Process pending syscalls from all processes */
  poll_syscalls(): number;
  /** Process pending messages from worker threads */
  process_worker_messages(): number;

  // ===========================================================================
  // Process Management
  // ===========================================================================

  /** Kill a process by PID */
  kill_process(pid: number): void;
  /** Kill all processes */
  kill_all_processes(): void;

  // ===========================================================================
  // System Metrics
  // ===========================================================================

  /** Get uptime in milliseconds (monotonic) */
  get_uptime_ms(): number;
  /** Get wall clock time in milliseconds */
  get_wallclock_ms(): number;
  /** Get count of running processes */
  get_process_count(): number;
  /** Get total memory used by all processes */
  get_total_memory(): number;
  /** Get count of IPC endpoints */
  get_endpoint_count(): number;
  /** Get count of pending IPC messages */
  get_pending_messages(): number;
  /** Get total IPC messages processed */
  get_total_ipc_messages(): number;

  // ===========================================================================
  // JSON APIs for UI
  // ===========================================================================

  /** Get process list as JSON */
  get_process_list_json(): string;
  /** Get capabilities for a specific process as JSON */
  get_process_capabilities_json(pid: number): string;
  /** Get all processes with their capabilities as JSON */
  get_processes_with_capabilities_json(): string;
  /** Get endpoint list as JSON */
  get_endpoint_list_json(): string;
  /** Get recent IPC traffic as JSON */
  get_ipc_traffic_json(count: number): string;
  /** Get system metrics as JSON */
  get_system_metrics_json(): string;
  /** Get Axiom storage stats as JSON */
  get_axiom_stats_json(): string;
  /** Get commit log entries as JSON */
  get_commitlog_json(count: number): string;
  /** Get system log entries as JSON */
  get_syslog_json(count: number): string;

  // ===========================================================================
  // Capability Management
  // ===========================================================================

  /** Revoke/delete a capability from any process (supervisor privilege) */
  revoke_capability(pid: bigint, slot: number): boolean;

  // ===========================================================================
  // Generic Service IPC API (Thin Boundary Layer)
  // ===========================================================================

  /**
   * Register a callback for IPC responses from services.
   *
   * This callback is invoked immediately when a SERVICE:RESPONSE debug message
   * is received from a service process. The callback receives:
   * - requestId: The response tag as hex string (e.g., "00007055")
   * - data: The JSON response data as a string
   *
   * This is event-based, not polling-based.
   */
  set_ipc_response_callback(callback: (requestId: string, data: string) => void): void;

  /**
   * Send an IPC message to a named service.
   *
   * This is a generic method that:
   * 1. Finds the service by name (e.g., "identity", "vfs", "time")
   * 2. Delivers the message to the service's input endpoint (slot 1)
   * 3. Returns a request_id for tracking the response
   *
   * @param serviceName - Service name (e.g., "identity", "vfs", "time")
   * @param tag - Request message tag (e.g., 0x7054 for MSG_GENERATE_NEURAL_KEY)
   * @param data - JSON request data as a string
   * @returns On success: request_id string (e.g., "00007055"); On error: "error:..." string
   */
  send_service_ipc(serviceName: string, tag: number, data: string): string;
}

// =============================================================================
// Minimal Supervisor (for service clients that only need IPC)
// =============================================================================

/**
 * Minimal Supervisor interface for service clients.
 *
 * This subset is all that's needed for IPC-based service clients
 * like IdentityServiceClient and TimeServiceClient.
 */
export type MinimalSupervisor = Pick<
  Supervisor,
  'set_ipc_response_callback' | 'send_service_ipc' | 'poll_syscalls'
>;
