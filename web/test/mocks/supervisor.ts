import { vi } from 'vitest';
import type { Supervisor } from '../../desktop/hooks/useSupervisor';

export interface MockProcessData {
  pid: number;
  name: string;
  state: 'running' | 'blocked' | 'zombie';
  memory: number;
}

export interface MockSupervisorState {
  booted: boolean;
  processes: MockProcessData[];
  uptime: number;
  totalMemory: number;
  endpointCount: number;
  pendingMessages: number;
  totalIpcMessages: number;
}

const defaultState: MockSupervisorState = {
  booted: false,
  processes: [],
  uptime: 0,
  totalMemory: 0,
  endpointCount: 0,
  pendingMessages: 0,
  totalIpcMessages: 0,
};

export function createMockSupervisor(
  initialState: Partial<MockSupervisorState> = {}
): Supervisor & { _state: MockSupervisorState; _updateState: (updates: Partial<MockSupervisorState>) => void } {
  const state: MockSupervisorState = { ...defaultState, ...initialState };

  const updateState = (updates: Partial<MockSupervisorState>) => {
    Object.assign(state, updates);
  };

  return {
    _state: state,
    _updateState: updateState,

    // Kernel API
    boot: vi.fn(() => {
      state.booted = true;
    }),
    spawn_init: vi.fn(() => {
      state.processes.push({
        pid: 1,
        name: 'init',
        state: 'running',
        memory: 1024,
      });
    }),
    send_input: vi.fn((input: string) => {}),
    set_console_callback: vi.fn((callback: (text: string) => void) => {}),
    set_spawn_callback: vi.fn((callback: (procType: string, name: string) => void) => {}),
    complete_spawn: vi.fn((name: string, binary: Uint8Array) => {
      const pid = state.processes.length + 1;
      state.processes.push({
        pid,
        name,
        state: 'running',
        memory: binary.length,
      });
      return BigInt(pid);
    }),
    init_axiom_storage: vi.fn(async () => true),
    sync_axiom_log: vi.fn(async () => 0),
    poll_syscalls: vi.fn(() => 0),
    process_worker_messages: vi.fn(() => 0),
    kill_process: vi.fn((pid: number) => {
      const process = state.processes.find(p => p.pid === pid);
      if (process) {
        process.state = 'zombie';
      }
    }),
    kill_all_processes: vi.fn(() => {
      state.processes.forEach(p => p.state = 'zombie');
    }),
    get_uptime_ms: vi.fn(() => state.uptime),
    get_process_count: vi.fn(() => state.processes.filter(p => p.state !== 'zombie').length),
    get_total_memory: vi.fn(() => state.totalMemory),
    get_endpoint_count: vi.fn(() => state.endpointCount),
    get_pending_messages: vi.fn(() => state.pendingMessages),
    get_total_ipc_messages: vi.fn(() => state.totalIpcMessages),
    get_process_list_json: vi.fn(() => JSON.stringify(state.processes)),
    get_process_capabilities_json: vi.fn((_pid: number) => JSON.stringify([])),
    get_processes_with_capabilities_json: vi.fn(() => JSON.stringify(
      state.processes.map(p => ({
        pid: p.pid,
        name: p.name,
        state: p.state === 'running' ? 'Running' : p.state === 'blocked' ? 'Blocked' : 'Zombie',
        capabilities: [],
      }))
    )),
    get_endpoint_list_json: vi.fn(() => JSON.stringify([])),
    get_ipc_traffic_json: vi.fn((count: number) => JSON.stringify([])),
    get_system_metrics_json: vi.fn(() => JSON.stringify({
      uptime: state.uptime,
      processCount: state.processes.length,
      totalMemory: state.totalMemory,
      endpointCount: state.endpointCount,
      pendingMessages: state.pendingMessages,
      totalIpcMessages: state.totalIpcMessages,
    })),
    get_axiom_stats_json: vi.fn(() => JSON.stringify({
      commitCount: 0,
      syslogCount: 0,
    })),
    get_commitlog_json: vi.fn((count: number) => JSON.stringify([])),
    get_syslog_json: vi.fn((count: number) => JSON.stringify([])),
    
    // Process isolation APIs
    send_input_to_process: vi.fn((pid: number, input: string) => {}),
    register_console_callback: vi.fn((pid: number, callback: (text: string) => void) => {}),
    unregister_console_callback: vi.fn((pid: number) => {}),
    
    // Capability API
    revoke_capability: vi.fn((pid: bigint, slot: number) => true),
    
    // Generic Service IPC API (Thin Boundary Layer)
    set_ipc_response_callback: vi.fn((callback: (requestId: string, data: string) => void) => {}),
    send_service_ipc: vi.fn((serviceName: string, tag: number, data: string) => {
      // Return a mock request_id based on tag+1 convention
      const responseTag = tag + 1;
      return responseTag.toString(16).padStart(8, '0');
    }),
  };
}

// Helper to create supervisor with pre-configured processes
export function createMockSupervisorWithProcesses(
  processes: Partial<MockProcessData>[]
): ReturnType<typeof createMockSupervisor> {
  const fullProcesses = processes.map((p, i) => ({
    pid: p.pid ?? i + 1,
    name: p.name ?? `process-${i + 1}`,
    state: p.state ?? 'running',
    memory: p.memory ?? 1024,
  })) as MockProcessData[];

  return createMockSupervisor({
    booted: true,
    processes: fullProcesses,
  });
}
