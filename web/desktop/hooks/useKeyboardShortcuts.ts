import { useEffect } from 'react';
import type { DesktopController, Supervisor } from './useSupervisor';

interface UseKeyboardShortcutsOptions {
  initialized: boolean;
  desktop: DesktopController;
  supervisor?: Supervisor | null;
  launchTerminal: () => void;
}

/**
 * Hook for managing desktop keyboard shortcuts.
 * 
 * Supported shortcuts:
 * - T: Create new terminal with its own process
 * - C: Close focused window
 * - Ctrl+` or F3: Toggle void view
 * - Arrow keys: Cycle between windows
 * - Ctrl+Arrow: Switch between desktops
 */
export function useKeyboardShortcuts({
  initialized,
  desktop,
  supervisor,
  launchTerminal,
}: UseKeyboardShortcutsOptions) {
  useEffect(() => {
    if (!initialized) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      // Ignore if focus is in an input field
      const target = e.target as HTMLElement;
      const tagName = target.tagName.toLowerCase();
      if (tagName === 'input' || tagName === 'textarea' || target.isContentEditable) {
        return;
      }

      // T key: Create new terminal with its own process
      if (e.key === 't' || e.key === 'T') {
        e.preventDefault();
        launchTerminal();
        return;
      }

      // C key: Close focused window
      if (e.key === 'c' || e.key === 'C') {
        e.preventDefault();
        handleCloseWindow(desktop, supervisor);
        return;
      }

      // Ctrl+` (backtick) or F3: Toggle void view
      if ((e.ctrlKey && e.key === '`') || e.key === 'F3') {
        e.preventDefault();
        handleToggleVoid(desktop);
        return;
      }

      // Arrow keys: Cycle between windows (without Ctrl) or desktops (with Ctrl)
      if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
        e.preventDefault();
        handleArrowNavigation(e, desktop);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [initialized, desktop, supervisor, launchTerminal]);
}

/**
 * Handle closing the focused window and its associated process.
 */
function handleCloseWindow(desktop: DesktopController, supervisor?: Supervisor | null) {
  try {
    const focusedId = desktop.get_focused_window();
    if (focusedId === undefined) return;

    // Get the process ID associated with this window (if any)
    const processId = desktop.get_window_process_id(BigInt(focusedId));

    // Close the window
    desktop.close_window(BigInt(focusedId));

    // Kill the associated process if it exists
    if (processId !== undefined && supervisor) {
      supervisor.kill_process(processId);
    }
  } catch {
    // Ignore errors during window close
  }
}

/**
 * Handle toggling void view (overview of all desktops).
 */
function handleToggleVoid(desktop: DesktopController) {
  try {
    const viewMode = desktop.get_view_mode();
    // Accept both 'desktop' and legacy 'workspace' for entering void
    if (viewMode === 'desktop' || viewMode === 'workspace') {
      desktop.enter_void();
    } else if (viewMode === 'void') {
      desktop.exit_void(desktop.get_active_desktop());
    }
  } catch {
    // Ignore errors during view mode toggle
  }
}

/**
 * Handle arrow key navigation for windows and desktops.
 */
function handleArrowNavigation(e: KeyboardEvent, desktop: DesktopController) {
  const isLeft = e.key === 'ArrowLeft';

  if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey) {
    // Ctrl+Arrow: Switch desktops
    handleDesktopSwitch(desktop, isLeft);
  } else if (!e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey) {
    // Arrow only: Cycle between windows on current desktop
    handleWindowCycle(desktop, isLeft);
  }
}

/**
 * Switch to the previous or next desktop.
 */
function handleDesktopSwitch(desktop: DesktopController, isLeft: boolean) {
  try {
    const desktops = JSON.parse(desktop.get_desktops_json()) as Array<{ id: number }>;
    const count = desktops.length;
    if (count <= 1) return;

    const current = desktop.get_active_desktop();
    const next = isLeft
      ? (current > 0 ? current - 1 : count - 1)
      : (current < count - 1 ? current + 1 : 0);

    // If in void, exit to target desktop; otherwise switch
    if (desktop.get_view_mode() === 'void') {
      desktop.exit_void(next);
    } else {
      desktop.switch_desktop(next);
    }
  } catch {
    // Ignore errors during desktop switch
  }
}

/**
 * Cycle focus to the previous or next window.
 */
function handleWindowCycle(desktop: DesktopController, isLeft: boolean) {
  try {
    const windowsJson = desktop.get_windows_json();
    const windows = JSON.parse(windowsJson) as Array<{ id: number; state: string; zOrder: number }>;

    // Filter to only visible windows (not minimized)
    // Windows are already sorted by ID (creation order) from get_windows_json
    // This matches the order shown in the taskbar (left to right)
    const visibleWindows = windows.filter(w => w.state !== 'minimized');

    if (visibleWindows.length === 0) {
      return;
    }

    const focusedId = desktop.get_focused_window();
    // Convert BigInt to number for comparison
    const focusedIdNum = focusedId !== undefined ? Number(focusedId) : undefined;
    const currentIndex = visibleWindows.findIndex(w => w.id === focusedIdNum);

    let nextIndex;
    if (currentIndex === -1) {
      // No window focused, focus the first window (leftmost in taskbar)
      nextIndex = 0;
    } else if (isLeft) {
      // Previous window (left in taskbar = lower ID)
      nextIndex = currentIndex > 0 ? currentIndex - 1 : visibleWindows.length - 1;
    } else {
      // Next window (right in taskbar = higher ID)
      nextIndex = currentIndex < visibleWindows.length - 1 ? currentIndex + 1 : 0;
    }

    const nextWindow = visibleWindows[nextIndex];

    // Focus and pan to the next window
    desktop.focus_window(BigInt(nextWindow.id));
    desktop.pan_to_window(BigInt(nextWindow.id));
  } catch (err) {
    console.error('[Desktop] Error during window cycling:', err);
  }
}
