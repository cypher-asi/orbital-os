/**
 * Identity Service Error Classes
 *
 * Typed error classes for Identity Service operations.
 * Each error type corresponds to a specific failure mode.
 */

// =============================================================================
// Base Error Class
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

// =============================================================================
// Service Communication Errors
// =============================================================================

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

// =============================================================================
// Neural Key Errors
// =============================================================================

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
 * Neural Key verification failed - provided shards don't match stored identity.
 */
export class NeuralKeyMismatchError extends IdentityServiceError {
  constructor() {
    super('Neural Key verification failed - the provided shards do not match your stored identity');
    this.name = 'NeuralKeyMismatchError';
  }
}

// =============================================================================
// Credential Errors
// =============================================================================

/**
 * Credential already linked.
 */
export class CredentialAlreadyLinkedError extends IdentityServiceError {
  constructor() {
    super('This credential is already linked to your account');
    this.name = 'CredentialAlreadyLinkedError';
  }
}

/**
 * Invalid credential format.
 */
export class InvalidCredentialFormatError extends IdentityServiceError {
  constructor() {
    super('Invalid credential format');
    this.name = 'InvalidCredentialFormatError';
  }
}

/**
 * Verification failed (wrong code).
 */
export class VerificationFailedError extends IdentityServiceError {
  constructor() {
    super('Verification failed - invalid code');
    this.name = 'VerificationFailedError';
  }
}

/**
 * Verification code expired.
 */
export class VerificationCodeExpiredError extends IdentityServiceError {
  constructor() {
    super('Verification code has expired');
    this.name = 'VerificationCodeExpiredError';
  }
}

/**
 * No pending verification for this email.
 */
export class NoPendingVerificationError extends IdentityServiceError {
  constructor() {
    super('No pending verification for this email');
    this.name = 'NoPendingVerificationError';
  }
}

/**
 * Credential not found.
 */
export class CredentialNotFoundError extends IdentityServiceError {
  constructor() {
    super('Credential not found');
    this.name = 'CredentialNotFoundError';
  }
}

// =============================================================================
// ZID Auth Errors
// =============================================================================

/**
 * ZID network error during API call.
 */
export class ZidNetworkError extends IdentityServiceError {
  public readonly reason: string;

  constructor(reason: string) {
    super(`ZID network error: ${reason}`);
    this.name = 'ZidNetworkError';
    this.reason = reason;
  }
}

/**
 * ZID authentication failed (invalid signature, unknown machine).
 */
export class ZidAuthenticationFailedError extends IdentityServiceError {
  constructor() {
    super('ZID authentication failed');
    this.name = 'ZidAuthenticationFailedError';
  }
}

/**
 * ZID challenge expired or invalid.
 */
export class ZidInvalidChallengeError extends IdentityServiceError {
  constructor() {
    super('ZID challenge expired or invalid');
    this.name = 'ZidInvalidChallengeError';
  }
}

/**
 * Machine key not found for ZID login.
 */
export class ZidMachineKeyNotFoundError extends IdentityServiceError {
  constructor() {
    super('Machine key not found for ZID login');
    this.name = 'ZidMachineKeyNotFoundError';
  }
}

/**
 * ZID server error.
 */
export class ZidServerError extends IdentityServiceError {
  public readonly reason: string;

  constructor(reason: string) {
    super(`ZID server error: ${reason}`);
    this.name = 'ZidServerError';
    this.reason = reason;
  }
}

/**
 * Machine not registered with ZID server.
 * The local machine key exists but hasn't been enrolled with the ZID server.
 * User needs to register/enroll their machine first.
 */
export class ZidMachineNotRegisteredError extends IdentityServiceError {
  public readonly reason: string;

  constructor(reason: string) {
    super(`Machine not registered with ZID: ${reason}`);
    this.name = 'ZidMachineNotRegisteredError';
    this.reason = reason;
  }
}

/**
 * ZID session expired.
 */
export class ZidSessionExpiredError extends IdentityServiceError {
  constructor() {
    super('ZID session expired');
    this.name = 'ZidSessionExpiredError';
  }
}

/**
 * ZID invalid refresh token.
 */
export class ZidInvalidRefreshTokenError extends IdentityServiceError {
  constructor() {
    super('ZID refresh token is invalid or expired');
    this.name = 'ZidInvalidRefreshTokenError';
  }
}

/**
 * ZID enrollment failed.
 */
export class ZidEnrollmentFailedError extends IdentityServiceError {
  public readonly reason: string;

  constructor(reason: string) {
    super(`ZID enrollment failed: ${reason}`);
    this.name = 'ZidEnrollmentFailedError';
    this.reason = reason;
  }
}

// =============================================================================
// Error Parsing
// =============================================================================

/**
 * Parse string error codes to typed errors.
 */
function parseStringError(err: string): IdentityServiceError {
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
    case 'NeuralKeyMismatch':
      return new NeuralKeyMismatchError();
    // Credential errors
    case 'AlreadyLinked':
      return new CredentialAlreadyLinkedError();
    case 'InvalidFormat':
      return new InvalidCredentialFormatError();
    case 'VerificationFailed':
      return new VerificationFailedError();
    case 'CodeExpired':
      return new VerificationCodeExpiredError();
    case 'NoPendingVerification':
      return new NoPendingVerificationError();
    case 'NotFound':
      return new CredentialNotFoundError();
    // ZID errors
    case 'AuthenticationFailed':
      return new ZidAuthenticationFailedError();
    case 'InvalidChallenge':
      return new ZidInvalidChallengeError();
    case 'SessionExpired':
      return new ZidSessionExpiredError();
    case 'InvalidRefreshToken':
      return new ZidInvalidRefreshTokenError();
    default:
      return new IdentityServiceError(err);
  }
}

/**
 * Parse structured errors to typed errors.
 */
function parseStructuredError(err: Record<string, string>): IdentityServiceError {
  const keys = Object.keys(err);
  if (keys.length === 0) {
    return new IdentityServiceError('Unknown error');
  }

  const errorType = keys[0];
  const reason = err[errorType];

  switch (errorType) {
    case 'StorageError':
      return new StorageError(reason);
    case 'InvalidShard':
      return new InvalidShardError(reason);
    // ZID structured errors
    case 'NetworkError':
      return new ZidNetworkError(reason);
    case 'ServerError':
      return new ZidServerError(reason);
    case 'MachineNotRegistered':
      return new ZidMachineNotRegisteredError(reason);
    case 'EnrollmentFailed':
      return new ZidEnrollmentFailedError(reason);
    default:
      return new IdentityServiceError(`${errorType}: ${reason}`);
  }
}

/**
 * Parse error from service response.
 * Maps string or structured errors to typed error classes.
 */
export function parseServiceError(err: string | Record<string, string>): IdentityServiceError {
  if (typeof err === 'string') {
    return parseStringError(err);
  }
  return parseStructuredError(err);
}
