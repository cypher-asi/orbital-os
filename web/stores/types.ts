/**
 * Shared types used across Zustand stores.
 * 
 * These types are the single source of truth for state shape.
 * They consolidate types previously scattered across individual hooks.
 */

import type { Supervisor, DesktopController } from '../desktop/hooks/useSupervisor';

// =============================================================================
// WASM References
// =============================================================================

export interface WasmRefs {
  supervisor: Supervisor | null;
  desktop: DesktopController | null;
}

// =============================================================================
// Window Types
// =============================================================================

/** Window type determines the chrome/presentation style */
export type WindowType = 'standard' | 'widget';

/** Window state determines visibility and layout */
export type WindowState = 'normal' | 'minimized' | 'maximized' | 'fullscreen';

/**
 * Complete window information with screen-space rect.
 * This is the canonical type returned by Rust's tick_frame().
 */
export interface WindowInfo {
  id: number;
  title: string;
  appId: string;
  /** Associated process ID (for terminal windows) */
  processId?: number;
  state: WindowState;
  windowType: WindowType;
  focused: boolean;
  zOrder: number;
  /** Window opacity for crossfade transitions (0.0-1.0) */
  opacity: number;
  /** Whether content should receive pointer events */
  contentInteractive: boolean;
  screenRect: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
}

/**
 * Basic window data for non-animation-critical use cases.
 * Used by taskbar, window lists, etc.
 */
export interface WindowData {
  id: number;
  title: string;
  appId: string;
  position: { x: number; y: number };
  size: { width: number; height: number };
  state: WindowState;
  windowType: WindowType;
  zOrder: number;
  focused: boolean;
}

// =============================================================================
// Desktop Types
// =============================================================================

/**
 * Current viewing mode of the desktop.
 * - "desktop": Viewing a single desktop with infinite zoom/pan
 * - "void": Zoomed out to see all desktops
 * - "transitioning": Animation in progress between modes
 */
export type ViewMode = 'desktop' | 'workspace' | 'void' | 'transitioning';

/**
 * Desktop (workspace) information.
 */
export interface DesktopInfo {
  id: number;
  name: string;
  active: boolean;
  windowCount: number;
}

/**
 * Viewport state for camera position.
 */
export interface ViewportState {
  center: { x: number; y: number };
  zoom: number;
}

/**
 * Workspace information from frame data.
 */
export interface WorkspaceInfo {
  count: number;
  active: number;
  actualActive: number;
  backgrounds: string[];
}

// =============================================================================
// Frame Data (from Rust's tick_frame())
// =============================================================================

/**
 * Complete frame data from Rust's tick_frame() - single source of truth.
 * This is the atomic unit of state that comes from the render loop.
 */
export interface FrameData {
  viewport: ViewportState;
  windows: WindowInfo[];
  /** True during any activity (zoom/pan/drag) - for adaptive framerate */
  animating: boolean;
  /** True only during layer transitions (void enter/exit) - for crossfade */
  transitioning: boolean;
  /** True when void layer should be visible */
  showVoid: boolean;
  /** Current view mode */
  viewMode: ViewMode;
  workspaceInfo: WorkspaceInfo;
  workspaceDimensions: {
    width: number;
    height: number;
    gap: number;
  };
}

// =============================================================================
// Layer Opacities
// =============================================================================

/**
 * Layer opacity values during crossfade transitions.
 */
export interface LayerOpacities {
  /** Desktop layer opacity (0.0 = hidden, 1.0 = fully visible) */
  desktop: number;
  /** Void layer opacity (0.0 = hidden, 1.0 = fully visible) */
  void: number;
}
