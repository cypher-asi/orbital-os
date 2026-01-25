/**
 * App Protocol Module
 *
 * Versioned, platform-agnostic IPC protocol for communication between
 * app backends (WASM) and UI surfaces (React).
 *
 * Re-exports all protocol types, constants, and functions.
 */

// Types and constants
export {
  PROTOCOL_VERSION,
  MSG_APP_STATE,
  MSG_APP_INPUT,
  MSG_UI_READY,
  MSG_APP_FOCUS,
  MSG_APP_ERROR,
  MSG_CAP_REVOKED,
  REVOKE_REASON_EXPLICIT,
  REVOKE_REASON_EXPIRED,
  REVOKE_REASON_PROCESS_EXIT,
  TYPE_CLOCK_STATE,
  TYPE_CALCULATOR_STATE,
  TYPE_SETTINGS_STATE,
  TYPE_BUTTON_PRESS,
  TYPE_TEXT_INPUT,
  TYPE_KEY_PRESS,
  TYPE_FOCUS_CHANGE,
  TYPE_GRANT_REQUEST,
  TYPE_REVOKE_REQUEST,
  TYPE_PERMISSION_RESPONSE,
  TYPE_PERMISSION_LIST,
  MSG_GRANT_PERMISSION,
  MSG_REVOKE_PERMISSION,
  MSG_LIST_PERMISSIONS,
  MSG_PERMISSION_RESPONSE,
  MODIFIER_SHIFT,
  MODIFIER_CTRL,
  MODIFIER_ALT,
} from './types';

// Envelope encoding/decoding
export {
  type Envelope,
  encodeEnvelope,
  decodeEnvelope,
  encodeString,
  decodeString,
  decodeU8,
  decodeU32,
  decodeOptionalChar,
} from './envelope';

// Clock state
export { type ClockState, decodeClockState } from './clock';

// Calculator state
export { type CalculatorState, decodeCalculatorState } from './calculator';

// Settings state
export { type SettingsState, decodeSettingsState } from './settings';

// Input events
export { type InputEvent, buttonPress, encodeInputEvent } from './input';

// Permissions
export {
  type ObjectType,
  OBJECT_TYPE,
  type Permissions,
  encodePermissions,
  decodePermissions,
  type CapabilityRequest,
  type GrantRequest,
  encodeGrantRequest,
  type RevokeRequest,
  encodeRevokeRequest,
  type PermissionResponse,
  decodePermissionResponse,
  type CapabilityInfo,
  type PermissionListResponse,
  decodePermissionList,
} from './permissions';
