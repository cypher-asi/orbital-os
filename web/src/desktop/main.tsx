import { StrictMode, useState, useEffect, useRef } from 'react';
import { createRoot } from 'react-dom/client';
import { ThemeProvider } from '@cypher-asi/zui';
import { Desktop } from './Desktop';
import { OSLoading } from './OSLoading';
import { Supervisor, DesktopController } from './hooks/useSupervisor';
import { useSettingsStore } from '@/stores';
import '@cypher-asi/zui/styles';
import '@/styles/global.css';

// Boot progress tracking
const BOOT_STEPS = {
  WASM_LOAD: 15,
  WASM_INIT: 30,
  SUPERVISOR: 40,
  DESKTOP: 50,
  AXIOM: 60,
  NETWORK: 75,
  KERNEL_BOOT: 85,
  INIT_SPAWN: 100,
};

// Global cleanup state - accessible from beforeunload which fires before React unmount
interface CleanupState {
  supervisor: (Supervisor & { free?: () => void }) | null;
  desktop: (DesktopController & { free?: () => void }) | null;
  pollIntervalId: ReturnType<typeof setInterval> | null;
  axiomIntervalId: ReturnType<typeof setInterval> | null;
  cleaned: boolean;
}

const cleanupState: CleanupState = {
  supervisor: null,
  desktop: null,
  pollIntervalId: null,
  axiomIntervalId: null,
  cleaned: false,
};

// Cleanup for page unload - only terminates workers, does NOT free WASM memory
// (browser handles memory cleanup on unload, and .free() causes closure errors)
function performUnloadCleanup() {
  console.log('[main] Page unloading, terminating workers...');

  // Clear intervals first to stop any callbacks
  if (cleanupState.pollIntervalId !== null) {
    clearInterval(cleanupState.pollIntervalId);
    cleanupState.pollIntervalId = null;
  }
  if (cleanupState.axiomIntervalId !== null) {
    clearInterval(cleanupState.axiomIntervalId);
    cleanupState.axiomIntervalId = null;
  }

  // Kill all worker processes (terminates Web Workers)
  if (cleanupState.supervisor) {
    try {
      cleanupState.supervisor.kill_all_processes();
      console.log('[main] All processes killed');
    } catch (e) {
      console.warn('[main] Error killing processes:', e);
    }
  }

  // Clear global references that hold supervisor
  if (window.ZosStorage) {
    try {
      window.ZosStorage.initSupervisor(null as unknown as Supervisor);
    } catch (e) {
      // Ignore - page is unloading
    }
  }
  if (window.ZosKeystore) {
    try {
      window.ZosKeystore.initSupervisor(null as unknown as Supervisor);
    } catch (e) {
      // Ignore - page is unloading
    }
  }
  if (window.ZosNetwork) {
    try {
      window.ZosNetwork.initSupervisor(null as unknown as Supervisor);
    } catch (e) {
      // Ignore - page is unloading
    }
  }
}

// Full cleanup for HMR/React unmount - can safely free WASM memory
function performFullCleanup() {
  if (cleanupState.cleaned) return;
  cleanupState.cleaned = true;

  console.log('[main] Performing full cleanup (HMR/unmount)...');

  // Clear intervals first
  if (cleanupState.pollIntervalId !== null) {
    clearInterval(cleanupState.pollIntervalId);
    cleanupState.pollIntervalId = null;
  }
  if (cleanupState.axiomIntervalId !== null) {
    clearInterval(cleanupState.axiomIntervalId);
    cleanupState.axiomIntervalId = null;
  }

  // Clear global references BEFORE freeing (they may hold callbacks)
  if (window.ZosStorage) {
    window.ZosStorage.initSupervisor(null as unknown as Supervisor);
  }
  if (window.ZosKeystore) {
    window.ZosKeystore.initSupervisor(null as unknown as Supervisor);
  }
  if (window.ZosNetwork) {
    window.ZosNetwork.initSupervisor(null as unknown as Supervisor);
  }

  // Kill all processes
  if (cleanupState.supervisor) {
    try {
      cleanupState.supervisor.kill_all_processes();
    } catch (e) {
      console.warn('[main] Error killing processes:', e);
    }

    // For HMR, we can free WASM memory after a short delay to let pending operations complete
    const supervisorToFree = cleanupState.supervisor;
    const desktopToFree = cleanupState.desktop;
    cleanupState.supervisor = null;
    cleanupState.desktop = null;

    // Delay .free() to allow pending async operations to complete
    setTimeout(() => {
      try {
        if (supervisorToFree && typeof (supervisorToFree as { free?: () => void }).free === 'function') {
          (supervisorToFree as { free: () => void }).free();
          console.log('[main] Supervisor WASM memory freed');
        }
      } catch (e) {
        console.warn('[main] Error freeing supervisor:', e);
      }
      try {
        if (desktopToFree && typeof (desktopToFree as { free?: () => void }).free === 'function') {
          (desktopToFree as { free: () => void }).free();
          console.log('[main] Desktop WASM memory freed');
        }
      } catch (e) {
        console.warn('[main] Error freeing desktop:', e);
      }
    }, 100);
  }

  console.log('[main] Cleanup complete');
}

// Register beforeunload handler at module level (runs before React unmount)
// Only terminate workers - don't free WASM memory (causes closure errors)
window.addEventListener('beforeunload', performUnloadCleanup);
window.addEventListener('pagehide', performUnloadCleanup);

interface BootState {
  progress: number;
  status: string;
  error?: string;
  supervisor?: Supervisor;
  desktop?: DesktopController;
}

// App wrapper component
function App() {
  const [bootState, setBootState] = useState<BootState>({
    progress: 0,
    status: 'Initializing...',
  });
  const initializingRef = useRef(false);
  // Store cleanup function for access from useEffect cleanup
  const cleanupRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    // Prevent double initialization (React StrictMode runs effects twice)
    if (initializingRef.current) return;
    initializingRef.current = true;

    // Initialize the application with progress tracking
    async function init() {
      const updateProgress = (progress: number, status: string) => {
        setBootState((prev) => ({ ...prev, progress, status }));
      };

      const setError = (error: string) => {
        setBootState((prev) => ({ ...prev, error }));
      };

      try {
        updateProgress(BOOT_STEPS.WASM_LOAD, 'Loading WASM modules...');

        // Load both WASM modules in parallel
        const [supervisorModule, desktopModule] = await Promise.all([
          import('../../pkg/supervisor/zos_supervisor.js'),
          import('../../pkg/desktop/zos_desktop.js'),
        ]);

        // Initialize both modules
        await Promise.all([supervisorModule.default(), desktopModule.default()]);

        updateProgress(BOOT_STEPS.WASM_INIT, 'Initializing WASM...');

        updateProgress(BOOT_STEPS.SUPERVISOR, 'Creating supervisor...');
        const supervisor = new supervisorModule.Supervisor() as Supervisor;

        updateProgress(BOOT_STEPS.DESKTOP, 'Creating desktop controller...');
        const desktop = new desktopModule.DesktopController() as DesktopController;

        // Note: Console callbacks are now per-process. Each TerminalApp registers
        // its own callback with register_console_callback(pid, callback).
        // System messages are buffered until a callback is registered.

        // Set up spawn callback for loading WASM processes
        supervisor.set_spawn_callback((procType: string, name: string) => {
          setTimeout(async () => {
            try {
              const wasmFile = procType === 'terminal' ? 'terminal.wasm' : `${procType}.wasm`;
              console.log(`[spawn] Fetching /processes/${wasmFile}...`);

              const response = await fetch(`/processes/${wasmFile}`);
              if (!response.ok) {
                console.error(`[spawn] Fetch failed: ${response.status}`);
                return;
              }

              const binary = new Uint8Array(await response.arrayBuffer());
              console.log(`[spawn] Loaded ${binary.length} bytes, spawning ${name}...`);

              const pid = supervisor.complete_spawn(name, binary);
              console.log(`[spawn] complete_spawn returned PID ${pid}`);
            } catch (e) {
              console.error(`[spawn] Error:`, e);
            }
          }, 0);
        });

        // Initialize Axiom storage
        updateProgress(BOOT_STEPS.AXIOM, 'Initializing Axiom storage...');
        try {
          await supervisor.init_axiom_storage();
          console.log('[main] Axiom IndexedDB storage initialized');
        } catch (e) {
          console.warn('[main] Axiom storage init failed (non-fatal):', e);
        }

        // Initialize ZosStorage with the supervisor reference for async storage callbacks
        // This enables services like IdentityService and VfsService to use storage syscalls
        if (window.ZosStorage) {
          window.ZosStorage.initSupervisor(supervisor);
          console.log('[main] ZosStorage supervisor reference set');
        } else {
          console.warn('[main] ZosStorage not available - storage syscalls will fail');
        }

        // Initialize ZosStorage database (create zos-filesystem IndexedDB and root structure)
        try {
          await supervisor.init_vfs_storage();
          console.log('[main] ZosStorage IndexedDB initialized');
        } catch (e) {
          console.warn('[main] VFS storage init failed (non-fatal):', e);
        }

        // Initialize ZosKeystore with the supervisor reference for key storage callbacks
        // This enables KeyService to use keystore_* syscalls
        if (window.ZosKeystore) {
          window.ZosKeystore.initSupervisor(supervisor);
          console.log('[main] ZosKeystore supervisor reference set');
        } else {
          console.warn('[main] ZosKeystore not available - keystore syscalls will fail');
        }

        // Initialize ZosKeystore database (create zos-keystore IndexedDB if not exists)
        try {
          await supervisor.init_keystore();
          console.log('[main] ZosKeystore IndexedDB initialized');
        } catch (e) {
          console.warn('[main] Keystore init failed (non-fatal):', e);
        }

        // Initialize ZosNetwork with the supervisor reference for async network callbacks
        // This enables network_fetch_async syscalls for HTTP requests (used by identity service for ZID auth)
        updateProgress(BOOT_STEPS.NETWORK, 'Initializing network HAL...');

        // Helper to wait for ZosNetwork with retries
        const waitForZosNetwork = async (maxWaitMs = 2000): Promise<boolean> => {
          const startTime = Date.now();

          // First check if already available
          if (window.ZosNetwork) {
            return true;
          }

          console.log('[main] ZosNetwork not immediately available, waiting...');

          // Try dynamic load if not present
          if (!document.querySelector('script[src*="zos-network.js"]')) {
            const script = document.createElement('script');
            script.src = '/zos-network.js';
            document.head.appendChild(script);
          }

          // Poll for availability
          while (Date.now() - startTime < maxWaitMs) {
            if (window.ZosNetwork) {
              return true;
            }
            await new Promise((resolve) => setTimeout(resolve, 50));
          }

          return false;
        };

        const networkAvailable = await waitForZosNetwork();

        if (networkAvailable && window.ZosNetwork) {
          window.ZosNetwork.initSupervisor(supervisor);
          console.log('[main] ZosNetwork supervisor reference set');
        } else {
          console.error(
            '[main] ZosNetwork not available after waiting - network syscalls will fail'
          );
        }

        // Initialize settings store with supervisor reference
        // This enables time settings sync with time_service when it's running
        useSettingsStore.getState().initializeService(supervisor);
        console.log('[main] Settings store initialized with supervisor');

        // Boot the kernel
        updateProgress(BOOT_STEPS.KERNEL_BOOT, 'Booting kernel...');
        supervisor.boot();

        // Spawn init process
        updateProgress(BOOT_STEPS.INIT_SPAWN, 'Starting init process...');
        supervisor.spawn_init();

        // Store references in global cleanup state for beforeunload access
        cleanupState.supervisor = supervisor;
        cleanupState.desktop = desktop;
        cleanupState.cleaned = false;

        // Start main processing loop
        // Process syscalls from workers using SharedArrayBuffer + Atomics.
        // Workers make syscalls (including SYS_RECEIVE for IPC) which are processed here.
        //
        // Note: deliver_pending_messages() is DEPRECATED and intentionally not called.
        // See crates/zos-supervisor/src/supervisor/ipc.rs for details.
        cleanupState.pollIntervalId = setInterval(() => {
          supervisor.poll_syscalls();
          supervisor.process_worker_messages();
        }, 10);

        // Sync Axiom log periodically
        cleanupState.axiomIntervalId = setInterval(async () => {
          await supervisor.sync_axiom_log();
        }, 2000);

        // Context menu handler (stored locally, not needed in global cleanup)
        const handleContextMenu = (e: Event) => {
          e.preventDefault();
        };

        // Disable browser right-click context menu
        window.addEventListener('contextmenu', handleContextMenu);

        // Store cleanup function for useEffect cleanup and HMR
        cleanupRef.current = () => {
          console.log('[main] React cleanup triggered...');
          window.removeEventListener('contextmenu', handleContextMenu);
          performFullCleanup();
        };

        // Boot complete - update state with supervisor and desktop
        setBootState({
          progress: 100,
          status: 'Ready',
          supervisor,
          desktop,
        });
      } catch (e) {
        console.error('Boot error:', e);
        setError(e instanceof Error ? e.message : String(e));
      }
    }

    init();

    // Cleanup function for React unmount (HMR, navigation, etc.)
    // IMPORTANT: Only reset initializingRef when cleanup actually runs (cleanupRef is set).
    // StrictMode unmounts before init() finishes (cleanupRef is null), so we must NOT
    // reset initializingRef or StrictMode's remount will start a second concurrent init.
    return () => {
      if (cleanupRef.current) {
        cleanupRef.current();
        cleanupRef.current = null;
        // Only reset after cleanup completes - this allows HMR to reinitialize
        initializingRef.current = false;
      }
    };
  }, []);

  // Show loading screen until boot is complete
  if (!bootState.supervisor || !bootState.desktop) {
    return (
      <OSLoading progress={bootState.progress} status={bootState.status} error={bootState.error} />
    );
  }

  // Boot complete - render desktop
  return (
    <ThemeProvider defaultTheme="dark" defaultAccent="cyan">
      <Desktop supervisor={bootState.supervisor} desktop={bootState.desktop} />
    </ThemeProvider>
  );
}

// Start the React app
const root = document.getElementById('root');
if (!root) {
  throw new Error('Root element not found');
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>
);
