import { useState, useEffect, useCallback, useRef } from 'react';
import { useSupervisor } from './useSupervisor';
import type { LayerOpacities, ViewMode } from '../types';

// Desktop info from Rust
export interface DesktopInfo {
  id: number;
  name: string;
  active: boolean;
  windowCount: number;
  background: string;
}

const DESKTOP_STORAGE_KEY = 'orbital-desktop-settings';

// Hook to get all desktops
export function useDesktops(): DesktopInfo[] {
  const supervisor = useSupervisor();
  const [desktops, setDesktops] = useState<DesktopInfo[]>([]);

  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      try {
        // The Rust API still uses get_workspaces_json for backward compatibility
        const json = supervisor.get_workspaces_json();
        const parsed = JSON.parse(json) as DesktopInfo[];
        setDesktops(parsed);
      } catch (e) {
        console.error('Failed to parse desktops:', e);
      }
    };

    update();
    const interval = setInterval(update, 200);
    return () => clearInterval(interval);
  }, [supervisor]);

  return desktops;
}

// Hook to get active desktop index
export function useActiveDesktop(): number {
  const supervisor = useSupervisor();
  const [active, setActive] = useState(0);

  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      // The Rust API still uses get_active_workspace for backward compatibility
      setActive(supervisor.get_active_workspace());
    };

    update();
    const interval = setInterval(update, 200);
    return () => clearInterval(interval);
  }, [supervisor]);

  return active;
}

// Hook for desktop actions
export function useDesktopActions() {
  const supervisor = useSupervisor();

  const createDesktop = useCallback(
    (name: string) => {
      if (!supervisor) return null;
      // The Rust API still uses create_workspace for backward compatibility
      return supervisor.create_workspace(name);
    },
    [supervisor]
  );

  const switchDesktop = useCallback(
    (index: number) => {
      // The Rust API still uses switch_workspace for backward compatibility
      supervisor?.switch_workspace(index);
    },
    [supervisor]
  );

  const setDesktopBackground = useCallback(
    (index: number, backgroundId: string) => {
      if (!supervisor) return false;
      // The Rust API still uses set_workspace_background for backward compatibility
      const success = supervisor.set_workspace_background(index, backgroundId);
      if (success) {
        persistDesktopSettings(supervisor);
      }
      return success;
    },
    [supervisor]
  );

  const setActiveDesktopBackground = useCallback(
    (backgroundId: string) => {
      if (!supervisor) return false;
      // The Rust API still uses set_active_workspace_background for backward compatibility
      const success = supervisor.set_active_workspace_background(backgroundId);
      if (success) {
        persistDesktopSettings(supervisor);
      }
      return success;
    },
    [supervisor]
  );

  return {
    createDesktop,
    switchDesktop,
    setDesktopBackground,
    setActiveDesktopBackground,
  };
}

// Helper to persist desktop settings to localStorage
function persistDesktopSettings(supervisor: ReturnType<typeof useSupervisor>) {
  if (!supervisor) return;
  try {
    // The Rust API still uses export_workspace_settings for backward compatibility
    const settings = supervisor.export_workspace_settings();
    localStorage.setItem(DESKTOP_STORAGE_KEY, settings);
    console.log('[desktops] Persisted desktop settings');
  } catch (e) {
    console.error('[desktops] Failed to persist settings:', e);
  }
}

// Hook to restore desktop settings on init
export function useDesktopSettingsRestore() {
  const supervisor = useSupervisor();
  const restoredRef = useRef(false);

  useEffect(() => {
    if (!supervisor || restoredRef.current) return;

    try {
      const saved = localStorage.getItem(DESKTOP_STORAGE_KEY);
      if (saved) {
        // The Rust API still uses import_workspace_settings for backward compatibility
        const success = supervisor.import_workspace_settings(saved);
        if (success) {
          console.log('[desktops] Restored desktop settings from localStorage');
        }
      }
    } catch (e) {
      console.error('[desktops] Failed to restore settings:', e);
    }

    restoredRef.current = true;
  }, [supervisor]);
}

// Hook to get the active desktop's background
export function useActiveDesktopBackground(): string {
  const supervisor = useSupervisor();
  const [background, setBackground] = useState<string>('grain');

  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      try {
        // The Rust API still uses get_active_workspace_background for backward compatibility
        const bg = supervisor.get_active_workspace_background();
        setBackground(bg);
      } catch (e) {
        // Supervisor may not have this method yet
      }
    };

    update();
    const interval = setInterval(update, 200);
    return () => clearInterval(interval);
  }, [supervisor]);

  return background;
}

// Hook to get the current view mode
export function useViewMode(): ViewMode {
  const supervisor = useSupervisor();
  const [viewMode, setViewMode] = useState<ViewMode>('desktop');

  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      try {
        const mode = supervisor.get_view_mode() as string;
        // Map legacy 'workspace' to 'desktop'
        if (mode === 'workspace') {
          setViewMode('desktop');
        } else {
          setViewMode(mode as ViewMode);
        }
      } catch (e) {
        // Supervisor may not have this method yet
      }
    };

    update();
    const interval = setInterval(update, 100); // More frequent for responsive UI
    return () => clearInterval(interval);
  }, [supervisor]);

  return viewMode;
}

// Hook to check if in void mode
export function useIsInVoid(): boolean {
  const supervisor = useSupervisor();
  const [isInVoid, setIsInVoid] = useState(false);

  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      try {
        setIsInVoid(supervisor.is_in_void());
      } catch (e) {
        // Supervisor may not have this method yet
      }
    };

    update();
    const interval = setInterval(update, 100);
    return () => clearInterval(interval);
  }, [supervisor]);

  return isInVoid;
}

// Hook for void actions
export function useVoidActions() {
  const supervisor = useSupervisor();

  const enterVoid = useCallback(() => {
    supervisor?.enter_void();
  }, [supervisor]);

  const exitVoid = useCallback(
    (desktopIndex: number) => {
      supervisor?.exit_void(desktopIndex);
    },
    [supervisor]
  );

  return { enterVoid, exitVoid };
}

// Hook to get layer opacities during crossfade transitions
// Returns { desktop: number, void: number } where values are 0.0-1.0
export function useLayerOpacities(): LayerOpacities {
  const supervisor = useSupervisor();
  const [opacities, setOpacities] = useState<LayerOpacities>({ desktop: 1.0, void: 0.0 });

  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      try {
        const mode = supervisor.get_view_mode() as string;
        const transitioning = supervisor.is_animating_viewport?.() ?? false;

        if (transitioning) {
          // During transition, both layers visible with 50/50 opacity
          setOpacities({ desktop: 0.5, void: 0.5 });
        } else if (mode === 'workspace' || mode === 'desktop') {
          setOpacities({ desktop: 1.0, void: 0.0 });
        } else if (mode === 'void') {
          setOpacities({ desktop: 0.0, void: 1.0 });
        }
      } catch (e) {
        // Default to desktop visible
        setOpacities({ desktop: 1.0, void: 0.0 });
      }
    };

    update();
    const interval = setInterval(update, 50); // Fast updates for smooth transitions
    return () => clearInterval(interval);
  }, [supervisor]);

  return opacities;
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
    setWorkspaceBackground: actions.setDesktopBackground,
    setActiveWorkspaceBackground: actions.setActiveDesktopBackground,
  };
}

/** @deprecated Use useDesktopSettingsRestore instead */
export const useWorkspaceSettingsRestore = useDesktopSettingsRestore;

/** @deprecated Use useActiveDesktopBackground instead */
export const useActiveWorkspaceBackground = useActiveDesktopBackground;
