import { useRef, useEffect, useState, useCallback, createContext, useContext } from 'react';
import { SupervisorProvider, Supervisor, DesktopControllerProvider, DesktopController } from '../../desktop/hooks/useSupervisor';
import { usePermissions, PermissionsProvider } from '../../desktop/hooks/usePermissions';
import { useWindowActions } from '../../desktop/hooks/useWindows';
import { WindowContent } from '../WindowContent/WindowContent';
import { Taskbar } from '../Taskbar/Taskbar';
import { AppRouter } from '../../apps/AppRouter/AppRouter';
import { PermissionDialog } from '../PermissionDialog';
import { Menu, useTheme, THEMES, ACCENT_COLORS, type Theme, type AccentColor, type MenuItem } from '@cypher-asi/zui';
import styles from './Desktop.module.css';

// Human-readable labels for themes
const THEME_LABELS: Record<Theme, string> = {
  dark: 'Dark',
  light: 'Light',
  system: 'System',
};

// Human-readable labels for accent colors
const ACCENT_LABELS: Record<AccentColor, string> = {
  cyan: 'Cyan',
  blue: 'Blue',
  purple: 'Purple',
  green: 'Green',
  orange: 'Orange',
  rose: 'Rose',
};

interface DesktopProps {
  supervisor: Supervisor;
  desktop: DesktopController;
}

interface SelectionBox {
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
}

interface BackgroundMenuState {
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
  getActiveBackground: () => string;
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
  /** Associated process ID (for process isolation in terminal windows) */
  processId?: number;
  state: 'normal' | 'minimized' | 'maximized' | 'fullscreen';
  /** Window type determines the chrome/presentation style */
  windowType: 'standard' | 'widget';
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
  actualActive: number;
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
 * - `showVoid`: Controls void layer visibility (background shows all desktops)
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
  /** True when void layer should be visible (in void mode or during void transitions) */
  showVoid: boolean;
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
  desktop,
  backgroundRef,
  onBackgroundReady,
  workspaceInfoRef,
}: { 
  desktop: DesktopController;
  backgroundRef: React.MutableRefObject<DesktopBackgroundType | null>;
  onBackgroundReady: () => void;
  workspaceInfoRef: React.MutableRefObject<WorkspaceInfo | null>;
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
  
  // Track pending background change during transitions
  const pendingBackgroundRef = useRef<string | null>(null);
  const backgroundSwitchedRef = useRef<boolean>(false);

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
        console.log('[Desktop] Starting background initialization...');
        
        // Dynamically import the DesktopBackground from the WASM module
        const { DesktopBackground } = await import('../../pkg/orbital_web.js');
        const background = new DesktopBackground() as DesktopBackgroundType;
        
        // Add timeout for WebGPU init - don't block forever
        const initPromise = background.init(canvas);
        const timeoutPromise = new Promise((_, reject) => 
          setTimeout(() => reject(new Error('WebGPU init timeout')), 5000)
        );
        
        try {
          await Promise.race([initPromise, timeoutPromise]);
          backgroundRef.current = background;
          console.log('[Desktop] Background initialized successfully');
          onBackgroundReady();
        } catch (initError) {
          console.warn('[Desktop] Background init failed/timeout, continuing without GPU background:', initError);
        }
        
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
          
          // Adaptive framerate: 60fps when animating, 15fps when idle
          const fps = lastAnimating ? 60 : 15;
          const interval = 1000 / fps;
          
          // Track whether we have a working background renderer
          const hasBackground = backgroundRef.current?.is_initialized() ?? false;
          
          if (time - lastTime >= interval) {
            lastTime = time - ((time - lastTime) % interval);
            
            try {
              // SINGLE call to Rust - tick + get all data atomically
              const frameJson = desktop.tick_frame();
              const frame: FrameData = JSON.parse(frameJson);
              
              // Debug: Log first frame and any frame with windows
              if (lastTime === 0 || (frame.windows.length > 0 && windowIdsRef.current.size === 0)) {
                console.log('[Desktop] Frame data:', { 
                  windowCount: frame.windows.length, 
                  viewMode: frame.viewMode,
                  windows: frame.windows.map(w => ({ id: w.id, appId: w.appId }))
                });
              }
              
              // Update adaptive framerate based on animation state
              lastAnimating = frame.animating;
              
              // Update background renderer with frame data (only if available)
              const { viewport, workspaceInfo, workspaceDimensions } = frame;
              
              if (hasBackground) {
                backgroundRef.current!.set_viewport(
                  viewport.zoom, 
                  viewport.center.x, 
                  viewport.center.y
                );
                
                backgroundRef.current!.set_workspace_info(
                  workspaceInfo.count,
                  workspaceInfo.active,
                  JSON.stringify(workspaceInfo.backgrounds)
                );
                
                backgroundRef.current!.set_workspace_dimensions(
                  workspaceDimensions.width, 
                  workspaceDimensions.height, 
                  workspaceDimensions.gap
                );
                
                // Sync background renderer with active desktop's background from Rust state
                // Switch backgrounds during the blackout period (40-60% of transition)
                const targetBackground = workspaceInfo.backgrounds[workspaceInfo.actualActive] || 'grain';
                const currentRendererBg = backgroundRef.current!.get_current_background();
                
                if (frame.transitioning) {
                  // During transition: Switch background when visual workspace changes
                  // This happens during the extended blackout period where windows are fully transparent
                  if (targetBackground !== currentRendererBg && !backgroundSwitchedRef.current) {
                    if (workspaceInfo.active === workspaceInfo.actualActive) {
                      backgroundRef.current!.set_background(targetBackground);
                      backgroundSwitchedRef.current = true;
                    }
                  }
                } else {
                  // Transition complete: Ensure background is correct and reset switch flag
                  if (targetBackground !== currentRendererBg) {
                    backgroundRef.current!.set_background(targetBackground);
                  }
                  backgroundSwitchedRef.current = false;
                }
                
                // CROSSFADE MODEL: Show void layer (all desktops as tiles) when:
                // - In void mode (user is viewing all desktops)
                // - Transitioning TO or FROM void (not during desktop switches)
                //
                // During void transitions, both layers render simultaneously:
                // - Desktop layer (windows) fades out via window.opacity
                // - Void layer (background) shows all desktop tiles
                //
                // Desktop switches do NOT show void - they just fade windows.
                backgroundRef.current!.set_transitioning(frame.showVoid);
                
                backgroundRef.current!.render();
              }
              
              // Store workspace info for parent component access (context menu, etc.)
              workspaceInfoRef.current = workspaceInfo;
              
              // =============================================================
              // DIRECT DOM UPDATES - Bypass React for position changes
              // =============================================================
              // Update window positions directly via DOM manipulation.
              // This avoids React reconciliation overhead during animations.
              // React state only updates when window list changes (add/remove).
              
              // Build set of current window IDs for quick lookup
              const currentWindowIds = new Set(frame.windows.map(w => w.id));
              
              // IMPORTANT: Hide windows that are no longer in the frame
              // This handles the case where a window is filtered out during transitions
              // but React hasn't removed it from the DOM yet.
              // No CSS animation - Rust already faded it to opacity 0 before filtering.
              for (const [id, el] of windowRefsMap.current.entries()) {
                if (!currentWindowIds.has(id)) {
                  // Window was filtered out - hide immediately (Rust already faded it)
                  if (!fadingOutWindowsRef.current.has(id)) {
                    fadingOutWindowsRef.current.add(id);
                    el.style.visibility = 'hidden';
                    
                    // Capture id in closure for the timeout
                    const windowId = id;
                    
                    // Small delay to ensure Rust transition completed, then clean up
                    setTimeout(() => {
                      fadingOutWindowsRef.current.delete(windowId);
                      
                      // If all fade-outs complete and we have pending windows, apply them
                      if (fadingOutWindowsRef.current.size === 0 && pendingWindowsRef.current) {
                        windowIdsRef.current = new Set(pendingWindowsRef.current.map(w => w.id));
                        setWindows(pendingWindowsRef.current);
                        pendingWindowsRef.current = null;
                      }
                    }, 50);
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
                  
                  // Clear any CSS animations/transitions - opacity is driven directly by Rust
                  if (el.style.animation !== 'none' && el.style.animation !== '') {
                    el.style.animation = 'none';
                  }
                  if (el.style.transition !== 'none') {
                    el.style.transition = 'none';
                  }
                  
                  // Update previous opacity tracking
                  prevOpacityRef.current.set(win.id, targetOpacity);
                  
                  // GPU-accelerated transform instead of left/top
                  el.style.transform = `translate3d(${win.screenRect.x}px, ${win.screenRect.y}px, 0)`;
                  el.style.width = `${win.screenRect.width}px`;
                  el.style.height = `${win.screenRect.height}px`;
                  el.style.zIndex = String(win.zOrder + 10);
                  
                  // CRITICAL: Override position to absolute if Panel set it to relative
                  if (el.style.position !== 'absolute') {
                    el.style.position = 'absolute';
                    el.style.left = '0';
                    el.style.top = '0';
                  }
                  
                  // Always set opacity directly - Rust controls the smooth transitions
                  el.style.opacity = String(win.opacity);
                }
              }
              
              // Only update React state when window LIST changes (add/remove)
              // This triggers re-render to create/destroy window components
              if (windowListChanged(frame.windows, windowIdsRef.current)) {
                console.log('[Desktop] Window list changed:', frame.windows.map(w => ({ id: w.id, appId: w.appId, state: w.state })));
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
  }, [desktop, backgroundRef, onBackgroundReady]);
  
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
            <AppRouter appId={w.appId} windowId={w.id} processId={w.processId} />
          </WindowContent>
        ))}

      <Taskbar />
    </>
  );
}

// =============================================================================
// DesktopWithPermissions - Inner component that uses permissions hook
// =============================================================================

function DesktopWithPermissions({ 
  supervisor, 
  desktop 
}: DesktopProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const backgroundRef = useRef<DesktopBackgroundType | null>(null);
  const [initialized, setInitialized] = useState(false);
  const [selectionBox, setSelectionBox] = useState<SelectionBox | null>(null);
  const [backgroundMenu, setBackgroundMenu] = useState<BackgroundMenuState>({ x: 0, y: 0, visible: false });
  const [backgrounds, setBackgrounds] = useState<BackgroundInfo[]>([]);
  const [settingsRestored, setSettingsRestored] = useState(false);
  
  // Theme state from zui
  const { theme, accent, setTheme, setAccent } = useTheme();
  
  // Permissions state
  const permissions = usePermissions();
  
  // Window actions (includes launchTerminal for spawning terminal with process)
  const { launchTerminal } = useWindowActions();
  
  // Ref to track current workspace info (updated by render loop in DesktopInner)
  const workspaceInfoRef = useRef<WorkspaceInfo | null>(null);

  // Initialize desktop engine
  useEffect(() => {
    if (initialized) return;

    const container = containerRef.current;
    if (!container) return;

    const rect = container.getBoundingClientRect();
    desktop.init(rect.width, rect.height);
    
    // NOTE: Terminal is NOT auto-spawned here.
    // The boot sequence is: init (PID 1) → permission_manager (PID 2) → ready
    // Terminals are spawned on-demand via BeginMenu or 'T' keyboard shortcut,
    // which ensures permission_manager is ready before any terminal processes.
    
    setInitialized(true);
  }, [desktop, initialized]);

  // Handle resize
  useEffect(() => {
    if (!initialized) return;

    const handleResize = () => {
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

    const checkOrphanedWindows = () => {
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
        // Note: UI-only apps (like Permissions) have no backing process, so processId is null/undefined
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
      desktop.pointer_move(e.clientX, e.clientY);
    };

    const handleGlobalPointerUp = () => {
      desktop.pointer_up();
    };

    window.addEventListener('pointermove', handleGlobalPointerMove);
    window.addEventListener('pointerup', handleGlobalPointerUp);
    return () => {
      window.removeEventListener('pointermove', handleGlobalPointerMove);
      window.removeEventListener('pointerup', handleGlobalPointerUp);
    };
  }, [desktop, initialized]);

  // Use capture phase for panning so it intercepts before windows
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const handleCapturePointerDown = (e: PointerEvent) => {
      const isPanGesture = e.button === 1 || (e.button === 0 && (e.ctrlKey || e.shiftKey));
      if (isPanGesture) {
        const result = JSON.parse(
          desktop.pointer_down(e.clientX, e.clientY, e.button, e.ctrlKey, e.shiftKey)
        );
        if (result.type === 'handled') {
          e.preventDefault();
          e.stopPropagation();
        }
      }
    };

    container.addEventListener('pointerdown', handleCapturePointerDown, { capture: true });
    return () => container.removeEventListener('pointerdown', handleCapturePointerDown, { capture: true });
  }, [desktop]);

  // Callback when background renderer is ready
  const handleBackgroundReady = useCallback(() => {
    if (backgroundRef.current) {
      try {
        const availableJson = backgroundRef.current.get_available_backgrounds();
        const available = JSON.parse(availableJson) as BackgroundInfo[];
        setBackgrounds(available);
        setSettingsRestored(true);
      } catch (e) {
        console.error('[desktop] Failed to initialize backgrounds:', e);
      }
    }
  }, []);

  // Get active background from workspace info
  const getActiveBackground = useCallback(() => {
    if (workspaceInfoRef.current) {
      return workspaceInfoRef.current.backgrounds[workspaceInfoRef.current.actualActive] || 'grain';
    }
    return 'grain';
  }, []);

  // Set background for active desktop - updates Rust state which will sync to renderer
  const setBackground = useCallback((id: string) => {
    const activeIndex = desktop.get_active_desktop();
    desktop.set_desktop_background(activeIndex, id);
    // Background renderer will sync automatically via render loop
  }, [desktop]);

  // Forward pointer events to Rust (bubble phase for normal interactions)
  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      // Don't process if the event is from the background menu (it stops propagation)
      // Close background menu only if clicking directly on the desktop or canvas
      const target = e.target as HTMLElement;
      const isDesktopClick = target === containerRef.current || target.tagName === 'CANVAS';
      
      if (backgroundMenu.visible && isDesktopClick) {
        setBackgroundMenu({ ...backgroundMenu, visible: false });
        return; // Don't process further, just close the menu
      }

      const result = JSON.parse(
        desktop.pointer_down(e.clientX, e.clientY, e.button, e.ctrlKey, e.shiftKey)
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
    [desktop, backgroundMenu]
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent) => {
      desktop.pointer_move(e.clientX, e.clientY);

      if (selectionBox) {
        setSelectionBox((prev) =>
          prev ? { ...prev, currentX: e.clientX, currentY: e.clientY } : null
        );
      }
    },
    [desktop, selectionBox]
  );

  const handlePointerUp = useCallback(() => {
    desktop.pointer_up();
    setSelectionBox(null);
  }, [desktop]);

  const handlePointerLeave = useCallback(() => {
    desktop.pointer_up();
    setSelectionBox(null);
  }, [desktop]);

  const handleWheel = useCallback(
    (e: React.WheelEvent) => {
      if (e.ctrlKey) {
        desktop.wheel(
          e.deltaX,
          e.deltaY,
          e.clientX,
          e.clientY,
          e.ctrlKey
        );
      }
    },
    [desktop]
  );

  // Handle right-click for background menu
  const handleContextMenu = useCallback(
    (e: React.MouseEvent) => {
      // Only show background menu when right-clicking on the desktop background itself
      if (e.target === containerRef.current || (e.target as HTMLElement).tagName === 'CANVAS') {
        e.preventDefault();
        setBackgroundMenu({
          x: e.clientX,
          y: e.clientY,
          visible: true,
        });
      }
    },
    []
  );

  const closeBackgroundMenu = useCallback(() => {
    setBackgroundMenu({ ...backgroundMenu, visible: false });
  }, [backgroundMenu]);

  const handleBackgroundSelect = useCallback((id: string) => {
    setBackground(id);
    closeBackgroundMenu();
  }, [setBackground, closeBackgroundMenu]);

  // Close background menu when clicking outside
  useEffect(() => {
    if (!backgroundMenu.visible) return;

    const handleClickOutside = (e: MouseEvent) => {
      closeBackgroundMenu();
    };

    // Small delay to prevent immediate close on right-click
    const timeoutId = setTimeout(() => {
      document.addEventListener('click', handleClickOutside);
    }, 10);

    return () => {
      clearTimeout(timeoutId);
      document.removeEventListener('click', handleClickOutside);
    };
  }, [backgroundMenu.visible, closeBackgroundMenu]);

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
      
      // T key: Create new terminal with its own process
      if (e.key === 't' || e.key === 'T') {
        e.preventDefault();
        launchTerminal();
        return;
      }
      
      // C key: Close focused window
      if (e.key === 'c' || e.key === 'C') {
        e.preventDefault();
        try {
          const focusedId = desktop.get_focused_window();
          if (focusedId !== undefined) {
            // Get the process ID associated with this window (if any)
            // Note: Returns BigInt (u64) from Rust, or undefined
            const processId = desktop.get_window_process_id(BigInt(focusedId));
            
            // Close the window
            desktop.close_window(BigInt(focusedId));
            
            // Kill the associated process if it exists
            // Note: kill_process takes u64 (BigInt), processId is already BigInt from Rust
            if (processId !== undefined && supervisor) {
              supervisor.kill_process(processId);
            }
          }
        } catch {
          // Ignore errors during window close
        }
        return;
      }
      
      // Ctrl+` (backtick) or F3: Toggle void view
      if ((e.ctrlKey && e.key === '`') || e.key === 'F3') {
        e.preventDefault();
        try {
          const viewMode = desktop.get_view_mode();
          // Accept both 'desktop' and legacy 'workspace' for entering void
          if (viewMode === 'desktop' || viewMode === 'workspace') {
            desktop.enter_void();
          } else if (viewMode === 'void') {
            desktop.exit_void(desktop.get_active_desktop());
          }
        } catch {
          // Ignore errors during view mode toggle
        }
        return;
      }

      // Arrow keys: Cycle between windows (without Ctrl) or desktops (with Ctrl)
      if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
        e.preventDefault();
        
        if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey) {
          // Ctrl+Arrow: Switch desktops
          try {
            const desktops = JSON.parse(desktop.get_desktops_json()) as Array<{ id: number }>;
            const count = desktops.length;
            if (count <= 1) return;

            const current = desktop.get_active_desktop();
            const next = e.key === 'ArrowLeft'
              ? (current > 0 ? current - 1 : count - 1)
              : (current < count - 1 ? current + 1 : 0);

            // If in void, exit to target desktop; otherwise switch
            if (desktop.get_view_mode() === 'void') {
              desktop.exit_void(next);
            } else {
              desktop.switch_desktop(next);
            }
          } catch {
            // Ignore errors during desktop switch
          }
        } else if (!e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey) {
          // Arrow only: Cycle between windows on current desktop
          try {
            const windowsJson = desktop.get_windows_json();
            const windows = JSON.parse(windowsJson) as Array<{ id: number; state: string; zOrder: number }>;
            
            // Filter to only visible windows (not minimized)
            // Windows are already sorted by ID (creation order) from get_windows_json
            // This matches the order shown in the taskbar (left to right)
            const visibleWindows = windows.filter(w => w.state !== 'minimized');
            
            if (visibleWindows.length === 0) {
              return;
            }
            
            const focusedId = desktop.get_focused_window();
            // Convert BigInt to number for comparison
            const focusedIdNum = focusedId !== undefined ? Number(focusedId) : undefined;
            const currentIndex = visibleWindows.findIndex(w => w.id === focusedIdNum);
            
            let nextIndex;
            if (currentIndex === -1) {
              // No window focused, focus the first window (leftmost in taskbar)
              nextIndex = 0;
            } else if (e.key === 'ArrowLeft') {
              // Previous window (left in taskbar = lower ID)
              nextIndex = currentIndex > 0 ? currentIndex - 1 : visibleWindows.length - 1;
            } else {
              // Next window (right in taskbar = higher ID)
              nextIndex = currentIndex < visibleWindows.length - 1 ? currentIndex + 1 : 0;
            }
            
            const nextWindow = visibleWindows[nextIndex];
            
            // Focus and pan to the next window
            desktop.focus_window(BigInt(nextWindow.id));
            desktop.pan_to_window(BigInt(nextWindow.id));
          } catch (err) {
            console.error('[Desktop] Error during window cycling:', err);
          }
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [initialized, desktop, launchTerminal]);

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
            <div
              className={styles.contextMenu}
              style={{ 
                position: 'fixed',
                left: backgroundMenu.x,
                top: backgroundMenu.y,
                zIndex: 10000,
              }}
              onClick={(e) => e.stopPropagation()}
              onMouseDown={(e) => e.stopPropagation()}
            >
              <Menu
                items={[
                  {
                    id: 'background',
                    label: 'Background',
                    children: backgrounds.map((bg) => ({
                      id: `bg:${bg.id}`,
                      label: bg.name,
                    })),
                  },
                  {
                    id: 'theme',
                    label: 'Theme',
                    children: THEMES.map((t) => ({
                      id: `theme:${t}`,
                      label: THEME_LABELS[t],
                    })),
                  },
                  {
                    id: 'accent',
                    label: 'Accent',
                    children: ACCENT_COLORS.map((c) => ({
                      id: `accent:${c}`,
                      label: ACCENT_LABELS[c],
                    })),
                  },
                ] as MenuItem[]}
                value={[
                  `bg:${getActiveBackground()}`,
                  `theme:${theme}`,
                  `accent:${accent}`,
                ]}
                onChange={(id) => {
                  const [category, value] = id.split(':');
                  if (category === 'bg' && value) {
                    handleBackgroundSelect(value);
                  } else if (category === 'theme' && value) {
                    setTheme(value as Theme);
                    closeBackgroundMenu();
                  } else if (category === 'accent' && value) {
                    setAccent(value as AccentColor);
                    closeBackgroundMenu();
                  }
                }}
                variant="glass"
                border="future"
                rounded="md"
                width={200}
              />
            </div>
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

// =============================================================================
// Desktop - Main export with provider wrappers
// =============================================================================

export function Desktop({ supervisor, desktop }: DesktopProps) {
  return (
    <SupervisorProvider value={supervisor}>
      <DesktopControllerProvider value={desktop}>
        <DesktopWithPermissions supervisor={supervisor} desktop={desktop} />
      </DesktopControllerProvider>
    </SupervisorProvider>
  );
}
