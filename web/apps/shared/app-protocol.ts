/**
 * App Protocol - TypeScript Bindings
 *
 * Versioned, platform-agnostic IPC protocol for communication between
 * app backends (WASM) and UI surfaces (React).
 *
 * Wire format (envelope):
 * ┌─────────┬──────────┬─────────────┬─────────────────────┐
 * │ version │ type_tag │ payload_len │       payload       │
 * │  (u8)   │   (u8)   │    (u16)    │      (bytes)        │
 * └─────────┴──────────┴─────────────┴─────────────────────┘
 */

// Protocol version
export const PROTOCOL_VERSION = 0x01;

// Message tags for IPC communication
export const MSG_APP_STATE = 0x2000;
export const MSG_APP_INPUT = 0x2001;
export const MSG_UI_READY = 0x2002;
export const MSG_APP_FOCUS = 0x2003;
export const MSG_APP_ERROR = 0x2004;

// Capability revocation notification (supervisor -> process)
// Payload: [slot: u32, object_type: u8, object_id: u64, reason: u8]
export const MSG_CAP_REVOKED = 0x3010;

// Revocation reasons
export const REVOKE_REASON_EXPLICIT = 1; // Supervisor/user revoked
export const REVOKE_REASON_EXPIRED = 2; // Capability expired
export const REVOKE_REASON_PROCESS_EXIT = 3; // Source process exited

// Type tags for payload identification
export const TYPE_CLOCK_STATE = 0x01;
export const TYPE_CALCULATOR_STATE = 0x02;
export const TYPE_BUTTON_PRESS = 0x10;
export const TYPE_TEXT_INPUT = 0x11;
export const TYPE_KEY_PRESS = 0x12;
export const TYPE_FOCUS_CHANGE = 0x13;

// ============================================================================
// Envelope (Wire Format)
// ============================================================================

export interface Envelope {
  version: number;
  typeTag: number;
  payload: Uint8Array;
}

/**
 * Encode an envelope to bytes
 */
export function encodeEnvelope(envelope: Envelope): Uint8Array {
  const payloadLen = envelope.payload.length;
  const bytes = new Uint8Array(4 + payloadLen);

  bytes[0] = envelope.version;
  bytes[1] = envelope.typeTag;
  bytes[2] = payloadLen & 0xff;
  bytes[3] = (payloadLen >> 8) & 0xff;
  bytes.set(envelope.payload, 4);

  return bytes;
}

/**
 * Decode an envelope from bytes
 */
export function decodeEnvelope(data: Uint8Array): Envelope | null {
  if (data.length < 4) {
    console.error('App protocol: data too short for envelope header');
    return null;
  }

  const version = data[0];
  if (version !== PROTOCOL_VERSION) {
    console.error(`App protocol: unknown version ${version}`);
    return null;
  }

  const typeTag = data[1];
  const payloadLen = data[2] | (data[3] << 8);

  if (data.length < 4 + payloadLen) {
    console.error(
      `App protocol: payload overflow (declared ${payloadLen}, available ${data.length - 4})`
    );
    return null;
  }

  const payload = data.slice(4, 4 + payloadLen);

  return { version, typeTag, payload };
}

// ============================================================================
// String Encoding Helpers
// ============================================================================

/**
 * Encode a string as length-prefixed UTF-8 (u16 length)
 */
export function encodeString(s: string): Uint8Array {
  const encoder = new TextEncoder();
  const bytes = encoder.encode(s);
  const result = new Uint8Array(2 + bytes.length);
  result[0] = bytes.length & 0xff;
  result[1] = (bytes.length >> 8) & 0xff;
  result.set(bytes, 2);
  return result;
}

/**
 * Decode a length-prefixed string from data at the given cursor position
 */
export function decodeString(
  data: Uint8Array,
  cursor: { pos: number }
): string | null {
  if (cursor.pos + 2 > data.length) {
    return null;
  }

  const len = data[cursor.pos] | (data[cursor.pos + 1] << 8);
  cursor.pos += 2;

  if (cursor.pos + len > data.length) {
    return null;
  }

  const bytes = data.slice(cursor.pos, cursor.pos + len);
  cursor.pos += len;

  const decoder = new TextDecoder();
  return decoder.decode(bytes);
}

/**
 * Decode a u8 from data at the given cursor position
 */
export function decodeU8(
  data: Uint8Array,
  cursor: { pos: number }
): number | null {
  if (cursor.pos >= data.length) {
    return null;
  }
  const value = data[cursor.pos];
  cursor.pos += 1;
  return value;
}

/**
 * Decode a u32 (little-endian) from data at the given cursor position
 */
export function decodeU32(
  data: Uint8Array,
  cursor: { pos: number }
): number | null {
  if (cursor.pos + 4 > data.length) {
    return null;
  }
  const value =
    data[cursor.pos] |
    (data[cursor.pos + 1] << 8) |
    (data[cursor.pos + 2] << 16) |
    (data[cursor.pos + 3] << 24);
  cursor.pos += 4;
  return value >>> 0; // Convert to unsigned
}

/**
 * Decode an optional char (0x00 = None, 0x01 + u32 = Some(char))
 */
export function decodeOptionalChar(
  data: Uint8Array,
  cursor: { pos: number }
): string | null {
  const hasValue = decodeU8(data, cursor);
  if (hasValue === null) return null;

  if (hasValue === 0) {
    return null;
  }

  const code = decodeU32(data, cursor);
  if (code === null) return null;

  return String.fromCodePoint(code);
}

// ============================================================================
// Clock State
// ============================================================================

export interface ClockState {
  timeDisplay: string;
  dateDisplay: string;
  is24Hour: boolean;
  timezone: string;
}

/**
 * Decode ClockState from bytes (received via IPC)
 */
export function decodeClockState(data: Uint8Array): ClockState | null {
  const envelope = decodeEnvelope(data);
  if (!envelope) return null;

  if (envelope.typeTag !== TYPE_CLOCK_STATE) {
    console.error(
      `Expected CLOCK_STATE (${TYPE_CLOCK_STATE}), got ${envelope.typeTag}`
    );
    return null;
  }

  const payload = envelope.payload;
  if (payload.length === 0) {
    return null;
  }

  // Skip type tag in payload (byte 0)
  const cursor = { pos: 1 };

  const timeDisplay = decodeString(payload, cursor);
  if (timeDisplay === null) return null;

  const dateDisplay = decodeString(payload, cursor);
  if (dateDisplay === null) return null;

  const is24Hour = decodeU8(payload, cursor);
  if (is24Hour === null) return null;

  const timezone = decodeString(payload, cursor);
  if (timezone === null) return null;

  return {
    timeDisplay,
    dateDisplay,
    is24Hour: is24Hour !== 0,
    timezone,
  };
}

// ============================================================================
// Calculator State
// ============================================================================

export interface CalculatorState {
  display: string;
  pendingOp: string | null;
  hasError: boolean;
  memoryIndicator: boolean;
}

/**
 * Decode CalculatorState from bytes (received via IPC)
 */
export function decodeCalculatorState(data: Uint8Array): CalculatorState | null {
  const envelope = decodeEnvelope(data);
  if (!envelope) return null;

  if (envelope.typeTag !== TYPE_CALCULATOR_STATE) {
    console.error(
      `Expected CALCULATOR_STATE (${TYPE_CALCULATOR_STATE}), got ${envelope.typeTag}`
    );
    return null;
  }

  const payload = envelope.payload;
  if (payload.length === 0) {
    return null;
  }

  // Skip type tag in payload (byte 0)
  const cursor = { pos: 1 };

  const display = decodeString(payload, cursor);
  if (display === null) return null;

  const pendingOp = decodeOptionalChar(payload, cursor);

  const hasError = decodeU8(payload, cursor);
  if (hasError === null) return null;

  const memoryIndicator = decodeU8(payload, cursor);
  if (memoryIndicator === null) return null;

  return {
    display,
    pendingOp,
    hasError: hasError !== 0,
    memoryIndicator: memoryIndicator !== 0,
  };
}

// ============================================================================
// Input Events (UI -> App)
// ============================================================================

export type InputEvent =
  | { type: 'button'; name: string }
  | { type: 'text'; text: string }
  | { type: 'key'; keyCode: number; modifiers: number }
  | { type: 'focus'; gained: boolean };

/**
 * Create a button press input event
 */
export function buttonPress(name: string): InputEvent {
  return { type: 'button', name };
}

/**
 * Encode an InputEvent to bytes (for sending via IPC)
 */
export function encodeInputEvent(event: InputEvent): Uint8Array {
  let typeTag: number;
  let payloadParts: Uint8Array[];

  switch (event.type) {
    case 'button': {
      typeTag = TYPE_BUTTON_PRESS;
      payloadParts = [new Uint8Array([TYPE_BUTTON_PRESS]), encodeString(event.name)];
      break;
    }
    case 'text': {
      typeTag = TYPE_TEXT_INPUT;
      payloadParts = [new Uint8Array([TYPE_TEXT_INPUT]), encodeString(event.text)];
      break;
    }
    case 'key': {
      typeTag = TYPE_KEY_PRESS;
      const payload = new Uint8Array(6);
      payload[0] = TYPE_KEY_PRESS;
      payload[1] = event.keyCode & 0xff;
      payload[2] = (event.keyCode >> 8) & 0xff;
      payload[3] = (event.keyCode >> 16) & 0xff;
      payload[4] = (event.keyCode >> 24) & 0xff;
      payload[5] = event.modifiers;
      payloadParts = [payload];
      break;
    }
    case 'focus': {
      typeTag = TYPE_FOCUS_CHANGE;
      payloadParts = [new Uint8Array([TYPE_FOCUS_CHANGE, event.gained ? 1 : 0])];
      break;
    }
  }

  // Calculate total payload length
  const payloadLen = payloadParts.reduce((sum, p) => sum + p.length, 0);
  const payload = new Uint8Array(payloadLen);
  let offset = 0;
  for (const part of payloadParts) {
    payload.set(part, offset);
    offset += part.length;
  }

  return encodeEnvelope({
    version: PROTOCOL_VERSION,
    typeTag,
    payload,
  });
}

// Modifier key constants
export const MODIFIER_SHIFT = 1;
export const MODIFIER_CTRL = 2;
export const MODIFIER_ALT = 4;

// ============================================================================
// Permission Protocol (03-security.md)
// Messages for permission management (Desktop/Supervisor -> Init)
// ============================================================================

/** Request Init to grant a capability to a process */
export const MSG_GRANT_PERMISSION = 0x1010;

/** Request Init to revoke a capability from a process */
export const MSG_REVOKE_PERMISSION = 0x1011;

/** Query what permissions a process has */
export const MSG_LIST_PERMISSIONS = 0x1012;

/** Response from Init with grant/revoke result */
export const MSG_PERMISSION_RESPONSE = 0x1013;

// Type tags for permission payloads
export const TYPE_GRANT_REQUEST = 0x20;
export const TYPE_REVOKE_REQUEST = 0x21;
export const TYPE_PERMISSION_RESPONSE = 0x22;
export const TYPE_PERMISSION_LIST = 0x23;

// ============================================================================
// Permission Types
// ============================================================================

/**
 * Types of kernel objects that can be accessed via capabilities
 */
export type ObjectType =
  | 'Endpoint'
  | 'Console'
  | 'Storage'
  | 'Network'
  | 'Process'
  | 'Memory';

/** Object type enum values (matching Rust kernel) */
export const OBJECT_TYPE = {
  Endpoint: 1,
  Console: 2,
  Storage: 3,
  Network: 4,
  Process: 5,
  Memory: 6,
} as const;

/**
 * Permission bits for capabilities
 */
export interface Permissions {
  read: boolean;
  write: boolean;
  grant: boolean;
}

/**
 * Encode permissions as a single byte
 */
export function encodePermissions(perms: Permissions): number {
  let byte = 0;
  if (perms.read) byte |= 0x01;
  if (perms.write) byte |= 0x02;
  if (perms.grant) byte |= 0x04;
  return byte;
}

/**
 * Decode permissions from a single byte
 */
export function decodePermissions(byte: number): Permissions {
  return {
    read: (byte & 0x01) !== 0,
    write: (byte & 0x02) !== 0,
    grant: (byte & 0x04) !== 0,
  };
}

/**
 * Capability request from an app's manifest
 */
export interface CapabilityRequest {
  /** Type of kernel object being requested */
  objectType: ObjectType;
  /** Permissions needed on this object */
  permissions: Permissions;
  /** Human-readable reason (shown to user in permission dialog) */
  reason: string;
  /** Whether this permission is required for the app to function */
  required: boolean;
}

// ============================================================================
// Grant Request (Desktop -> Init)
// ============================================================================

export interface GrantRequest {
  /** Target process to grant capability to */
  targetPid: number;
  /** Type of object being granted */
  objectType: ObjectType;
  /** Permissions to grant */
  permissions: Permissions;
  /** Reason (from AppManifest, for logging) */
  reason: string;
}

/**
 * Encode a grant request to bytes
 */
export function encodeGrantRequest(request: GrantRequest): Uint8Array {
  const reasonBytes = new TextEncoder().encode(request.reason);

  // Format: type_tag (1) + target_pid (4) + object_type (1) + perms (1) + reason_len (2) + reason
  const size = 1 + 4 + 1 + 1 + 2 + reasonBytes.length;
  const bytes = new Uint8Array(size);
  let offset = 0;

  // Type tag
  bytes[offset++] = TYPE_GRANT_REQUEST;

  // Target PID (little-endian u32)
  bytes[offset++] = request.targetPid & 0xff;
  bytes[offset++] = (request.targetPid >> 8) & 0xff;
  bytes[offset++] = (request.targetPid >> 16) & 0xff;
  bytes[offset++] = (request.targetPid >> 24) & 0xff;

  // Object type
  bytes[offset++] = OBJECT_TYPE[request.objectType];

  // Permissions
  bytes[offset++] = encodePermissions(request.permissions);

  // Reason (length-prefixed)
  bytes[offset++] = reasonBytes.length & 0xff;
  bytes[offset++] = (reasonBytes.length >> 8) & 0xff;
  bytes.set(reasonBytes, offset);

  return encodeEnvelope({
    version: PROTOCOL_VERSION,
    typeTag: TYPE_GRANT_REQUEST,
    payload: bytes,
  });
}

// ============================================================================
// Revoke Request (Desktop -> Init)
// ============================================================================

export interface RevokeRequest {
  /** Target process to revoke capability from */
  targetPid: number;
  /** Type of object being revoked */
  objectType: ObjectType;
}

/**
 * Encode a revoke request to bytes
 */
export function encodeRevokeRequest(request: RevokeRequest): Uint8Array {
  // Format: type_tag (1) + target_pid (4) + object_type (1)
  const bytes = new Uint8Array(6);

  bytes[0] = TYPE_REVOKE_REQUEST;

  // Target PID (little-endian u32)
  bytes[1] = request.targetPid & 0xff;
  bytes[2] = (request.targetPid >> 8) & 0xff;
  bytes[3] = (request.targetPid >> 16) & 0xff;
  bytes[4] = (request.targetPid >> 24) & 0xff;

  // Object type
  bytes[5] = OBJECT_TYPE[request.objectType];

  return encodeEnvelope({
    version: PROTOCOL_VERSION,
    typeTag: TYPE_REVOKE_REQUEST,
    payload: bytes,
  });
}

// ============================================================================
// Permission Response (Init -> Desktop)
// ============================================================================

export interface PermissionResponse {
  /** Whether the operation succeeded */
  success: boolean;
  /** Capability slot where cap was inserted (if success) */
  slot?: number;
  /** Error message (if !success) */
  error?: string;
}

/**
 * Decode a permission response from bytes
 */
export function decodePermissionResponse(
  data: Uint8Array
): PermissionResponse | null {
  const envelope = decodeEnvelope(data);
  if (!envelope) return null;

  if (envelope.typeTag !== TYPE_PERMISSION_RESPONSE) {
    console.error(
      `Expected PERMISSION_RESPONSE (${TYPE_PERMISSION_RESPONSE}), got ${envelope.typeTag}`
    );
    return null;
  }

  const payload = envelope.payload;
  if (payload.length < 2) return null;

  const cursor = { pos: 1 }; // Skip type tag

  const success = decodeU8(payload, cursor);
  if (success === null) return null;

  if (success !== 0) {
    // Success: read slot
    const slot = decodeU32(payload, cursor);
    return { success: true, slot: slot ?? undefined };
  } else {
    // Failure: read error message
    const error = decodeString(payload, cursor);
    return { success: false, error: error ?? 'Unknown error' };
  }
}

// ============================================================================
// Permission List (Init -> Desktop)
// ============================================================================

export interface CapabilityInfo {
  /** Capability slot */
  slot: number;
  /** Object type */
  objectType: ObjectType;
  /** Permissions */
  permissions: Permissions;
}

export interface PermissionListResponse {
  /** Process ID */
  pid: number;
  /** List of granted capabilities */
  capabilities: CapabilityInfo[];
}

/**
 * Decode object type from numeric value
 */
function decodeObjectType(value: number): ObjectType | null {
  switch (value) {
    case 1:
      return 'Endpoint';
    case 2:
      return 'Console';
    case 3:
      return 'Storage';
    case 4:
      return 'Network';
    case 5:
      return 'Process';
    case 6:
      return 'Memory';
    default:
      return null;
  }
}

/**
 * Decode a permission list response from bytes
 */
export function decodePermissionList(
  data: Uint8Array
): PermissionListResponse | null {
  const envelope = decodeEnvelope(data);
  if (!envelope) return null;

  if (envelope.typeTag !== TYPE_PERMISSION_LIST) {
    console.error(
      `Expected PERMISSION_LIST (${TYPE_PERMISSION_LIST}), got ${envelope.typeTag}`
    );
    return null;
  }

  const payload = envelope.payload;
  if (payload.length < 6) return null;

  const cursor = { pos: 1 }; // Skip type tag

  // PID
  const pid = decodeU32(payload, cursor);
  if (pid === null) return null;

  // Capability count
  const count = decodeU8(payload, cursor);
  if (count === null) return null;

  const capabilities: CapabilityInfo[] = [];

  for (let i = 0; i < count; i++) {
    const slot = decodeU32(payload, cursor);
    const objectTypeValue = decodeU8(payload, cursor);
    const permsValue = decodeU8(payload, cursor);

    if (slot === null || objectTypeValue === null || permsValue === null) {
      return null;
    }

    const objectType = decodeObjectType(objectTypeValue);
    if (objectType === null) continue;

    capabilities.push({
      slot,
      objectType,
      permissions: decodePermissions(permsValue),
    });
  }

  return { pid, capabilities };
}
