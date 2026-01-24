import { useCallback } from 'react';
import { useDesktopController, useSupervisor } from './useSupervisor';
import { 
  useWindowStore, 
  selectWindows, 
  selectFocusedId,
  type WindowInfo,
  type WindowData,
  type WindowType,
} from '../../stores';

// =============================================================================
// Re-export Types from Store
// =============================================================================

export type { WindowInfo, WindowData, WindowType };

// =============================================================================
// DEPRECATED POLLING HOOKS
// =============================================================================
// These hooks now use Zustand stores instead of polling.
// The stores are updated by the unified render loop in Desktop.tsx.
// =============================================================================

/**
 * Hook to get all windows data.
 * 
 * @deprecated Use `useWindowStore(selectWindows)` directly for better performance.
 * This hook is kept for backward compatibility.
 */
export function useWindows(): WindowInfo[] {
  return useWindowStore(selectWindows);
}

/**
 * Hook to get focused window ID.
 * 
 * @deprecated Use `useWindowStore(selectFocusedId)` directly for better performance.
 * This hook is kept for backward compatibility.
 */
export function useFocusedWindow(): number | null {
  return useWindowStore(selectFocusedId);
}

// Module-level cache for terminal WASM binary
let terminalWasmCache: Uint8Array | null = null;

// Hook for window actions
export function useWindowActions() {
  const desktop = useDesktopController();
  const supervisor = useSupervisor();

  const createWindow = useCallback(
    (title: string, x: number, y: number, w: number, h: number, appId: string, contentInteractive: boolean = false) => {
      if (!desktop) return null;
      return Number(desktop.create_window(title, x, y, w, h, appId, contentInteractive));
    },
    [desktop]
  );

  const closeWindow = useCallback(
    (id: number) => {
      if (!desktop) return;
      
      // Get the process ID associated with this window (if any)
      // Note: Returns BigInt (u64) from Rust, or undefined
      const processId = desktop.get_window_process_id(BigInt(id));
      
      // Close the window
      desktop.close_window(BigInt(id));
      
      // Kill the associated process if it exists
      // Note: kill_process takes u64 (BigInt), processId is already BigInt from Rust
      if (processId !== undefined && supervisor) {
        console.log(`[useWindows] Killing process ${processId} for window ${id}`);
        supervisor.kill_process(processId);
      }
    },
    [desktop, supervisor]
  );

  const focusWindow = useCallback(
    (id: number) => {
      desktop?.focus_window(BigInt(id));
    },
    [desktop]
  );

  const panToWindow = useCallback(
    (id: number) => {
      desktop?.pan_to_window(BigInt(id));
    },
    [desktop]
  );

  const minimizeWindow = useCallback(
    (id: number) => {
      desktop?.minimize_window(BigInt(id));
    },
    [desktop]
  );

  const maximizeWindow = useCallback(
    (id: number) => {
      desktop?.maximize_window(BigInt(id));
    },
    [desktop]
  );

  const restoreWindow = useCallback(
    (id: number) => {
      desktop?.restore_window(BigInt(id));
    },
    [desktop]
  );

  const launchApp = useCallback(
    (appId: string) => {
      if (!desktop) return null;
      return Number(desktop.launch_app(appId));
    },
    [desktop]
  );

  // Launch terminal with its own isolated process
  // This spawns the process first, then creates the window and links them
  const launchTerminal = useCallback(async () => {
    if (!supervisor || !desktop) return null;

    try {
      // 1. Fetch terminal WASM binary (use cache if available)
      if (!terminalWasmCache) {
        console.log('[useWindows] Fetching terminal.wasm...');
        const response = await fetch('/processes/terminal.wasm');
        if (!response.ok) {
          console.error('[useWindows] Failed to fetch terminal.wasm:', response.status);
          return null;
        }
        terminalWasmCache = new Uint8Array(await response.arrayBuffer());
        console.log('[useWindows] Loaded terminal.wasm:', terminalWasmCache.length, 'bytes');
      }

      // 2. Spawn the terminal process FIRST (before creating window)
      const pid = supervisor.complete_spawn('terminal', terminalWasmCache);
      console.log('[useWindows] Spawned terminal process with PID:', pid);

      // 3. Create the window
      const windowId = desktop.launch_app('terminal');
      console.log('[useWindows] Created terminal window:', windowId);

      // 4. Link window to process (this also updates the title to show PID)
      desktop.set_window_process_id(windowId, pid);
      console.log('[useWindows] Linked window', windowId, 'to process', pid);

      return Number(windowId);
    } catch (e) {
      console.error('[useWindows] Error launching terminal:', e);
      return null;
    }
  }, [supervisor, desktop]);

  // Launch app or focus existing window if already open
  // Optionally restores minimized windows
  const launchOrFocusApp = useCallback(
    (appId: string, restoreMinimized: boolean = true): number | null => {
      if (!desktop) return null;

      try {
        // Get all windows and find one with matching appId
        const windowsJson = desktop.get_windows_json();
        const windows = JSON.parse(windowsJson) as Array<{
          id: number;
          appId: string;
          state: string;
        }>;

        const existingWindow = windows.find((w) => w.appId === appId);

        if (existingWindow) {
          // Window exists - focus it and optionally restore if minimized
          if (existingWindow.state === 'minimized' && restoreMinimized) {
            desktop.restore_window(BigInt(existingWindow.id));
          }
          desktop.focus_window(BigInt(existingWindow.id));
          desktop.pan_to_window(BigInt(existingWindow.id));
          return existingWindow.id;
        } else {
          // No existing window - launch new one
          return Number(desktop.launch_app(appId));
        }
      } catch (e) {
        console.error('[useWindows] Error in launchOrFocusApp:', e);
        // Fall back to launching new window
        return Number(desktop.launch_app(appId));
      }
    },
    [desktop]
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
    launchTerminal,
    launchOrFocusApp,
  };
}
