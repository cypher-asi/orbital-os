/**
 * Desktop Store - Centralized state for desktop/viewport management.
 * 
 * State is synchronized from Rust's tick_frame() via the render loop.
 * This replaces the polling-based useDesktops, useViewMode, useIsInVoid,
 * and useLayerOpacities hooks.
 */

import { create } from 'zustand';
import { subscribeWithSelector } from 'zustand/middleware';
import type { DesktopInfo, ViewMode, ViewportState, LayerOpacities, WorkspaceInfo } from './types';

// =============================================================================
// Store Types
// =============================================================================

interface DesktopStoreState {
  // State
  desktops: DesktopInfo[];
  activeIndex: number;
  viewMode: ViewMode;
  inVoid: boolean;
  viewport: ViewportState;
  showVoid: boolean;
  workspaceInfo: WorkspaceInfo | null;
  
  // Actions
  setDesktops: (desktops: DesktopInfo[]) => void;
  setActiveIndex: (index: number) => void;
  setViewMode: (mode: ViewMode) => void;
  setInVoid: (inVoid: boolean) => void;
  setViewport: (viewport: ViewportState) => void;
  
  // Atomic sync from render loop
  syncFromFrame: (frame: {
    viewMode: ViewMode;
    showVoid: boolean;
    viewport: ViewportState;
    workspaceInfo: WorkspaceInfo;
  }) => void;
}

// =============================================================================
// Store Creation
// =============================================================================

export const useDesktopStore = create<DesktopStoreState>()(
  subscribeWithSelector((set) => ({
    desktops: [],
    activeIndex: 0,
    viewMode: 'desktop',
    inVoid: false,
    viewport: { center: { x: 0, y: 0 }, zoom: 1 },
    showVoid: false,
    workspaceInfo: null,
    
    setDesktops: (desktops) => set({ desktops }),
    setActiveIndex: (activeIndex) => set({ activeIndex }),
    setViewMode: (viewMode) => set({ viewMode, inVoid: viewMode === 'void' }),
    setInVoid: (inVoid) => set({ inVoid }),
    setViewport: (viewport) => set({ viewport }),
    
    syncFromFrame: (frame) => set({
      // Map legacy 'workspace' to 'desktop'
      viewMode: frame.viewMode === 'workspace' ? 'desktop' : frame.viewMode,
      inVoid: frame.viewMode === 'void',
      showVoid: frame.showVoid,
      viewport: frame.viewport,
      activeIndex: frame.workspaceInfo.active,
      workspaceInfo: frame.workspaceInfo,
    }),
  }))
);

// =============================================================================
// Selectors for Fine-Grained Subscriptions
// =============================================================================

/** Select all desktops */
export const selectDesktops = (state: DesktopStoreState) => state.desktops;

/** Select active desktop by index */
export const selectActiveDesktop = (state: DesktopStoreState) => 
  state.desktops[state.activeIndex];

/** Select active desktop index */
export const selectActiveIndex = (state: DesktopStoreState) => state.activeIndex;

/** Select current view mode */
export const selectViewMode = (state: DesktopStoreState) => state.viewMode;

/** Select whether in void mode */
export const selectInVoid = (state: DesktopStoreState) => state.inVoid;

/** Select viewport state */
export const selectViewport = (state: DesktopStoreState) => state.viewport;

/** Select whether void layer should be visible */
export const selectShowVoid = (state: DesktopStoreState) => state.showVoid;

/** Select workspace info */
export const selectWorkspaceInfo = (state: DesktopStoreState) => state.workspaceInfo;

/** Select desktop count */
export const selectDesktopCount = (state: DesktopStoreState) => state.desktops.length;

/**
 * Select layer opacities for crossfade transitions.
 * Computes opacities based on viewMode and showVoid state.
 */
export const selectLayerOpacities = (state: DesktopStoreState): LayerOpacities => {
  if (state.showVoid && state.viewMode !== 'void') {
    // During transition, both layers visible
    return { desktop: 0.5, void: 0.5 };
  } else if (state.viewMode === 'void') {
    return { desktop: 0.0, void: 1.0 };
  } else {
    return { desktop: 1.0, void: 0.0 };
  }
};
