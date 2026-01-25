import { useState, useCallback } from 'react';
import type { PanelDrillItem } from '@cypher-asi/zui';
import { useMachineKeys } from '../../../../desktop/hooks/useMachineKeys';
import { useNeuralKey } from '../../../../desktop/hooks/useNeuralKey';
import { useIdentityServiceClient } from '../../../../desktop/hooks/useIdentityServiceClient';
import { useMachineKeysStore } from '../../../../stores/machineKeysStore';
import { usePanelDrillOptional } from '../../context';
import { GenerateMachineKeyPanel } from '../GenerateMachineKeyPanel';
import {
  MachineKeysPanelView,
  type ConfirmationState,
  type MachineAction,
} from './MachineKeysPanelView';

interface MachineKeysPanelProps {
  /** Optional drill-down handler (for backwards compatibility with prop-based navigation) */
  onDrillDown?: (item: PanelDrillItem) => void;
}

/**
 * MachineKeysPanel - Container component
 *
 * Handles data fetching and state management for machine keys.
 * Delegates presentation to MachineKeysPanelView.
 *
 * Navigation:
 * - Uses PanelDrill context when available (preferred)
 * - Falls back to onDrillDown prop for backwards compatibility
 */
export function MachineKeysPanel({ onDrillDown }: MachineKeysPanelProps) {
  // Data hooks
  const { state, revokeMachineKey, rotateMachineKey } = useMachineKeys();
  const { state: neuralKeyState } = useNeuralKey();
  const { getUserIdOrThrow } = useIdentityServiceClient();
  const { setDefaultKeySchemeFromMachine } = useMachineKeysStore();

  // Navigation - prefer context, fall back to prop
  const panelDrill = usePanelDrillOptional();

  // UI state - using discriminated union for confirmation dialog
  const [confirmationState, setConfirmationState] = useState<ConfirmationState>({ type: 'none' });
  const [isDeleting, setIsDeleting] = useState(false);
  const [isRotating, setIsRotating] = useState(false);

  // Handle delete confirmation
  const handleConfirmDelete = useCallback(
    async (machineId: string) => {
      setIsDeleting(true);
      try {
        await revokeMachineKey(machineId);
        setConfirmationState({ type: 'none' });
      } catch (err) {
        console.error('Failed to delete machine:', err);
      } finally {
        setIsDeleting(false);
      }
    },
    [revokeMachineKey]
  );

  // Handle rotate confirmation
  const handleConfirmRotate = useCallback(
    async (machineId: string) => {
      setIsRotating(true);
      try {
        await rotateMachineKey(machineId);
        setConfirmationState({ type: 'none' });
      } catch (err) {
        console.error('Failed to rotate machine key:', err);
      } finally {
        setIsRotating(false);
      }
    },
    [rotateMachineKey]
  );

  // Handle cancel confirmation
  const handleCancelConfirmation = useCallback(() => {
    setConfirmationState({ type: 'none' });
  }, []);

  // Handle machine action from menu
  const handleMachineAction = useCallback(
    async (machineId: string, action: MachineAction) => {
      if (action === 'rotate') {
        setConfirmationState({ type: 'rotate', machineId });
      } else if (action === 'delete') {
        setConfirmationState({ type: 'delete', machineId });
      } else if (action === 'set_default') {
        try {
          const userId = getUserIdOrThrow();
          await setDefaultKeySchemeFromMachine(userId, machineId);
          console.log(`Set default key scheme from machine ${machineId}`);
        } catch (err) {
          console.error('Failed to set default key scheme:', err);
        }
      }
    },
    [getUserIdOrThrow, setDefaultKeySchemeFromMachine]
  );

  // Handle add machine - navigate to generate key panel
  const handleAddMachine = useCallback(() => {
    const drillItem: PanelDrillItem = {
      id: 'generate-key',
      label: 'Generate Key',
      content: <GenerateMachineKeyPanel />,
    };

    if (panelDrill) {
      panelDrill.pushPanel(drillItem);
    } else if (onDrillDown) {
      onDrillDown(drillItem);
    }
  }, [panelDrill, onDrillDown]);

  return (
    <MachineKeysPanelView
      machines={state.machines}
      currentMachineId={state.currentMachineId ?? undefined}
      error={state.error}
      isInitializing={state.isInitializing}
      hasNeuralKey={neuralKeyState.hasNeuralKey}
      neuralKeyIsInitializing={neuralKeyState.isInitializing}
      confirmationState={confirmationState}
      isDeleting={isDeleting}
      isRotating={isRotating}
      onConfirmDelete={handleConfirmDelete}
      onConfirmRotate={handleConfirmRotate}
      onCancelConfirmation={handleCancelConfirmation}
      onMachineAction={handleMachineAction}
      onAddMachine={handleAddMachine}
    />
  );
}
