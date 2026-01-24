/**
 * VfsStorageClient - Type-safe TypeScript wrapper for ZosStorage
 *
 * This client provides synchronous read access to the VFS in-memory cache.
 * It follows the canonical Zero OS pattern where React reads state directly
 * from ZosStorage caches (read-only), while writes go through services.
 *
 * ## Architecture (per invariants.md)
 *
 * - Filesystems are userspace services - data at canonical paths
 * - React can read VFS directly via ZosStorage caches (synchronous, read-only)
 * - Services use async storage syscalls for writes (via HAL)
 * - Single ZosStorage JS object is the only IndexedDB interface
 *
 * ## Usage
 *
 * ```typescript
 * // Read identity key store synchronously from cache
 * const keyStore = VfsStorageClient.readJsonSync<LocalKeyStore>(
 *   `/home/${userId}/.zos/identity/public_keys.json`
 * );
 *
 * // Check if file exists
 * const exists = VfsStorageClient.existsSync(path);
 *
 * // List directory contents
 * const children = VfsStorageClient.listChildrenSync(parentPath);
 * ```
 */

/**
 * Inode type from VFS storage
 */
export interface VfsInode {
  path: string;
  parent_path: string;
  name: string;
  inode_type: 'file' | 'directory' | 'symlink';
  owner_id: string | null;
  size: number;
  created_at: number;
  modified_at: number;
  content_hash: string | null;
  encrypted: boolean;
  symlink_target: string | null;
}

/**
 * Global ZosStorage type declaration
 *
 * ZosStorage is the single unified IndexedDB storage object for Zero OS.
 * It provides:
 * - Sync cache access (for React UI read-only)
 * - Async IndexedDB operations (for core storage)
 * - Supervisor async API (HAL callbacks)
 */
declare global {
  interface Window {
    /** ZosStorage - unified IndexedDB storage for Zero OS */
    ZosStorage?: {
      // Sync cache access (React UI)
      existsSync(path: string): boolean;
      getInodeSync(path: string): VfsInode | null;
      getContentSync(path: string): Uint8Array | null;
      listChildrenSync(parentPath: string): VfsInode[];
      pathCache: Set<string>;
      inodeCache: Map<string, VfsInode>;
      contentCache: Map<string, Uint8Array>;
      // Supervisor initialization
      initSupervisor(supervisor: unknown): void;
      // Supervisor async API (HAL callbacks)
      startRead(requestId: number, key: string): Promise<void>;
      startWrite(requestId: number, key: string, value: Uint8Array): Promise<void>;
      startDelete(requestId: number, key: string): Promise<void>;
      startList(requestId: number, prefix: string): Promise<void>;
      startExists(requestId: number, key: string): Promise<void>;
    };
  }
}

/**
 * Type-safe client for synchronous VFS cache access.
 *
 * All methods are static and read from the in-memory cache populated
 * by ZosStorage during initialization. This allows React components
 * to read filesystem state without going through IPC (which could deadlock).
 *
 * Note: This client provides READ-ONLY access to ZosStorage caches.
 * Writes must go through services using syscalls routed via HAL.
 */
export class VfsStorageClient {
  /**
   * Check if ZosStorage is available.
   */
  static isAvailable(): boolean {
    return typeof window !== 'undefined' && window.ZosStorage !== undefined;
  }

  /**
   * Check if a path exists in the cache.
   *
   * @param path - The canonical filesystem path
   * @returns true if the path exists
   */
  static existsSync(path: string): boolean {
    if (!this.isAvailable()) return false;
    return window.ZosStorage!.existsSync(path);
  }

  /**
   * Read file content synchronously from cache.
   *
   * @param path - The canonical filesystem path
   * @returns The file content as Uint8Array, or null if not found
   */
  static readFileSync(path: string): Uint8Array | null {
    if (!this.isAvailable()) return null;
    return window.ZosStorage!.getContentSync(path);
  }

  /**
   * Read and parse JSON file synchronously from cache.
   *
   * @param path - The canonical filesystem path
   * @returns The parsed JSON object, or null if not found or parse error
   */
  static readJsonSync<T>(path: string): T | null {
    const content = this.readFileSync(path);
    if (!content) return null;

    try {
      const text = new TextDecoder().decode(content);
      return JSON.parse(text) as T;
    } catch (e) {
      console.warn(`[VfsStorageClient] Failed to parse JSON at ${path}:`, e);
      return null;
    }
  }

  /**
   * Get inode metadata for a path.
   *
   * @param path - The canonical filesystem path
   * @returns The inode metadata, or null if not found
   */
  static getInodeSync(path: string): VfsInode | null {
    if (!this.isAvailable()) return null;
    return window.ZosStorage!.getInodeSync(path);
  }

  /**
   * List children of a directory synchronously.
   *
   * @param parentPath - The parent directory path
   * @returns Array of child inodes
   */
  static listChildrenSync(parentPath: string): VfsInode[] {
    if (!this.isAvailable()) return [];
    return window.ZosStorage!.listChildrenSync(parentPath);
  }

  /**
   * List all files matching a path prefix.
   *
   * This is a helper that iterates through the cache to find files
   * under a given directory prefix.
   *
   * @param prefix - The path prefix to match
   * @returns Array of matching paths
   */
  static listPathsWithPrefix(prefix: string): string[] {
    if (!this.isAvailable()) return [];

    const paths: string[] = [];
    const pathCache = window.ZosStorage!.pathCache;

    for (const path of pathCache) {
      if (path.startsWith(prefix)) {
        paths.push(path);
      }
    }

    return paths;
  }

  /**
   * Get the number of entries in the content cache.
   * Useful for debugging cache population.
   */
  static getCacheStats(): { paths: number; inodes: number; content: number } {
    if (!this.isAvailable()) {
      return { paths: 0, inodes: 0, content: 0 };
    }

    return {
      paths: window.ZosStorage!.pathCache.size,
      inodes: window.ZosStorage!.inodeCache.size,
      content: window.ZosStorage!.contentCache.size,
    };
  }
}

// =============================================================================
// Identity-specific helpers
// =============================================================================

/**
 * Format a user ID as a 32-character hex string (128 bits).
 * This matches the Rust format: {:032x}
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

  // Format as 32-character lowercase hex
  return value.toString(16).padStart(32, '0');
}

/**
 * Get the canonical path for a user's identity public keys.
 */
export function getIdentityKeyStorePath(userId: bigint | string | number): string {
  return `/home/${formatUserId(userId)}/.zos/identity/public_keys.json`;
}

/**
 * Get the canonical path for a user's machine keys directory.
 */
export function getMachineKeysDir(userId: bigint | string | number): string {
  return `/home/${formatUserId(userId)}/.zos/identity/machine`;
}

/**
 * Get the canonical path for a specific machine key record.
 */
export function getMachineKeyPath(
  userId: bigint | string | number,
  machineId: bigint | string | number
): string {
  const machineIdHex =
    typeof machineId === 'bigint'
      ? machineId.toString(16).padStart(32, '0')
      : typeof machineId === 'string' && machineId.startsWith('0x')
        ? machineId.slice(2).padStart(32, '0')
        : BigInt(machineId).toString(16).padStart(32, '0');

  return `/home/${formatUserId(userId)}/.zos/identity/machine/${machineIdHex}.json`;
}
