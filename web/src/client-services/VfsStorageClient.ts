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
    /** ZosNetwork - network HAL for HTTP requests */
    ZosNetwork?: {
      // Supervisor initialization
      initSupervisor(supervisor: unknown): void;
      // Async fetch operation
      startFetch(requestId: number, pid: number, request: unknown): Promise<void>;
      // Cancel a pending request
      cancelRequest(requestId: number): void;
      // Get pending request count
      getPendingCount(): number;
    };
    /** ZosKeystore - key storage HAL for cryptographic keys (lazy init by KeyService) */
    ZosKeystore?: {
      // Database handle (set after init() completes)
      db: IDBDatabase | null;
      // Supervisor initialization
      initSupervisor(supervisor: unknown): void;
      // Async key operations (HAL callbacks)
      startRead(requestId: number, path: string): Promise<void>;
      startWrite(requestId: number, path: string, value: Uint8Array): Promise<void>;
      startDelete(requestId: number, path: string): Promise<void>;
      startList(requestId: number, prefix: string): Promise<void>;
      startExists(requestId: number, path: string): Promise<void>;
      // Sync cache access
      existsSync(path: string): boolean;
      getKeySync(path: string): Uint8Array | null;
      listKeysSync(prefix: string): string[];
      // Cache for stats
      keyCache: Map<string, Uint8Array>;
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
    const storage = window.ZosStorage;
    if (!storage) return false;
    return storage.existsSync(path);
  }

  /**
   * Read file content synchronously from cache.
   *
   * @param path - The canonical filesystem path
   * @returns The file content as Uint8Array, or null if not found
   */
  static readFileSync(path: string): Uint8Array | null {
    const storage = window.ZosStorage;
    if (!storage) return null;
    return storage.getContentSync(path);
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
      // Log detailed error info to help diagnose truncation issues
      const textPreview = new TextDecoder().decode(content);
      console.warn(
        `[VfsStorageClient] Failed to parse JSON at ${path}:`,
        e,
        `\n  Content length: ${content.length} bytes`,
        `\n  First 200 chars: ${textPreview.slice(0, 200)}...`,
        `\n  Last 100 chars: ...${textPreview.slice(-100)}`
      );
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
    const storage = window.ZosStorage;
    if (!storage) return null;
    return storage.getInodeSync(path);
  }

  /**
   * List children of a directory synchronously.
   *
   * @param parentPath - The parent directory path
   * @returns Array of child inodes
   */
  static listChildrenSync(parentPath: string): VfsInode[] {
    const storage = window.ZosStorage;
    if (!storage) return [];
    return storage.listChildrenSync(parentPath);
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
    const storage = window.ZosStorage;
    if (!storage) return [];

    const paths: string[] = [];
    const pathCache = storage.pathCache;

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
    const storage = window.ZosStorage;
    if (!storage) {
      return { paths: 0, inodes: 0, content: 0 };
    }

    return {
      paths: storage.pathCache.size,
      inodes: storage.inodeCache.size,
      content: storage.contentCache.size,
    };
  }

  /**
   * Validate JSON content at a path without parsing it.
   * Returns true if the content exists and is valid JSON.
   *
   * @param path - The canonical filesystem path
   * @returns Object with validation result and diagnostic info
   */
  static validateJsonSync(path: string): {
    exists: boolean;
    valid: boolean;
    contentLength: number;
    error?: string;
  } {
    const content = this.readFileSync(path);
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
   * @param path - The canonical filesystem path
   * @returns The raw content as string, or null if not found
   */
  static readTextSync(path: string): string | null {
    const content = this.readFileSync(path);
    if (!content) return null;
    return new TextDecoder().decode(content);
  }
}

// =============================================================================
// VFS Path Helpers
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
 * Get the canonical VFS path for a user's home directory.
 */
export function getUserHomeDir(userId: bigint | string | number): string {
  return `/home/${formatUserId(userId)}`;
}

/**
 * Get the canonical VFS path for a user's credentials store.
 * NOTE: Credentials are stored in VFS (not keystore) as they are not cryptographic keys.
 */
export function getCredentialsPath(userId: bigint | string | number): string {
  return `/home/${formatUserId(userId)}/.zos/credentials/credentials.json`;
}

/**
 * Get the canonical VFS path for a user's ZID session.
 * NOTE: ZID sessions are stored in VFS (not keystore) as they contain tokens, not keys.
 */
export function getZidSessionPath(userId: bigint | string | number): string {
  return `/home/${formatUserId(userId)}/.zos/identity/zid_session.json`;
}
