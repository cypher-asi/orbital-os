/**
 * Identity Service IPC Client
 *
 * This TypeScript client mirrors the Rust IPC types from zos-identity/src/ipc.rs
 * and provides a clean API for React hooks to interact with the identity service.
 *
 * Architecture:
 * - Client constructs JSON IPC messages with proper message tags
 * - Supervisor provides generic send_service_ipc() and callback registration
 * - No identity-specific logic in supervisor (thin boundary layer)
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
  public_identifiers: PublicIdentifiers;
  shards: NeuralShard[];
  created_at: number;
}

/** Machine key capabilities */
export interface MachineKeyCapabilities {
  can_authenticate: boolean;
  can_encrypt: boolean;
  can_sign_messages: boolean;
  can_authorize_machines: boolean;
  can_revoke_machines: boolean;
  expires_at: number | null;
}

/** Machine key record */
export interface MachineKeyRecord {
  machine_id: number | string;
  signing_public_key: number[];
  encryption_public_key: number[];
  authorized_at: number;
  authorized_by: number | string;
  capabilities: MachineKeyCapabilities;
  machine_name: string | null;
  last_seen_at: number;
  /** Key epoch (increments on rotation) */
  epoch: number;
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

// =============================================================================
// Response types
// =============================================================================

interface ResultOk<T> {
  Ok: T;
}

interface ResultErr {
  Err: string | Record<string, string>;
}

type Result<T> = ResultOk<T> | ResultErr;

// =============================================================================
// Typed Error Classes
// =============================================================================

/**
 * Base class for Identity Service errors.
 * All service-specific errors extend this class.
 */
export class IdentityServiceError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'IdentityServiceError';
    // Maintains proper stack trace for where error was thrown (V8 only)
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, this.constructor);
    }
  }
}

/**
 * Service was not found or is not running.
 */
export class ServiceNotFoundError extends IdentityServiceError {
  public readonly serviceName: string;

  constructor(serviceName: string) {
    super(`Service not found: ${serviceName}`);
    this.name = 'ServiceNotFoundError';
    this.serviceName = serviceName;
  }
}

/**
 * Failed to deliver IPC message to the service.
 */
export class DeliveryFailedError extends IdentityServiceError {
  public readonly reason: string;

  constructor(reason: string) {
    super(`Message delivery failed: ${reason}`);
    this.name = 'DeliveryFailedError';
    this.reason = reason;
  }
}

/**
 * Request timed out waiting for response.
 */
export class RequestTimeoutError extends IdentityServiceError {
  public readonly timeoutMs: number;

  constructor(timeoutMs: number) {
    super(`Request timed out after ${timeoutMs}ms`);
    this.name = 'RequestTimeoutError';
    this.timeoutMs = timeoutMs;
  }
}

/**
 * Identity key already exists for this user.
 */
export class IdentityKeyAlreadyExistsError extends IdentityServiceError {
  constructor() {
    super('Identity key already exists for this user');
    this.name = 'IdentityKeyAlreadyExistsError';
  }
}

/**
 * Identity key is required but not found.
 */
export class IdentityKeyRequiredError extends IdentityServiceError {
  constructor() {
    super('Identity key must exist before this operation');
    this.name = 'IdentityKeyRequiredError';
  }
}

/**
 * Machine key was not found.
 */
export class MachineKeyNotFoundError extends IdentityServiceError {
  constructor() {
    super('Machine key not found');
    this.name = 'MachineKeyNotFoundError';
  }
}

/**
 * Insufficient shards provided for key recovery.
 */
export class InsufficientShardsError extends IdentityServiceError {
  constructor() {
    super('At least 3 shards are required for key recovery');
    this.name = 'InsufficientShardsError';
  }
}

/**
 * Invalid shard data provided.
 */
export class InvalidShardError extends IdentityServiceError {
  public readonly reason: string;

  constructor(reason: string) {
    super(`Invalid shard: ${reason}`);
    this.name = 'InvalidShardError';
    this.reason = reason;
  }
}

/**
 * Storage operation failed (VFS error, serialization, etc.).
 */
export class StorageError extends IdentityServiceError {
  public readonly reason: string;

  constructor(reason: string) {
    super(`Storage error: ${reason}`);
    this.name = 'StorageError';
    this.reason = reason;
  }
}

/**
 * Key derivation failed.
 */
export class DerivationFailedError extends IdentityServiceError {
  constructor() {
    super('Key derivation failed');
    this.name = 'DerivationFailedError';
  }
}

/**
 * Parse error from service response.
 * Maps string or structured errors to typed error classes.
 */
function parseServiceError(err: string | Record<string, string>): IdentityServiceError {
  if (typeof err === 'string') {
    // Handle known string error codes
    switch (err) {
      case 'IdentityKeyAlreadyExists':
        return new IdentityKeyAlreadyExistsError();
      case 'IdentityKeyRequired':
        return new IdentityKeyRequiredError();
      case 'MachineKeyNotFound':
        return new MachineKeyNotFoundError();
      case 'InsufficientShards':
        return new InsufficientShardsError();
      case 'DerivationFailed':
        return new DerivationFailedError();
      default:
        return new IdentityServiceError(err);
    }
  }

  // Handle structured errors like { StorageError: "reason" }
  const keys = Object.keys(err);
  if (keys.length > 0) {
    const errorType = keys[0];
    const reason = err[errorType];
    
    switch (errorType) {
      case 'StorageError':
        return new StorageError(reason);
      case 'InvalidShard':
        return new InvalidShardError(reason);
      default:
        return new IdentityServiceError(`${errorType}: ${reason}`);
    }
  }

  return new IdentityServiceError('Unknown error');
}

interface GenerateNeuralKeyResponse {
  result: Result<NeuralKeyGenerated>;
}

interface RecoverNeuralKeyResponse {
  result: Result<NeuralKeyGenerated>;
}

interface GetIdentityKeyResponse {
  result: Result<LocalKeyStore | null>;
}

interface CreateMachineKeyResponse {
  result: Result<MachineKeyRecord>;
}

interface ListMachineKeysResponse {
  machines: MachineKeyRecord[];
}

interface RevokeMachineKeyResponse {
  result: Result<void>;
}

interface RotateMachineKeyResponse {
  result: Result<MachineKeyRecord>;
}

// =============================================================================
// Supervisor interface (minimal subset needed by this client)
// =============================================================================

export interface Supervisor {
  /** Register callback for IPC responses (event-based) */
  set_ipc_response_callback(callback: (requestId: string, data: string) => void): void;
  /** Send IPC to a named service, returns request_id */
  send_service_ipc(serviceName: string, tag: number, data: string): string;
  /** Process pending syscalls (needed to let service run) */
  poll_syscalls(): number;
}

// =============================================================================
// Pending request management
// =============================================================================

interface PendingRequest<T> {
  resolve: (data: T) => void;
  reject: (error: Error) => void;
  timeoutId: ReturnType<typeof setTimeout>;
  uniqueId: string;
}

/**
 * Counter for generating unique request IDs.
 * Combined with the response tag to create truly unique identifiers.
 */
let requestCounter = 0;

/**
 * Map of pending requests by response tag (hex).
 * Each tag has a FIFO queue of pending requests to handle concurrent requests
 * of the same message type.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const pendingRequestsByTag = new Map<string, PendingRequest<any>[]>();

/**
 * Map of pending requests by unique ID (for timeout cleanup).
 * This allows us to find and remove a specific request from its tag's queue.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const pendingRequestsById = new Map<string, { tagHex: string; request: PendingRequest<any> }>();

/** Track whether callback has been registered */
let callbackRegistered = false;

/** Track the supervisor we've registered with */
let registeredSupervisor: Supervisor | null = null;

/**
 * Generate a unique request ID.
 * Format: {counter}-{tag_hex}
 */
function generateUniqueRequestId(tagHex: string): string {
  return `${++requestCounter}-${tagHex}`;
}

/**
 * Add a pending request to the queue for its tag.
 */
function addPendingRequest<T>(tagHex: string, request: PendingRequest<T>): void {
  let queue = pendingRequestsByTag.get(tagHex);
  if (!queue) {
    queue = [];
    pendingRequestsByTag.set(tagHex, queue);
  }
  queue.push(request);
  pendingRequestsById.set(request.uniqueId, { tagHex, request });
}

/**
 * Remove a pending request by its unique ID (used for timeout cleanup).
 */
function removePendingRequestById(uniqueId: string): boolean {
  const entry = pendingRequestsById.get(uniqueId);
  if (!entry) return false;

  const { tagHex, request } = entry;
  const queue = pendingRequestsByTag.get(tagHex);
  if (queue) {
    const index = queue.indexOf(request);
    if (index !== -1) {
      queue.splice(index, 1);
      if (queue.length === 0) {
        pendingRequestsByTag.delete(tagHex);
      }
    }
  }
  pendingRequestsById.delete(uniqueId);
  return true;
}

/**
 * Resolve the oldest pending request for a given tag (FIFO).
 */
function resolveOldestPendingRequest(tagHex: string, data: unknown): boolean {
  const queue = pendingRequestsByTag.get(tagHex);
  if (!queue || queue.length === 0) {
    return false;
  }

  // FIFO: resolve the oldest request (first in queue)
  const request = queue.shift()!;
  clearTimeout(request.timeoutId);
  pendingRequestsById.delete(request.uniqueId);

  if (queue.length === 0) {
    pendingRequestsByTag.delete(tagHex);
  }

  request.resolve(data);
  return true;
}

/**
 * Ensure the IPC response callback is registered with the supervisor.
 * This is called once per supervisor instance.
 */
function ensureCallbackRegistered(supervisor: Supervisor): void {
  // Only register once per supervisor
  if (callbackRegistered && registeredSupervisor === supervisor) {
    return;
  }

  // Register the callback for ALL IPC responses (event-based, no polling)
  supervisor.set_ipc_response_callback((requestId: string, data: string) => {
    // requestId is the response tag hex (e.g., "00007055")
    // We use this to find the queue of pending requests for that tag
    try {
      const parsed = JSON.parse(data);
      if (resolveOldestPendingRequest(requestId, parsed)) {
        // Successfully resolved a pending request
      } else {
        console.log(`[IdentityServiceClient] Received response for tag ${requestId} with no pending requests`);
      }
    } catch (e) {
      // Try to reject the oldest pending request for this tag
      const queue = pendingRequestsByTag.get(requestId);
      if (queue && queue.length > 0) {
        const request = queue.shift()!;
        clearTimeout(request.timeoutId);
        pendingRequestsById.delete(request.uniqueId);
        if (queue.length === 0) {
          pendingRequestsByTag.delete(requestId);
        }
        request.reject(new Error(`Invalid response JSON: ${e}`));
      } else {
        console.log(`[IdentityServiceClient] Received invalid JSON for tag ${requestId} with no pending requests`);
      }
    }
  });

  callbackRegistered = true;
  registeredSupervisor = supervisor;
  console.log('[IdentityServiceClient] IPC response callback registered');
}

// =============================================================================
// IdentityServiceClient
// =============================================================================

/**
 * Client for Identity Service IPC communication.
 *
 * Uses the supervisor's generic IPC APIs to communicate with the identity
 * service. All message construction and parsing is done in TypeScript.
 */
export class IdentityServiceClient {
  private supervisor: Supervisor;
  private timeoutMs: number;

  constructor(supervisor: Supervisor, timeoutMs = 10000) {
    this.supervisor = supervisor;
    this.timeoutMs = timeoutMs;
    ensureCallbackRegistered(supervisor);
  }

  /**
   * Send a request to the identity service and wait for response.
   *
   * Uses a FIFO queue per response tag to handle concurrent requests of the
   * same type. Responses are resolved in the order requests were sent.
   *
   * @throws {ServiceNotFoundError} If the identity service is not running
   * @throws {DeliveryFailedError} If the message could not be delivered
   * @throws {RequestTimeoutError} If the request times out
   */
  private async request<T>(tag: number, data: object): Promise<T> {
    const requestJson = JSON.stringify(data);

    // Send via supervisor's generic IPC API
    // The returned requestId is the response tag hex (e.g., "00007055")
    const tagHex = this.supervisor.send_service_ipc('identity', tag, requestJson);

    // Check for immediate errors and throw typed errors
    if (tagHex.startsWith('error:service_not_found:')) {
      const serviceName = tagHex.replace('error:service_not_found:', '');
      throw new ServiceNotFoundError(serviceName);
    }
    if (tagHex.startsWith('error:delivery_failed:')) {
      const reason = tagHex.replace('error:delivery_failed:', '');
      throw new DeliveryFailedError(reason);
    }
    if (tagHex.startsWith('error:')) {
      throw new IdentityServiceError(tagHex);
    }

    // Generate a unique ID for this specific request (for timeout tracking)
    const uniqueId = generateUniqueRequestId(tagHex);
    const timeoutMs = this.timeoutMs;

    // Create a promise that will be resolved by the callback
    return new Promise<T>((resolve, reject) => {
      const timeoutId = setTimeout(() => {
        if (removePendingRequestById(uniqueId)) {
          reject(new RequestTimeoutError(timeoutMs));
        }
      }, timeoutMs);

      const pendingRequest: PendingRequest<T> = {
        resolve: resolve as (data: unknown) => void,
        reject,
        timeoutId,
        uniqueId,
      };

      // Add to the FIFO queue for this response tag
      addPendingRequest(tagHex, pendingRequest);

      // Note: We rely on the global polling loop in main.tsx (setInterval calling poll_syscalls)
      // to process syscalls. The IPC response callback will resolve this promise when
      // the response arrives. Having our own polling loop causes race conditions with
      // the global loop, leading to "recursive use of an object" errors in Rust WASM.
    });
  }

  // ===========================================================================
  // Neural Key Operations
  // ===========================================================================

  /**
   * Generate a new Neural Key for a user.
   *
   * @param userId - User ID (as bigint)
   * @returns NeuralKeyGenerated with shards and public identifiers
   */
  async generateNeuralKey(userId: bigint): Promise<NeuralKeyGenerated> {
    const response = await this.request<GenerateNeuralKeyResponse>(
      MSG.GENERATE_NEURAL_KEY,
      { user_id: Number(userId) }
    );
    return this.unwrapResult(response.result);
  }

  /**
   * Recover a Neural Key from Shamir shards.
   *
   * @param userId - User ID (as bigint)
   * @param shards - At least 3 Shamir shards
   * @returns NeuralKeyGenerated with new shards and public identifiers
   */
  async recoverNeuralKey(userId: bigint, shards: NeuralShard[]): Promise<NeuralKeyGenerated> {
    const response = await this.request<RecoverNeuralKeyResponse>(
      MSG.RECOVER_NEURAL_KEY,
      { user_id: Number(userId), shards }
    );
    return this.unwrapResult(response.result);
  }

  /**
   * Get the stored identity key for a user.
   *
   * @param userId - User ID (as bigint)
   * @returns LocalKeyStore if exists, null otherwise
   */
  async getIdentityKey(userId: bigint): Promise<LocalKeyStore | null> {
    const response = await this.request<GetIdentityKeyResponse>(
      MSG.GET_IDENTITY_KEY,
      { user_id: Number(userId) }
    );
    return this.unwrapResult(response.result);
  }

  // ===========================================================================
  // Machine Key Operations
  // ===========================================================================

  /**
   * Create a new machine key record for a user.
   *
   * The service generates keys from entropy - no public keys needed in request.
   *
   * @param userId - User ID (as bigint)
   * @param machineName - Human-readable machine name
   * @param capabilities - Machine capabilities
   * @returns The created MachineKeyRecord with derived keys
   */
  async createMachineKey(
    userId: bigint,
    machineName: string,
    capabilities: MachineKeyCapabilities
  ): Promise<MachineKeyRecord> {
    const response = await this.request<CreateMachineKeyResponse>(
      MSG.CREATE_MACHINE_KEY,
      {
        user_id: Number(userId),
        machine_name: machineName,
        capabilities,
      }
    );
    return this.unwrapResult(response.result);
  }

  /**
   * List all machine keys for a user.
   *
   * @param userId - User ID (as bigint)
   * @returns Array of MachineKeyRecord
   */
  async listMachineKeys(userId: bigint): Promise<MachineKeyRecord[]> {
    const response = await this.request<ListMachineKeysResponse>(
      MSG.LIST_MACHINE_KEYS,
      { user_id: Number(userId) }
    );
    return response.machines || [];
  }

  /**
   * Revoke/delete a machine key.
   *
   * @param userId - User ID (as bigint)
   * @param machineId - Machine ID to revoke (as bigint)
   */
  async revokeMachineKey(userId: bigint, machineId: bigint): Promise<void> {
    const response = await this.request<RevokeMachineKeyResponse>(
      MSG.REVOKE_MACHINE_KEY,
      { user_id: Number(userId), machine_id: `0x${machineId.toString(16).padStart(32, '0')}` }
    );
    this.unwrapResult(response.result);
  }

  /**
   * Rotate keys for a machine.
   *
   * The service generates new keys from entropy and increments epoch.
   *
   * @param userId - User ID (as bigint)
   * @param machineId - Machine ID to rotate (as bigint)
   * @returns Updated MachineKeyRecord with new keys and incremented epoch
   */
  async rotateMachineKey(userId: bigint, machineId: bigint): Promise<MachineKeyRecord> {
    const response = await this.request<RotateMachineKeyResponse>(
      MSG.ROTATE_MACHINE_KEY,
      {
        user_id: Number(userId),
        machine_id: `0x${machineId.toString(16).padStart(32, '0')}`,
      }
    );
    return this.unwrapResult(response.result);
  }

  // ===========================================================================
  // Helpers
  // ===========================================================================

  /**
   * Unwrap a Result<T> type, throwing typed error on failure.
   */
  private unwrapResult<T>(result: Result<T>): T {
    if ('Err' in result) {
      throw parseServiceError(result.Err);
    }
    return result.Ok;
  }
}
