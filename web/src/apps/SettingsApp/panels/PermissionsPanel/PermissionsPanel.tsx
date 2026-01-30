import { useState, useCallback, useEffect, useMemo } from 'react';
import { GroupCollapsible, Menu, type MenuItem, Label, type PanelDrillItem } from '@cypher-asi/zui';
import { useSupervisor } from '@desktop/hooks/useSupervisor';
import { withSupervisorGuard } from '@desktop/main';
import type { CapabilityInfo, ObjectType, AppManifest } from '@/types/permissions';
import { usePanelDrillOptional } from '../../context';
import { ProcessCapabilitiesPanel } from '../ProcessCapabilitiesPanel';
import { Terminal, Clock, Calculator, Zap, Moon, Smartphone, ChevronRight } from 'lucide-react';
import styles from './PermissionsPanel.module.css';

// =============================================================================
// Types
// =============================================================================

interface ProcessWithCapabilities {
  pid: number;
  name: string;
  state: string;
  capabilities: {
    slot: number;
    objectType: ObjectType;
    permissions: { read: boolean; write: boolean; grant: boolean };
  }[];
}

interface AppWithPermissions {
  manifest: AppManifest;
  pid: number;
  grantedCaps: CapabilityInfo[];
}

interface PermissionsPanelProps {
  onDrillDown?: (item: PanelDrillItem) => void;
}

// =============================================================================
// Helper Functions
// =============================================================================

function getAppIcon(name: string): React.ReactNode {
  const lowerName = name.toLowerCase();
  if (lowerName.includes('terminal')) return <Terminal size={14} />;
  if (lowerName.includes('clock')) return <Clock size={14} />;
  if (lowerName.includes('calculator')) return <Calculator size={14} />;
  if (lowerName.includes('init')) return <Zap size={14} />;
  if (lowerName.includes('idle')) return <Moon size={14} />;
  return <Smartphone size={14} />;
}

/**
 * Convert process data from supervisor to app manifest format
 */
function processToApp(proc: ProcessWithCapabilities): AppWithPermissions {
  // Determine if this is a factory (system) app
  const isFactory =
    proc.name === 'init' ||
    proc.name === 'terminal' ||
    proc.name === 'clock' ||
    proc.name === 'calculator';

  return {
    manifest: {
      id: `com.zero.${proc.name}`,
      name: proc.name.charAt(0).toUpperCase() + proc.name.slice(1),
      version: '1.0.0',
      description: `${proc.name} process`,
      capabilities: proc.capabilities.map((cap) => ({
        objectType: cap.objectType,
        permissions: cap.permissions,
        reason: `${cap.objectType} access`,
        required: true,
      })),
      isFactory,
    },
    pid: proc.pid,
    grantedCaps: proc.capabilities.map((cap) => ({
      slot: cap.slot,
      objectType: cap.objectType,
      permissions: cap.permissions,
    })),
  };
}

// =============================================================================
// PermissionsPanel Component
// =============================================================================

/**
 * Permissions Settings Panel
 * - Shows running apps with their capabilities
 * - Drill-down into individual app to manage capabilities
 *
 * Navigation:
 * - Uses PanelDrill context when available (preferred)
 * - Falls back to onDrillDown prop for backwards compatibility
 */
export function PermissionsPanel({ onDrillDown }: PermissionsPanelProps) {
  const supervisor = useSupervisor();
  const panelDrill = usePanelDrillOptional();
  const [processes, setProcesses] = useState<ProcessWithCapabilities[]>([]);
  const [refreshKey, setRefreshKey] = useState(0);

  // Trigger refresh
  const triggerRefresh = useCallback(() => {
    setRefreshKey((k) => k + 1);
  }, []);

  // Fetch real process data from supervisor
  useEffect(() => {
    if (!supervisor) {
      console.log('[PermissionsPanel] No supervisor available');
      return;
    }

    const fetchProcesses = () => {
      // Use supervisor guard to prevent "recursive use of an object" errors
      withSupervisorGuard(() => {
        try {
          // Check if the method exists
          if (typeof supervisor.get_processes_with_capabilities_json !== 'function') {
            console.error(
              '[PermissionsPanel] get_processes_with_capabilities_json is not a function - WASM may need rebuild'
            );
            // Fall back to get_process_list_json if available
            if (typeof supervisor.get_process_list_json === 'function') {
              const json = supervisor.get_process_list_json();
              const data = JSON.parse(json);
              const converted: ProcessWithCapabilities[] = data.map((p: unknown) => {
                const proc = p as { pid: number; name: string; state: string };
                return {
                  pid: proc.pid,
                  name: proc.name,
                  state: proc.state,
                  capabilities: [],
                };
              });
              const filtered = converted.filter((p) => p.name !== 'idle' && p.state !== 'Zombie');
              setProcesses(filtered);
            }
            return;
          }

          const json = supervisor.get_processes_with_capabilities_json();
          const data: ProcessWithCapabilities[] = JSON.parse(json);
          // Filter out system processes we don't want to show (like idle)
          const filtered = data.filter((p) => p.name !== 'idle' && p.state !== 'Zombie');
          setProcesses(filtered);
        } catch (e) {
          console.error('[PermissionsPanel] Failed to fetch processes:', e);
        }
      });
    };

    // Initial fetch
    fetchProcesses();

    // Poll for updates every 2 seconds
    const interval = setInterval(fetchProcesses, 2000);
    return () => clearInterval(interval);
  }, [supervisor, refreshKey]);

  // Convert processes to app format
  const apps = useMemo(() => {
    return processes.map(processToApp);
  }, [processes]);

  // Handle selecting an app - drill down to show capabilities
  const handleSelectApp = useCallback(
    (app: AppWithPermissions) => {
      const drillItem: PanelDrillItem = {
        id: String(app.pid),
        label: app.manifest.name,
        content: (
          <ProcessCapabilitiesPanel app={app} supervisor={supervisor} onRefresh={triggerRefresh} />
        ),
      };

      if (panelDrill) {
        panelDrill.pushPanel(drillItem);
      } else if (onDrillDown) {
        onDrillDown(drillItem);
      }
    },
    [panelDrill, onDrillDown, supervisor, triggerRefresh]
  );

  // Handle menu selection
  const handleMenuSelect = useCallback(
    (id: string) => {
      const app = apps.find((a) => String(a.pid) === id);
      if (app) {
        handleSelectApp(app);
      }
    },
    [apps, handleSelectApp]
  );

  // Build menu items from apps
  const menuItems: MenuItem[] = useMemo(() => {
    return apps.map((app) => ({
      id: String(app.pid),
      label: app.manifest.name,
      icon: getAppIcon(app.manifest.name),
      status: app.manifest.isFactory ? (
        <div className={styles.menuStatus}>
          <Label size="xs" variant="success">
            System
          </Label>
          <span>{app.grantedCaps.length} caps</span>
        </div>
      ) : (
        `${app.grantedCaps.length} caps`
      ),
      endIcon: <ChevronRight size={14} />,
    }));
  }, [apps]);

  return (
    <div className={styles.panelContainer}>
      <GroupCollapsible
        title="Active Processes"
        count={apps.length}
        defaultOpen
        className={styles.collapsibleSection}
      >
        <div className={styles.menuContent}>
          <Menu items={menuItems} onChange={handleMenuSelect} background="none" border="none" />
        </div>
      </GroupCollapsible>
    </div>
  );
}
