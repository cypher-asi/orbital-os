import { useRef, useEffect, useState, useCallback, createContext, useContext } from 'react';
import { SupervisorProvider, Supervisor } from '../../desktop/hooks/useSupervisor';
import { WindowContent } from '../WindowContent/WindowContent';
import { Taskbar } from '../Taskbar/Taskbar';
import { AppRouter } from '../../apps/AppRouter/AppRouter';
import { ContextMenu, MenuItem } from '../ContextMenu/ContextMenu';
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

interface ContextMenuState {
  x: number;
  y: number;
  visible: boolean;
}

// Type for the DesktopBackground WASM class
interface DesktopBackgroundType {
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

interface BackgroundInfo {
  id: string;
  name: string;
}

// Context to share background controller
interface BackgroundContextType {
  backgrounds: BackgroundInfo[];
  currentBackground: string;
  setBackground: (id: string) => void;
}

const BackgroundContext = createContext<BackgroundContextType | null>(null);

export function useBackground() {
  return useContext(BackgroundContext);
}

// =============================================================================
// Frame Data Types - All animation/layout state comes from Rust atomically
// =============================================================================
//
// The desktop has two layers that can render simultaneously:
// 
// 1. DESKTOP LAYER (windows): Current desktop's windows at their positions
//    - Opacity controlled by window.opacity field (0.0-1.0)
//    - Windows fade out immediately when transitioning starts
//
// 2. VOID LAYER (background): All desktops shown as tiles
//    - Rendered by background shader when transitioning=true
//    - Shows workspace tiles during transitions and void mode
//
// During transitions (crossfade model):
// - Both layers render simultaneously
// - Desktop layer fades out (windows opacity → 0)
// - Void layer fades in (background shows all desktops)
// - No complex zoom/pan animation - just smooth opacity crossfade
//
// =============================================================================

interface ViewportState {
  center: { x: number; y: number };
  zoom: number;
}

interface WindowInfo {
  id: number;
  title: string;
  appId: string;
  state: 'normal' | 'minimized' | 'maximized' | 'fullscreen';
  focused: boolean;
  zOrder: number;
  /** 
   * Window opacity for crossfade transitions.
   * 0.0 = invisible (during transitions to void), 1.0 = fully visible.
   * Used to fade out desktop layer during transitions.
   */
  opacity: number;
  screenRect: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
}

interface WorkspaceInfo {
  count: number;
  active: number;
  backgrounds: string[];
}

interface WorkspaceDimensions {
  width: number;
  height: number;
  gap: number;
}

/**
 * Complete frame data from Rust's tick_frame() - single source of truth.
 * 
 * The crossfade transition model uses:
 * - `transitioning`: Controls void layer visibility (background shows all desktops)
 * - `window.opacity`: Controls desktop layer visibility (windows fade out)
 * 
 * Both layers render simultaneously during transitions for smooth crossfade effect.
 */
interface FrameData {
  viewport: ViewportState;
  windows: WindowInfo[];
  /** True during any activity (zoom/pan/drag) - for adaptive framerate */
  animating: boolean;
  /** True only during layer transitions (void enter/exit) - for crossfade */
  transitioning: boolean;
  /** Current view mode (desktop/workspace = desktop view, void = all desktops) */
  viewMode: 'desktop' | 'workspace' | 'void' | 'transitioning';
  workspaceInfo: WorkspaceInfo;
  workspaceDimensions: WorkspaceDimensions;
}

// =============================================================================
// DesktopInner - Renders canvas and windows using frame data from Rust
// =============================================================================
// 
// PERFORMANCE OPTIMIZATION: Direct DOM updates bypass React reconciliation
// 
// All animation logic lives in Rust. This component:
// 1. Runs a single RAF loop that calls Rust's tick_frame()
// 2. Updates background renderer with viewport/workspace info
// 3. Updates window positions DIRECTLY via DOM (not React state)
// 4. Only triggers React re-render when window LIST changes (add/remove)
//
// This eliminates React reconciliation overhead during animations, achieving
// smooth 60fps even with many windows.
//

// Helper to check if window list changed (add/remove, not position)
function windowListChanged(newWindows: WindowInfo[], oldIds: Set<number>): boolean {
  if (newWindows.length !== oldIds.size) return true;
  for (const win of newWindows) {
    if (!oldIds.has(win.id)) return true;
  }
  return false;
}

function DesktopInner({ 
  supervisor,
  backgroundRef,
  onBackgroundReady,
  activeWorkspaceBackground,
}: { 
  supervisor: Supervisor;
  backgroundRef: React.MutableRefObject<DesktopBackgroundType | null>;
  onBackgroundReady: () => void;
  activeWorkspaceBackground: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animationFrameRef = useRef<number | null>(null);
  
  // Window list state - only updated when windows are added/removed
  // Position updates happen directly via DOM refs, bypassing React
  const [windows, setWindows] = useState<WindowInfo[]>([]);
  
  // Track window IDs to detect list changes
  const windowIdsRef = useRef<Set<number>>(new Set());
  
  // Map of window ID -> DOM element ref for direct position updates
  const windowRefsMap = useRef<Map<number, HTMLDivElement>>(new Map());
  
  // Store latest window data for each window (used by refs)
  const windowDataRef = useRef<Map<number, WindowInfo>>(new Map());
  
  // Track windows that are currently fading out (to avoid restarting animation)
  const fadingOutWindowsRef = useRef<Set<number>>(new Set());
  
  // Track previous opacity for each window to detect opacity transitions
  const prevOpacityRef = useRef<Map<number, number>>(new Map());
  
  // Store pending windows when we need to delay React update for fade-out
  const pendingWindowsRef = useRef<WindowInfo[] | null>(null);

  // Sync background renderer with active workspace's background
  useEffect(() => {
    if (backgroundRef.current?.is_initialized()) {
      const current = backgroundRef.current.get_current_background();
      if (current !== activeWorkspaceBackground) {
        backgroundRef.current.set_background(activeWorkspaceBackground);
      }
    }
  }, [activeWorkspaceBackground, backgroundRef]);

  // Initialize WebGPU background renderer and run unified render loop
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    // Set canvas size to match display size
    const updateCanvasSize = () => {
      const rect = canvas.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.floor(rect.width * dpr);
      canvas.height = Math.floor(rect.height * dpr);
      
      // Resize renderer if initialized
      if (backgroundRef.current?.is_initialized()) {
        backgroundRef.current.resize(canvas.width, canvas.height);
      }
    };

    // Initialize the background renderer
    const initBackground = async () => {
      try {
        updateCanvasSize();
        
        // Dynamically import the DesktopBackground from the WASM module
        const { DesktopBackground } = await import('../../pkg/orbital_web.js');
        const background = new DesktopBackground() as DesktopBackgroundType;
        await background.init(canvas);
        backgroundRef.current = background;
        
        onBackgroundReady();
        
        // =================================================================
        // UNIFIED RENDER LOOP - Direct DOM Updates
        // =================================================================
        // Single RAF loop that:
        // 1. Calls Rust's tick_frame() to get ALL state atomically
        // 2. Updates background renderer with viewport/workspace info
        // 3. Updates window positions DIRECTLY via DOM (GPU-accelerated transforms)
        // 4. Only triggers React re-render when window list changes
        //
        // This bypasses React reconciliation for position updates, enabling
        // smooth 60fps animations without React overhead.
        // =================================================================
        
        let lastTime = 0;
        let lastAnimating = false;
        
        const render = (time: number) => {
          animationFrameRef.current = requestAnimationFrame(render);
          
          if (!backgroundRef.current?.is_initialized()) return;
          
          // Adaptive framerate: 60fps when animating, 15fps when idle
          const fps = lastAnimating ? 60 : 15;
          const interval = 1000 / fps;
          
          if (time - lastTime >= interval) {
            lastTime = time - ((time - lastTime) % interval);
            
            try {
              // SINGLE call to Rust - tick + get all data atomically
              const frameJson = supervisor.tick_frame();
              const frame: FrameData = JSON.parse(frameJson);
              
              // Update adaptive framerate based on animation state
              lastAnimating = frame.animating;
              
              // Update background renderer with frame data
              const { viewport, workspaceInfo, workspaceDimensions, viewMode } = frame;
              
              backgroundRef.current.set_viewport(
                viewport.zoom, 
                viewport.center.x, 
                viewport.center.y
              );
              
              backgroundRef.current.set_workspace_info(
                workspaceInfo.count,
                workspaceInfo.active,
                JSON.stringify(workspaceInfo.backgrounds)
              );
              
              backgroundRef.current.set_workspace_dimensions(
                workspaceDimensions.width, 
                workspaceDimensions.height, 
                workspaceDimensions.gap
              );
              
                              // CROSSFADE MODEL: Show void layer (all desktops as tiles) when:
                              // - In void mode (user is viewing all desktops)
                              // - Transitioning between modes (crossfade in progress)
                              //
                              // During transitions, both layers render simultaneously:
                              // - Desktop layer (windows) fades out via window.opacity
                              // - Void layer (background) shows all desktop tiles
                              //
                              // This is NOT triggered by user zoom/pan within a desktop.
                              backgroundRef.current.set_transitioning(
                                viewMode === 'void' || frame.transitioning
                              );
              
              backgroundRef.current.render();
              
              // =============================================================
              // DIRECT DOM UPDATES - Bypass React for position changes
              // =============================================================
              // Update window positions directly via DOM manipulation.
              // This avoids React reconciliation overhead during animations.
              // React state only updates when window list changes (add/remove).
              
              // Build set of current window IDs for quick lookup
              const currentWindowIds = new Set(frame.windows.map(w => w.id));
              
              // IMPORTANT: Fade out windows that are no longer in the frame
              // This handles the case where a window is filtered out during transitions
              // but React hasn't removed it from the DOM yet
              for (const [id, el] of windowRefsMap.current.entries()) {
                if (!currentWindowIds.has(id)) {
                  // Window was filtered out - fade it out (if not already fading)
                  if (!fadingOutWindowsRef.current.has(id)) {
                    fadingOutWindowsRef.current.add(id);
                    el.style.animation = 'windowFadeOut 150ms ease-out forwards';
                    
                    // Capture id in closure for the timeout
                    const windowId = id;
                    
                    // After animation completes, hide and clean up
                    setTimeout(() => {
                      el.style.visibility = 'hidden';
                      el.style.animation = '';
                      fadingOutWindowsRef.current.delete(windowId);
                      
                      // If all fade-outs complete and we have pending windows, apply them
                      if (fadingOutWindowsRef.current.size === 0 && pendingWindowsRef.current) {
                        windowIdsRef.current = new Set(pendingWindowsRef.current.map(w => w.id));
                        setWindows(pendingWindowsRef.current);
                        pendingWindowsRef.current = null;
                      }
                    }, 150);
                  }
                }
              }
              
              // Update stored window data
              for (const win of frame.windows) {
                windowDataRef.current.set(win.id, win);
              }
              
              // Direct DOM updates for existing windows (no React re-render)
              for (const win of frame.windows) {
                const el = windowRefsMap.current.get(win.id);
                if (el && win.state !== 'minimized') {
                  // Check if window was hidden or fading out
                  const wasHidden = el.style.visibility === 'hidden';
                  const wasFadingOut = fadingOutWindowsRef.current.has(win.id);
                  
                  // Track previous opacity to detect transitions (not the CSS value which we set)
                  const prevOpacity = prevOpacityRef.current.get(win.id) ?? 1;
                  const targetOpacity = win.opacity;
                  
                  // Cancel fade-out if window reappeared with opacity > 0
                  if (wasFadingOut && targetOpacity > 0) {
                    fadingOutWindowsRef.current.delete(win.id);
                  }
                  
                  // Make sure it's visible (in case it was hidden)
                  el.style.visibility = 'visible';
                  
                  // Fade OUT: opacity going from 1 to 0 (start of workspace transition)
                  if (prevOpacity > 0 && targetOpacity === 0 && !wasFadingOut) {
                    fadingOutWindowsRef.current.add(win.id);
                    el.style.animation = 'none';
                    void el.offsetHeight; // Force reflow to reset animation
                    el.style.animation = 'windowFadeOut 100ms ease-out forwards';
                    
                    // Remove from fading set after animation completes
                    const windowId = win.id;
                    setTimeout(() => {
                      fadingOutWindowsRef.current.delete(windowId);
                    }, 100);
                  }
                  // Fade IN: opacity going from 0 to 1 (end of workspace transition)
                  else if ((wasHidden || wasFadingOut || prevOpacity === 0) && targetOpacity > 0) {
                    fadingOutWindowsRef.current.delete(win.id);
                    el.style.animation = 'none';
                    void el.offsetHeight; // Force reflow to reset animation
                    el.style.animation = 'windowFadeIn 150ms ease-out forwards';
                  }
                  
                  // Update previous opacity tracking
                  prevOpacityRef.current.set(win.id, targetOpacity);
                  
                  // GPU-accelerated transform instead of left/top
                  el.style.transform = `translate3d(${win.screenRect.x}px, ${win.screenRect.y}px, 0)`;
                  el.style.width = `${win.screenRect.width}px`;
                  el.style.height = `${win.screenRect.height}px`;
                  el.style.zIndex = String(win.zOrder + 10);
                  
                  // Don't override opacity while animation is running - let the animation control it
                  // Only set opacity directly if not currently animating
                  if (!wasFadingOut && prevOpacity === targetOpacity) {
                    el.style.opacity = String(win.opacity);
                  }
                }
              }
              
              // Only update React state when window LIST changes (add/remove)
              // This triggers re-render to create/destroy window components
              if (windowListChanged(frame.windows, windowIdsRef.current)) {
                if (fadingOutWindowsRef.current.size > 0) {
                  // Delay React update until fade-outs complete to keep elements in DOM
                  pendingWindowsRef.current = frame.windows;
                } else {
                  windowIdsRef.current = new Set(frame.windows.map(w => w.id));
                  setWindows(frame.windows);
                }
              }
              
            } catch (e) {
              console.error('[desktop] Render error:', e);
            }
          }
        };
        
        animationFrameRef.current = requestAnimationFrame(render);
      } catch (e) {
        console.warn('[desktop] WebGPU not available, falling back to CSS background:', e);
        // CSS fallback is already in place via the .desktop class
      }
    };

    initBackground();

    // Handle resize
    const handleResize = () => {
      updateCanvasSize();
    };
    window.addEventListener('resize', handleResize);

    return () => {
      window.removeEventListener('resize', handleResize);
      if (animationFrameRef.current !== null) {
        cancelAnimationFrame(animationFrameRef.current);
      }
    };
  }, [supervisor, backgroundRef, onBackgroundReady]);
  
  // Callback to register window DOM refs
  const setWindowRef = useCallback((id: number, el: HTMLDivElement | null) => {
    if (el) {
      windowRefsMap.current.set(id, el);
    } else {
      windowRefsMap.current.delete(id);
      windowDataRef.current.delete(id);
      prevOpacityRef.current.delete(id);
    }
  }, []);

  return (
    <>
      {/* WebGPU canvas for background with procedural shaders */}
      <canvas
        id="desktop-canvas"
        ref={canvasRef}
        className={styles.canvas}
      />

      {/* React overlays for window content - positions updated via direct DOM */}
      {windows
        .filter((w) => w.state !== 'minimized')
        .map((w) => (
          <WindowContent 
            key={w.id} 
            ref={(el) => setWindowRef(w.id, el)}
            window={w}
          >
            <AppRouter appId={w.appId} windowId={w.id} />
          </WindowContent>
        ))}

      <Taskbar />
    </>
  );
}

export function Desktop({ supervisor }: DesktopProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const backgroundRef = useRef<DesktopBackgroundType | null>(null);
  const [initialized, setInitialized] = useState(false);
  const [selectionBox, setSelectionBox] = useState<SelectionBox | null>(null);
  const [contextMenu, setContextMenu] = useState<ContextMenuState>({ x: 0, y: 0, visible: false });
  const [backgrounds, setBackgrounds] = useState<BackgroundInfo[]>([]);
  const [currentBackground, setCurrentBackgroundState] = useState<string>('grain');
  const [settingsRestored, setSettingsRestored] = useState(false);

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
      if (e.ctrlKey) {
        e.preventDefault();
      }
    };

    window.addEventListener('wheel', handleNativeWheel, { passive: false, capture: true });
    return () => window.removeEventListener('wheel', handleNativeWheel, { capture: true });
  }, []);

  // Global pointer move/up handlers to catch drag events even when pointer is over window content
  // This is necessary because window content has stopPropagation which blocks events from reaching Desktop
  useEffect(() => {
    if (!initialized) return;

    const handleGlobalPointerMove = (e: PointerEvent) => {
      supervisor.desktop_pointer_move(e.clientX, e.clientY);
    };

    const handleGlobalPointerUp = () => {
      supervisor.desktop_pointer_up();
    };

    window.addEventListener('pointermove', handleGlobalPointerMove);
    window.addEventListener('pointerup', handleGlobalPointerUp);
    return () => {
      window.removeEventListener('pointermove', handleGlobalPointerMove);
      window.removeEventListener('pointerup', handleGlobalPointerUp);
    };
  }, [supervisor, initialized]);

  // Use capture phase for panning so it intercepts before windows
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const handleCapturePointerDown = (e: PointerEvent) => {
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

    container.addEventListener('pointerdown', handleCapturePointerDown, { capture: true });
    return () => container.removeEventListener('pointerdown', handleCapturePointerDown, { capture: true });
  }, [supervisor]);

  // Callback when background renderer is ready
  const handleBackgroundReady = useCallback(() => {
    if (backgroundRef.current) {
      try {
        const availableJson = backgroundRef.current.get_available_backgrounds();
        const available = JSON.parse(availableJson) as BackgroundInfo[];
        setBackgrounds(available);
        
        // Restore workspace settings from localStorage
        const WORKSPACE_STORAGE_KEY = 'orbital-workspace-settings';
        const savedSettings = localStorage.getItem(WORKSPACE_STORAGE_KEY);
        if (savedSettings && !settingsRestored) {
          supervisor.import_workspace_settings(savedSettings);
          setSettingsRestored(true);
        }
        
        // Sync renderer to active workspace's background
        const activeBackground = supervisor.get_active_workspace_background();
        if (activeBackground && available.some(bg => bg.id === activeBackground)) {
          backgroundRef.current.set_background(activeBackground);
          setCurrentBackgroundState(activeBackground);
        } else {
          const current = backgroundRef.current.get_current_background();
          setCurrentBackgroundState(current);
        }
      } catch (e) {
        console.error('[desktop] Failed to initialize backgrounds:', e);
      }
    }
  }, [supervisor, settingsRestored]);

  // Set background - updates renderer, workspace state, and persists
  const setBackground = useCallback((id: string) => {
    // Update the renderer
    if (backgroundRef.current?.set_background(id)) {
      setCurrentBackgroundState(id);
    }
    
    // Update workspace state in Rust and persist to localStorage
    if (supervisor.set_active_workspace_background(id)) {
      const WORKSPACE_STORAGE_KEY = 'orbital-workspace-settings';
      try {
        const settings = supervisor.export_workspace_settings();
        localStorage.setItem(WORKSPACE_STORAGE_KEY, settings);
      } catch (e) {
        console.error('[desktop] Failed to persist settings:', e);
      }
    }
  }, [supervisor]);

  // Forward pointer events to Rust (bubble phase for normal interactions)
  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      // Don't process if the event is from the context menu (it stops propagation)
      // Close context menu only if clicking directly on the desktop or canvas
      const target = e.target as HTMLElement;
      const isDesktopClick = target === containerRef.current || target.tagName === 'CANVAS';
      
      if (contextMenu.visible && isDesktopClick) {
        setContextMenu({ ...contextMenu, visible: false });
        return; // Don't process further, just close the menu
      }

      const result = JSON.parse(
        supervisor.desktop_pointer_down(e.clientX, e.clientY, e.button, e.ctrlKey, e.shiftKey)
      );
      if (result.type === 'handled') {
        e.preventDefault();
      }

      // Start selection box on left-click directly on desktop background
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
    [supervisor, contextMenu]
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent) => {
      supervisor.desktop_pointer_move(e.clientX, e.clientY);

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

  const handlePointerLeave = useCallback(() => {
    supervisor.desktop_pointer_up();
    setSelectionBox(null);
  }, [supervisor]);

  const handleWheel = useCallback(
    (e: React.WheelEvent) => {
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

  // Handle right-click context menu
  const handleContextMenu = useCallback(
    (e: React.MouseEvent) => {
      // Only show context menu when right-clicking on the desktop background itself
      if (e.target === containerRef.current || (e.target as HTMLElement).tagName === 'CANVAS') {
        e.preventDefault();
        setContextMenu({
          x: e.clientX,
          y: e.clientY,
          visible: true,
        });
      }
    },
    []
  );

  const closeContextMenu = useCallback(() => {
    setContextMenu({ ...contextMenu, visible: false });
  }, [contextMenu]);

  // Build context menu items
  const contextMenuItems: MenuItem[] = [
    {
      id: 'background-header',
      label: 'Change Background',
      disabled: true,
    },
    { id: 'separator', label: '' },
    ...backgrounds.map((bg) => ({
      id: `bg-${bg.id}`,
      label: bg.name,
      checked: bg.id === currentBackground,
      onClick: () => setBackground(bg.id),
    })),
  ];

  // Handle keyboard shortcuts for workspace navigation and void entry/exit
  useEffect(() => {
    if (!initialized) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const tagName = target.tagName.toLowerCase();
      // Ignore if focus is in an input field
      if (tagName === 'input' || tagName === 'textarea' || target.isContentEditable) {
        return;
      }
      
      // Ctrl+` (backtick) or F3: Toggle void view
      if ((e.ctrlKey && e.key === '`') || e.key === 'F3') {
        e.preventDefault();
        try {
          const viewMode = supervisor.get_view_mode();
          // Accept both 'desktop' and legacy 'workspace' for entering void
          if (viewMode === 'desktop' || viewMode === 'workspace') {
            supervisor.enter_void();
          } else if (viewMode === 'void') {
            supervisor.exit_void(supervisor.get_active_workspace());
          }
        } catch {
          // Ignore errors during view mode toggle
        }
        return;
      }

      // Only handle Ctrl+Arrow (not in input fields)
      if (!e.ctrlKey || e.shiftKey || e.altKey || e.metaKey) return;

      if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
        e.preventDefault();
        
        try {
          const workspaces = JSON.parse(supervisor.get_workspaces_json()) as Array<{ id: number }>;
          const count = workspaces.length;
          if (count <= 1) return;

          const current = supervisor.get_active_workspace();
          const next = e.key === 'ArrowLeft'
            ? (current > 0 ? current - 1 : count - 1)
            : (current < count - 1 ? current + 1 : 0);

          // If in void, exit to target workspace; otherwise switch
          if (supervisor.get_view_mode() === 'void') {
            supervisor.exit_void(next);
          } else {
            supervisor.switch_workspace(next);
          }
        } catch {
          // Ignore errors during workspace switch
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [initialized, supervisor]);

  // Sync React state when active workspace changes (e.g., via keyboard shortcuts)
  useEffect(() => {
    if (!initialized || !backgroundRef.current?.is_initialized()) return;

    const syncBackground = () => {
      try {
        const activeBackground = supervisor.get_active_workspace_background();
        if (activeBackground && activeBackground !== currentBackground) {
          backgroundRef.current?.set_background(activeBackground);
          setCurrentBackgroundState(activeBackground);
        }
      } catch {
        // Ignore - method may not exist yet
      }
    };

    const interval = setInterval(syncBackground, 200);
    return () => clearInterval(interval);
  }, [initialized, supervisor, currentBackground]);

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
    <SupervisorProvider value={supervisor}>
      <BackgroundContext.Provider value={{ backgrounds, currentBackground, setBackground }}>
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
              supervisor={supervisor}
              backgroundRef={backgroundRef}
              onBackgroundReady={handleBackgroundReady}
              activeWorkspaceBackground={currentBackground}
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
          {contextMenu.visible && (
            <ContextMenu
              x={contextMenu.x}
              y={contextMenu.y}
              items={contextMenuItems}
              onClose={closeContextMenu}
            />
          )}
        </div>
      </BackgroundContext.Provider>
    </SupervisorProvider>
  );
}
