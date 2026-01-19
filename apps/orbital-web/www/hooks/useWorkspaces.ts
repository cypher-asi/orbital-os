import { useState, useEffect, useCallback, useRef } from 'react';
import { useSupervisor } from './useSupervisor';

// Workspace info from Rust
export interface WorkspaceInfo {
  id: number;
  name: string;
  active: boolean;
  windowCount: number;
  background: string;
}

// View mode from Rust
export type ViewMode = 'workspace' | 'void' | 'transitioning';

const WORKSPACE_STORAGE_KEY = 'orbital-workspace-settings';

// Hook to get all workspaces
export function useWorkspaces(): WorkspaceInfo[] {
  const supervisor = useSupervisor();
  const [workspaces, setWorkspaces] = useState<WorkspaceInfo[]>([]);

  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      try {
        const json = supervisor.get_workspaces_json();
        const parsed = JSON.parse(json) as WorkspaceInfo[];
        setWorkspaces(parsed);
      } catch (e) {
        console.error('Failed to parse workspaces:', e);
      }
    };

    update();
    const interval = setInterval(update, 200);
    return () => clearInterval(interval);
  }, [supervisor]);

  return workspaces;
}

// Hook to get active workspace index
export function useActiveWorkspace(): number {
  const supervisor = useSupervisor();
  const [active, setActive] = useState(0);

  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      setActive(supervisor.get_active_workspace());
    };

    update();
    const interval = setInterval(update, 200);
    return () => clearInterval(interval);
  }, [supervisor]);

  return active;
}

// Hook for workspace actions
export function useWorkspaceActions() {
  const supervisor = useSupervisor();

  const createWorkspace = useCallback(
    (name: string) => {
      if (!supervisor) return null;
      return supervisor.create_workspace(name);
    },
    [supervisor]
  );

  const switchWorkspace = useCallback(
    (index: number) => {
      supervisor?.switch_workspace(index);
    },
    [supervisor]
  );

  const setWorkspaceBackground = useCallback(
    (index: number, backgroundId: string) => {
      if (!supervisor) return false;
      const success = supervisor.set_workspace_background(index, backgroundId);
      if (success) {
        // Persist settings after change
        persistWorkspaceSettings(supervisor);
      }
      return success;
    },
    [supervisor]
  );

  const setActiveWorkspaceBackground = useCallback(
    (backgroundId: string) => {
      if (!supervisor) return false;
      const success = supervisor.set_active_workspace_background(backgroundId);
      if (success) {
        // Persist settings after change
        persistWorkspaceSettings(supervisor);
      }
      return success;
    },
    [supervisor]
  );

  return {
    createWorkspace,
    switchWorkspace,
    setWorkspaceBackground,
    setActiveWorkspaceBackground,
  };
}

// Helper to persist workspace settings to localStorage
function persistWorkspaceSettings(supervisor: ReturnType<typeof useSupervisor>) {
  if (!supervisor) return;
  try {
    const settings = supervisor.export_workspace_settings();
    localStorage.setItem(WORKSPACE_STORAGE_KEY, settings);
    console.log('[workspaces] Persisted workspace settings');
  } catch (e) {
    console.error('[workspaces] Failed to persist settings:', e);
  }
}

// Hook to restore workspace settings on init
export function useWorkspaceSettingsRestore() {
  const supervisor = useSupervisor();
  const restoredRef = useRef(false);

  useEffect(() => {
    if (!supervisor || restoredRef.current) return;

    try {
      const saved = localStorage.getItem(WORKSPACE_STORAGE_KEY);
      if (saved) {
        const success = supervisor.import_workspace_settings(saved);
        if (success) {
          console.log('[workspaces] Restored workspace settings from localStorage');
        }
      }
    } catch (e) {
      console.error('[workspaces] Failed to restore settings:', e);
    }

    restoredRef.current = true;
  }, [supervisor]);
}

// Hook to get the active workspace's background
export function useActiveWorkspaceBackground(): string {
  const supervisor = useSupervisor();
  const [background, setBackground] = useState<string>('grain');

  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      try {
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
  const [viewMode, setViewMode] = useState<ViewMode>('workspace');

  useEffect(() => {
    if (!supervisor) return;

    const update = () => {
      try {
        const mode = supervisor.get_view_mode() as ViewMode;
        setViewMode(mode);
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
    (workspaceIndex: number) => {
      supervisor?.exit_void(workspaceIndex);
    },
    [supervisor]
  );

  return { enterVoid, exitVoid };
}
