/**
 * Identity Service Module
 *
 * Re-exports all types, errors, and the client for Identity Service IPC.
 */

// Types
export {
  MSG,
  formatUserIdForRust,
  isLegacyCapabilities,
  convertLegacyCapabilities,
  type NeuralShard,
  type PublicIdentifiers,
  type NeuralKeyGenerated,
  type KeyScheme,
  type MachineKeyCapability,
  type MachineKeyCapabilities,
  type LegacyMachineKeyCapabilities,
  type MachineKeyRecord,
  type LocalKeyStore,
  type CredentialType,
  type LinkedCredential,
  type ZidLoginType,
  type ZidTokens,
  type ZidSession,
  type MachineKeyAndTokens,
  type Supervisor,
  type Result,
  type ResultOk,
  type ResultErr,
} from './types';

// Errors
export {
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
  CredentialAlreadyLinkedError,
  InvalidCredentialFormatError,
  VerificationFailedError,
  VerificationCodeExpiredError,
  NoPendingVerificationError,
  CredentialNotFoundError,
  ZidNetworkError,
  ZidAuthenticationFailedError,
  ZidInvalidChallengeError,
  ZidMachineKeyNotFoundError,
  ZidServerError,
  ZidMachineNotRegisteredError,
  ZidSessionExpiredError,
  ZidInvalidRefreshTokenError,
  ZidEnrollmentFailedError,
  parseServiceError,
} from './errors';

// Client
export { IdentityServiceClient } from './IdentityServiceClient';
