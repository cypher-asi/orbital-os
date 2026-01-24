import { useCallback, useMemo } from 'react';
import { Menu, GroupCollapsible, Label, ButtonPlus, type MenuItem, type PanelDrillItem } from '@cypher-asi/zui';
import { Brain, Cpu, Users, ChevronRight, CheckCircle, XCircle } from 'lucide-react';
import { useNeuralKey } from '../../../desktop/hooks/useNeuralKey';
import { useMachineKeys } from '../../../desktop/hooks/useMachineKeys';
import { useLinkedAccounts } from '../../../desktop/hooks/useLinkedAccounts';
import { NeuralKeyPanel } from './NeuralKeyPanel';
import { MachineKeysPanel, GenerateMachineKeyPanel } from './MachineKeysPanel';
import { LinkedAccountsPanel } from './LinkedAccountsPanel';
import styles from './panels.module.css';

interface IdentitySettingsPanelProps {
  onDrillDown?: (item: PanelDrillItem) => void;
}

/**
 * Identity Settings Panel
 * - Neural Key status and management
 * - Machine Keys overview
 * - Linked Accounts overview
 * 
 * Uses hooks for shared state - both this panel and subpanels consume
 * the same hooks, so status badges update automatically when subpanels mutate state.
 */
export function IdentitySettingsPanel({ onDrillDown }: IdentitySettingsPanelProps) {
  // Consume hooks for live state
  const { state: neuralKeyState } = useNeuralKey();
  const { state: machineKeysState } = useMachineKeys();
  const { state: linkedAccountsState } = useLinkedAccounts();

  // Handle drill-down navigation
  const handleDrillToNeuralKey = useCallback(() => {
    if (onDrillDown) {
      onDrillDown({
        id: 'neural-key',
        label: 'Neural Key',
        content: <NeuralKeyPanel />,
      });
    }
  }, [onDrillDown]);

  const handleDrillToMachineKeys = useCallback(() => {
    if (onDrillDown) {
      // Handler for the + button in the header - drills to Generate Key form
      const handleAddClick = () => {
        onDrillDown({
          id: 'generate-key',
          label: 'Generate Key',
          content: <GenerateMachineKeyPanel />,
        });
      };

      onDrillDown({
        id: 'machine-keys',
        label: 'Machine Keys',
        action: <ButtonPlus onClick={handleAddClick} />,
        content: <MachineKeysPanel onDrillDown={onDrillDown} />,
      });
    }
  }, [onDrillDown]);

  const handleDrillToLinkedAccounts = useCallback(() => {
    if (onDrillDown) {
      onDrillDown({
        id: 'linked-accounts',
        label: 'Linked Accounts',
        content: <LinkedAccountsPanel />,
      });
    }
  }, [onDrillDown]);

  // Handle menu selection
  const handleSelect = useCallback((id: string) => {
    switch (id) {
      case 'neural-key':
        handleDrillToNeuralKey();
        break;
      case 'machine-keys':
        handleDrillToMachineKeys();
        break;
      case 'linked-accounts':
        handleDrillToLinkedAccounts();
        break;
    }
  }, [handleDrillToNeuralKey, handleDrillToMachineKeys, handleDrillToLinkedAccounts]);

  // Neural Key menu items
  const neuralKeyItems: MenuItem[] = useMemo(() => [
    {
      id: 'neural-key',
      label: 'Neural Key Status',
      icon: <Brain size={14} />,
      suffix: neuralKeyState.hasNeuralKey ? (
        <div className={styles.statusBadge}>
          <CheckCircle size={12} className={styles.successIcon} />
          <Label size="xs" variant="success">Active</Label>
        </div>
      ) : (
        <div className={styles.statusBadge}>
          <XCircle size={12} className={styles.warningIcon} />
          <Label size="xs" variant="warning">Not Set</Label>
        </div>
      ),
      endIcon: <ChevronRight size={14} />,
    },
  ], [neuralKeyState.hasNeuralKey]);

  // Machine Keys menu items
  const machineKeyItems: MenuItem[] = useMemo(() => [
    {
      id: 'machine-keys',
      label: 'Registered Machines',
      icon: <Cpu size={14} />,
      suffix: (
        <div className={styles.countBadge}>
          <Label size="xs">{machineKeysState.machines.length} device{machineKeysState.machines.length !== 1 ? 's' : ''}</Label>
        </div>
      ),
      endIcon: <ChevronRight size={14} />,
    },
  ], [machineKeysState.machines.length]);

  // Linked Accounts menu items
  const linkedAccountItems: MenuItem[] = useMemo(() => {
    const verifiedCount = linkedAccountsState.credentials.filter(c => c.verified).length;
    return [
      {
        id: 'linked-accounts',
        label: 'Connected Services',
        icon: <Users size={14} />,
        suffix: verifiedCount > 0 ? (
          <div className={styles.countBadge}>
            <Label size="xs">{verifiedCount} connected</Label>
          </div>
        ) : undefined,
        endIcon: <ChevronRight size={14} />,
      },
    ];
  }, [linkedAccountsState.credentials]);

  return (
    <div className={styles.panelContainer}>
      <GroupCollapsible
        title="Neural Key"
        count={neuralKeyState.hasNeuralKey ? 1 : 0}
        defaultOpen
        className={styles.collapsibleSection}
      >
        <div className={styles.menuContent}>
          <Menu items={neuralKeyItems} onChange={handleSelect} background="none" border="none" />
        </div>
      </GroupCollapsible>

      <GroupCollapsible
        title="Machine Keys"
        count={machineKeysState.machines.length}
        defaultOpen
        className={styles.collapsibleSection}
      >
        <div className={styles.menuContent}>
          <Menu items={machineKeyItems} onChange={handleSelect} background="none" border="none" />
        </div>
      </GroupCollapsible>

      <GroupCollapsible
        title="Linked Accounts"
        count={linkedAccountsState.credentials.filter(c => c.verified).length}
        defaultOpen
        className={styles.collapsibleSection}
      >
        <div className={styles.menuContent}>
          <Menu items={linkedAccountItems} onChange={handleSelect} background="none" border="none" />
        </div>
      </GroupCollapsible>
    </div>
  );
}
