import { useCallback } from 'react';
import { useDesktopController } from './useSupervisor';
import { 
  useDesktopStore, 
  selectDesktops,
  selectActiveIndex,
  selectViewMode, 
  selectInVoid,
  selectLayerOpacities,
  type DesktopInfo,
  type ViewMode,
  type LayerOpacities,
} from '../../stores';

// =============================================================================
// Re-export Types from Store
// =============================================================================

export type { DesktopInfo, ViewMode, LayerOpacities };

// =============================================================================
// DEPRECATED POLLING HOOKS
// =============================================================================
// These hooks now use Zustand stores instead of polling.
// The stores are updated by the unified render loop in Desktop.tsx.
// =============================================================================

/**
 * Hook to get all desktops.
 * 
 * @deprecated Use `useDesktopStore(selectDesktops)` directly for better performance.
 * This hook is kept for backward compatibility.
 */
export function useDesktops(): DesktopInfo[] {
  return useDesktopStore(selectDesktops);
}

/**
 * Hook to get active desktop index.
 * 
 * @deprecated Use `useDesktopStore(selectActiveIndex)` directly for better performance.
 * This hook is kept for backward compatibility.
 */
export function useActiveDesktop(): number {
  return useDesktopStore(selectActiveIndex);
}

// Hook for desktop actions (kept unchanged - wraps WASM calls)
export function useDesktopActions() {
  const desktop = useDesktopController();

  const createDesktop = useCallback(
    (name: string) => {
      if (!desktop) return null;
      return desktop.create_desktop(name);
    },
    [desktop]
  );

  const switchDesktop = useCallback(
    (index: number) => {
      desktop?.switch_desktop(index);
    },
    [desktop]
  );

  return {
    createDesktop,
    switchDesktop,
  };
}

/**
 * Hook to get the current view mode.
 * 
 * @deprecated Use `useDesktopStore(selectViewMode)` directly for better performance.
 * This hook is kept for backward compatibility.
 */
export function useViewMode(): ViewMode {
  return useDesktopStore(selectViewMode);
}

/**
 * Hook to check if in void mode.
 * 
 * @deprecated Use `useDesktopStore(selectInVoid)` directly for better performance.
 * This hook is kept for backward compatibility.
 */
export function useIsInVoid(): boolean {
  return useDesktopStore(selectInVoid);
}

// Hook for void actions (kept unchanged - wraps WASM calls)
export function useVoidActions() {
  const desktop = useDesktopController();

  const enterVoid = useCallback(() => {
    desktop?.enter_void();
  }, [desktop]);

  const exitVoid = useCallback(
    (desktopIndex: number) => {
      desktop?.exit_void(desktopIndex);
    },
    [desktop]
  );

  return { enterVoid, exitVoid };
}

/**
 * Hook to get layer opacities during crossfade transitions.
 * Returns { desktop: number, void: number } where values are 0.0-1.0.
 * 
 * @deprecated Use `useDesktopStore(selectLayerOpacities)` directly for better performance.
 * This hook is kept for backward compatibility.
 */
export function useLayerOpacities(): LayerOpacities {
  return useDesktopStore(selectLayerOpacities);
}

// =============================================================================
// Backward Compatibility Aliases (deprecated)
// =============================================================================

/** @deprecated Use DesktopInfo instead */
export type WorkspaceInfo = DesktopInfo;

/** @deprecated Use useDesktops instead */
export const useWorkspaces = useDesktops;

/** @deprecated Use useActiveDesktop instead */
export const useActiveWorkspace = useActiveDesktop;

/** @deprecated Use useDesktopActions instead */
export function useWorkspaceActions() {
  const actions = useDesktopActions();
  return {
    createWorkspace: actions.createDesktop,
    switchWorkspace: actions.switchDesktop,
  };
}
