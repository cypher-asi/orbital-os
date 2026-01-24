/**
 * Zustand Stores - Central State Management
 * 
 * Re-exports all stores and selectors for convenient importing.
 * 
 * Usage:
 * ```ts
 * import { useWindowStore, selectWindows, useIdentityStore } from '../stores';
 * 
 * // In component:
 * const windows = useWindowStore(selectWindows);
 * const currentUser = useIdentityStore(state => state.currentUser);
 * ```
 */

// Window store
export { 
  useWindowStore,
  selectWindows,
  selectFocusedId,
  selectFocusedWindow,
  selectWindowById,
  selectVisibleWindows,
  selectAnimating,
  selectTransitioning,
  selectWindowsByZOrder,
  selectWindowCount,
} from './windowStore';

// Desktop store
export {
  useDesktopStore,
  selectDesktops,
  selectActiveDesktop,
  selectActiveIndex,
  selectViewMode,
  selectInVoid,
  selectViewport,
  selectShowVoid,
  selectWorkspaceInfo,
  selectDesktopCount,
  selectLayerOpacities,
} from './desktopStore';

// Identity store
export {
  useIdentityStore,
  selectCurrentUser,
  selectCurrentSession,
  selectUsers,
  selectIsLoading as selectIdentityIsLoading,
  selectError as selectIdentityError,
  selectIsLoggedIn,
  selectUserById,
  formatUserId,
  getSessionTimeRemaining,
  isSessionExpired,
  type User,
  type Session,
  type UserId,
  type SessionId,
  type UserStatus,
} from './identityStore';

// Permission store
export {
  usePermissionStore,
  selectPendingRequest,
  selectIsLoading as selectPermissionIsLoading,
  selectAllGrantedCapabilities,
  selectGrantedCapabilities,
  selectHasCapabilities,
  selectProcessCount,
  selectTotalCapabilityCount,
  type AppManifest,
  type CapabilityInfo,
  type PermissionRequest,
} from './permissionStore';

// Settings store
export {
  useSettingsStore,
  selectTimeFormat24h,
  selectTimezone,
  selectSettingsIsLoading,
  selectSettingsIsSynced,
  selectSettingsError,
  formatTime,
  formatDate,
  formatShortDate,
  COMMON_TIMEZONES,
} from './settingsStore';

// Machine Keys store
export {
  useMachineKeysStore,
  selectMachines,
  selectMachineCount,
  selectCurrentMachineId,
  selectMachineKeysIsLoading,
  selectMachineKeysIsInitializing,
  selectMachineKeysError,
  selectMachineById,
  selectCurrentDevice,
  selectMachineKeysState,
  type MachineKeyCapabilities,
  type MachineKeyRecord,
  type MachineKeysState,
} from './machineKeysStore';

// Shared types
export type {
  WasmRefs,
  WindowType,
  WindowState,
  WindowInfo,
  WindowData,
  ViewMode,
  DesktopInfo,
  ViewportState,
  WorkspaceInfo,
  FrameData,
  LayerOpacities,
} from './types';
