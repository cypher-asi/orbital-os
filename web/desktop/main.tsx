import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { Desktop } from '../components/Desktop/Desktop';
import { Supervisor } from './hooks/useSupervisor';
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
    showLoading('Loading WASM module...');

    // Dynamic import of the WASM module
    const { default: wasmInit, Supervisor: WasmSupervisor } = await import('../pkg/orbital_web.js');
    await wasmInit();

    showLoading('Creating supervisor...');
    const supervisor = new WasmSupervisor() as Supervisor;

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

    // Boot the kernel
    showLoading('Booting kernel...');
    supervisor.boot();

    // Spawn init process
    showLoading('Starting init process...');
    supervisor.spawn_init();

    // Start main processing loop
    setInterval(() => {
      supervisor.poll_syscalls();
      supervisor.process_worker_messages();
      supervisor.deliver_pending_messages();
    }, 10);

    // Sync Axiom log periodically
    setInterval(async () => {
      await supervisor.sync_axiom_log();
    }, 2000);

    // Render React app
    hideLoading();

    const root = document.getElementById('root');
    if (!root) {
      throw new Error('Root element not found');
    }

    createRoot(root).render(
      <StrictMode>
        <Desktop supervisor={supervisor} />
      </StrictMode>
    );
  } catch (e) {
    console.error('Boot error:', e);
    showError(e instanceof Error ? e.message : String(e));
  }
}

// Start initialization
init();
