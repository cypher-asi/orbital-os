/**
 * Render Loop Sync - Syncs Zustand stores from tick_frame() data.
 * 
 * Called once per frame from the unified render loop in Desktop.tsx.
 * Only updates stores when data actually changes to minimize re-renders.
 * 
 * This is the primary sync mechanism for animation-critical state:
 * - Window positions, states, and focus
 * - Viewport position and zoom
 * - View mode and transition state
 */

import { useWindowStore } from '../../stores/windowStore';
import { useDesktopStore } from '../../stores/desktopStore';
import type { FrameData, DesktopInfo } from '../../stores/types';

// Track previous state to avoid unnecessary updates
let prevWindowCount = 0;
let prevFocusedId: number | null = null;
let prevAnimating = false;
let prevTransitioning = false;
let prevViewMode = 'desktop';
let prevShowVoid = false;
let prevActiveIndex = 0;
let prevDesktopCount = 0;

/**
 * Sync Zustand stores from tick_frame() data.
 * 
 * This function is called from the render loop and performs
 * shallow comparisons to avoid unnecessary store updates.
 * 
 * @param frame - Complete frame data from Rust's tick_frame()
 */
export function syncStoresFromFrame(frame: FrameData): void {
  const windowStore = useWindowStore.getState();
  const desktopStore = useDesktopStore.getState();
  
  // =========================================================================
  // Sync Window Store
  // =========================================================================
  
  // Detect changes that require window store update
  const focusedId = frame.windows.find(w => w.focused)?.id ?? null;
  const windowsChanged = 
    frame.windows.length !== prevWindowCount ||
    focusedId !== prevFocusedId ||
    frame.animating !== prevAnimating ||
    frame.transitioning !== prevTransitioning;
  
  // For more detailed comparison, check if any window ID changed
  // This catches add/remove even when count stays same (rare edge case)
  let windowListChanged = windowsChanged;
  if (!windowListChanged && frame.windows.length > 0) {
    const currentIds = windowStore.windows.map(w => w.id);
    const newIds = frame.windows.map(w => w.id);
    windowListChanged = !arraysEqual(currentIds, newIds);
  }
  
  if (windowListChanged || 
      frame.animating !== prevAnimating ||
      frame.transitioning !== prevTransitioning) {
    windowStore.syncFromFrame({
      windows: frame.windows,
      animating: frame.animating,
      transitioning: frame.transitioning,
    });
    
    // Update tracking
    prevWindowCount = frame.windows.length;
    prevFocusedId = focusedId;
    prevAnimating = frame.animating;
    prevTransitioning = frame.transitioning;
  }
  
  // =========================================================================
  // Sync Desktop Store
  // =========================================================================
  
  const desktopCountChanged = frame.workspaceInfo.count !== prevDesktopCount;
  const activeIndexChanged = frame.workspaceInfo.active !== prevActiveIndex;
  const desktopChanged = 
    frame.viewMode !== prevViewMode ||
    frame.showVoid !== prevShowVoid ||
    activeIndexChanged ||
    desktopCountChanged;
  
  if (desktopChanged) {
    desktopStore.syncFromFrame({
      viewMode: frame.viewMode,
      showVoid: frame.showVoid,
      viewport: frame.viewport,
      workspaceInfo: frame.workspaceInfo,
    });
    
    // Update tracking
    prevViewMode = frame.viewMode;
    prevShowVoid = frame.showVoid;
    prevActiveIndex = frame.workspaceInfo.active;
  }
  
  // Sync desktops array when count changes
  // Generate DesktopInfo[] from workspaceInfo
  if (desktopCountChanged) {
    const desktops: DesktopInfo[] = [];
    for (let i = 0; i < frame.workspaceInfo.count; i++) {
      desktops.push({
        id: i,
        name: `Desktop ${i + 1}`,
        active: i === frame.workspaceInfo.active,
        windowCount: 0, // Not tracked per-desktop currently
      });
    }
    desktopStore.setDesktops(desktops);
    prevDesktopCount = frame.workspaceInfo.count;
  } else if (activeIndexChanged) {
    // Update active flag when active desktop changes (but count didn't)
    const currentDesktops = desktopStore.desktops;
    if (currentDesktops.length > 0) {
      const updatedDesktops = currentDesktops.map((d, i) => ({
        ...d,
        active: i === frame.workspaceInfo.active,
      }));
      desktopStore.setDesktops(updatedDesktops);
    }
  }
}

/**
 * Reset sync state. Call when desktop is unmounted/remounted.
 */
export function resetSyncState(): void {
  prevWindowCount = 0;
  prevFocusedId = null;
  prevAnimating = false;
  prevTransitioning = false;
  prevViewMode = 'desktop';
  prevShowVoid = false;
  prevActiveIndex = 0;
  prevDesktopCount = 0;
}

// Helper function for array comparison
function arraysEqual<T>(a: T[], b: T[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}
