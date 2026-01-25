/**
 * Desktop Component Types
 *
 * Re-exports types from stores and adds desktop-specific types.
 */

import type { Supervisor, DesktopController } from '../../desktop/hooks/useSupervisor';

// Re-export core types from stores (single source of truth)
export type {
  WindowInfo,
  WindowState,
  WindowType,
  ViewportState,
  WorkspaceInfo,
  FrameData,
  ViewMode,
} from '../../stores/types';

export interface DesktopProps {
  supervisor: Supervisor;
  desktop: DesktopController;
}

export interface SelectionBox {
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
}

export interface BackgroundMenuState {
  x: number;
  y: number;
  visible: boolean;
}

/** Type for the DesktopBackground WASM class */
export interface DesktopBackgroundType {
  init(canvas: HTMLCanvasElement): Promise<void>;
  is_initialized(): boolean;
  resize(width: number, height: number): void;
  render(): void;
  get_available_backgrounds(): string;
  get_current_background(): string;
  set_background(id: string): boolean;
  set_viewport(zoom: number, center_x: number, center_y: number): void;
  set_workspace_info(count: number, active: number, backgrounds_json: string): void;
  set_transitioning(transitioning: boolean): void;
  set_workspace_dimensions(width: number, height: number, gap: number): void;
}

export interface WorkspaceDimensions {
  width: number;
  height: number;
  gap: number;
}

/** Background info type from DesktopContextMenu */
export interface BackgroundInfo {
  id: string;
  name: string;
}

/** Helper to check if window list changed (add/remove, not position) */
export function windowListChanged(newWindows: Array<{ id: number }>, oldIds: Set<number>): boolean {
  if (newWindows.length !== oldIds.size) return true;
  for (const win of newWindows) {
    if (!oldIds.has(win.id)) return true;
  }
  return false;
}
