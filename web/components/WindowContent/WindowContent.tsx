import type { ReactNode, ForwardedRef } from 'react';
import { forwardRef, useRef } from 'react';
import type { WindowInfo } from '../../desktop/hooks/useWindows';
import { useWindowActions } from '../../desktop/hooks/useWindows';
import { useDesktopController } from '../../desktop/hooks/useSupervisor';
import { Panel, ButtonWindow } from '@cypher-asi/zui';
import styles from './WindowContent.module.css';

// Frame style constants (must match Rust FRAME_STYLE)
const FRAME_STYLE = {
  titleBarHeight: 22,
  resizeHandleSize: 6,
  cornerHandleSize: 12, // Larger corners for easier diagonal targeting
};

// Drag threshold in pixels - must move this much before drag starts
const DRAG_THRESHOLD = 5;

interface WindowContentProps {
  window: WindowInfo;
  children: ReactNode;
}

// Use forwardRef so parent can update position directly via DOM
export const WindowContent = forwardRef(function WindowContent(
  { window: win, children }: WindowContentProps,
  ref: ForwardedRef<HTMLDivElement>
) {
  const { focusWindow, minimizeWindow, maximizeWindow, closeWindow } = useWindowActions();
  const desktopController = useDesktopController();

  // Track potential drag start position
  const dragStartRef = useRef<{ x: number; y: number; started: boolean } | null>(null);

  // Initial position using GPU-accelerated transform instead of left/top
  // Subsequent position updates happen directly via DOM, bypassing React
  // IMPORTANT: Set initial opacity to match window state to avoid flash during transitions
  const style: React.CSSProperties = {
    display: 'flex',
    flexDirection: 'column',
    transform: `translate3d(${win.screenRect.x}px, ${win.screenRect.y}px, 0)`,
    width: win.screenRect.width,
    height: win.screenRect.height,
    zIndex: win.zOrder + 10, // +10 so windows are above selection marquee (z-index: 2)
    opacity: win.opacity, // Match Rust-provided opacity to avoid flash on creation
    transition: 'none', // Explicitly disable CSS transitions - opacity controlled by Rust
    pointerEvents: 'auto',
  };

  // Handle pointer down on window - always focus to bring to front
  const handleWindowPointerDown = (e: React.PointerEvent) => {
    // Always call focus - Rust will update z-order to bring window to front
    focusWindow(win.id);
  };

  const handleMinimize = (e: React.MouseEvent) => {
    e.stopPropagation();
    minimizeWindow(win.id);
  };

  const handleMaximize = (e: React.MouseEvent) => {
    e.stopPropagation();
    maximizeWindow(win.id);
  };

  const handleClose = (e: React.MouseEvent) => {
    e.stopPropagation();
    closeWindow(win.id);
  };

  const handleSize = FRAME_STYLE.resizeHandleSize;
  const cornerSize = FRAME_STYLE.cornerHandleSize;

  // Handle resize start - directly calls Rust to start resize drag
  const handleResizeStart = (direction: string) => (e: React.PointerEvent) => {
    e.stopPropagation();
    focusWindow(win.id);
    desktopController?.start_window_resize(BigInt(win.id), direction, e.clientX, e.clientY);
  };

  // Handle drag start from title bar - directly calls Rust to start move drag
  const handleDragStart = (e: React.PointerEvent) => {
    e.stopPropagation();
    focusWindow(win.id);
    desktopController?.start_window_drag(BigInt(win.id), e.clientX, e.clientY);
  };

  return (
    <Panel
      ref={ref}
      className={`${styles.window} ${win.focused ? styles.focused : ''}`}
      variant="glass"
      border="future"
      style={style}
      data-window-id={win.id}
      onPointerDown={handleWindowPointerDown}
    >
      {/* Resize handles - directly start resize drag operation */}
      <div className={`${styles.resizeHandle} ${styles.resizeN}`} style={{ height: handleSize }} onPointerDown={handleResizeStart('n')} />
      <div className={`${styles.resizeHandle} ${styles.resizeS}`} style={{ height: handleSize }} onPointerDown={handleResizeStart('s')} />
      <div className={`${styles.resizeHandle} ${styles.resizeE}`} style={{ width: handleSize }} onPointerDown={handleResizeStart('e')} />
      <div className={`${styles.resizeHandle} ${styles.resizeW}`} style={{ width: handleSize }} onPointerDown={handleResizeStart('w')} />
      {/* Corners use larger handles for easier diagonal targeting */}
      <div className={`${styles.resizeHandle} ${styles.resizeNE}`} style={{ width: cornerSize, height: cornerSize }} onPointerDown={handleResizeStart('ne')} />
      <div className={`${styles.resizeHandle} ${styles.resizeNW}`} style={{ width: cornerSize, height: cornerSize }} onPointerDown={handleResizeStart('nw')} />
      <div className={`${styles.resizeHandle} ${styles.resizeSE}`} style={{ width: cornerSize, height: cornerSize }} onPointerDown={handleResizeStart('se')} />
      <div className={`${styles.resizeHandle} ${styles.resizeSW}`} style={{ width: cornerSize, height: cornerSize }} onPointerDown={handleResizeStart('sw')} />

      {/* Title bar with title and buttons */}
      <div className={styles.titleBar} style={{ height: FRAME_STYLE.titleBarHeight }} onPointerDown={handleDragStart}>
        <span className={`${styles.title} ${win.focused ? styles.titleFocused : ''}`}>{win.title} (ID:{win.id} Y:{Math.round(win.screenRect.y)})</span>
        <div className={styles.buttons} onPointerDown={(e) => e.stopPropagation()}>
          <ButtonWindow action="minimize" size="xs" rounded="none" onClick={handleMinimize} />
          <ButtonWindow action="maximize" size="xs" rounded="none" onClick={handleMaximize} />
          <ButtonWindow action="close" size="xs" rounded="none" onClick={handleClose} />
        </div>
      </div>
      
      {/* Content area - supports drag threshold for all windows */}
      <div 
        className={styles.content}
        onPointerDown={(e) => {
          // Always focus/bring to front when clicking anywhere in content
          focusWindow(win.id);

          // Don't set up drag tracking for interactive elements (buttons, inputs, etc.)
          const target = e.target as HTMLElement;
          if (target.tagName === 'BUTTON' || target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT') {
            e.stopPropagation();
            return;
          }

          // Track potential drag start
          dragStartRef.current = {
            x: e.clientX,
            y: e.clientY,
            started: false,
          };

          // Capture pointer to track movement even outside element
          (e.target as HTMLElement).setPointerCapture(e.pointerId);
          e.stopPropagation();
        }}
        onPointerMove={(e) => {
          if (!dragStartRef.current) return;

          const dx = e.clientX - dragStartRef.current.x;
          const dy = e.clientY - dragStartRef.current.y;
          const distance = Math.sqrt(dx * dx + dy * dy);

          // If moved beyond threshold, start dragging
          if (distance > DRAG_THRESHOLD && !dragStartRef.current.started) {
            dragStartRef.current.started = true;
            desktopController?.start_window_drag(BigInt(win.id), dragStartRef.current.x, dragStartRef.current.y);
          }
        }}
        onPointerUp={(e) => {
          if (dragStartRef.current) {
            (e.target as HTMLElement).releasePointerCapture(e.pointerId);
            dragStartRef.current = null;
          }
        }}
        onPointerCancel={(e) => {
          if (dragStartRef.current) {
            (e.target as HTMLElement).releasePointerCapture(e.pointerId);
            dragStartRef.current = null;
          }
        }}
        onWheel={(e) => {
          // Stop wheel events from bubbling to desktop unless Ctrl is held
          if (!e.ctrlKey) {
            e.stopPropagation();
          }
        }}
      >
        {children}
      </div>
    </Panel>
  );
});
