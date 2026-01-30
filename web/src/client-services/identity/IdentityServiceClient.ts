/**
 * Identity Service IPC Client
 *
 * Client for Identity Service IPC communication.
 * Uses the supervisor's generic IPC APIs to communicate with the identity
 * service. All message construction and parsing is done in TypeScript.
 */

import {
  MSG,
  formatUserIdForRust,
  type Supervisor,
  type NeuralShard,
  type NeuralKeyGenerated,
  type LocalKeyStore,
  type MachineKeyRecord,
  type MachineKeyCapabilities,
  type KeyScheme,
  type LinkedCredential,
  type CredentialType,
  type ZidTokens,
  type Result,
  type GenerateNeuralKeyResponse,
  type RecoverNeuralKeyResponse,
  type GetIdentityKeyResponse,
  type CreateMachineKeyResponse,
  type ListMachineKeysResponse,
  type RevokeMachineKeyResponse,
  type RotateMachineKeyResponse,
  type AttachEmailResponse,
  type GetCredentialsResponse,
  type UnlinkCredentialResponse,
  type ZidLoginResponse,
  type ZidEnrollMachineResponse,
  type ZidLogoutResponse,
  type ZidRefreshResponse,
  type ZidEmailLoginResponse,
  type IdentityPreferences,
  type GetIdentityPreferencesResponse,
  type SetDefaultKeySchemeResponse,
  type SetDefaultMachineKeyResponse,
  type MachineKeyAndTokens,
  type CreateMachineKeyAndEnrollResponse,
} from './types';

import {
  IdentityServiceError,
  ServiceNotFoundError,
  DeliveryFailedError,
  parseServiceError,
} from './errors';

import { uuidToHex } from '../identityUtils';

import {
  ensureCallbackRegistered,
  addRequestWithPromise,
} from './pendingRequests';

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

    // Use shared request queue to wait for response
    return addRequestWithPromise<T>(tagHex, this.timeoutMs);
  }

  // ===========================================================================
  // Neural Key Operations
  // ===========================================================================

  /**
   * Generate a new Neural Key for a user.
   *
   * @param userId - User ID (as bigint or hex string)
   * @param password - Password for encrypting 2 shards (minimum 12 characters)
   * @returns NeuralKeyGenerated with 3 external shards and public identifiers
   */
  async generateNeuralKey(userId: bigint | string, password: string): Promise<NeuralKeyGenerated> {
    const response = await this.request<GenerateNeuralKeyResponse>(MSG.GENERATE_NEURAL_KEY, {
      user_id: formatUserIdForRust(userId),
      password,
    });
    return this.unwrapResult(response.result);
  }

  /**
   * Recover a Neural Key from Shamir shards.
   *
   * @param userId - User ID (as bigint or hex string)
   * @param shards - At least 3 Shamir shards
   * @returns NeuralKeyGenerated with new shards and public identifiers
   */
  async recoverNeuralKey(
    userId: bigint | string,
    shards: NeuralShard[]
  ): Promise<NeuralKeyGenerated> {
    const response = await this.request<RecoverNeuralKeyResponse>(MSG.RECOVER_NEURAL_KEY, {
      user_id: formatUserIdForRust(userId),
      shards,
    });
    return this.unwrapResult(response.result);
  }

  /**
   * Get the stored identity key for a user.
   *
   * @param userId - User ID (as bigint or hex string)
   * @returns LocalKeyStore if exists, null otherwise
   */
  async getIdentityKey(userId: bigint | string): Promise<LocalKeyStore | null> {
    const response = await this.request<GetIdentityKeyResponse>(MSG.GET_IDENTITY_KEY, {
      user_id: formatUserIdForRust(userId),
    });
    return this.unwrapResult(response.result);
  }

  // ===========================================================================
  // Machine Key Operations
  // ===========================================================================

  /**
   * Create a new machine key record for a user.
   *
   * Machine keys are derived from the user's Neural Key using:
   * - 1 external shard (from paper backup)
   * - Password (to decrypt 2 stored shards from keystore)
   *
   * @param userId - User ID (as bigint)
   * @param machineName - Human-readable machine name
   * @param capabilities - Machine capabilities
   * @param keyScheme - Key scheme to use (defaults to 'classical')
   * @param externalShard - One external Neural shard (from paper backup)
   * @param password - Password to decrypt stored shards
   * @returns The created MachineKeyRecord with derived keys
   */
  async createMachineKey(
    userId: bigint | string,
    machineName: string,
    capabilities: MachineKeyCapabilities,
    keyScheme: KeyScheme | undefined,
    externalShard: NeuralShard,
    password: string
  ): Promise<MachineKeyRecord> {
    if (!externalShard) {
      throw new IdentityServiceError('An external Neural shard is required to create a machine key');
    }
    if (!password) {
      throw new IdentityServiceError('Password is required to create a machine key');
    }
    const response = await this.request<CreateMachineKeyResponse>(MSG.CREATE_MACHINE_KEY, {
      user_id: formatUserIdForRust(userId),
      machine_name: machineName,
      capabilities,
      key_scheme: keyScheme ?? 'classical',
      external_shard: externalShard,
      password,
    });
    return this.unwrapResult(response.result);
  }

  /**
   * List all machine keys for a user.
   *
   * @param userId - User ID (as bigint)
   * @returns Array of MachineKeyRecord
   */
  async listMachineKeys(userId: bigint | string): Promise<MachineKeyRecord[]> {
    const response = await this.request<ListMachineKeysResponse>(MSG.LIST_MACHINE_KEYS, {
      user_id: formatUserIdForRust(userId),
    });
    return response.machines || [];
  }

  /**
   * Revoke/delete a machine key.
   *
   * @param userId - User ID (as bigint)
   * @param machineId - Machine ID to revoke (as bigint)
   */
  async revokeMachineKey(userId: bigint | string, machineId: bigint): Promise<void> {
    const response = await this.request<RevokeMachineKeyResponse>(MSG.REVOKE_MACHINE_KEY, {
      user_id: formatUserIdForRust(userId),
      machine_id: `0x${machineId.toString(16).padStart(32, '0')}`,
    });
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
  async rotateMachineKey(userId: bigint | string, machineId: bigint): Promise<MachineKeyRecord> {
    const response = await this.request<RotateMachineKeyResponse>(MSG.ROTATE_MACHINE_KEY, {
      user_id: formatUserIdForRust(userId),
      machine_id: `0x${machineId.toString(16).padStart(32, '0')}`,
    });
    return this.unwrapResult(response.result);
  }

  // ===========================================================================
  // Credential Operations
  // ===========================================================================

  /**
   * Attach an email address to a user's account via ZID API.
   *
   * This requires an active ZID session (access token from loginWithMachineKey).
   * The password is sent to ZID server where it's hashed with Argon2id.
   * Email is verified immediately by ZID - no separate verification step needed.
   *
   * @param userId - User ID (as bigint)
   * @param email - Email address to attach
   * @param password - Password for the ZID account (12+ characters)
   * @param accessToken - JWT access token from ZID login
   * @param zidEndpoint - ZID API endpoint (e.g., "https://api.zero-id.io")
   * @throws {CredentialAlreadyLinkedError} If email is already registered
   * @throws {InvalidCredentialFormatError} If email format is invalid
   * @throws {StorageError} If password doesn't meet requirements
   */
  async attachEmail(
    userId: bigint | string,
    email: string,
    password: string,
    accessToken: string,
    zidEndpoint: string
  ): Promise<void> {
    const response = await this.request<AttachEmailResponse>(MSG.ATTACH_EMAIL, {
      user_id: formatUserIdForRust(userId),
      email,
      password,
      access_token: accessToken,
      zid_endpoint: zidEndpoint,
    });
    this.unwrapResult(response.result);
  }

  /**
   * @deprecated ZID handles email verification server-side.
   * This method is no longer functional and will throw an error.
   */
  async verifyEmail(_userId: bigint | string, _email: string, _code: string): Promise<void> {
    throw new IdentityServiceError(
      'Email verification is deprecated. Use ZID login + attachEmail instead.'
    );
  }

  /**
   * Get all linked credentials for a user.
   *
   * @param userId - User ID (as bigint)
   * @returns Array of LinkedCredential
   */
  async getCredentials(userId: bigint | string): Promise<LinkedCredential[]> {
    const response = await this.request<GetCredentialsResponse>(MSG.GET_CREDENTIALS, {
      user_id: formatUserIdForRust(userId),
    });
    return response.credentials || [];
  }

  /**
   * Unlink a credential by type.
   *
   * @param userId - User ID (as bigint)
   * @param credentialType - Type of credential to unlink ('Email', 'Phone', etc.)
   */
  async unlinkCredential(userId: bigint | string, credentialType: CredentialType): Promise<void> {
    const response = await this.request<UnlinkCredentialResponse>(MSG.UNLINK_CREDENTIAL, {
      user_id: formatUserIdForRust(userId),
      credential_type: credentialType,
    });
    this.unwrapResult(response.result);
  }

  // ===========================================================================
  // ZID Auth Operations
  // ===========================================================================

  /**
   * Login to ZERO-ID using machine key challenge-response.
   *
   * This initiates the challenge-response authentication flow:
   * 1. Identity service reads machine key from VFS
   * 2. Requests challenge from ZID server
   * 3. Signs challenge with machine key
   * 4. Submits signed challenge for verification
   * 5. Returns tokens on success
   *
   * @param userId - User ID (as bigint)
   * @param zidEndpoint - ZID API endpoint (e.g., "https://api.zero-id.io")
   * @returns ZidTokens containing access and refresh tokens
   * @throws {ZidMachineKeyNotFoundError} If no machine key exists for the user
   * @throws {ZidAuthenticationFailedError} If authentication fails
   * @throws {ZidNetworkError} If network error occurs
   */
  async loginWithMachineKey(userId: bigint | string, zidEndpoint: string): Promise<ZidTokens> {
    const response = await this.request<ZidLoginResponse>(MSG.ZID_LOGIN, {
      user_id: formatUserIdForRust(userId),
      zid_endpoint: zidEndpoint,
    });
    return this.unwrapResult(response.result);
  }

  /**
   * Enroll/register this machine with ZERO-ID server.
   *
   * This registers a new identity with the ZID server:
   * 1. Identity service reads machine key from VFS
   * 2. Posts to /v1/identity with machine's public key
   * 3. Creates identity + first machine on ZID server
   * 4. Returns tokens on success (auto-login after enrollment)
   *
   * Use this when:
   * - First time connecting a machine to ZID
   * - Machine key login fails with "Machine not registered"
   *
   * @param userId - User ID (as bigint)
   * @param zidEndpoint - ZID API endpoint (e.g., "https://api.zero-id.io")
   * @returns ZidTokens containing access and refresh tokens
   * @throws {ZidMachineKeyNotFoundError} If no machine key exists for the user
   * @throws {ZidEnrollmentFailedError} If enrollment fails
   * @throws {ZidNetworkError} If network error occurs
   */
  async enrollMachine(userId: bigint | string, zidEndpoint: string): Promise<ZidTokens> {
    const response = await this.request<ZidEnrollMachineResponse>(MSG.ZID_ENROLL_MACHINE, {
      user_id: formatUserIdForRust(userId),
      zid_endpoint: zidEndpoint,
    });
    return this.unwrapResult(response.result);
  }

  /**
   * Create a machine key AND enroll with ZID in one atomic operation.
   *
   * This combined endpoint solves the signature mismatch problem where
   * separate createMachineKey + enrollMachine calls would generate different keypairs.
   *
   * With this combined flow:
   * 1. Reconstructs Neural Key from shards + password
   * 2. Derives machine keypair canonically
   * 3. Stores machine key record with SK seeds
   * 4. Enrolls with ZID using the same derived keypair
   * 5. Returns both MachineKeyRecord and ZidTokens
   *
   * @param userId - User ID (as bigint)
   * @param machineName - Human-readable machine name
   * @param capabilities - Machine capabilities
   * @param keyScheme - Key scheme to use (defaults to 'classical')
   * @param externalShard - One external Neural shard (from paper backup)
   * @param password - Password to decrypt stored shards
   * @param zidEndpoint - ZID API endpoint (e.g., "https://api.zero-id.io")
   * @returns Combined result with MachineKeyRecord and ZidTokens
   * @throws {IdentityKeyRequiredError} If identity key doesn't exist
   * @throws {ZidEnrollmentFailedError} If enrollment fails
   * @throws {ZidNetworkError} If network error occurs
   */
  async createMachineKeyAndEnroll(
    userId: bigint | string,
    machineName: string,
    capabilities: MachineKeyCapabilities,
    keyScheme: KeyScheme | undefined,
    externalShard: NeuralShard,
    password: string,
    zidEndpoint: string
  ): Promise<MachineKeyAndTokens> {
    if (!externalShard) {
      throw new IdentityServiceError('An external Neural shard is required');
    }
    if (!password) {
      throw new IdentityServiceError('Password is required');
    }
    if (!zidEndpoint) {
      throw new IdentityServiceError('ZID endpoint is required');
    }
    const response = await this.request<CreateMachineKeyAndEnrollResponse>(
      MSG.CREATE_MACHINE_KEY_AND_ENROLL,
      {
        user_id: formatUserIdForRust(userId),
        machine_name: machineName,
        capabilities,
        key_scheme: keyScheme ?? 'classical',
        external_shard: externalShard,
        password,
        zid_endpoint: zidEndpoint,
      }
    );
    return this.unwrapResult(response.result);
  }

  /**
   * Logout from ZERO-ID, deleting the session from VFS.
   *
   * This clears the stored session file so that:
   * - Refreshing the page does NOT restore the session
   * - A different user can log in
   *
   * @param userId - User ID whose session should be cleared
   */
  async zidLogout(userId: bigint | string): Promise<void> {
    const response = await this.request<ZidLogoutResponse>(MSG.ZID_LOGOUT, {
      user_id: formatUserIdForRust(userId),
    });
    this.unwrapResult(response.result);
  }

  /**
   * Refresh the ZID access token using the stored refresh token.
   *
   * This calls the ZID server's refresh endpoint to get new tokens:
   * 1. Identity service reads stored session from VFS (to get refresh_token)
   * 2. POSTs to /v1/auth/session/refresh with refresh_token
   * 3. Updates stored session with new tokens
   * 4. Returns new tokens
   *
   * Use this when:
   * - Access token is about to expire (recommended: 5 min before expiry)
   * - Access token has expired (may fail if refresh token also expired)
   *
   * @param userId - User ID whose session should be refreshed
   * @param zidEndpoint - ZID API endpoint (e.g., "https://api.zero-id.io")
   * @returns ZidTokens containing new access and refresh tokens
   * @throws {ZidInvalidRefreshTokenError} If refresh token is expired/invalid
   * @throws {ZidNetworkError} If network error occurs
   */
  async refreshToken(userId: bigint | string, zidEndpoint: string): Promise<ZidTokens> {
    const response = await this.request<ZidRefreshResponse>(MSG.ZID_REFRESH, {
      user_id: formatUserIdForRust(userId),
      zid_endpoint: zidEndpoint,
    });
    return this.unwrapResult(response.result);
  }

  /**
   * Login to ZERO-ID using email and password.
   *
   * This is an alternative to machine key challenge-response authentication.
   * Uses email/password credentials to authenticate directly with ZID server.
   *
   * @param userId - User ID to associate with this session
   * @param email - Email address for authentication
   * @param password - Password for authentication
   * @param zidEndpoint - ZID API endpoint (e.g., "https://api.zero-id.io")
   * @param machineId - Optional machine ID to associate with this session
   * @param mfaCode - Optional MFA code if MFA is enabled on the account
   * @returns ZidTokens containing access and refresh tokens
   * @throws {ZidAuthenticationFailedError} If email/password is invalid
   * @throws {ZidNetworkError} If network error occurs
   */
  async loginWithEmail(
    userId: bigint | string,
    email: string,
    password: string,
    zidEndpoint: string,
    machineId?: string,
    mfaCode?: string
  ): Promise<ZidTokens> {
    const response = await this.request<ZidEmailLoginResponse>(MSG.ZID_LOGIN_EMAIL, {
      user_id: formatUserIdForRust(userId),
      email,
      password,
      zid_endpoint: zidEndpoint,
      machine_id: machineId ?? null,
      mfa_code: mfaCode ?? null,
    });
    return this.unwrapResult(response.result);
  }

  // ===========================================================================
  // Identity Preferences
  // ===========================================================================

  /**
   * Get identity preferences from VFS
   * @param userId - User ID
   * @returns IdentityPreferences containing default key scheme
   */
  async getIdentityPreferences(userId: bigint | string): Promise<IdentityPreferences> {
    const response = await this.request<GetIdentityPreferencesResponse>(
      MSG.GET_IDENTITY_PREFERENCES,
      { user_id: formatUserIdForRust(userId) }
    );
    return response.preferences;
  }

  /**
   * Set default key scheme preference in VFS
   * @param userId - User ID
   * @param keyScheme - Key scheme to set as default
   */
  async setDefaultKeyScheme(userId: bigint | string, keyScheme: KeyScheme): Promise<void> {
    const response = await this.request<SetDefaultKeySchemeResponse>(MSG.SET_DEFAULT_KEY_SCHEME, {
      user_id: formatUserIdForRust(userId),
      key_scheme: keyScheme,
    });
    this.unwrapResult(response.result);
  }

  /**
   * Set default machine key for authentication in VFS preferences.
   * This key will be used by ZID login.
   * @param userId - User ID
   * @param machineId - Machine ID to set as default (UUID or hex string)
   */
  async setDefaultMachineKey(userId: bigint | string, machineId: string): Promise<void> {
    // Convert from UUID format (with dashes) to hex format for Rust backend
    const machineIdHex = uuidToHex(machineId);
    const response = await this.request<SetDefaultMachineKeyResponse>(MSG.SET_DEFAULT_MACHINE_KEY, {
      user_id: formatUserIdForRust(userId),
      machine_id: machineIdHex,
    });
    this.unwrapResult(response.result);
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
