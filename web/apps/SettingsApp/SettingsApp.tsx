import { useState, useCallback, useMemo, useRef, useEffect } from 'react';
import {
  Panel,
  Navigator,
  type NavigatorItem,
  PanelDrill,
  type PanelDrillItem,
  ButtonPlus,
} from '@cypher-asi/zui';
import { Clock, User, Shield, Palette, Network } from 'lucide-react';
import { GeneralPanel } from './panels/GeneralPanel';
import { IdentitySettingsPanel } from './panels/IdentitySettingsPanel';
import { PermissionsPanel } from './panels/PermissionsPanel';
import { ThemePanel } from './panels/ThemePanel';
import { NetworkPanel } from './panels/NetworkPanel';
import { NeuralKeyPanel } from './panels/NeuralKeyPanel';
import { MachineKeysPanel } from './panels/MachineKeysPanel';
import { LinkedAccountsPanel } from './panels/LinkedAccountsPanel';
import { GenerateMachineKeyPanel } from './panels/GenerateMachineKeyPanel';
import { PanelDrillProvider } from './context';
import {
  useSettingsStore,
  selectTimeFormat24h,
  selectTimezone,
  selectRpcEndpoint,
  selectPendingNavigation,
  type SettingsArea,
  type SettingsSubPanel,
} from '../../stores';
import { useIdentityServiceClient } from '../../desktop/hooks/useIdentityServiceClient';
import styles from './SettingsApp.module.css';

// Area labels
const AREA_LABELS: Record<SettingsArea, string> = {
  general: 'Time',
  identity: 'Identity',
  network: 'Network',
  permissions: 'Permissions',
  theme: 'Theme',
};

/**
 * Settings App - System settings management
 *
 * Uses ZUI components: Panel, Navigator, PanelDrill
 * Layout: Split-pane with left navigation and right PanelDrill content
 *
 * Time settings are managed via the global settingsStore which syncs with
 * the time_service WASM process for persistence.
 */
export function SettingsApp() {
  // Navigation state (local to this component)
  const [activeArea, setActiveArea] = useState<SettingsArea>('identity');

  // Identity service client for loading preferences
  const { userId } = useIdentityServiceClient();

  // Time settings from global store (synced with time_service)
  const timeFormat24h = useSettingsStore(selectTimeFormat24h);
  const timezone = useSettingsStore(selectTimezone);
  const setTimeFormat24h = useSettingsStore((state) => state.setTimeFormat24h);
  const setTimezone = useSettingsStore((state) => state.setTimezone);

  // Network settings
  const rpcEndpoint = useSettingsStore(selectRpcEndpoint);
  const setRpcEndpoint = useSettingsStore((state) => state.setRpcEndpoint);

  // Navigation state from store (replaces module-level pendingNavigation)
  const pendingNavigation = useSettingsStore(selectPendingNavigation);
  const clearPendingNavigation = useSettingsStore((state) => state.clearPendingNavigation);

  // Load identity preferences when user is available
  useEffect(() => {
    if (userId) {
      useSettingsStore.getState().loadIdentityPreferences(userId);
    }
  }, [userId]);

  // Use ref for pushPanel to avoid circular dependency in content creation
  const pushPanelRef = useRef<(item: PanelDrillItem) => void>(() => {});

  // Helper to create content for a given area
  // Called once when switching sections, not on every render
  const createContentForArea = useCallback(
    (area: SettingsArea): React.ReactNode => {
      switch (area) {
        case 'general':
          return (
            <GeneralPanel
              timeFormat24h={timeFormat24h}
              timezone={timezone}
              onTimeFormatChange={setTimeFormat24h}
              onTimezoneChange={setTimezone}
            />
          );
        case 'identity':
          // Use ref to avoid recreating when pushPanel changes
          return <IdentitySettingsPanel onDrillDown={(item) => pushPanelRef.current(item)} />;
        case 'network':
          return <NetworkPanel rpcEndpoint={rpcEndpoint} onRpcEndpointChange={setRpcEndpoint} />;
        case 'permissions':
          // Use ref to avoid recreating when pushPanel changes
          return <PermissionsPanel onDrillDown={(item) => pushPanelRef.current(item)} />;
        case 'theme':
          return <ThemePanel />;
      }
    },
    [timeFormat24h, timezone, setTimeFormat24h, setTimezone, rpcEndpoint, setRpcEndpoint]
  );

  // Helper to create PanelDrillItem for a given sub-panel (for deep-linking)
  const createSubPanelItem = useCallback((subPanel: SettingsSubPanel): PanelDrillItem | null => {
    switch (subPanel) {
      case 'neural-key':
        return {
          id: 'neural-key',
          label: 'Neural Key',
          content: <NeuralKeyPanel />,
        };
      case 'machine-keys':
        // Handler for the + button in the header - drills to Generate Key form
        const handleAddMachineKey = () => {
          pushPanelRef.current({
            id: 'generate-key',
            label: 'Generate Key',
            content: <GenerateMachineKeyPanel />,
          });
        };
        return {
          id: 'machine-keys',
          label: 'Machine Keys',
          action: <ButtonPlus onClick={handleAddMachineKey} />,
          content: <MachineKeysPanel />,
        };
      case 'linked-accounts':
        return {
          id: 'linked-accounts',
          label: 'Linked Accounts',
          content: <LinkedAccountsPanel />,
        };
      default:
        return null;
    }
  }, []);

  // Initialize stack with root item - content created once on mount
  // We use a ref to track initialization and update once store values are available
  const [stack, setStack] = useState<PanelDrillItem[]>(() => [
    {
      id: 'identity',
      label: AREA_LABELS.identity,
      content: null, // Will be populated in useEffect
    },
  ]);

  // Navigation items
  const navItems: NavigatorItem[] = useMemo(
    () => [
      {
        id: 'identity',
        label: 'Identity',
        icon: <User size={14} />,
      },
      {
        id: 'network',
        label: 'Network',
        icon: <Network size={14} />,
      },
      {
        id: 'general',
        label: 'Time',
        icon: <Clock size={14} />,
      },
      {
        id: 'permissions',
        label: 'Permissions',
        icon: <Shield size={14} />,
      },
      {
        id: 'theme',
        label: 'Theme',
        icon: <Palette size={14} />,
      },
    ],
    []
  );

  // Push a new panel onto the stack
  const pushPanel = useCallback((item: PanelDrillItem) => {
    setStack((prev) => [...prev, item]);
  }, []);

  // Navigate back one level in the panel stack
  const navigateBack = useCallback(() => {
    setStack((prev) => {
      if (prev.length <= 1) return prev;
      return prev.slice(0, -1);
    });
  }, []);

  // Keep ref in sync with pushPanel
  useEffect(() => {
    pushPanelRef.current = pushPanel;
  }, [pushPanel]);

  // Handle area selection from left menu - reset stack to new section
  const handleAreaSelect = useCallback(
    (id: string) => {
      const area = id as SettingsArea;
      setActiveArea(area);
      // Reset stack to just the new section's root item
      setStack([
        {
          id: area,
          label: AREA_LABELS[area],
          content: createContentForArea(area),
        },
      ]);
    },
    [createContentForArea]
  );

  // Handle breadcrumb navigation - truncate stack to clicked index
  const handleNavigate = useCallback((_id: string, index: number) => {
    setStack((prev) => prev.slice(0, index + 1));
  }, []);

  // Update root panel content when relevant state changes
  // This ensures panels see updated values after user interaction
  useEffect(() => {
    setStack((prev) => {
      if (prev.length === 0) return prev;
      const rootArea = prev[0].id as SettingsArea;
      // Only update if we're at root level (no drill-down) to preserve drill state
      if (prev.length === 1) {
        return [
          {
            ...prev[0],
            content: createContentForArea(rootArea),
          },
        ];
      }
      return prev;
    });
  }, [timeFormat24h, timezone, rpcEndpoint, createContentForArea]);

  // Check for pending navigation from store (handles race condition when Settings is opened)
  // This effect runs when pendingNavigation changes and navigates to the requested section
  // Supports deep-linking to sub-panels by building the complete stack upfront
  useEffect(() => {
    if (pendingNavigation) {
      console.log('[SettingsApp] Found pending navigation:', pendingNavigation);

      const { area, subPanel } = pendingNavigation;
      setActiveArea(area);

      // Build the stack - root panel + optional sub-panel
      const rootItem: PanelDrillItem = {
        id: area,
        label: AREA_LABELS[area],
        content: createContentForArea(area),
      };

      const newStack: PanelDrillItem[] = [rootItem];

      // If deep-linking to a sub-panel, add it to the initial stack
      if (subPanel) {
        const subPanelItem = createSubPanelItem(subPanel);
        if (subPanelItem) {
          newStack.push(subPanelItem);
        }
      }

      setStack(newStack);
      clearPendingNavigation();
    }
  }, [pendingNavigation, createContentForArea, createSubPanelItem, clearPendingNavigation]);

  return (
    <Panel border="none" background="none" className={styles.container}>
      {/* Left Navigation Sidebar */}
      <div className={styles.sidebar}>
        <Navigator items={navItems} value={activeArea} onChange={handleAreaSelect} />
      </div>

      {/* Right Content Area with PanelDrill */}
      <Panel border="none" background="none" className={styles.content}>
        <PanelDrillProvider onNavigateBack={navigateBack} onPushPanel={pushPanel}>
          <PanelDrill
            stack={stack}
            onNavigate={handleNavigate}
            showBreadcrumb={true}
            border="none"
            background="none"
            className={styles.panelDrill}
          />
        </PanelDrillProvider>
      </Panel>
    </Panel>
  );
}
