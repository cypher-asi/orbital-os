/**
 * Identity Service IPC Types
 *
 * TypeScript types mirroring the Rust IPC types from zos-identity/src/ipc.rs
 */

// =============================================================================
// Message Tags (mirrors zos-identity/src/ipc.rs key_msg module)
// =============================================================================

/** IPC message tags for identity service requests/responses */
export const MSG = {
  // Neural Key operations
  GENERATE_NEURAL_KEY: 0x7054,
  GENERATE_NEURAL_KEY_RESPONSE: 0x7055,
  RECOVER_NEURAL_KEY: 0x7056,
  RECOVER_NEURAL_KEY_RESPONSE: 0x7057,
  GET_IDENTITY_KEY: 0x7052,
  GET_IDENTITY_KEY_RESPONSE: 0x7053,
  // Credential operations
  ATTACH_EMAIL: 0x7040,
  ATTACH_EMAIL_RESPONSE: 0x7041,
  GET_CREDENTIALS: 0x7042,
  GET_CREDENTIALS_RESPONSE: 0x7043,
  /** @deprecated ZID handles email verification server-side */
  VERIFY_EMAIL: 0x7044,
  /** @deprecated ZID handles email verification server-side */
  VERIFY_EMAIL_RESPONSE: 0x7045,
  UNLINK_CREDENTIAL: 0x7046,
  UNLINK_CREDENTIAL_RESPONSE: 0x7047,
  // Machine Key operations
  CREATE_MACHINE_KEY: 0x7060,
  CREATE_MACHINE_KEY_RESPONSE: 0x7061,
  LIST_MACHINE_KEYS: 0x7062,
  LIST_MACHINE_KEYS_RESPONSE: 0x7063,
  GET_MACHINE_KEY: 0x7064,
  GET_MACHINE_KEY_RESPONSE: 0x7065,
  REVOKE_MACHINE_KEY: 0x7066,
  REVOKE_MACHINE_KEY_RESPONSE: 0x7067,
  ROTATE_MACHINE_KEY: 0x7068,
  ROTATE_MACHINE_KEY_RESPONSE: 0x7069,
  CREATE_MACHINE_KEY_AND_ENROLL: 0x706a,
  CREATE_MACHINE_KEY_AND_ENROLL_RESPONSE: 0x706b,
  // ZID Auth operations
  ZID_LOGIN: 0x7080,
  ZID_LOGIN_RESPONSE: 0x7081,
  ZID_REFRESH: 0x7082,
  ZID_REFRESH_RESPONSE: 0x7083,
  ZID_ENROLL_MACHINE: 0x7084,
  ZID_ENROLL_MACHINE_RESPONSE: 0x7085,
  ZID_LOGOUT: 0x7086,
  ZID_LOGOUT_RESPONSE: 0x7087,
  // Identity Preferences
  GET_IDENTITY_PREFERENCES: 0x7090,
  GET_IDENTITY_PREFERENCES_RESPONSE: 0x7091,
  SET_DEFAULT_KEY_SCHEME: 0x7092,
  SET_DEFAULT_KEY_SCHEME_RESPONSE: 0x7093,
  SET_DEFAULT_MACHINE_KEY: 0x7094,
  SET_DEFAULT_MACHINE_KEY_RESPONSE: 0x7095,
  // ZID Email Login
  ZID_LOGIN_EMAIL: 0x7088,
  ZID_LOGIN_EMAIL_RESPONSE: 0x7089,
} as const;

// =============================================================================
// Types (mirrors zos-identity/src/ipc.rs)
// =============================================================================

/** A Shamir shard for Neural Key backup */
export interface NeuralShard {
  index: number;
  hex: string;
}

/** Public identifiers derived from Neural Key */
export interface PublicIdentifiers {
  identity_signing_pub_key: string;
  machine_signing_pub_key: string;
  machine_encryption_pub_key: string;
}

/** Result of successful Neural Key generation */
export interface NeuralKeyGenerated {
  /** The derived user ID (hex string from identity signing public key) */
  user_id: string;
  public_identifiers: PublicIdentifiers;
  /**
   * External Shamir shards (3 of 5) for paper backup.
   * The other 2 shards are encrypted with the password and stored in keystore.
   * To create a machine key, user needs 1 external shard + password.
   */
  shards: NeuralShard[];
  created_at: number;
}

/** Key scheme for machine keys (matches Rust snake_case) */
export type KeyScheme = 'classical' | 'pq_hybrid';

/** Machine key capability strings */
export type MachineKeyCapability =
  | 'AUTHENTICATE'
  | 'SIGN'
  | 'ENCRYPT'
  | 'SVK_UNWRAP'
  | 'MLS_MESSAGING'
  | 'VAULT_OPERATIONS'
  | 'AUTHORIZE_MACHINES'
  | 'REVOKE_MACHINES';

/**
 * Machine key capabilities.
 * Modern format uses string array, but supports deserialization from legacy boolean struct.
 */
export interface MachineKeyCapabilities {
  /** List of capability strings */
  capabilities: MachineKeyCapability[];
  /** Expiry time (null = no expiry) */
  expires_at: number | null;
}

/**
 * Legacy machine key capabilities (boolean struct format).
 * Used for backward compatibility with older stored data.
 */
export interface LegacyMachineKeyCapabilities {
  can_authenticate: boolean;
  can_encrypt: boolean;
  can_sign_messages: boolean;
  can_authorize_machines: boolean;
  can_revoke_machines: boolean;
  expires_at: number | null;
}

/**
 * Check if capabilities are in legacy format.
 */
export function isLegacyCapabilities(
  caps: MachineKeyCapabilities | LegacyMachineKeyCapabilities
): caps is LegacyMachineKeyCapabilities {
  return 'can_authenticate' in caps;
}

/**
 * Convert legacy capabilities to modern format.
 */
export function convertLegacyCapabilities(
  legacy: LegacyMachineKeyCapabilities
): MachineKeyCapabilities {
  const capabilities: MachineKeyCapability[] = [];
  if (legacy.can_authenticate) capabilities.push('AUTHENTICATE');
  if (legacy.can_encrypt) capabilities.push('ENCRYPT');
  if (legacy.can_sign_messages) capabilities.push('SIGN');
  if (legacy.can_authorize_machines) capabilities.push('AUTHORIZE_MACHINES');
  if (legacy.can_revoke_machines) capabilities.push('REVOKE_MACHINES');
  return {
    capabilities,
    expires_at: legacy.expires_at,
  };
}

/** Machine key record */
export interface MachineKeyRecord {
  machine_id: number | string;
  signing_public_key: number[];
  encryption_public_key: number[];
  authorized_at: number;
  authorized_by: number | string;
  capabilities: MachineKeyCapabilities | LegacyMachineKeyCapabilities;
  machine_name: string | null;
  last_seen_at: number;
  /** Key epoch (increments on rotation) */
  epoch: number;
  /** Key scheme used (defaults to 'classical' for backward compatibility) */
  key_scheme?: KeyScheme;
  /** ML-DSA-65 PQ signing public key (1952 bytes, only for pq_hybrid).
   * Can be hex string (new format) or number[] (backward compatibility). */
  pq_signing_public_key?: string | number[];
  /** ML-KEM-768 PQ encryption public key (1184 bytes, only for pq_hybrid).
   * Can be hex string (new format) or number[] (backward compatibility). */
  pq_encryption_public_key?: string | number[];
}

/** Local key store (public keys only) */
export interface LocalKeyStore {
  user_id: number;
  identity_signing_public_key: number[];
  machine_signing_public_key: number[];
  machine_encryption_public_key: number[];
  epoch: number;
  /** Timestamp when the key was created (milliseconds since Unix epoch).
   * Optional for backward compatibility with keys created before this field existed. */
  created_at?: number;
}

/** Credential types */
export type CredentialType = 'Email' | 'Phone' | 'OAuth' | 'WebAuthn';

/** A linked credential (mirrors zos-identity/src/keystore.rs LinkedCredential) */
export interface LinkedCredential {
  credential_type: CredentialType;
  value: string;
  verified: boolean;
  linked_at: number;
  verified_at: number | null;
  is_primary: boolean;
}

/** Login type indicating how the session was authenticated */
export type ZidLoginType =
  | 'machine_key'
  | 'neural_key'
  | 'email'
  | 'oauth'
  | 'web_authn'
  | 'recovery';

/** Tokens returned from successful ZID authentication */
export interface ZidTokens {
  /** JWT access token for API calls */
  access_token: string;
  /** Refresh token for obtaining new access tokens */
  refresh_token: string;
  /** Unique session identifier */
  session_id: string;
  /** Machine ID (UUID string) */
  machine_id: string;
  /** When the access token expires (RFC3339 timestamp) */
  expires_at: string;
  /** How this session was authenticated */
  login_type?: ZidLoginType;
  /** Optional warning message */
  warning?: string;
}

/** Persisted ZID session (stored in VFS) */
export interface ZidSession {
  /** ZID API endpoint used for this session */
  zid_endpoint: string;
  /** JWT access token */
  access_token: string;
  /** Refresh token */
  refresh_token: string;
  /** Session ID from ZID server */
  session_id: string;
  /** Machine ID used for authentication (UUID string, optional for backward compat) */
  machine_id?: string;
  /** How this session was authenticated (optional for backward compat) */
  login_type?: ZidLoginType;
  /** When the access token expires (Unix timestamp ms) */
  expires_at: number;
  /** When this session was created (Unix timestamp ms) */
  created_at: number;
}

// =============================================================================
// Response types
// =============================================================================

export interface ResultOk<T> {
  Ok: T;
}

export interface ResultErr {
  Err: string | Record<string, string>;
}

export type Result<T> = ResultOk<T> | ResultErr;

export interface GenerateNeuralKeyResponse {
  result: Result<NeuralKeyGenerated>;
}

export interface RecoverNeuralKeyResponse {
  result: Result<NeuralKeyGenerated>;
}

export interface GetIdentityKeyResponse {
  result: Result<LocalKeyStore | null>;
}

/** Request to create a machine key (mirrors Rust CreateMachineKeyRequest) */
export interface CreateMachineKeyRequest {
  user_id: string;
  machine_name: string | null;
  capabilities: MachineKeyCapabilities;
  key_scheme: KeyScheme;
  /** Single external Neural shard (from paper backup) */
  external_shard: NeuralShard;
  /** Password to decrypt stored shards */
  password: string;
}

export interface CreateMachineKeyResponse {
  result: Result<MachineKeyRecord>;
}

export interface ListMachineKeysResponse {
  machines: MachineKeyRecord[];
}

export interface RevokeMachineKeyResponse {
  result: Result<void>;
}

export interface RotateMachineKeyResponse {
  result: Result<MachineKeyRecord>;
}

export interface AttachEmailResponse {
  // With ZID integration, result is simply success (void) or error
  result: Result<void>;
}

export interface GetCredentialsResponse {
  credentials: LinkedCredential[];
}

export interface UnlinkCredentialResponse {
  result: Result<void>;
}

export interface ZidLoginResponse {
  result: Result<ZidTokens>;
}

export interface ZidEnrollMachineResponse {
  result: Result<ZidTokens>;
}

export interface ZidLogoutResponse {
  result: Result<void>;
}

export interface ZidRefreshResponse {
  result: Result<ZidTokens>;
}

/** ZID email login response */
export interface ZidEmailLoginResponse {
  result: Result<ZidTokens>;
}

/** Combined result of machine key creation and ZID enrollment */
export interface MachineKeyAndTokens {
  /** The created machine key record */
  machine_key: MachineKeyRecord;
  /** ZID tokens from successful enrollment */
  tokens: ZidTokens;
}

/** Create machine key and enroll response */
export interface CreateMachineKeyAndEnrollResponse {
  result: Result<MachineKeyAndTokens>;
}

// =============================================================================
// Identity Preferences
// =============================================================================

/** Identity preferences stored in VFS */
export interface IdentityPreferences {
  /** Default key scheme for new machine keys */
  default_key_scheme: KeyScheme;
  /** Default machine key ID for authentication (hex string) */
  default_machine_id?: string;
}

/** Get identity preferences response */
export interface GetIdentityPreferencesResponse {
  preferences: IdentityPreferences;
}

/** Set default key scheme response */
export interface SetDefaultKeySchemeResponse {
  result: Result<void>;
}

/** Set default machine key response */
export interface SetDefaultMachineKeyResponse {
  result: Result<void>;
}

// =============================================================================
// Supervisor interface (re-exported from shared types)
// =============================================================================

// Import and re-export from the shared types module (single source of truth)
export type { MinimalSupervisor as Supervisor } from '../../shared/types';

// =============================================================================
// Helpers
// =============================================================================

/**
 * Format a user ID (bigint or hex string) as a hex string for Rust interop.
 * Rust expects "0x" prefixed hex strings for u128 values.
 */
export function formatUserIdForRust(userId: bigint | string): string {
  if (typeof userId === 'string') {
    // Already a hex string, ensure it has 0x prefix
    const cleanHex = userId.replace(/^0x/i, '').padStart(32, '0');
    return `0x${cleanHex}`;
  }
  // Convert bigint to hex string
  return `0x${userId.toString(16).padStart(32, '0')}`;
}
