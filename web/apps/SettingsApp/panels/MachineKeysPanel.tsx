import { useState, useCallback } from 'react';
import { GroupCollapsible, Button, Card, CardItem, Label, Input, Text, ButtonMore, ButtonCopy, type PanelDrillItem } from '@cypher-asi/zui';
import { 
  Cpu, 
  Plus, 
  Trash2, 
  RefreshCw, 
  Check, 
  X, 
  AlertTriangle,
  Smartphone,
  Loader,
} from 'lucide-react';
import { useMachineKeys, type MachineKeyRecord } from '../../../desktop/hooks/useMachineKeys';
import { useNeuralKey } from '../../../desktop/hooks/useNeuralKey';
import styles from './panels.module.css';

interface MachineKeysPanelProps {
  onDrillDown?: (item: PanelDrillItem) => void;
}

/**
 * Machine Keys Panel
 * 
 * Features:
 * - List all registered machines with status
 * - Add new machine key via drill-down flow
 * - Delete with confirmation
 * - Rotate key (new epoch)
 */
export function MachineKeysPanel({ onDrillDown }: MachineKeysPanelProps) {
  const { state, revokeMachineKey, rotateMachineKey } = useMachineKeys();
  const { state: neuralKeyState } = useNeuralKey();
  
  // UI state
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [confirmRotate, setConfirmRotate] = useState<string | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);
  const [isRotating, setIsRotating] = useState(false);

  // Handle delete confirmation
  const handleConfirmDelete = useCallback(async (machineId: string) => {
    setIsDeleting(true);
    try {
      await revokeMachineKey(machineId);
      setConfirmDelete(null);
    } catch (err) {
      console.error('Failed to delete machine:', err);
    } finally {
      setIsDeleting(false);
    }
  }, [revokeMachineKey]);

  // Handle rotate confirmation
  const handleConfirmRotate = useCallback(async (machineId: string) => {
    setIsRotating(true);
    try {
      await rotateMachineKey(machineId);
      setConfirmRotate(null);
    } catch (err) {
      console.error('Failed to rotate machine key:', err);
    } finally {
      setIsRotating(false);
    }
  }, [rotateMachineKey]);

  // Handle machine action from ButtonMore menu
  const handleMachineAction = useCallback((machineId: string, action: string) => {
    if (action === 'rotate') {
      setConfirmRotate(machineId);
    } else if (action === 'delete') {
      setConfirmDelete(machineId);
    }
  }, []);

  // Truncate public key for display
  const truncateKey = (key: string) => {
    if (key.length <= 16) return key;
    return `${key.slice(0, 8)}...${key.slice(-8)}`;
  };

  // Get machine by ID
  const getMachineById = (id: string): MachineKeyRecord | undefined => {
    return state.machines.find(m => m.machineId === id);
  };

  // Show nothing during initial settling period (matches Neural Key pattern)
  // This prevents layout jump and avoids "Loading..." text blink
  if (state.isInitializing || neuralKeyState.isInitializing) {
    return null;
  }

  // Show error state
  if (state.error && state.machines.length === 0) {
    return (
      <div className={styles.centeredContent}>
        <Card className={styles.dangerCard}>
          <CardItem
            icon={<AlertTriangle size={16} />}
            title="Error"
            description={state.error}
            className={styles.dangerCardItem}
          />
        </Card>
      </div>
    );
  }

  // Show empty state with appropriate content based on Neural Key existence
  if (state.machines.length === 0) {
    // If no Neural Key, show message to generate one first
    if (!neuralKeyState.hasNeuralKey) {
      return (
        <div className={styles.centeredContent}>
          <div className={styles.heroIcon}>
            <Cpu size={48} strokeWidth={1} />
          </div>
          <Text size="md" className={styles.heroTitle}>
            No Machines Yet
          </Text>
          <Text size="sm" className={styles.heroDescription}>
            Generate a Neural Key first to register devices. Your Neural Key creates
            your cryptographic identity, which is required before adding machine keys.
          </Text>
        </div>
      );
    }
    
    // Has Neural Key but no machines - show add button
    const handleAddClick = () => {
      if (onDrillDown) {
        onDrillDown({
          id: 'generate-key',
          label: 'Generate Key',
          content: <GenerateMachineKeyPanel />,
        });
      }
    };

    return (
      <div className={styles.centeredContent}>
        <div className={styles.heroIcon}>
          <Cpu size={48} strokeWidth={1} />
        </div>
        <Text size="md" className={styles.heroTitle}>
          Register Your First Machine
        </Text>
        <Text size="sm" className={styles.heroDescription}>
          Machine keys allow this machine to securely access your identity.
          Each machine gets its own key that can be rotated or revoked.
        </Text>
        
        <Button variant="primary" size="lg" onClick={handleAddClick}>
          <Plus size={16} /> Add This Machine
        </Button>
      </div>
    );
  }

  // Render delete confirmation
  if (confirmDelete) {
    const machine = getMachineById(confirmDelete);
    if (!machine) {
      setConfirmDelete(null);
      return null;
    }

    return (
      <div className={styles.panelContainer}>
        <GroupCollapsible
          title="Confirm Delete"
          defaultOpen
          className={styles.collapsibleSection}
        >
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
                onClick={() => setConfirmDelete(null)}
                disabled={isDeleting}
              >
                <X size={14} />
                Cancel
              </Button>
              <Button 
                variant="danger" 
                size="md"
                onClick={() => handleConfirmDelete(confirmDelete)}
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
  if (confirmRotate) {
    const machine = getMachineById(confirmRotate);
    if (!machine) {
      setConfirmRotate(null);
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
                onClick={() => setConfirmRotate(null)}
                disabled={isRotating}
              >
                <X size={14} />
                Cancel
              </Button>
              <Button 
                variant="primary" 
                size="md"
                onClick={() => handleConfirmRotate(confirmRotate)}
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

  return (
    <div className={styles.panelContainer}>
      {/* Machine List */}
      <GroupCollapsible
        title="Registered Machines"
        count={state.machines.length}
        defaultOpen
        className={styles.collapsibleSection}
      >
        <div className={styles.menuContent}>
          {state.machines.map((machine) => (
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
              {machine.isCurrentDevice && (
                <Label size="xs" variant="success">Current</Label>
              )}
              <div className={styles.machineItemAction}>
                <ButtonMore
                  items={[
                    { id: 'rotate', label: 'Rotate', icon: <RefreshCw size={14} /> },
                    ...(!machine.isCurrentDevice ? [{ id: 'delete', label: 'Delete', icon: <Trash2 size={14} /> }] : []),
                  ]}
                  onSelect={(id) => handleMachineAction(machine.machineId, id)}
                />
              </div>
            </div>
          ))}
        </div>
      </GroupCollapsible>
    </div>
  );
}

/** Dispatch event to navigate back in the panel drill stack */
function navigateBack() {
  window.dispatchEvent(new CustomEvent('paneldrill:back'));
}

/**
 * Generate Machine Key Panel
 * 
 * Drill-down panel for creating a new machine key.
 * Shows a form with machine name input and Generate/Cancel buttons.
 */
export function GenerateMachineKeyPanel() {
  const { createMachineKey } = useMachineKeys();
  const [machineName, setMachineName] = useState('');
  const [isGenerating, setIsGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleGenerate = useCallback(async () => {
    if (!machineName.trim()) return;
    
    setIsGenerating(true);
    setError(null);
    try {
      await createMachineKey(machineName.trim());
      // Navigate back after successful creation
      navigateBack();
    } catch (err) {
      console.error('Failed to create machine key:', err);
      setError(err instanceof Error ? err.message : 'Failed to generate machine key');
    } finally {
      setIsGenerating(false);
    }
  }, [machineName, createMachineKey]);

  const handleCancel = useCallback(() => {
    // Navigate back to Machine Keys panel
    navigateBack();
  }, []);

  return (
    <div className={styles.panelContainer}>
      <div className={styles.identitySection}>
        <div className={styles.heroIcon}>
          <Cpu size={48} strokeWidth={1} />
        </div>
        <Text size="md" className={styles.heroTitle}>
          Generate Machine Key
        </Text>
        <Text size="sm" className={styles.heroDescription}>
          Give this machine a recognizable name to identify it in your list of registered devices.
        </Text>

        <div className={styles.addForm}>
          <Input
            value={machineName}
            onChange={(e) => setMachineName(e.target.value)}
            placeholder="Machine name (e.g., Work Laptop)"
            autoFocus
            onKeyDown={(e) => {
              if (e.key === 'Enter' && machineName.trim() && !isGenerating) {
                handleGenerate();
              }
            }}
          />
          
          {error && (
            <Card className={styles.dangerCard}>
              <CardItem
                icon={<AlertTriangle size={14} />}
                title="Error"
                description={error}
                className={styles.dangerCardItem}
              />
            </Card>
          )}

          <div className={styles.addFormButtons}>
            <Button 
              variant="ghost" 
              size="md"
              onClick={handleCancel}
              disabled={isGenerating}
            >
              <X size={14} />
              Cancel
            </Button>
            <Button 
              variant="primary" 
              size="md"
              onClick={handleGenerate}
              disabled={isGenerating || !machineName.trim()}
            >
              {isGenerating ? (
                <>
                  <Loader size={14} className={styles.spinner} />
                  Generating...
                </>
              ) : (
                <>
                  <Check size={14} />
                  Generate
                </>
              )}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
