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
  type MachineKeyRecord,
  type LocalKeyStore,
  type Supervisor,
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
} from './IdentityServiceClient';

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
  type VfsInode,
} from './VfsStorageClient';
