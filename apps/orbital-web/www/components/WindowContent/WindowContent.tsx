import type { ReactNode, ForwardedRef } from 'react';
import { forwardRef } from 'react';
import type { WindowInfo } from '../../hooks/useWindows';
import { useWindowActions } from '../../hooks/useWindows';
import { useSupervisor } from '../../hooks/useSupervisor';
import { WindowButton } from '@cypher-asi/zui';
import { Panel } from '../Panel';
import styles from './WindowContent.module.css';

// Frame style constants (must match Rust FRAME_STYLE)
const FRAME_STYLE = {
  titleBarHeight: 22,
  resizeHandleSize: 6,
  cornerHandleSize: 12, // Larger corners for easier diagonal targeting
};

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
  const supervisor = useSupervisor();

  // Initial position using GPU-accelerated transform instead of left/top
  // Subsequent position updates happen directly via DOM, bypassing React
  const style: React.CSSProperties = {
    display: 'flex',
    flexDirection: 'column',
    transform: `translate3d(${win.screenRect.x}px, ${win.screenRect.y}px, 0)`,
    width: win.screenRect.width,
    height: win.screenRect.height,
    zIndex: win.zOrder + 10, // +10 so windows are above selection marquee (z-index: 2)
    pointerEvents: 'auto',
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

  // Handle drag start - directly calls Rust to start move drag
  const handleDragStart = (e: React.PointerEvent) => {
    e.stopPropagation();
    if (!win.focused) {
      focusWindow(win.id);
    }
    supervisor?.start_window_drag(BigInt(win.id), e.clientX, e.clientY);
  };

  return (
    <Panel
      ref={ref}
      className={styles.window}
      focused={win.focused}
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

      {/* Title bar with title and buttons */}
      <div className={styles.titleBar} style={{ height: FRAME_STYLE.titleBarHeight }} onPointerDown={handleDragStart}>
        <span className={`${styles.title} ${win.focused ? styles.titleFocused : ''}`}>{win.title}</span>
        <div className={styles.buttons} onPointerDown={(e) => e.stopPropagation()}>
          <WindowButton action="minimize" size="sm" rounded="none" onClick={handleMinimize} />
          <WindowButton action="maximize" size="sm" rounded="none" onClick={handleMaximize} />
          <WindowButton action="close" size="sm" rounded="none" onClick={handleClose} />
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
    </Panel>
  );
});
