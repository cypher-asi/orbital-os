import { useState, useEffect, useCallback } from 'react';
import { useSupervisor } from './useSupervisor';

// =============================================================================
// Window Types
// =============================================================================

// Window info with screen-space rect (for React positioning)
// Note: This is returned by Rust's tick_frame() in the unified render loop.
// Components that need screen rects should receive them as props from Desktop.tsx,
// NOT by polling independently (which causes animation jank).
export interface WindowInfo {
  id: number;
  title: string;
  appId: string;
  state: 'normal' | 'minimized' | 'maximized' | 'fullscreen';
  focused: boolean;
  zOrder: number;
  screenRect: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
}

// Basic window data (for taskbar, window lists - not animation-critical)
export interface WindowData {
  id: number;
  title: string;
  appId: string;
  position: { x: number; y: number };
  size: { width: number; height: number };
  state: 'normal' | 'minimized' | 'maximized' | 'fullscreen';
  zOrder: number;
  focused: boolean;
}

// =============================================================================
// DEPRECATED: useWindowScreenRects
// =============================================================================
// This hook has been removed. Window screen rects are now provided by the
// unified render loop in Desktop.tsx via Rust's tick_frame() method.
// This ensures windows and background are always in sync during animations.
// =============================================================================

// Hook to get all windows data
export function useWindows(): WindowData[] {
  const supervisor = useSupervisor();
  const [windows, setWindows] = useState<WindowData[]>([]);

  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      try {
        const json = supervisor.get_windows_json();
        const parsed = JSON.parse(json) as WindowData[];
        setWindows(parsed);
      } catch (e) {
        console.error('Failed to parse windows:', e);
      }
    };

    // Update periodically
    update();
    const interval = setInterval(update, 100);
    return () => clearInterval(interval);
  }, [supervisor]);

  return windows;
}

// Hook to get focused window ID
export function useFocusedWindow(): number | null {
  const supervisor = useSupervisor();
  const [focusedId, setFocusedId] = useState<number | null>(null);

  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      const id = supervisor.get_focused_window();
      setFocusedId(id !== undefined ? Number(id) : null);
    };

    update();
    const interval = setInterval(update, 100);
    return () => clearInterval(interval);
  }, [supervisor]);

  return focusedId;
}

// Hook for window actions
export function useWindowActions() {
  const supervisor = useSupervisor();

  const createWindow = useCallback(
    (title: string, x: number, y: number, w: number, h: number, appId: string) => {
      if (!supervisor) return null;
      return Number(supervisor.create_window(title, x, y, w, h, appId));
    },
    [supervisor]
  );

  const closeWindow = useCallback(
    (id: number) => {
      supervisor?.close_window(BigInt(id));
    },
    [supervisor]
  );

  const focusWindow = useCallback(
    (id: number) => {
      supervisor?.focus_window(BigInt(id));
    },
    [supervisor]
  );

  const panToWindow = useCallback(
    (id: number) => {
      supervisor?.pan_to_window(BigInt(id));
    },
    [supervisor]
  );

  const minimizeWindow = useCallback(
    (id: number) => {
      supervisor?.minimize_window(BigInt(id));
    },
    [supervisor]
  );

  const maximizeWindow = useCallback(
    (id: number) => {
      supervisor?.maximize_window(BigInt(id));
    },
    [supervisor]
  );

  const restoreWindow = useCallback(
    (id: number) => {
      supervisor?.restore_window(BigInt(id));
    },
    [supervisor]
  );

  const launchApp = useCallback(
    (appId: string) => {
      if (!supervisor) return null;
      return Number(supervisor.launch_app(appId));
    },
    [supervisor]
  );

  return {
    createWindow,
    closeWindow,
    focusWindow,
    panToWindow,
    minimizeWindow,
    maximizeWindow,
    restoreWindow,
    launchApp,
  };
}
