import { vi } from 'vitest';
import type { DesktopController } from '../../desktop/hooks/useSupervisor';

export interface MockWindowData {
  id: number;
  title: string;
  appId: string;
  position: { x: number; y: number };
  size: { width: number; height: number };
  state: 'normal' | 'minimized' | 'maximized' | 'fullscreen';
  zOrder: number;
  focused: boolean;
}

export interface MockDesktopData {
  id: number;
  name: string;
  active: boolean;
  windowCount: number;
}

export interface MockDesktopControllerState {
  windows: MockWindowData[];
  desktops: MockDesktopData[];
  activeDesktop: number;
  focusedWindow: number | null;
  viewMode: 'desktop' | 'void' | 'transitioning';
  isAnimating: boolean;
  isTransitioning: boolean;
  viewport: {
    center: { x: number; y: number };
    zoom: number;
  };
}

const defaultState: MockDesktopControllerState = {
  windows: [],
  desktops: [{ id: 1, name: 'Main', active: true, windowCount: 0 }],
  activeDesktop: 0,
  focusedWindow: null,
  viewMode: 'desktop',
  isAnimating: false,
  isTransitioning: false,
  viewport: {
    center: { x: 0, y: 0 },
    zoom: 1.0,
  },
};

export function createMockDesktopController(
  initialState: Partial<MockDesktopControllerState> = {}
): DesktopController & { _state: MockDesktopControllerState; _updateState: (updates: Partial<MockDesktopControllerState>) => void } {
  const state: MockDesktopControllerState = { ...defaultState, ...initialState };

  const updateState = (updates: Partial<MockDesktopControllerState>) => {
    Object.assign(state, updates);
  };

  return {
    _state: state,
    _updateState: updateState,

    // Initialization
    init: vi.fn((width: number, height: number) => {
      state.viewport = { center: { x: width / 2, y: height / 2 }, zoom: 1.0 };
    }),
    resize: vi.fn((width: number, height: number) => {
      // Resize logic
    }),

    // Viewport
    pan: vi.fn((dx: number, dy: number) => {
      state.viewport.center.x += dx;
      state.viewport.center.y += dy;
    }),
    zoom_at: vi.fn((factor: number, anchor_x: number, anchor_y: number) => {
      state.viewport.zoom *= factor;
    }),
    get_viewport_json: vi.fn(() => JSON.stringify(state.viewport)),

    // Windows
    create_window: vi.fn((title: string, x: number, y: number, w: number, h: number, app_id: string, content_interactive: boolean) => {
      const id = state.windows.length + 1;
      const window: MockWindowData = {
        id,
        title,
        appId: app_id,
        position: { x, y },
        size: { width: w, height: h },
        state: 'normal',
        zOrder: state.windows.length,
        focused: true,
      };
      state.windows.push(window);
      state.focusedWindow = id;
      return BigInt(id);
    }),
    close_window: vi.fn((id: bigint) => {
      const idNum = Number(id);
      state.windows = state.windows.filter(w => w.id !== idNum);
      if (state.focusedWindow === idNum) {
        state.focusedWindow = state.windows.length > 0 ? state.windows[state.windows.length - 1].id : null;
      }
    }),
    get_window_process_id: vi.fn((id: bigint) => undefined),
    focus_window: vi.fn((id: bigint) => {
      const idNum = Number(id);
      state.focusedWindow = idNum;
      state.windows.forEach(w => w.focused = w.id === idNum);
    }),
    move_window: vi.fn((id: bigint, x: number, y: number) => {
      const window = state.windows.find(w => w.id === Number(id));
      if (window) {
        window.position = { x, y };
      }
    }),
    resize_window: vi.fn((id: bigint, w: number, h: number) => {
      const window = state.windows.find(win => win.id === Number(id));
      if (window) {
        window.size = { width: w, height: h };
      }
    }),
    minimize_window: vi.fn((id: bigint) => {
      const window = state.windows.find(w => w.id === Number(id));
      if (window) {
        window.state = 'minimized';
      }
    }),
    maximize_window: vi.fn((id: bigint) => {
      const window = state.windows.find(w => w.id === Number(id));
      if (window) {
        window.state = window.state === 'maximized' ? 'normal' : 'maximized';
      }
    }),
    restore_window: vi.fn((id: bigint) => {
      const window = state.windows.find(w => w.id === Number(id));
      if (window) {
        window.state = 'normal';
      }
    }),
    get_focused_window: vi.fn(() => state.focusedWindow !== null ? BigInt(state.focusedWindow) : undefined),
    pan_to_window: vi.fn((id: bigint) => {
      const window = state.windows.find(w => w.id === Number(id));
      if (window) {
        state.viewport.center = { ...window.position };
      }
    }),
    get_windows_json: vi.fn(() => JSON.stringify(state.windows)),
    get_window_screen_rects_json: vi.fn(() => JSON.stringify(
      state.windows.map((w, i) => ({
        id: w.id,
        title: w.title,
        appId: w.appId,
        state: w.state,
        focused: w.focused,
        zOrder: i,
        opacity: 1.0,
        screenRect: {
          x: w.position.x,
          y: w.position.y,
          width: w.size.width,
          height: w.size.height,
        },
      }))
    )),
    launch_app: vi.fn((app_id: string) => {
      const id = state.windows.length + 1;
      const window: MockWindowData = {
        id,
        title: app_id,
        appId: app_id,
        position: { x: 100 + id * 30, y: 100 + id * 30 },
        size: { width: 800, height: 600 },
        state: 'normal',
        zOrder: state.windows.length,
        focused: true,
      };
      state.windows.push(window);
      state.focusedWindow = id;
      return BigInt(id);
    }),

    // Desktops
    create_desktop: vi.fn((name: string) => {
      const id = state.desktops.length + 1;
      state.desktops.push({ id, name, active: false, windowCount: 0 });
      return id;
    }),
    switch_desktop: vi.fn((index: number) => {
      if (index >= 0 && index < state.desktops.length) {
        state.desktops.forEach((d, i) => d.active = i === index);
        state.activeDesktop = index;
      }
    }),
    get_active_desktop: vi.fn(() => state.activeDesktop),
    get_visual_active_desktop: vi.fn(() => state.activeDesktop),
    get_desktops_json: vi.fn(() => JSON.stringify(state.desktops)),
    get_desktop_dimensions_json: vi.fn(() => JSON.stringify({
      width: 1920,
      height: 1080,
      gap: 100,
    })),

    // Void mode
    get_view_mode: vi.fn(() => state.viewMode),
    is_in_void: vi.fn(() => state.viewMode === 'void'),
    enter_void: vi.fn(() => {
      state.viewMode = 'void';
    }),
    exit_void: vi.fn((desktop_index: number) => {
      state.viewMode = 'desktop';
      if (desktop_index >= 0 && desktop_index < state.desktops.length) {
        state.activeDesktop = desktop_index;
        state.desktops.forEach((d, i) => d.active = i === desktop_index);
      }
    }),

    // Animation state
    is_animating: vi.fn(() => state.isAnimating),
    is_animating_viewport: vi.fn(() => state.isTransitioning),
    is_transitioning: vi.fn(() => state.isTransitioning),
    tick_transition: vi.fn(() => state.isTransitioning),

    // Input handling
    pointer_down: vi.fn((x: number, y: number, button: number, ctrl: boolean, shift: boolean) => 
      JSON.stringify({ type: 'unhandled' })
    ),
    pointer_move: vi.fn((x: number, y: number) => 
      JSON.stringify({ type: 'unhandled' })
    ),
    pointer_up: vi.fn(() => 
      JSON.stringify({ type: 'unhandled' })
    ),
    wheel: vi.fn((dx: number, dy: number, x: number, y: number, ctrl: boolean) => 
      JSON.stringify({ type: 'unhandled' })
    ),
    start_window_resize: vi.fn((window_id: bigint, direction: string, x: number, y: number) => {}),
    start_window_drag: vi.fn((window_id: bigint, x: number, y: number) => {}),

    // Unified frame tick
    tick_frame: vi.fn(() => JSON.stringify({
      viewport: state.viewport,
      windows: state.windows.map((w, i) => ({
        id: w.id,
        title: w.title,
        appId: w.appId,
        state: w.state,
        focused: w.focused,
        zOrder: i,
        opacity: 1.0,
        contentInteractive: false,
        screenRect: {
          x: w.position.x,
          y: w.position.y,
          width: w.size.width,
          height: w.size.height,
        },
      })),
      animating: state.isAnimating,
      transitioning: state.isTransitioning,
      showVoid: state.viewMode === 'void',
      viewMode: state.viewMode,
      workspaceInfo: {
        count: state.desktops.length,
        active: state.activeDesktop,
        actualActive: state.activeDesktop,
        backgrounds: state.desktops.map(() => 'grain'),
      },
      workspaceDimensions: {
        width: 1920,
        height: 1080,
        gap: 100,
      },
    })),
  };
}

// Helper to create a controller with pre-configured windows
export function createMockDesktopControllerWithWindows(
  windows: Partial<MockWindowData>[]
): ReturnType<typeof createMockDesktopController> {
  const fullWindows = windows.map((w, i) => ({
    id: w.id ?? i + 1,
    title: w.title ?? `Window ${i + 1}`,
    appId: w.appId ?? 'test-app',
    position: w.position ?? { x: 100 + i * 50, y: 100 + i * 50 },
    size: w.size ?? { width: 800, height: 600 },
    state: w.state ?? 'normal',
    zOrder: w.zOrder ?? i,
    focused: w.focused ?? (i === windows.length - 1),
  })) as MockWindowData[];

  return createMockDesktopController({
    windows: fullWindows,
    focusedWindow: fullWindows.length > 0 ? fullWindows[fullWindows.length - 1].id : null,
  });
}
