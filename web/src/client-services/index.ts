/**
 * Service clients for Zero OS
 *
 * These TypeScript clients provide type-safe APIs for interacting with
 * system services via the supervisor's generic IPC routing.
 */

export {
  IdentityServiceClient,
  MSG,
  type NeuralShard,
  type PublicIdentifiers,
  type NeuralKeyGenerated,
  type MachineKeyCapabilities,
  type MachineKeyCapability,
  type LegacyMachineKeyCapabilities,
  type MachineKeyRecord,
  type LocalKeyStore,
  type Supervisor,
  type KeyScheme,
  // Capability format helpers
  isLegacyCapabilities,
  convertLegacyCapabilities,
  // Credential types
  type CredentialType,
  type LinkedCredential,
  // ZID types
  type ZidTokens,
  type ZidSession,
  // Typed Error Classes
  IdentityServiceError,
  ServiceNotFoundError,
  DeliveryFailedError,
  RequestTimeoutError,
  IdentityKeyAlreadyExistsError,
  IdentityKeyRequiredError,
  MachineKeyNotFoundError,
  InsufficientShardsError,
  InvalidShardError,
  StorageError,
  DerivationFailedError,
  NeuralKeyMismatchError,
  // Credential Error Classes
  CredentialAlreadyLinkedError,
  InvalidCredentialFormatError,
  VerificationFailedError,
  VerificationCodeExpiredError,
  NoPendingVerificationError,
  CredentialNotFoundError,
  // ZID Error Classes
  ZidNetworkError,
  ZidAuthenticationFailedError,
  ZidInvalidChallengeError,
  ZidMachineKeyNotFoundError,
  ZidServerError,
  ZidMachineNotRegisteredError,
  ZidSessionExpiredError,
  ZidInvalidRefreshTokenError,
  ZidEnrollmentFailedError,
} from './identity';

// Time service for time settings
export {
  TimeServiceClient,
  TIME_MSG,
  DEFAULT_TIME_SETTINGS,
  type TimeSettings,
  TimeServiceError,
  TimeServiceNotFoundError,
  TimeRequestTimeoutError,
} from './TimeServiceClient';

// VFS direct access for React components (reads only)
// NOTE: Identity keys are stored in keystore at /keys/ paths, not in VFS
export {
  VfsStorageClient,
  formatUserId,
  getUserHomeDir,
  getCredentialsPath,
  getZidSessionPath,
  type VfsInode,
} from './VfsStorageClient';

// Keystore direct access for React components (reads only) - for identity keys
// All cryptographic key material is stored in the dedicated keystore at /keys/ paths
export {
  KeystoreClient,
  formatMachineIdHex,
  getIdentityKeystorePath,
  getMachineKeysDir,
  getMachineKeyPath,
} from './KeystoreClient';

// Identity utilities
export { userIdToBigInt, bytesToHex, u128ToHex } from './identityUtils';
