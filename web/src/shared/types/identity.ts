/**
 * Canonical UI Identity Types
 *
 * These are the "public API" types used by React components and hooks.
 * They use camelCase naming convention.
 *
 * Service layer types (snake_case) are defined in client-services/identity/types.ts
 * and conversion functions are provided in shared/converters/identity.ts
 */

// =============================================================================
// Key Scheme
// =============================================================================

/** Key scheme for machine keys (matches service snake_case) */
export type KeyScheme = 'classical' | 'pq_hybrid';

// =============================================================================
// Machine Key Types
// =============================================================================

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
 * Capabilities of machine-level keys (UI format).
 * Modern format using string array with camelCase properties.
 */
export interface MachineKeyCapabilities {
  /** List of capability strings */
  capabilities: MachineKeyCapability[];
  /** Expiry time (null = no expiry) */
  expiresAt: number | null;
}

/**
 * Per-machine key record (UI format).
 * Uses camelCase for all properties.
 */
export interface MachineKeyRecord {
  /** Machine ID (128-bit as hex string) */
  machineId: string;
  /** Machine-specific signing public key (Ed25519, hex) */
  signingPublicKey: string;
  /** Machine-specific encryption public key (X25519, hex) */
  encryptionPublicKey: string;
  /** When this machine was authorized */
  authorizedAt: number;
  /** Who authorized this machine (user_id or machine_id as hex) */
  authorizedBy: string;
  /** Machine capabilities */
  capabilities: MachineKeyCapabilities;
  /** Human-readable machine name */
  machineName: string | null;
  /** Last seen timestamp */
  lastSeenAt: number;
  /** Whether this is the current device */
  isCurrentDevice: boolean;
  /** Key epoch (increments on rotation) */
  epoch: number;
  /** Key scheme used (defaults to 'classical') */
  keyScheme: KeyScheme;
  /** PQ signing public key (hex, only for pq_hybrid) */
  pqSigningPublicKey?: string;
  /** PQ encryption public key (hex, only for pq_hybrid) */
  pqEncryptionPublicKey?: string;
}

/**
 * Machine Keys state for UI components.
 */
export interface MachineKeysState {
  /** List of machine key records */
  machines: MachineKeyRecord[];
  /** Current machine ID (if applicable) */
  currentMachineId: string | null;
  /** Loading state */
  isLoading: boolean;
  /** Whether we're in the initial settling period (component should show nothing) */
  isInitializing: boolean;
  /** Error message */
  error: string | null;
}

// =============================================================================
// Neural Key Types
// =============================================================================

/**
 * A Shamir shard for Neural Key backup.
 */
export interface NeuralShard {
  /** Shard index (1-5) */
  index: number;
  /** Shard data as hex string */
  hex: string;
}

/**
 * Public identifiers derived from the Neural Key.
 */
export interface PublicIdentifiers {
  /** Identity-level signing public key (Ed25519, hex string) */
  identitySigningPubKey: string;
  /** Machine-level signing public key (Ed25519, hex string) */
  machineSigningPubKey: string;
  /** Machine-level encryption public key (X25519, hex string) */
  machineEncryptionPubKey: string;
}

/**
 * Result of successful Neural Key generation (UI format).
 */
export interface NeuralKeyGenerated {
  /** The derived user ID (hex string from identity signing public key) */
  userId: string;
  /** Public identifiers (stored server-side) */
  publicIdentifiers: PublicIdentifiers;
  /** Shamir shards (3-of-5) - returned to UI for backup, NOT stored */
  shards: NeuralShard[];
  /** Timestamp when the key was created */
  createdAt: number;
}

/**
 * Neural Key state for UI components.
 */
export interface NeuralKeyState {
  /** Whether a Neural Key exists for the current user */
  hasNeuralKey: boolean;
  /** Public identifiers (if Neural Key exists) */
  publicIdentifiers: PublicIdentifiers | null;
  /** When the key was created */
  createdAt: number | null;
  /** Pending shards (shown during generation, cleared after confirmation) */
  pendingShards: NeuralShard[] | null;
  /** Loading state */
  isLoading: boolean;
  /** Whether we're in the initial settling period */
  isInitializing: boolean;
  /** Error message */
  error: string | null;
}

// =============================================================================
// Credential Types
// =============================================================================

/**
 * Types of linkable credentials (UI format - lowercase).
 */
export type CredentialType = 'email' | 'phone' | 'oauth' | 'webauthn';

/**
 * A linked external credential (UI format).
 */
export interface LinkedCredential {
  /** Credential type */
  type: CredentialType;
  /** Credential value (email address, phone number, etc.) */
  identifier: string;
  /** Whether this credential is verified */
  verified: boolean;
  /** When the credential was linked */
  linkedAt: number;
  /** When verification was completed */
  verifiedAt: number | null;
  /** Is this the primary credential of its type? */
  isPrimary: boolean;
}

/**
 * Linked Accounts state for UI components.
 */
export interface LinkedAccountsState {
  /** Linked credentials */
  credentials: LinkedCredential[];
  /** Loading state */
  isLoading: boolean;
  /** Error message */
  error: string | null;
}
