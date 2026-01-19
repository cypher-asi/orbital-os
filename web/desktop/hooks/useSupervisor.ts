import { createContext, useContext } from 'react';

// Type definition for the WASM Supervisor
// This matches the wasm-bindgen exports from orbital-web
export interface Supervisor {
  // Desktop API
  init_desktop(width: number, height: number): void;
  resize_desktop(width: number, height: number): void;
  desktop_pan(dx: number, dy: number): void;
  desktop_zoom(factor: number, anchor_x: number, anchor_y: number): void;
  get_window_screen_rects_json(): string;
  create_window(title: string, x: number, y: number, w: number, h: number, app_id: string): bigint;
  close_window(id: bigint): void;
  focus_window(id: bigint): void;
  pan_to_window(id: bigint): void;
  move_window(id: bigint, x: number, y: number): void;
  resize_window(id: bigint, w: number, h: number): void;
  minimize_window(id: bigint): void;
  maximize_window(id: bigint): void;
  restore_window(id: bigint): void;
  get_focused_window(): bigint | undefined;
  get_windows_json(): string;
  create_workspace(name: string): number;
  switch_workspace(index: number): void;
  get_workspaces_json(): string;
  get_workspace_dimensions_json(): string;
  get_active_workspace(): number;
  get_active_workspace_background(): string;
  is_workspace_transitioning(): boolean;
  get_view_mode(): string;
  is_in_void(): boolean;
  enter_void(): void;
  exit_void(workspace_index: number): void;
  set_workspace_background(index: number, background_id: string): boolean;
  set_active_workspace_background(background_id: string): boolean;
  import_workspace_settings(json: string): boolean;
  export_workspace_settings(): string;
  start_window_resize(window_id: bigint, direction: string, x: number, y: number): void;
  start_window_drag(window_id: bigint, x: number, y: number): void;
  desktop_pointer_down(x: number, y: number, button: number, ctrl: boolean, shift: boolean): string;
  desktop_pointer_move(x: number, y: number): string;
  desktop_pointer_up(): string;
  desktop_wheel(dx: number, dy: number, x: number, y: number, ctrl: boolean): string;
  launch_app(app_id: string): bigint;
  get_viewport_json(): string;
  tick_desktop_transition(): boolean;
  get_visual_active_workspace(): number;
  is_animating(): boolean;
  
  // Unified frame tick - updates all animation state and returns complete frame data
  // This is the preferred method for the render loop as it guarantees consistent state
  tick_frame(): string;

  // Existing Supervisor API
  boot(): void;
  spawn_init(): void;
  send_input(input: string): void;
  set_console_callback(callback: (text: string) => void): void;
  set_spawn_callback(callback: (procType: string, name: string) => void): void;
  complete_spawn(name: string, binary: Uint8Array): bigint;
  init_axiom_storage(): Promise<boolean>;
  sync_axiom_log(): Promise<number>;
  poll_syscalls(): number;
  process_worker_messages(): number;
  deliver_pending_messages(): number;
  get_uptime_ms(): number;
  get_process_count(): number;
  get_total_memory(): number;
  get_endpoint_count(): number;
  get_pending_messages(): number;
  get_total_ipc_messages(): number;
  get_process_list_json(): string;
  get_endpoint_list_json(): string;
  get_ipc_traffic_json(count: number): string;
  get_system_metrics_json(): string;
  get_axiom_stats_json(): string;
  get_commitlog_json(count: number): string;
  get_syslog_json(count: number): string;
}

// Context for the Supervisor instance
export const SupervisorContext = createContext<Supervisor | null>(null);

// Hook to access the Supervisor
export function useSupervisor(): Supervisor | null {
  return useContext(SupervisorContext);
}

// Provider component
export const SupervisorProvider = SupervisorContext.Provider;
