import { useState, useCallback, useEffect, useMemo } from 'react';
import { Card, CardItem, Panel, Text, Label, Button } from '@cypher-asi/zui';
import { AppPermissions, type CapabilityInfo, type ObjectType } from '../../components/AppPermissions';
import type { AppManifest } from '../../components/PermissionDialog';
import { useSupervisor } from '../../desktop/hooks/useSupervisor';
import {
  Shield,
  Lock,
  Terminal,
  Clock,
  Calculator,
  Zap,
  Moon,
  Smartphone,
  ArrowLeftRight,
  Keyboard,
  HardDrive,
  Globe,
  Cog,
  Cpu,
  ArrowLeft,
} from 'lucide-react';
import styles from './PermissionsApp.module.css';

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

// =============================================================================
// Helper Functions
// =============================================================================

function getAppIcon(name: string): React.ReactNode {
  const lowerName = name.toLowerCase();
  if (lowerName.includes('terminal')) return <Terminal size={18} />;
  if (lowerName.includes('clock')) return <Clock size={18} />;
  if (lowerName.includes('calculator')) return <Calculator size={18} />;
  if (lowerName.includes('init')) return <Zap size={18} />;
  if (lowerName.includes('idle')) return <Moon size={18} />;
  return <Smartphone size={18} />;
}

function getObjectTypeIcon(type: ObjectType): React.ReactNode {
  switch (type) {
    case 'Endpoint': return <ArrowLeftRight size={12} />;
    case 'Console': return <Keyboard size={12} />;
    case 'Storage': return <HardDrive size={12} />;
    case 'Network': return <Globe size={12} />;
    case 'Process': return <Cog size={12} />;
    case 'Memory': return <Cpu size={12} />;
    default: return null;
  }
}

function formatObjectType(type: ObjectType): string {
  switch (type) {
    case 'Endpoint': return 'IPC Endpoint';
    case 'Console': return 'Console Access';
    case 'Storage': return 'Storage';
    case 'Network': return 'Network';
    case 'Process': return 'Process Management';
    case 'Memory': return 'Memory Access';
    default: return type;
  }
}

function formatPermissions(perms: { read: boolean; write: boolean; grant: boolean }): string {
  const parts: string[] = [];
  if (perms.read) parts.push('R');
  if (perms.write) parts.push('W');
  if (perms.grant) parts.push('G');
  return parts.join('/') || 'none';
}

/**
 * Convert process data from supervisor to app manifest format
 */
function processToApp(proc: ProcessWithCapabilities): AppWithPermissions {
  // Determine if this is a factory (system) app
  const isFactory = proc.name === 'init' || proc.name === 'terminal' || 
                    proc.name === 'clock' || proc.name === 'calculator';
  
  return {
    manifest: {
      id: `com.orbital.${proc.name}`,
      name: proc.name.charAt(0).toUpperCase() + proc.name.slice(1),
      version: '1.0.0',
      description: `${proc.name} process`,
      capabilities: proc.capabilities.map(cap => ({
        objectType: cap.objectType,
        permissions: cap.permissions,
        reason: `${cap.objectType} access`,
        required: true,
      })),
      isFactory,
    },
    pid: proc.pid,
    grantedCaps: proc.capabilities.map(cap => ({
      slot: cap.slot,
      objectType: cap.objectType,
      permissions: cap.permissions,
    })),
  };
}

// =============================================================================
// PermissionsApp Component
// =============================================================================

export function PermissionsApp() {
  const supervisor = useSupervisor();
  const [processes, setProcesses] = useState<ProcessWithCapabilities[]>([]);
  const [selectedApp, setSelectedApp] = useState<AppWithPermissions | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);

  // Fetch real process data from supervisor
  useEffect(() => {
    if (!supervisor) {
      console.log('[PermissionsApp] No supervisor available');
      return;
    }

    const fetchProcesses = () => {
      try {
        // Check if the method exists
        if (typeof supervisor.get_processes_with_capabilities_json !== 'function') {
          console.error('[PermissionsApp] get_processes_with_capabilities_json is not a function - WASM may need rebuild');
          // Fall back to get_process_list_json if available
          if (typeof supervisor.get_process_list_json === 'function') {
            const json = supervisor.get_process_list_json();
            console.log('[PermissionsApp] Fallback process list:', json);
            const data = JSON.parse(json);
            const converted: ProcessWithCapabilities[] = data.map((p: any) => ({
              pid: p.pid,
              name: p.name,
              state: p.state,
              capabilities: [],
            }));
            const filtered = converted.filter(p => p.name !== 'idle' && p.state !== 'Zombie');
            setProcesses(filtered);
          }
          return;
        }

        const json = supervisor.get_processes_with_capabilities_json();
        console.log('[PermissionsApp] Processes with caps:', json);
        const data: ProcessWithCapabilities[] = JSON.parse(json);
        // Filter out system processes we don't want to show (like idle)
        const filtered = data.filter(p => p.name !== 'idle' && p.state !== 'Zombie');
        setProcesses(filtered);
      } catch (e) {
        console.error('[PermissionsApp] Failed to fetch processes:', e);
      }
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

  // Update selectedApp when processes change
  useEffect(() => {
    if (selectedApp) {
      const updated = apps.find(a => a.pid === selectedApp.pid);
      if (updated) {
        setSelectedApp(updated);
      } else {
        setSelectedApp(null);
      }
    }
  }, [apps, selectedApp?.pid]);

  // Calculate stats
  const totalApps = apps.length;
  const totalCapabilities = apps.reduce((sum, app) => sum + app.grantedCaps.length, 0);

  // Handle revoking a capability
  const handleRevoke = useCallback((objectType: ObjectType) => {
    if (!selectedApp || !supervisor) return;

    // Find the capability slot for this object type
    const cap = selectedApp.grantedCaps.find(c => c.objectType === objectType);
    if (!cap) return;

    // Use direct supervisor API to revoke capability from the target process
    // Note: pid must be BigInt for wasm-bindgen u64 parameter
    const success = supervisor.revoke_capability(BigInt(selectedApp.pid), cap.slot);
    if (success) {
      console.log(`[PermissionsApp] Revoked ${objectType} from PID ${selectedApp.pid} slot ${cap.slot}`);
    } else {
      console.error(`[PermissionsApp] Failed to revoke ${objectType} from PID ${selectedApp.pid}`);
    }

    // Trigger refresh
    setRefreshKey(k => k + 1);
  }, [selectedApp, supervisor]);

  // Handle revoking all capabilities
  const handleRevokeAll = useCallback(() => {
    if (!selectedApp || !supervisor) return;

    // Revoke each capability using direct supervisor API
    // Note: pid must be BigInt for wasm-bindgen u64 parameter
    const pidBigInt = BigInt(selectedApp.pid);
    let successCount = 0;
    for (const cap of selectedApp.grantedCaps) {
      if (supervisor.revoke_capability(pidBigInt, cap.slot)) {
        successCount++;
      }
    }
    console.log(`[PermissionsApp] Revoked ${successCount}/${selectedApp.grantedCaps.length} caps from PID ${selectedApp.pid}`);

    // Trigger refresh
    setRefreshKey(k => k + 1);
  }, [selectedApp, supervisor]);

  return (
    <div className={styles.container}>
      {/* Header */}
      <div className={styles.header}>
        <div className={styles.title}>
          <div className={styles.titleIcon}>
            <Shield size={16} />
          </div>
          <Text as="span" size="lg" weight="semibold">
            Capabilities
          </Text>
        </div>
        <div className={styles.stats}>
          <div className={styles.stat}>
            <span className={styles.statValue}>{totalApps}</span>
            <span className={styles.statLabel}>apps</span>
          </div>
          <div className={styles.stat}>
            <span className={styles.statValue}>{totalCapabilities}</span>
            <span className={styles.statLabel}>capabilities</span>
          </div>
        </div>
      </div>

      {/* Content */}
      <div className={styles.content}>
        {apps.length === 0 ? (
          <div className={styles.emptyState}>
            <Lock size={48} strokeWidth={1} />
            <div className={styles.emptyTitle}>No Apps</div>
            <div className={styles.emptyText}>
              No applications are currently running.
            </div>
          </div>
        ) : (
          <Card className={styles.appList}>
            {apps.map((app) => (
              <CardItem
                key={app.pid}
                icon={getAppIcon(app.manifest.name)}
                title={
                  <span className={styles.appName}>
                    {app.manifest.name}
                    {app.manifest.isFactory && (
                      <Label size="xs" variant="success">System</Label>
                    )}
                    <Label size="xs" variant="default">PID {app.pid}</Label>
                  </span>
                }
                description={app.manifest.id}
                onClick={() => setSelectedApp(app)}
                className={styles.appCard}
              >
                <div className={styles.permissionTags}>
                  {app.grantedCaps.length === 0 ? (
                    <span className={styles.noPermissions}>No capabilities</span>
                  ) : (
                    app.grantedCaps.map((cap) => (
                      <div key={`${cap.objectType}-${cap.slot}`} className={styles.permissionTag}>
                        <span className={styles.permissionTagIcon}>
                          {getObjectTypeIcon(cap.objectType)}
                        </span>
                        <span>{formatObjectType(cap.objectType)}</span>
                        <span>({formatPermissions(cap.permissions)})</span>
                      </div>
                    ))
                  )}
                </div>
              </CardItem>
            ))}
          </Card>
        )}
      </div>

      {/* Detail Panel */}
      {selectedApp && (
        <Panel variant="glass" border="none" className={styles.detailPanel}>
          <div className={styles.detailHeader}>
            <Button
              variant="ghost"
              size="sm"
              iconOnly
              onClick={() => setSelectedApp(null)}
            >
              <ArrowLeft size={16} />
            </Button>
            <div className={styles.appIcon}>{getAppIcon(selectedApp.manifest.name)}</div>
            <div className={styles.appInfo}>
              <div className={styles.appName}>
                {selectedApp.manifest.name}
                {selectedApp.manifest.isFactory && (
                  <Label size="xs" variant="success">System</Label>
                )}
              </div>
              <div className={styles.appId}>{selectedApp.manifest.id}</div>
            </div>
          </div>
          <div className={styles.detailContent}>
            <AppPermissions
              app={selectedApp.manifest}
              grantedCaps={selectedApp.grantedCaps}
              onRevoke={handleRevoke}
              onRevokeAll={selectedApp.grantedCaps.length > 1 ? handleRevokeAll : undefined}
            />
          </div>
        </Panel>
      )}
    </div>
  );
}
