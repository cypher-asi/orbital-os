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
export {
  VfsStorageClient,
  formatUserId,
  getIdentityKeyStorePath,
  getMachineKeysDir,
  getMachineKeyPath,
  getCredentialsPath,
  getZidSessionPath,
  type VfsInode,
} from './VfsStorageClient';

// Identity utilities
export { userIdToBigInt } from './identityUtils';
