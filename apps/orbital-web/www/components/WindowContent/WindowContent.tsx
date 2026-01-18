import type { ReactNode } from 'react';
import type { WindowInfo } from '../../hooks/useWindows';
import { useWindowActions } from '../../hooks/useWindows';
import { useSupervisor } from '../../hooks/useSupervisor';
import styles from './WindowContent.module.css';

// Frame style constants (must match Rust FRAME_STYLE)
const FRAME_STYLE = {
  titleBarHeight: 20,
  borderRadius: 0,
  resizeHandleSize: 8,
  cornerHandleSize: 12, // Larger corners for easier diagonal targeting
};

interface WindowContentProps {
  window: WindowInfo;
  children: ReactNode;
}

export function WindowContent({ window: win, children }: WindowContentProps) {
  const { focusWindow, minimizeWindow, maximizeWindow, closeWindow } = useWindowActions();
  const supervisor = useSupervisor();

  // Position the entire window frame (title bar + content)
  const style: React.CSSProperties = {
    left: win.screenRect.x,
    top: win.screenRect.y,
    width: win.screenRect.width,
    height: win.screenRect.height,
    zIndex: win.zOrder + 1, // +1 so windows are above desktop background
    borderRadius: FRAME_STYLE.borderRadius,
  };

  const handleWindowClick = () => {
    if (!win.focused) {
      focusWindow(win.id);
    }
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
    if (!win.focused) {
      focusWindow(win.id);
    }
    supervisor?.start_window_resize(BigInt(win.id), direction, e.clientX, e.clientY);
  };

  return (
    <div
      className={`${styles.window} ${win.focused ? styles.focused : ''}`}
      style={style}
      data-window-id={win.id}
      onPointerDown={handleWindowClick}
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

      {/* Sci-fi trapezoid title tab */}
      <div className={styles.titleTab}>
        <span className={styles.title}>{win.title}</span>
      </div>

      {/* Title bar with buttons only */}
      <div className={styles.titleBar} style={{ height: FRAME_STYLE.titleBarHeight }}>
        <div className={styles.buttons}>
          <button 
            className={`${styles.btn} ${styles.minimize}`} 
            aria-label="Minimize"
            onClick={handleMinimize}
          >
            −
          </button>
          <button 
            className={`${styles.btn} ${styles.maximize}`} 
            aria-label="Maximize"
            onClick={handleMaximize}
          >
            □
          </button>
          <button 
            className={`${styles.btn} ${styles.close}`} 
            aria-label="Close"
            onClick={handleClose}
          >
            ×
          </button>
        </div>
      </div>
      
      {/* Content area - focus window but stop propagation to allow input focus */}
      <div 
        className={styles.content} 
        onPointerDown={(e) => {
          if (!win.focused) {
            focusWindow(win.id);
          }
          e.stopPropagation();
        }}
        onWheel={(e) => {
          // Stop wheel events from bubbling to desktop unless Ctrl is held
          // This allows normal scrolling within windows without zooming the desktop
          if (!e.ctrlKey) {
            e.stopPropagation();
          }
        }}
      >
        {children}
      </div>
    </div>
  );
}
