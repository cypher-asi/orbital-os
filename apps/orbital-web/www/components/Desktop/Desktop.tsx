import { useRef, useEffect, useState, useCallback } from 'react';
import { SupervisorProvider, Supervisor } from '../../hooks/useSupervisor';
import { useWindowScreenRects, WindowInfo } from '../../hooks/useWindows';
import { WindowContent } from '../WindowContent/WindowContent';
import { Taskbar } from '../Taskbar/Taskbar';
import { AppRouter } from '../../apps/AppRouter/AppRouter';
import styles from './Desktop.module.css';

interface DesktopProps {
  supervisor: Supervisor;
}

interface SelectionBox {
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
}

// Inner component that uses the Supervisor context
function DesktopInner() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const windows = useWindowScreenRects();

  return (
    <>
      {/* WebGPU canvas for rendering window frames (placeholder for now) */}
      <canvas
        id="desktop-canvas"
        ref={canvasRef}
        className={styles.canvas}
      />

      {/* React overlays for window content - positioned by Rust */}
      {windows
        .filter((w) => w.state !== 'minimized')
        .map((w) => (
          <WindowContent key={w.id} window={w}>
            <AppRouter appId={w.appId} windowId={w.id} />
          </WindowContent>
        ))}

      <Taskbar />
    </>
  );
}

export function Desktop({ supervisor }: DesktopProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [initialized, setInitialized] = useState(false);
  const [selectionBox, setSelectionBox] = useState<SelectionBox | null>(null);

  // Initialize desktop engine
  useEffect(() => {
    if (initialized) return;

    const container = containerRef.current;
    if (!container) return;

    const rect = container.getBoundingClientRect();
    supervisor.init_desktop(rect.width, rect.height);
    setInitialized(true);
  }, [supervisor, initialized]);

  // Handle resize
  useEffect(() => {
    if (!initialized) return;

    const handleResize = () => {
      const container = containerRef.current;
      if (!container) return;

      const rect = container.getBoundingClientRect();
      supervisor.resize_desktop(rect.width, rect.height);
    };

    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, [supervisor, initialized]);

  // Prevent browser zoom on Ctrl+scroll at window level (capture phase to intercept early)
  useEffect(() => {
    const handleNativeWheel = (e: WheelEvent) => {
      // Prevent browser zoom when Ctrl is held (with or without Shift)
      if (e.ctrlKey) {
        e.preventDefault();
      }
    };

    // Use window-level listener with capture to intercept before browser can process
    window.addEventListener('wheel', handleNativeWheel, { passive: false, capture: true });
    return () => window.removeEventListener('wheel', handleNativeWheel, { capture: true });
  }, []);

  // Use capture phase for panning so it intercepts before windows
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const handleCapturePointerDown = (e: PointerEvent) => {
      // Middle mouse button OR Ctrl/Shift + primary button = pan (intercept before windows)
      const isPanGesture = e.button === 1 || (e.button === 0 && (e.ctrlKey || e.shiftKey));
      if (isPanGesture) {
        const result = JSON.parse(
          supervisor.desktop_pointer_down(e.clientX, e.clientY, e.button, e.ctrlKey, e.shiftKey)
        );
        if (result.type === 'handled') {
          e.preventDefault();
          e.stopPropagation();
        }
      }
    };

    // Capture phase runs before bubble phase, so we get the event first
    container.addEventListener('pointerdown', handleCapturePointerDown, { capture: true });
    return () => container.removeEventListener('pointerdown', handleCapturePointerDown, { capture: true });
  }, [supervisor]);

  // Forward pointer events to Rust (bubble phase for normal interactions)
  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      const result = JSON.parse(
        supervisor.desktop_pointer_down(e.clientX, e.clientY, e.button, e.ctrlKey, e.shiftKey)
      );
      if (result.type === 'handled') {
        e.preventDefault();
      }

      // Start selection box on left-click directly on desktop background
      // (not on windows, not with modifiers, and only if Rust didn't handle it)
      if (
        e.button === 0 &&
        !e.ctrlKey &&
        !e.shiftKey &&
        result.type !== 'handled' &&
        e.target === containerRef.current
      ) {
        setSelectionBox({
          startX: e.clientX,
          startY: e.clientY,
          currentX: e.clientX,
          currentY: e.clientY,
        });
      }
    },
    [supervisor]
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent) => {
      supervisor.desktop_pointer_move(e.clientX, e.clientY);

      // Update selection box if active
      if (selectionBox) {
        setSelectionBox((prev) =>
          prev ? { ...prev, currentX: e.clientX, currentY: e.clientY } : null
        );
      }
    },
    [supervisor, selectionBox]
  );

  const handlePointerUp = useCallback(() => {
    supervisor.desktop_pointer_up();
    setSelectionBox(null);
  }, [supervisor]);

  // Release drag state when pointer leaves the desktop (e.g., goes off-screen)
  const handlePointerLeave = useCallback(() => {
    supervisor.desktop_pointer_up();
    setSelectionBox(null);
  }, [supervisor]);

  const handleWheel = useCallback(
    (e: React.WheelEvent) => {
      // Only zoom desktop canvas when Ctrl is held
      if (e.ctrlKey) {
        supervisor.desktop_wheel(
          e.deltaX,
          e.deltaY,
          e.clientX,
          e.clientY,
          e.ctrlKey
        );
      }
    },
    [supervisor]
  );

  // Compute selection box rectangle (handle drag in any direction)
  const selectionRect = selectionBox
    ? {
        left: Math.min(selectionBox.startX, selectionBox.currentX),
        top: Math.min(selectionBox.startY, selectionBox.currentY),
        width: Math.abs(selectionBox.currentX - selectionBox.startX),
        height: Math.abs(selectionBox.currentY - selectionBox.startY),
      }
    : null;

  return (
    <SupervisorProvider value={supervisor}>
      <div
        ref={containerRef}
        className={styles.desktop}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerLeave={handlePointerLeave}
        onWheel={handleWheel}
      >
        {initialized && <DesktopInner />}

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
      </div>
    </SupervisorProvider>
  );
}
