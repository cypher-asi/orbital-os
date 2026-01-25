/**
 * Desktop Component
 *
 * Main export with provider wrappers.
 * Implementation split into smaller modules for maintainability:
 *
 * - types.ts - Type definitions for frame data, windows, etc.
 * - BackgroundContext.tsx - Context for background controller
 * - DesktopInner.tsx - Canvas and window rendering
 * - DesktopWithPermissions.tsx - Main component with state
 * - hooks/useRenderLoop.ts - Render loop with direct DOM updates
 * - hooks/usePointerHandlers.ts - Pointer event handlers
 * - hooks/useBackgroundMenu.ts - Background menu state
 */

import { SupervisorProvider, DesktopControllerProvider } from '../../desktop/hooks/useSupervisor';
import { DesktopWithPermissions } from './DesktopWithPermissions';
import type { DesktopProps } from './types';

export { useBackground } from './BackgroundContext';

export function Desktop({ supervisor, desktop }: DesktopProps): JSX.Element {
  return (
    <SupervisorProvider value={supervisor}>
      <DesktopControllerProvider value={desktop}>
        <DesktopWithPermissions supervisor={supervisor} desktop={desktop} />
      </DesktopControllerProvider>
    </SupervisorProvider>
  );
}
