/**
 * Window Store - Centralized state for window management.
 * 
 * State is synchronized from Rust's tick_frame() via the render loop.
 * This replaces the polling-based useWindows and useFocusedWindow hooks.
 */

import { create } from 'zustand';
import { subscribeWithSelector } from 'zustand/middleware';
import type { WindowInfo } from './types';

// =============================================================================
// Store Types
// =============================================================================

interface WindowStoreState {
  // State
  windows: WindowInfo[];
  focusedId: number | null;
  animating: boolean;
  transitioning: boolean;
  
  // Actions (called from render loop sync)
  setWindows: (windows: WindowInfo[]) => void;
  setFocusedId: (id: number | null) => void;
  setAnimating: (animating: boolean) => void;
  setTransitioning: (transitioning: boolean) => void;
  
  // Sync all from frame data (single atomic update)
  syncFromFrame: (frame: {
    windows: WindowInfo[];
    animating: boolean;
    transitioning: boolean;
  }) => void;
}

// =============================================================================
// Store Creation
// =============================================================================

export const useWindowStore = create<WindowStoreState>()(
  subscribeWithSelector((set) => ({
    // Initial state
    windows: [],
    focusedId: null,
    animating: false,
    transitioning: false,
    
    // Individual setters
    setWindows: (windows) => set({ windows }),
    setFocusedId: (focusedId) => set({ focusedId }),
    setAnimating: (animating) => set({ animating }),
    setTransitioning: (transitioning) => set({ transitioning }),
    
    // Atomic sync from render loop
    syncFromFrame: (frame) => set({
      windows: frame.windows,
      focusedId: frame.windows.find(w => w.focused)?.id ?? null,
      animating: frame.animating,
      transitioning: frame.transitioning,
    }),
  }))
);

// =============================================================================
// Selectors for Fine-Grained Subscriptions
// =============================================================================

/** Select all windows */
export const selectWindows = (state: WindowStoreState) => state.windows;

/** Select focused window ID */
export const selectFocusedId = (state: WindowStoreState) => state.focusedId;

/** Select the focused window object */
export const selectFocusedWindow = (state: WindowStoreState) => 
  state.windows.find(w => w.id === state.focusedId) ?? null;

/** Select a window by ID (returns selector function) */
export const selectWindowById = (id: number) => (state: WindowStoreState) =>
  state.windows.find(w => w.id === id);

/** Select visible (non-minimized) windows */
export const selectVisibleWindows = (state: WindowStoreState) =>
  state.windows.filter(w => w.state !== 'minimized');

/** Select whether any animation is in progress */
export const selectAnimating = (state: WindowStoreState) => state.animating;

/** Select whether a layer transition is in progress */
export const selectTransitioning = (state: WindowStoreState) => state.transitioning;

/** Select windows sorted by z-order (topmost first) */
export const selectWindowsByZOrder = (state: WindowStoreState) =>
  [...state.windows].sort((a, b) => b.zOrder - a.zOrder);

/** Select window count */
export const selectWindowCount = (state: WindowStoreState) => state.windows.length;
