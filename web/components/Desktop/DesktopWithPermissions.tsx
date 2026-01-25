/**
 * Desktop With Permissions Component
 *
 * Inner component that uses permissions hook and manages desktop state.
 */

import { useRef, useEffect, useState } from 'react';
import { usePermissions, PermissionsProvider } from '../../desktop/hooks/usePermissions';
import { useWindowActions } from '../../desktop/hooks/useWindows';
import { useKeyboardShortcuts } from '../../desktop/hooks/useKeyboardShortcuts';
import { PermissionDialog } from '../PermissionDialog';
import { DesktopContextMenu } from './DesktopContextMenu';
import { useTheme } from '@cypher-asi/zui';
import { BackgroundContext } from './BackgroundContext';
import { DesktopInner } from './DesktopInner';
import { usePointerHandlers } from './hooks/usePointerHandlers';
import { useBackgroundMenu } from './hooks/useBackgroundMenu';
import type { WorkspaceInfo } from '../../stores/types';
import type { DesktopProps, SelectionBox, DesktopBackgroundType } from './types';
import styles from './Desktop.module.css';

export function DesktopWithPermissions({ supervisor, desktop }: DesktopProps): JSX.Element {
  const containerRef = useRef<HTMLDivElement>(null);
  const backgroundRef = useRef<DesktopBackgroundType | null>(null);
  const [initialized, setInitialized] = useState(false);
  const [selectionBox, setSelectionBox] = useState<SelectionBox | null>(null);

  // Theme state from zui
  const { theme, accent, setTheme, setAccent } = useTheme();

  // Permissions state
  const permissions = usePermissions();

  // Window actions (includes launchTerminal for spawning terminal with process)
  const { launchTerminal } = useWindowActions();

  // Ref to track current workspace info (updated by render loop in DesktopInner)
  const workspaceInfoRef = useRef<WorkspaceInfo | null>(null);

  // Background menu state and handlers
  const {
    backgroundMenu,
    setBackgroundMenu,
    backgrounds,
    getActiveBackground,
    setBackground,
    handleBackgroundSelect,
    closeBackgroundMenu,
    handleBackgroundReady,
  } = useBackgroundMenu({ desktop, backgroundRef, workspaceInfoRef });

  // Pointer event handlers
  const {
    handlePointerDown,
    handlePointerMove,
    handlePointerUp,
    handlePointerLeave,
    handleWheel,
    handleContextMenu,
  } = usePointerHandlers({
    desktop,
    initialized,
    containerRef,
    backgroundMenu,
    setBackgroundMenu,
    selectionBox,
    setSelectionBox,
  });

  // Initialize desktop engine
  useEffect(() => {
    if (initialized) return;

    const container = containerRef.current;
    if (!container) return;

    const rect = container.getBoundingClientRect();
    desktop.init(rect.width, rect.height);

    setInitialized(true);
  }, [desktop, initialized]);

  // Handle resize
  useEffect(() => {
    if (!initialized) return;

    const handleResize = (): void => {
      const container = containerRef.current;
      if (!container) return;

      const rect = container.getBoundingClientRect();
      desktop.resize(rect.width, rect.height);
    };

    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, [desktop, initialized]);

  // Handle orphaned windows (process died but window still exists)
  useEffect(() => {
    if (!initialized || !supervisor || !desktop) return;

    const checkOrphanedWindows = (): void => {
      try {
        // Get all windows with process IDs
        const windows = JSON.parse(desktop.get_windows_json()) as Array<{
          id: number;
          processId?: number;
        }>;

        // Get current processes
        const processes = JSON.parse(supervisor.get_process_list_json()) as Array<{
          pid: number;
        }>;
        const processPids = new Set(processes.map((p) => p.pid));

        // Check each window with a processId
        for (const win of windows) {
          if (win.processId != null && !processPids.has(win.processId)) {
            // Process died - close the orphaned window
            console.log(
              `[Desktop] Process ${win.processId} died, closing orphaned window ${win.id}`
            );
            desktop.close_window(BigInt(win.id));
          }
        }
      } catch {
        // Ignore errors during orphan check
      }
    };

    // Check every second
    const interval = setInterval(checkOrphanedWindows, 1000);
    return () => clearInterval(interval);
  }, [initialized, supervisor, desktop]);

  // Prevent browser zoom on Ctrl+scroll at window level
  useEffect(() => {
    const handleNativeWheel = (e: WheelEvent): void => {
      if (e.ctrlKey) {
        e.preventDefault();
      }
    };

    window.addEventListener('wheel', handleNativeWheel, { passive: false, capture: true });
    return () => window.removeEventListener('wheel', handleNativeWheel, { capture: true });
  }, []);

  // Handle keyboard shortcuts for workspace navigation and void entry/exit
  useKeyboardShortcuts({
    initialized,
    desktop,
    supervisor,
    launchTerminal,
  });

  // Compute selection box rectangle
  const selectionRect = selectionBox
    ? {
        left: Math.min(selectionBox.startX, selectionBox.currentX),
        top: Math.min(selectionBox.startY, selectionBox.currentY),
        width: Math.abs(selectionBox.currentX - selectionBox.startX),
        height: Math.abs(selectionBox.currentY - selectionBox.startY),
      }
    : null;

  return (
    <PermissionsProvider value={permissions}>
      <BackgroundContext.Provider value={{ backgrounds, getActiveBackground, setBackground }}>
        <div
          ref={containerRef}
          className={styles.desktop}
          onPointerDown={handlePointerDown}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerUp}
          onPointerLeave={handlePointerLeave}
          onWheel={handleWheel}
          onContextMenu={handleContextMenu}
        >
          {initialized && (
            <DesktopInner
              desktop={desktop}
              backgroundRef={backgroundRef}
              onBackgroundReady={handleBackgroundReady}
              workspaceInfoRef={workspaceInfoRef}
            />
          )}

          {/* Selection bounding box */}
          {selectionRect && selectionRect.width > 2 && selectionRect.height > 2 && (
            <div
              className={styles.selectionBox}
              style={{
                left: selectionRect.left,
                top: selectionRect.top,
                width: selectionRect.width,
                height: selectionRect.height,
              }}
            />
          )}

          {/* Desktop context menu */}
          {backgroundMenu.visible && (
            <DesktopContextMenu
              x={backgroundMenu.x}
              y={backgroundMenu.y}
              backgrounds={backgrounds}
              currentBackground={getActiveBackground()}
              theme={theme}
              accent={accent}
              onBackgroundSelect={handleBackgroundSelect}
              onThemeSelect={setTheme}
              onAccentSelect={setAccent}
              onClose={closeBackgroundMenu}
            />
          )}

          {/* Permission Dialog - shown when an app requests permissions */}
          {permissions.pendingRequest && (
            <PermissionDialog
              app={permissions.pendingRequest.app}
              onApprove={permissions.pendingRequest.onApprove}
              onDeny={permissions.pendingRequest.onDeny}
            />
          )}
        </div>
      </BackgroundContext.Provider>
    </PermissionsProvider>
  );
}
