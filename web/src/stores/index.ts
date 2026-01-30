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
  selectRemoteAuthState,
  formatUserId,
  getSessionTimeRemaining,
  isSessionExpired,
  formatLoginType,
  type User,
  type Session,
  type UserId,
  type SessionId,
  type UserStatus,
  type LoginType,
  type RemoteAuthState,
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
  selectRpcEndpoint,
  selectSettingsIsLoading,
  selectSettingsIsSynced,
  selectSettingsError,
  selectPendingNavigation,
  formatTime,
  formatDate,
  formatShortDate,
  COMMON_TIMEZONES,
  DEFAULT_RPC_ENDPOINT,
  type SettingsArea,
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
  type MachineKeyCapability,
  type MachineKeyRecord,
  type MachineKeysState,
  type KeyScheme,
} from './machineKeysStore';

// Desktop Preferences store (localStorage persistence)
export {
  useDesktopPrefsStore,
  selectActiveWorkspace,
  selectBackgrounds,
} from './desktopPrefsStore';

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
