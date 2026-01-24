import { createContext, useContext } from 'react';

// =============================================================================
// Supervisor Type - Kernel/Process management (from zos-supervisor-web)
// =============================================================================

export interface Supervisor {
  // Kernel API
  boot(): void;
  spawn_init(): void;
  /** Legacy: Send input to first terminal (use send_input_to_process for isolation) */
  send_input(input: string): void;
  /** Send input to a specific process by PID */
  send_input_to_process(pid: number, input: string): void;
  /** Legacy: Set global console callback (use register_console_callback for isolation) */
  set_console_callback(callback: (text: string) => void): void;
  /** Register a console callback for a specific process */
  register_console_callback(pid: number, callback: (text: string) => void): void;
  /** Unregister the console callback for a process */
  unregister_console_callback(pid: number): void;
  set_spawn_callback(callback: (procType: string, name: string) => void): void;
  complete_spawn(name: string, binary: Uint8Array): bigint;
  init_axiom_storage(): Promise<boolean>;
  sync_axiom_log(): Promise<number>;
  poll_syscalls(): number;
  process_worker_messages(): number;
  kill_process(pid: number): void;
  kill_all_processes(): void;
  get_uptime_ms(): number;
  get_wallclock_ms(): number;
  get_process_count(): number;
  get_total_memory(): number;
  get_endpoint_count(): number;
  get_pending_messages(): number;
  get_total_ipc_messages(): number;
  get_process_list_json(): string;
  get_process_capabilities_json(pid: number): string;
  get_processes_with_capabilities_json(): string;
  get_endpoint_list_json(): string;
  get_ipc_traffic_json(count: number): string;
  get_system_metrics_json(): string;
  get_axiom_stats_json(): string;
  get_commitlog_json(count: number): string;
  get_syslog_json(count: number): string;
  /** Revoke/delete a capability from any process (supervisor privilege) */
  revoke_capability(pid: bigint, slot: number): boolean;

  // ==========================================================================
  // Generic Service IPC API (Thin Boundary Layer)
  // ==========================================================================

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
   * 1. Finds the service by name (e.g., "identity" -> "identity_service")
   * 2. Delivers the message to the service's input endpoint (slot 1)
   * 3. Returns a request_id for tracking the response
   * 
   * @param serviceName - Service name without "_service" suffix (e.g., "identity", "vfs")
   * @param tag - Request message tag (e.g., 0x7054 for MSG_GENERATE_NEURAL_KEY)
   * @param data - JSON request data as a string
   * @returns On success: request_id string (e.g., "00007055"); On error: "error:..." string
   */
  send_service_ipc(serviceName: string, tag: number, data: string): string;
}

// =============================================================================
// DesktopController Type - Desktop/Window management (from zero-desktop)
// =============================================================================

export interface DesktopController {
  // Initialization
  init(width: number, height: number): void;
  resize(width: number, height: number): void;

  // Viewport
  pan(dx: number, dy: number): void;
  zoom_at(factor: number, anchor_x: number, anchor_y: number): void;
  get_viewport_json(): string;

  // Windows
  create_window(title: string, x: number, y: number, w: number, h: number, app_id: string, content_interactive: boolean): bigint;
  close_window(id: bigint): void;
  get_window_process_id(id: bigint): bigint | undefined;
  /** Link a window to its associated process */
  set_window_process_id(window_id: bigint, process_id: bigint): void;
  focus_window(id: bigint): void;
  move_window(id: bigint, x: number, y: number): void;
  resize_window(id: bigint, w: number, h: number): void;
  minimize_window(id: bigint): void;
  maximize_window(id: bigint): void;
  restore_window(id: bigint): void;
  get_focused_window(): bigint | undefined;
  pan_to_window(id: bigint): void;
  get_windows_json(): string;
  get_window_screen_rects_json(): string;
  launch_app(app_id: string): bigint;

  // Desktops (workspaces)
  create_desktop(name: string): number;
  switch_desktop(index: number): void;
  get_active_desktop(): number;
  get_visual_active_desktop(): number;
  get_desktops_json(): string;
  get_desktop_dimensions_json(): string;

  // Void mode
  get_view_mode(): string;
  is_in_void(): boolean;
  enter_void(): void;
  exit_void(desktop_index: number): void;

  // Animation state
  is_animating(): boolean;
  is_animating_viewport(): boolean;
  is_transitioning(): boolean;
  tick_transition(): boolean;

  // Input handling
  pointer_down(x: number, y: number, button: number, ctrl: boolean, shift: boolean): string;
  pointer_move(x: number, y: number): string;
  pointer_up(): string;
  wheel(dx: number, dy: number, x: number, y: number, ctrl: boolean): string;
  start_window_resize(window_id: bigint, direction: string, x: number, y: number): void;
  start_window_drag(window_id: bigint, x: number, y: number): void;

  // Unified frame tick
  tick_frame(): string;
}

// =============================================================================
// Contexts and Hooks
// =============================================================================

// Supervisor context (kernel operations)
export const SupervisorContext = createContext<Supervisor | null>(null);

// DesktopController context (desktop operations)
export const DesktopControllerContext = createContext<DesktopController | null>(null);

// Hook to access the Supervisor
export function useSupervisor(): Supervisor | null {
  return useContext(SupervisorContext);
}

// Hook to access the DesktopController
export function useDesktopController(): DesktopController | null {
  return useContext(DesktopControllerContext);
}

// Provider components
export const SupervisorProvider = SupervisorContext.Provider;
export const DesktopControllerProvider = DesktopControllerContext.Provider;
