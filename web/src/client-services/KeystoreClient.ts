/**
 * KeystoreClient - Type-safe TypeScript wrapper for ZosKeystore
 *
 * This client provides synchronous read access to the keystore in-memory cache.
 * It follows the canonical Zero OS pattern where React reads state directly
 * from ZosKeystore caches (read-only), while writes go through services.
 *
 * ## Architecture (per invariants.md)
 *
 * - Identity keys are stored in dedicated zos-keystore IndexedDB
 * - React can read keystore directly via ZosKeystore caches (synchronous, read-only)
 * - Services use VFS IPC with /keys/ paths which route to keystore syscalls
 * - Single ZosKeystore JS object is the only keystore IndexedDB interface
 *
 * ## Usage
 *
 * ```typescript
 * // Read identity key store synchronously from cache
 * const keyStore = KeystoreClient.readJsonSync<LocalKeyStore>(
 *   `/keys/${userId}/identity/public_keys.json`
 * );
 *
 * // Check if key exists
 * const exists = KeystoreClient.existsSync(path);
 *
 * // List keys with prefix
 * const keys = KeystoreClient.listKeysSync(`/keys/${userId}/identity/machine`);
 * ```
 */

/**
 * Type-safe client for synchronous keystore cache access.
 *
 * All methods are static and read from the in-memory cache populated
 * by ZosKeystore during initialization. This allows React components
 * to read key state without going through IPC (which could deadlock).
 *
 * Note: This client provides READ-ONLY access to ZosKeystore caches.
 * Writes must go through services using VFS IPC with /keys/ paths.
 */
export class KeystoreClient {
  /**
   * Check if ZosKeystore is available AND initialized with populated caches.
   * Returns false if ZosKeystore exists but init() hasn't completed yet.
   */
  static isAvailable(): boolean {
    if (typeof window === 'undefined' || window.ZosKeystore === undefined) {
      return false;
    }
    // Also check if the database has been initialized (db is set after init())
    // This prevents reads before populateCaches() has run
    return window.ZosKeystore.db !== undefined && window.ZosKeystore.db !== null;
  }

  /**
   * Check if a key exists in the cache.
   *
   * @param path - The keystore path (e.g., "/keys/{user_id}/identity/public_keys.json")
   * @returns true if the key exists
   */
  static existsSync(path: string): boolean {
    const keystore = window.ZosKeystore;
    if (!keystore) return false;
    return keystore.existsSync(path);
  }

  /**
   * Read key data synchronously from cache.
   *
   * @param path - The keystore path
   * @returns The key data as Uint8Array, or null if not found
   */
  static readKeySync(path: string): Uint8Array | null {
    const keystore = window.ZosKeystore;
    if (!keystore) return null;
    return keystore.getKeySync(path);
  }

  /**
   * Read and parse JSON key data synchronously from cache.
   *
   * @param path - The keystore path
   * @returns The parsed JSON object, or null if not found or parse error
   */
  static readJsonSync<T>(path: string): T | null {
    const content = this.readKeySync(path);
    if (!content) return null;

    try {
      const text = new TextDecoder().decode(content);
      return JSON.parse(text) as T;
    } catch (e) {
      // Log detailed error info to help diagnose issues
      const textPreview = new TextDecoder().decode(content);
      console.warn(
        `[KeystoreClient] Failed to parse JSON at ${path}:`,
        e,
        `\n  Content length: ${content.length} bytes`,
        `\n  First 200 chars: ${textPreview.slice(0, 200)}...`,
        `\n  Last 100 chars: ...${textPreview.slice(-100)}`
      );
      return null;
    }
  }

  /**
   * List keys matching a prefix synchronously from cache.
   *
   * @param prefix - The path prefix to match (e.g., "/keys/{user_id}/identity/machine")
   * @returns Array of matching key paths
   */
  static listKeysSync(prefix: string): string[] {
    const keystore = window.ZosKeystore;
    if (!keystore) return [];
    return keystore.listKeysSync(prefix);
  }

  /**
   * Get the number of entries in the key cache.
   * Useful for debugging cache population.
   */
  static getCacheStats(): { keys: number } {
    const keystore = window.ZosKeystore;
    if (!keystore) {
      return { keys: 0 };
    }

    return {
      keys: keystore.keyCache?.size ?? 0,
    };
  }

  /**
   * Validate JSON content at a path without parsing it.
   * Returns true if the content exists and is valid JSON.
   *
   * @param path - The keystore path
   * @returns Object with validation result and diagnostic info
   */
  static validateJsonSync(path: string): {
    exists: boolean;
    valid: boolean;
    contentLength: number;
    error?: string;
  } {
    const content = this.readKeySync(path);
    if (!content) {
      return { exists: false, valid: false, contentLength: 0 };
    }

    try {
      const text = new TextDecoder().decode(content);
      JSON.parse(text);
      return { exists: true, valid: true, contentLength: content.length };
    } catch (e) {
      return {
        exists: true,
        valid: false,
        contentLength: content.length,
        error: e instanceof Error ? e.message : 'Unknown parse error',
      };
    }
  }

  /**
   * Get raw content as a string for debugging.
   * Useful for inspecting corrupt/truncated files.
   *
   * @param path - The keystore path
   * @returns The raw content as string, or null if not found
   */
  static readTextSync(path: string): string | null {
    const content = this.readKeySync(path);
    if (!content) return null;
    return new TextDecoder().decode(content);
  }
}

// =============================================================================
// Identity Keystore Path Helpers
// =============================================================================

/**
 * Format a user ID as a decimal string.
 * This matches the Rust format: {} (Display trait for u128)
 */
export function formatUserId(userId: bigint | string | number): string {
  let value: bigint;

  if (typeof userId === 'bigint') {
    value = userId;
  } else if (typeof userId === 'string') {
    // Handle hex strings with 0x prefix
    if (userId.startsWith('0x')) {
      value = BigInt(userId);
    } else {
      value = BigInt(userId);
    }
  } else {
    value = BigInt(userId);
  }

  // Format as decimal string to match Rust's {} format
  return value.toString(10);
}

/**
 * Format a machine ID as a 32-character hex string.
 * This matches the Rust format: {:032x}
 * Handles UUID format (with dashes), hex format (with 0x prefix), and numeric values.
 */
export function formatMachineIdHex(machineId: bigint | string | number): string {
  let value: bigint;

  if (typeof machineId === 'bigint') {
    value = machineId;
  } else if (typeof machineId === 'string') {
    if (machineId.startsWith('0x')) {
      value = BigInt(machineId);
    } else if (machineId.includes('-')) {
      // UUID format: remove dashes and parse as hex
      const hex = machineId.replace(/-/g, '');
      value = BigInt('0x' + hex);
    } else {
      // Try to parse as hex if it looks like hex
      if (/^[0-9a-fA-F]+$/.test(machineId)) {
        value = BigInt('0x' + machineId);
      } else {
        value = BigInt(machineId);
      }
    }
  } else {
    value = BigInt(machineId);
  }

  return value.toString(16).padStart(32, '0');
}

/**
 * Get the canonical keystore path for a user's identity public keys.
 *
 * Matches Rust: LocalKeyStore::storage_path()
 */
export function getIdentityKeystorePath(userId: bigint | string | number): string {
  return `/keys/${formatUserId(userId)}/identity/public_keys.json`;
}

/**
 * Get the canonical keystore path for a user's machine keys directory.
 *
 * Used for listing machine keys with KeystoreClient.listKeysSync()
 */
export function getMachineKeysDir(userId: bigint | string | number): string {
  return `/keys/${formatUserId(userId)}/identity/machine`;
}

/**
 * Get the canonical keystore path for a specific machine key record.
 *
 * Matches Rust: MachineKeyRecord::storage_path()
 */
export function getMachineKeyPath(
  userId: bigint | string | number,
  machineId: bigint | string | number
): string {
  return `/keys/${formatUserId(userId)}/identity/machine/${formatMachineIdHex(machineId)}.json`;
}
