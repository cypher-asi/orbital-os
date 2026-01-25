import {
  GroupCollapsible,
  Button,
  Card,
  CardItem,
  Label,
  Text,
  ButtonMore,
  ButtonCopy,
} from '@cypher-asi/zui';
import { Cpu, Plus, Trash2, RefreshCw, X, AlertTriangle, Smartphone, Loader, Star } from 'lucide-react';
import type { MachineKeyRecord } from '../../../../stores';
import styles from './MachineKeysPanel.module.css';

// =============================================================================
// Types
// =============================================================================

/** Machine action type for type-safe action handling */
export type MachineAction = 'rotate' | 'delete' | 'set_default';

/** Confirmation dialog state - discriminated union pattern */
export type ConfirmationState =
  | { type: 'none' }
  | { type: 'delete'; machineId: string }
  | { type: 'rotate'; machineId: string };

export interface MachineKeysPanelViewProps {
  /** List of registered machines */
  machines: MachineKeyRecord[];
  /** Current machine ID for highlighting */
  currentMachineId?: string;
  /** Error message to display */
  error: string | null;
  /** Whether initial data is loading */
  isInitializing: boolean;
  /** Whether user has a neural key */
  hasNeuralKey: boolean;
  /** Whether neural key state is initializing */
  neuralKeyIsInitializing: boolean;
  /** Current confirmation dialog state */
  confirmationState: ConfirmationState;
  /** Whether delete operation is in progress */
  isDeleting: boolean;
  /** Whether rotate operation is in progress */
  isRotating: boolean;
  /** Callback when user confirms delete */
  onConfirmDelete: (machineId: string) => void;
  /** Callback when user confirms rotate */
  onConfirmRotate: (machineId: string) => void;
  /** Callback when user cancels confirmation dialog */
  onCancelConfirmation: () => void;
  /** Callback when user selects an action from machine menu */
  onMachineAction: (machineId: string, action: MachineAction) => void;
  /** Callback when user clicks add machine */
  onAddMachine: () => void;
}

// =============================================================================
// Helper Functions
// =============================================================================

/** Truncate public key for display */
function truncateKey(key: string): string {
  if (key.length <= 16) return key;
  return `${key.slice(0, 8)}...${key.slice(-8)}`;
}

/** Get machine by ID from list */
function getMachineById(machines: MachineKeyRecord[], id: string): MachineKeyRecord | undefined {
  return machines.find((m) => m.machineId === id);
}

// =============================================================================
// Component
// =============================================================================

/**
 * MachineKeysPanelView - Pure presentation component
 *
 * Renders the machine keys panel UI based on the provided state.
 * No hooks that touch global state - all data comes through props.
 */
export function MachineKeysPanelView({
  machines,
  error,
  isInitializing,
  hasNeuralKey,
  neuralKeyIsInitializing,
  confirmationState,
  isDeleting,
  isRotating,
  onConfirmDelete,
  onConfirmRotate,
  onCancelConfirmation,
  onMachineAction,
  onAddMachine,
}: MachineKeysPanelViewProps) {
  // Show nothing during initial settling period
  // This prevents layout jump and avoids "Loading..." text blink
  if (isInitializing || neuralKeyIsInitializing) {
    return null;
  }

  // Show error state
  if (error && machines.length === 0) {
    return (
      <div className={styles.centeredContent}>
        <Card className={styles.dangerCard}>
          <CardItem
            icon={<AlertTriangle size={16} />}
            title="Error"
            description={error}
            className={styles.dangerCardItem}
          />
        </Card>
      </div>
    );
  }

  // Show empty state with appropriate content based on Neural Key existence
  if (machines.length === 0) {
    // If no Neural Key, show message to generate one first
    if (!hasNeuralKey) {
      return (
        <div className={styles.centeredContent}>
          <div className={styles.heroIcon}>
            <Cpu size={48} strokeWidth={1} />
          </div>
          <Text size="md" className={styles.heroTitle}>
            No Machines Yet
          </Text>
          <Text size="sm" className={styles.heroDescription}>
            Generate a Neural Key first to register devices. Your Neural Key creates your
            cryptographic identity, which is required before adding machine keys.
          </Text>
        </div>
      );
    }

    // Has Neural Key but no machines - show add button
    return (
      <div className={styles.centeredContent}>
        <div className={styles.heroIcon}>
          <Cpu size={48} strokeWidth={1} />
        </div>
        <Text size="md" className={styles.heroTitle}>
          Register Your First Machine
        </Text>
        <Text size="sm" className={styles.heroDescription}>
          Machine keys allow this machine to securely access your identity. Each machine gets its
          own key that can be rotated or revoked.
        </Text>

        <Button variant="primary" size="lg" onClick={onAddMachine}>
          <Plus size={16} /> Add This Machine
        </Button>
      </div>
    );
  }

  // Render delete confirmation
  if (confirmationState.type === 'delete') {
    const machine = getMachineById(machines, confirmationState.machineId);
    if (!machine) {
      return null;
    }

    return (
      <div className={styles.panelContainer}>
        <GroupCollapsible title="Confirm Delete" defaultOpen className={styles.collapsibleSection}>
          <div className={styles.identitySection}>
            <Card className={styles.dangerCard}>
              <CardItem
                icon={<AlertTriangle size={16} />}
                title={`Delete "${machine.machineName || 'Unnamed Machine'}"?`}
                description="This device will no longer be able to access your identity. This action cannot be undone."
                className={styles.dangerCardItem}
              />
            </Card>

            <div className={styles.confirmButtons}>
              <Button
                variant="ghost"
                size="md"
                onClick={onCancelConfirmation}
                disabled={isDeleting}
              >
                <X size={14} />
                Cancel
              </Button>
              <Button
                variant="danger"
                size="md"
                onClick={() => onConfirmDelete(confirmationState.machineId)}
                disabled={isDeleting}
              >
                {isDeleting ? (
                  <>
                    <Loader size={14} className={styles.spinner} />
                    Deleting...
                  </>
                ) : (
                  <>
                    <Trash2 size={14} />
                    Delete Machine
                  </>
                )}
              </Button>
            </div>
          </div>
        </GroupCollapsible>
      </div>
    );
  }

  // Render rotate confirmation
  if (confirmationState.type === 'rotate') {
    const machine = getMachineById(machines, confirmationState.machineId);
    if (!machine) {
      return null;
    }

    return (
      <div className={styles.panelContainer}>
        <GroupCollapsible
          title="Confirm Rotation"
          defaultOpen
          className={styles.collapsibleSection}
        >
          <div className={styles.identitySection}>
            <Card className={styles.warningCard}>
              <CardItem
                icon={<RefreshCw size={16} />}
                title={`Rotate key for "${machine.machineName || 'Unnamed Machine'}"?`}
                description="This will generate a new key pair for this device. The device will need to re-authenticate. Machine ID will be preserved."
                className={styles.warningCardItem}
              />
            </Card>

            <div className={styles.confirmButtons}>
              <Button
                variant="ghost"
                size="md"
                onClick={onCancelConfirmation}
                disabled={isRotating}
              >
                <X size={14} />
                Cancel
              </Button>
              <Button
                variant="primary"
                size="md"
                onClick={() => onConfirmRotate(confirmationState.machineId)}
                disabled={isRotating}
              >
                {isRotating ? (
                  <>
                    <Loader size={14} className={styles.spinner} />
                    Rotating...
                  </>
                ) : (
                  <>
                    <RefreshCw size={14} />
                    Rotate Key
                  </>
                )}
              </Button>
            </div>
          </div>
        </GroupCollapsible>
      </div>
    );
  }

  // Default: Render machine list
  return (
    <div className={styles.panelContainer}>
      <GroupCollapsible
        title="Registered Machines"
        count={machines.length}
        defaultOpen
        className={styles.collapsibleSection}
      >
        <div className={styles.menuContent}>
          {machines.map((machine) => (
            <div key={machine.machineId} className={styles.machineItem}>
              <div className={styles.machineItemIcon}>
                {machine.isCurrentDevice ? <Smartphone size={14} /> : <Cpu size={14} />}
              </div>
              <div className={styles.machineItemContentSingle}>
                <span className={styles.machineItemLabel}>
                  {machine.machineName || 'Unnamed Machine'}
                </span>
                <code className={styles.machineItemKey}>
                  {truncateKey(machine.signingPublicKey)}
                </code>
              </div>
              <ButtonCopy text={machine.signingPublicKey} />
              <Label
                size="xs"
                variant={machine.keyScheme === 'pq_hybrid' ? 'info' : 'default'}
                title={
                  machine.keyScheme === 'pq_hybrid'
                    ? 'Post-Quantum Hybrid: Ed25519/X25519 + ML-DSA-65/ML-KEM-768'
                    : 'Classical: Ed25519 + X25519'
                }
              >
                {machine.keyScheme === 'pq_hybrid' ? 'PQ-Hybrid' : 'Classical'}
              </Label>
              <Label size="xs" variant="default">
                Epoch {machine.epoch}
              </Label>
              {machine.isCurrentDevice && (
                <Label size="xs" variant="success">
                  Current
                </Label>
              )}
              <div className={styles.machineItemAction}>
                <ButtonMore
                  items={[
                    { id: 'set_default', label: 'Set as Default', icon: <Star size={14} /> },
                    { id: 'rotate', label: 'Rotate', icon: <RefreshCw size={14} /> },
                    ...(!machine.isCurrentDevice
                      ? [{ id: 'delete', label: 'Delete', icon: <Trash2 size={14} /> }]
                      : []),
                  ]}
                  onSelect={(id) => onMachineAction(machine.machineId, id as MachineAction)}
                />
              </div>
            </div>
          ))}
        </div>
      </GroupCollapsible>
    </div>
  );
}
