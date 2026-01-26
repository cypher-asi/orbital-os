/**
 * ZosKeystore - IndexedDB persistence for Zero OS Key Storage
 *
 * This is the dedicated storage layer for cryptographic key data.
 * It is separate from the filesystem to provide:
 * - Physical isolation of key material from general user data
 * - Dedicated access control (only KeyService can access)
 * - Independent lifecycle management
 *
 * Database: zos-keystore
 * Object Stores:
 *   - keys: Key data (path -> key bytes)
 *   - key_metadata: Key metadata (path -> metadata object)
 *
 * ## Architecture
 *
 * ZosKeystore provides async access for KeyService:
 *
 * 1. **Runtime Path (KeyService)**: KeyService → syscall → HAL → ZosKeystore
 *
 * ## Security
 *
 * - Only accessible via keystore_* syscalls
 * - KeyService is the sole accessor (PID 5)
 * - Key material never exposed to filesystem or other storage layers
 */

const ZosKeystore = {
  // === Database ===
  /** @type {IDBDatabase|null} */
  db: null,

  /** Database name */
  DB_NAME: 'zos-keystore',

  /** Database version */
  DB_VERSION: 1,

  /** Object store names */
  KEYS_STORE: 'keys',
  METADATA_STORE: 'key_metadata',

  // === In-Memory Caches ===
  /** @type {Map<string, Uint8Array>} In-memory key cache for synchronous reads */
  keyCache: new Map(),

  /** @type {Map<string, object>} In-memory metadata cache */
  metadataCache: new Map(),

  // === Supervisor Reference ===
  /** @type {object|null} Reference to the WASM supervisor for callbacks */
  supervisor: null,

  // ==========================================================================
  // Initialization
  // ==========================================================================

  /**
   * Initialize the ZosKeystore database.
   * @returns {Promise<boolean>} True if successful
   */
  async init() {
    if (this.db) {
      console.log('[ZosKeystore] Already initialized');
      return true;
    }

    return new Promise((resolve, reject) => {
      const request = indexedDB.open(this.DB_NAME, this.DB_VERSION);

      request.onupgradeneeded = (event) => {
        const db = event.target.result;
        console.log('[ZosKeystore] Creating object stores...');

        // Keys store: path (string) -> key data
        if (!db.objectStoreNames.contains(this.KEYS_STORE)) {
          const keyStore = db.createObjectStore(this.KEYS_STORE, { keyPath: 'path' });
          // Index for querying by user_id (extracted from path)
          keyStore.createIndex('user_id', 'user_id', { unique: false });
        }

        // Metadata store: path (string) -> metadata object
        if (!db.objectStoreNames.contains(this.METADATA_STORE)) {
          const metaStore = db.createObjectStore(this.METADATA_STORE, { keyPath: 'path' });
          metaStore.createIndex('user_id', 'user_id', { unique: false });
          metaStore.createIndex('key_type', 'key_type', { unique: false });
        }
      };

      request.onsuccess = async (event) => {
        this.db = event.target.result;
        console.log('[ZosKeystore] Database initialized');

        // Populate caches for synchronous reads
        await this.populateCaches();

        resolve(true);
      };

      request.onerror = (event) => {
        console.error('[ZosKeystore] Failed to open database:', event.target.error);
        reject(event.target.error);
      };
    });
  },

  /**
   * Initialize ZosKeystore with the supervisor reference.
   * Must be called before supervisor key storage operations.
   * @param {object} supervisor - The WASM supervisor instance
   */
  initSupervisor(supervisor) {
    this.supervisor = supervisor;
    console.log('[ZosKeystore] Supervisor reference set');
  },

  /**
   * Populate all in-memory caches from IndexedDB.
   * Called during init() to enable fast reads.
   * @returns {Promise<void>}
   */
  async populateCaches() {
    // Load all keys into cache
    const keys = await this.getAllKeys();
    this.keyCache.clear();
    for (const record of keys) {
      if (record.path && record.data) {
        this.keyCache.set(record.path, record.data);
      }
    }

    // Load all metadata into cache
    const metadata = await this.getAllMetadata();
    this.metadataCache.clear();
    for (const record of metadata) {
      if (record.path) {
        this.metadataCache.set(record.path, record);
      }
    }

    console.log(
      `[ZosKeystore] Caches populated: ${this.keyCache.size} keys, ${this.metadataCache.size} metadata entries`
    );
  },

  /**
   * Refresh caches from IndexedDB.
   * @returns {Promise<void>}
   */
  async refreshCaches() {
    if (!this.db) {
      console.warn('[ZosKeystore] Cannot refresh caches - not initialized');
      return;
    }
    console.log('[ZosKeystore] Refreshing caches from IndexedDB...');
    await this.populateCaches();
  },

  /**
   * Clear all data from IndexedDB and caches.
   * @returns {Promise<void>}
   */
  async clearAll() {
    if (!this.db) {
      throw new Error('ZosKeystore not initialized');
    }

    console.log('[ZosKeystore] Clearing all data...');

    await new Promise((resolve, reject) => {
      const tx = this.db.transaction([this.KEYS_STORE, this.METADATA_STORE], 'readwrite');
      tx.objectStore(this.KEYS_STORE).clear();
      tx.objectStore(this.METADATA_STORE).clear();
      tx.oncomplete = () => resolve();
      tx.onerror = (e) => reject(e.target.error);
    });

    // Clear caches
    this.keyCache.clear();
    this.metadataCache.clear();

    console.log('[ZosKeystore] All data cleared');
  },

  // ==========================================================================
  // Sync Cache Access - Read-only from in-memory caches
  // ==========================================================================

  /**
   * Synchronous key exists check using the in-memory cache.
   * @param {string} path - The key path
   * @returns {boolean} True if the key exists in the cache
   */
  existsSync(path) {
    return this.keyCache.has(path);
  },

  /**
   * Synchronous key read using the in-memory cache.
   * @param {string} path - The key path
   * @returns {Uint8Array|null} The key data or null if not found
   */
  getKeySync(path) {
    return this.keyCache.get(path) || null;
  },

  /**
   * Synchronous metadata read using the in-memory cache.
   * @param {string} path - The key path
   * @returns {object|null} The metadata or null if not found
   */
  getMetadataSync(path) {
    return this.metadataCache.get(path) || null;
  },

  /**
   * Synchronous list keys by prefix using the in-memory cache.
   * @param {string} prefix - The path prefix
   * @returns {string[]} Array of matching key paths
   */
  listKeysSync(prefix) {
    const matches = [];
    for (const path of this.keyCache.keys()) {
      if (path.startsWith(prefix)) {
        matches.push(path);
      }
    }
    return matches;
  },

  // ==========================================================================
  // Async IndexedDB Operations (Core storage methods)
  // ==========================================================================

  /**
   * Store key data.
   * @param {string} path - The key path
   * @param {Uint8Array} data - The key bytes
   * @param {string} userId - The user ID (for indexing)
   * @returns {Promise<boolean>} True if successful
   */
  async putKey(path, data, userId) {
    if (!this.db) {
      throw new Error('ZosKeystore not initialized');
    }

    const record = { path, data, user_id: userId };

    // Update cache synchronously (optimistic)
    this.keyCache.set(path, data);

    return new Promise((resolve, reject) => {
      const tx = this.db.transaction([this.KEYS_STORE], 'readwrite');
      const store = tx.objectStore(this.KEYS_STORE);
      const request = store.put(record);

      request.onsuccess = () => resolve(true);
      request.onerror = (event) => {
        console.error('[ZosKeystore] putKey failed:', event.target.error);
        // Revert cache on failure
        this.keyCache.delete(path);
        reject(event.target.error);
      };
    });
  },

  /**
   * Get key data by path.
   * @param {string} path - The key path
   * @returns {Promise<Uint8Array|null>} The key data or null if not found
   */
  async getKey(path) {
    if (!this.db) {
      throw new Error('ZosKeystore not initialized');
    }

    return new Promise((resolve, reject) => {
      const tx = this.db.transaction([this.KEYS_STORE], 'readonly');
      const store = tx.objectStore(this.KEYS_STORE);
      const request = store.get(path);

      request.onsuccess = () => {
        const result = request.result;
        resolve(result ? result.data : null);
      };
      request.onerror = (event) => {
        console.error('[ZosKeystore] getKey failed:', event.target.error);
        reject(event.target.error);
      };
    });
  },

  /**
   * Delete key data by path.
   * @param {string} path - The key path
   * @returns {Promise<boolean>} True if successful
   */
  async deleteKey(path) {
    if (!this.db) {
      throw new Error('ZosKeystore not initialized');
    }

    // Update cache synchronously (optimistic)
    this.keyCache.delete(path);
    this.metadataCache.delete(path);

    return new Promise((resolve, reject) => {
      const tx = this.db.transaction([this.KEYS_STORE, this.METADATA_STORE], 'readwrite');
      tx.objectStore(this.KEYS_STORE).delete(path);
      tx.objectStore(this.METADATA_STORE).delete(path);

      tx.oncomplete = () => resolve(true);
      tx.onerror = (event) => {
        console.error('[ZosKeystore] deleteKey failed:', event.target.error);
        reject(event.target.error);
      };
    });
  },

  /**
   * List keys by prefix.
   * @param {string} prefix - The path prefix to match
   * @returns {Promise<string[]>} Array of matching key paths
   */
  async listKeys(prefix) {
    if (!this.db) {
      throw new Error('ZosKeystore not initialized');
    }

    return new Promise((resolve, reject) => {
      const tx = this.db.transaction([this.KEYS_STORE], 'readonly');
      const store = tx.objectStore(this.KEYS_STORE);
      const request = store.getAll();

      request.onsuccess = () => {
        const results = request.result || [];
        const paths = results
          .filter((r) => r.path && r.path.startsWith(prefix))
          .map((r) => r.path);
        resolve(paths);
      };
      request.onerror = (event) => {
        console.error('[ZosKeystore] listKeys failed:', event.target.error);
        reject(event.target.error);
      };
    });
  },

  /**
   * Check if a key exists.
   * @param {string} path - The key path
   * @returns {Promise<boolean>} True if exists
   */
  async exists(path) {
    const key = await this.getKey(path);
    return key !== null;
  },

  /**
   * Get all key records.
   * @returns {Promise<object[]>} Array of all key records
   */
  async getAllKeys() {
    if (!this.db) {
      throw new Error('ZosKeystore not initialized');
    }

    return new Promise((resolve, reject) => {
      const tx = this.db.transaction([this.KEYS_STORE], 'readonly');
      const store = tx.objectStore(this.KEYS_STORE);
      const request = store.getAll();

      request.onsuccess = () => resolve(request.result || []);
      request.onerror = (event) => {
        console.error('[ZosKeystore] getAllKeys failed:', event.target.error);
        reject(event.target.error);
      };
    });
  },

  /**
   * Get all metadata records.
   * @returns {Promise<object[]>} Array of all metadata records
   */
  async getAllMetadata() {
    if (!this.db) {
      throw new Error('ZosKeystore not initialized');
    }

    return new Promise((resolve, reject) => {
      const tx = this.db.transaction([this.METADATA_STORE], 'readonly');
      const store = tx.objectStore(this.METADATA_STORE);
      const request = store.getAll();

      request.onsuccess = () => resolve(request.result || []);
      request.onerror = (event) => {
        console.error('[ZosKeystore] getAllMetadata failed:', event.target.error);
        reject(event.target.error);
      };
    });
  },

  /**
   * Store key metadata.
   * @param {string} path - The key path
   * @param {object} metadata - The metadata object
   * @returns {Promise<boolean>} True if successful
   */
  async putMetadata(path, metadata) {
    if (!this.db) {
      throw new Error('ZosKeystore not initialized');
    }

    const record = { ...metadata, path };

    // Update cache synchronously (optimistic)
    this.metadataCache.set(path, record);

    return new Promise((resolve, reject) => {
      const tx = this.db.transaction([this.METADATA_STORE], 'readwrite');
      const store = tx.objectStore(this.METADATA_STORE);
      const request = store.put(record);

      request.onsuccess = () => resolve(true);
      request.onerror = (event) => {
        console.error('[ZosKeystore] putMetadata failed:', event.target.error);
        // Revert cache on failure
        this.metadataCache.delete(path);
        reject(event.target.error);
      };
    });
  },

  /**
   * Get key metadata by path.
   * @param {string} path - The key path
   * @returns {Promise<object|null>} The metadata or null if not found
   */
  async getMetadata(path) {
    if (!this.db) {
      throw new Error('ZosKeystore not initialized');
    }

    return new Promise((resolve, reject) => {
      const tx = this.db.transaction([this.METADATA_STORE], 'readonly');
      const store = tx.objectStore(this.METADATA_STORE);
      const request = store.get(path);

      request.onsuccess = () => resolve(request.result || null);
      request.onerror = (event) => {
        console.error('[ZosKeystore] getMetadata failed:', event.target.error);
        reject(event.target.error);
      };
    });
  },

  /**
   * Delete the entire database.
   * @returns {Promise<boolean>} True if successful
   */
  async deleteDatabase() {
    if (this.db) {
      this.db.close();
      this.db = null;
    }

    // Clear all caches
    this.keyCache.clear();
    this.metadataCache.clear();

    return new Promise((resolve, reject) => {
      const request = indexedDB.deleteDatabase(this.DB_NAME);

      request.onsuccess = () => {
        console.log('[ZosKeystore] Database deleted');
        resolve(true);
      };

      request.onerror = (event) => {
        console.error('[ZosKeystore] deleteDatabase failed:', event.target.error);
        reject(event.target.error);
      };
    });
  },

  // ==========================================================================
  // Supervisor Async API (HAL callbacks)
  // These methods are called by HAL and notify the supervisor when complete.
  // ==========================================================================

  /**
   * Start async key read operation.
   * Calls supervisor.notify_keystore_read_complete or notify_keystore_not_found when done.
   * @param {number} requestId - Unique request ID
   * @param {string} path - Key path to read
   */
  async startRead(requestId, path) {
    console.log(`[ZosKeystore] startRead: request_id=${requestId}, path=${path}`);

    if (!this.supervisor) {
      console.error('[ZosKeystore] startRead: supervisor not initialized');
      return;
    }

    // Capture supervisor reference for deferred callback
    const supervisor = this.supervisor;

    try {
      await this.init();

      const data = await this.getKey(path);

      // Defer callback to avoid re-entrancy with wasm-bindgen's RefCell borrow
      if (data) {
        setTimeout(() => supervisor.notify_keystore_read_complete(requestId, data), 0);
      } else {
        setTimeout(() => supervisor.notify_keystore_not_found(requestId), 0);
      }
    } catch (e) {
      console.error(`[ZosKeystore] startRead error: ${e.message}`);
      setTimeout(() => supervisor.notify_keystore_error(requestId, e.message), 0);
    }
  },

  /**
   * Start async key write operation.
   * Calls supervisor.notify_keystore_write_complete when done.
   * @param {number} requestId - Unique request ID
   * @param {string} path - Key path to write
   * @param {Uint8Array} value - Key data to store
   */
  async startWrite(requestId, path, value) {
    console.log(
      `[ZosKeystore] startWrite: request_id=${requestId}, path=${path}, len=${value.length}`
    );

    if (!this.supervisor) {
      console.error('[ZosKeystore] startWrite: supervisor not initialized');
      return;
    }

    // Capture supervisor reference for deferred callback
    const supervisor = this.supervisor;

    try {
      await this.init();

      // Extract user_id from path (format: /keys/{user_id}/...)
      const userId = this.extractUserIdFromPath(path);
      await this.putKey(path, value, userId);

      // Defer callback to avoid re-entrancy with wasm-bindgen's RefCell borrow
      setTimeout(() => supervisor.notify_keystore_write_complete(requestId), 0);
    } catch (e) {
      console.error(`[ZosKeystore] startWrite error: ${e.message}`);
      setTimeout(() => supervisor.notify_keystore_error(requestId, e.message), 0);
    }
  },

  /**
   * Start async key delete operation.
   * Calls supervisor.notify_keystore_write_complete when done.
   * @param {number} requestId - Unique request ID
   * @param {string} path - Key path to delete
   */
  async startDelete(requestId, path) {
    console.log(`[ZosKeystore] startDelete: request_id=${requestId}, path=${path}`);

    if (!this.supervisor) {
      console.error('[ZosKeystore] startDelete: supervisor not initialized');
      return;
    }

    // Capture supervisor reference for deferred callback
    const supervisor = this.supervisor;

    try {
      await this.init();
      await this.deleteKey(path);

      // Defer callback to avoid re-entrancy with wasm-bindgen's RefCell borrow
      setTimeout(() => supervisor.notify_keystore_write_complete(requestId), 0);
    } catch (e) {
      console.error(`[ZosKeystore] startDelete error: ${e.message}`);
      setTimeout(() => supervisor.notify_keystore_error(requestId, e.message), 0);
    }
  },

  /**
   * Start async key list operation.
   * Calls supervisor.notify_keystore_list_complete with JSON array of paths.
   * @param {number} requestId - Unique request ID
   * @param {string} prefix - Path prefix to match
   */
  async startList(requestId, prefix) {
    console.log(`[ZosKeystore] startList: request_id=${requestId}, prefix=${prefix}`);

    if (!this.supervisor) {
      console.error('[ZosKeystore] startList: supervisor not initialized');
      return;
    }

    // Capture supervisor reference for deferred callback
    const supervisor = this.supervisor;

    try {
      await this.init();

      const paths = await this.listKeys(prefix);
      const pathsJson = JSON.stringify(paths);

      // Defer callback to avoid re-entrancy with wasm-bindgen's RefCell borrow
      setTimeout(() => supervisor.notify_keystore_list_complete(requestId, pathsJson), 0);
    } catch (e) {
      console.error(`[ZosKeystore] startList error: ${e.message}`);
      setTimeout(() => supervisor.notify_keystore_error(requestId, e.message), 0);
    }
  },

  /**
   * Start async key exists check.
   * Calls supervisor.notify_keystore_exists_complete with boolean.
   * @param {number} requestId - Unique request ID
   * @param {string} path - Key path to check
   */
  async startExists(requestId, path) {
    console.log(`[ZosKeystore] startExists: request_id=${requestId}, path=${path}`);

    if (!this.supervisor) {
      console.error('[ZosKeystore] startExists: supervisor not initialized');
      return;
    }

    // Capture supervisor reference for deferred callback
    const supervisor = this.supervisor;

    try {
      await this.init();

      // Check cache first, then IndexedDB
      let exists = this.keyCache.has(path);
      if (!exists) {
        const key = await this.getKey(path);
        exists = key !== null;
      }

      // Defer callback to avoid re-entrancy with wasm-bindgen's RefCell borrow
      setTimeout(() => supervisor.notify_keystore_exists_complete(requestId, exists), 0);
    } catch (e) {
      console.error(`[ZosKeystore] startExists error: ${e.message}`);
      setTimeout(() => supervisor.notify_keystore_error(requestId, e.message), 0);
    }
  },

  // ==========================================================================
  // Helper Methods
  // ==========================================================================

  /**
   * Extract user_id from a key path.
   * Path format: /keys/{user_id}/...
   * @param {string} path - The key path
   * @returns {string} The user ID or empty string
   */
  extractUserIdFromPath(path) {
    if (!path.startsWith('/keys/')) {
      return '';
    }
    const parts = path.split('/');
    // /keys/{user_id}/... -> parts[0]='', parts[1]='keys', parts[2]=user_id
    return parts.length > 2 ? parts[2] : '';
  },
};

// Make ZosKeystore available globally
if (typeof window !== 'undefined') {
  window.ZosKeystore = ZosKeystore;
}
