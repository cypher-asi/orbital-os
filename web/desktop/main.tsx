import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { ThemeProvider } from '@cypher-asi/zui';
import { Desktop } from '../components/Desktop/Desktop';
import { Supervisor, DesktopController } from './hooks/useSupervisor';
import { useSettingsStore } from '../stores';
import '@cypher-asi/zui/styles';
import '../styles/global.css';

// Loading state management
function showLoading(message: string) {
  const status = document.getElementById('loading-status');
  if (status) status.textContent = message;
}

function hideLoading() {
  const loading = document.getElementById('loading');
  if (loading) loading.classList.add('hidden');
}

function showError(message: string) {
  const loading = document.getElementById('loading');
  const status = document.getElementById('loading-status');
  if (loading && status) {
    status.innerHTML = `<span style="color: #f87171">Error: ${message}</span>`;
  }
}

// Initialize the application
async function init() {
  try {
    showLoading('Loading WASM modules...');

    // Load both WASM modules in parallel
    const [supervisorModule, desktopModule] = await Promise.all([
      import('../pkg/zos_supervisor_web.js'),
      import('../pkg-desktop/zos_desktop.js'),
    ]);

    // Initialize both modules
    await Promise.all([
      supervisorModule.default(),
      desktopModule.default(),
    ]);

    showLoading('Creating supervisor...');
    const supervisor = new supervisorModule.Supervisor() as Supervisor;

    showLoading('Creating desktop controller...');
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
    showLoading('Initializing Axiom storage...');
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

    // Initialize settings store with supervisor reference
    // This enables time settings sync with time_service when it's running
    useSettingsStore.getState().initializeService(supervisor);
    console.log('[main] Settings store initialized with supervisor');

    // Boot the kernel
    showLoading('Booting kernel...');
    supervisor.boot();

    // Spawn init process
    showLoading('Starting init process...');
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

    // Render React app
    hideLoading();

    const root = document.getElementById('root');
    if (!root) {
      throw new Error('Root element not found');
    }

    createRoot(root).render(
      <StrictMode>
        <ThemeProvider defaultTheme="dark" defaultAccent="cyan">
          <Desktop supervisor={supervisor} desktop={desktop} />
        </ThemeProvider>
      </StrictMode>
    );
  } catch (e) {
    console.error('Boot error:', e);
    showError(e instanceof Error ? e.message : String(e));
  }
}

// Start initialization
init();
