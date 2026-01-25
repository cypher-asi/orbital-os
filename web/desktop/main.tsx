import { StrictMode, useState, useEffect, useRef } from 'react';
import { createRoot } from 'react-dom/client';
import { ThemeProvider } from '@cypher-asi/zui';
import { Desktop } from '../components/Desktop/Desktop';
import { OSLoading } from '../components/OSLoading';
import { Supervisor, DesktopController } from './hooks/useSupervisor';
import { useSettingsStore } from '../stores';
import '@cypher-asi/zui/styles';
import '../styles/global.css';

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
          import('../pkg/zos_supervisor_web.js'),
          import('../pkg-desktop/zos_desktop.js'),
        ]);

        // Initialize both modules
        await Promise.all([supervisorModule.default(), desktopModule.default()]);

        updateProgress(BOOT_STEPS.WASM_INIT, 'Initializing WASM...');

        updateProgress(BOOT_STEPS.SUPERVISOR, 'Creating supervisor...');
        const supervisor = new supervisorModule.Supervisor() as Supervisor;

        updateProgress(BOOT_STEPS.DESKTOP, 'Creating desktop controller...');
        const desktop = new desktopModule.DesktopController() as DesktopController;

        // Set up a default console callback to log output before UI is ready
        // TerminalApp will override this when it mounts
        supervisor.set_console_callback((text: string) => {
          console.log('[console-fallback]', JSON.stringify(text));
        });

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

        // Start main processing loop
        // Process syscalls from workers using SharedArrayBuffer + Atomics.
        // Workers make syscalls (including SYS_RECEIVE for IPC) which are processed here.
        //
        // Note: deliver_pending_messages() is DEPRECATED and intentionally not called.
        // See crates/zos-supervisor-web/src/supervisor/ipc.rs for details.
        setInterval(() => {
          supervisor.poll_syscalls();
          supervisor.process_worker_messages();
        }, 10);

        // Sync Axiom log periodically
        setInterval(async () => {
          await supervisor.sync_axiom_log();
        }, 2000);

        // Clean up all processes when page unloads/reloads
        window.addEventListener('beforeunload', () => {
          console.log('[main] Page unloading, cleaning up all processes...');
          supervisor.kill_all_processes();
        });

        // Disable browser right-click context menu
        window.addEventListener('contextmenu', (e) => {
          e.preventDefault();
        });

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
