/**
 * ZosStorage - Unified IndexedDB persistence for Zero OS
 *
 * This is the SINGLE consolidated storage object for Zero OS. All IndexedDB
 * access goes through this object.
 *
 * Database: zos-userspace
 * Object Stores:
 *   - inodes: Filesystem metadata (path -> Inode)
 *   - content: File content blobs (path -> Uint8Array)
 *
 * ## Architecture
 *
 * ZosStorage provides three access patterns:
 *
 * 1. **Runtime Path (Processes)**: VFS Service → syscall → HAL → ZosStorage
 * 2. **Bootstrap Path (Supervisor)**: Supervisor Boot → HAL bootstrap_storage_* → ZosStorage
 * 3. **Read-Only Path (React UI)**: React → ZosStorageClient → ZosStorage sync caches
 *
 * ## Usage
 *
 * React app calls `window.ZosStorage.initSupervisor(supervisor)` to set up callbacks.
 * HAL calls the start* methods which perform IndexedDB operations and notify the
 * supervisor when complete.
 */

const ZosStorage = {
    // === Database ===
    /** @type {IDBDatabase|null} */
    db: null,

    /** Database name */
    DB_NAME: 'zos-userspace',

    /** Database version */
    DB_VERSION: 1,

    /** Object store names */
    INODES_STORE: 'inodes',
    CONTENT_STORE: 'content',

    // === In-Memory Caches ===
    /** @type {Set<string>} In-memory path cache for synchronous exists checks */
    pathCache: new Set(),

    /** @type {Map<string, object>} In-memory inode cache for synchronous reads */
    inodeCache: new Map(),

    /** @type {Map<string, Uint8Array>} In-memory content cache for synchronous reads */
    contentCache: new Map(),

    // === Supervisor Reference ===
    /** @type {object|null} Reference to the WASM supervisor for callbacks */
    supervisor: null,

    // ==========================================================================
    // Initialization
    // ==========================================================================

    /**
     * Initialize the ZosStorage database.
     * @returns {Promise<boolean>} True if successful
     */
    async init() {
        if (this.db) {
            console.log('[ZosStorage] Already initialized');
            return true;
        }

        return new Promise((resolve, reject) => {
            const request = indexedDB.open(this.DB_NAME, this.DB_VERSION);

            request.onupgradeneeded = (event) => {
                const db = event.target.result;
                console.log('[ZosStorage] Creating object stores...');

                // Inodes store: path (string) -> inode object
                if (!db.objectStoreNames.contains(this.INODES_STORE)) {
                    const inodeStore = db.createObjectStore(this.INODES_STORE, { keyPath: 'path' });
                    // Index for querying by parent path (for readdir)
                    inodeStore.createIndex('parent_path', 'parent_path', { unique: false });
                    // Index for querying by owner (for user data)
                    inodeStore.createIndex('owner_id', 'owner_id', { unique: false });
                }

                // Content store: path (string) -> content blob
                if (!db.objectStoreNames.contains(this.CONTENT_STORE)) {
                    db.createObjectStore(this.CONTENT_STORE, { keyPath: 'path' });
                }
            };

            request.onsuccess = async (event) => {
                this.db = event.target.result;
                console.log('[ZosStorage] Database initialized');

                // Populate caches for synchronous reads
                await this.populateCaches();

                resolve(true);
            };

            request.onerror = (event) => {
                console.error('[ZosStorage] Failed to open database:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Initialize ZosStorage with the supervisor reference.
     * Must be called before supervisor storage operations.
     * @param {object} supervisor - The WASM supervisor instance
     */
    initSupervisor(supervisor) {
        this.supervisor = supervisor;
        console.log('[ZosStorage] Supervisor reference set');
    },

    /**
     * Populate all in-memory caches from IndexedDB.
     * Called during init() to enable synchronous reads.
     * @returns {Promise<void>}
     */
    async populateCaches() {
        // Load all inodes into cache
        const inodes = await this.getAllInodes();
        this.pathCache.clear();
        this.inodeCache.clear();
        for (const inode of inodes) {
            this.pathCache.add(inode.path);
            this.inodeCache.set(inode.path, inode);
        }

        // Load all content into cache
        const allContent = await this.getAllContent();
        this.contentCache.clear();
        for (const record of allContent) {
            if (record.path && record.data) {
                this.contentCache.set(record.path, record.data);
            }
        }

        console.log(`[ZosStorage] Caches populated: ${this.pathCache.size} paths, ${this.inodeCache.size} inodes, ${this.contentCache.size} content entries`);
    },

    // ==========================================================================
    // Sync Cache Access (React UI) - Read-only from in-memory caches
    // ==========================================================================

    /**
     * Synchronous exists check using the in-memory path cache.
     * @param {string} path - The canonical path
     * @returns {boolean} True if the path exists in the cache
     */
    existsSync(path) {
        return this.pathCache.has(path);
    },

    /**
     * Synchronous inode read using the in-memory cache.
     * @param {string} path - The canonical path
     * @returns {object|null} The inode or null if not found
     */
    getInodeSync(path) {
        return this.inodeCache.get(path) || null;
    },

    /**
     * Synchronous content read using the in-memory cache.
     * @param {string} path - The canonical path
     * @returns {Uint8Array|null} The content or null if not found
     */
    getContentSync(path) {
        return this.contentCache.get(path) || null;
    },

    /**
     * Synchronous list children using the in-memory cache.
     * @param {string} parentPath - The parent directory path
     * @returns {object[]} Array of child inodes
     */
    listChildrenSync(parentPath) {
        const children = [];
        for (const [path, inode] of this.inodeCache) {
            if (inode.parent_path === parentPath) {
                children.push(inode);
            }
        }
        return children;
    },

    // ==========================================================================
    // Async IndexedDB Operations (Core storage methods)
    // ==========================================================================

    /**
     * Store an inode.
     * @param {string} path - The canonical path
     * @param {object} inode - The inode object
     * @returns {Promise<boolean>} True if successful
     */
    async putInode(path, inode) {
        if (!this.db) {
            throw new Error('ZosStorage not initialized');
        }

        // Ensure path is set as the key
        const record = { ...inode, path };

        // Update caches synchronously (optimistic)
        this.pathCache.add(path);
        this.inodeCache.set(path, record);

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readwrite');
            const store = tx.objectStore(this.INODES_STORE);
            const request = store.put(record);

            request.onsuccess = () => resolve(true);
            request.onerror = (event) => {
                console.error('[ZosStorage] putInode failed:', event.target.error);
                // Revert cache on failure
                this.pathCache.delete(path);
                this.inodeCache.delete(path);
                reject(event.target.error);
            };
        });
    },

    /**
     * Get an inode by path.
     * @param {string} path - The canonical path
     * @returns {Promise<object|null>} The inode or null if not found
     */
    async getInode(path) {
        if (!this.db) {
            throw new Error('ZosStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readonly');
            const store = tx.objectStore(this.INODES_STORE);
            const request = store.get(path);

            request.onsuccess = () => resolve(request.result || null);
            request.onerror = (event) => {
                console.error('[ZosStorage] getInode failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Delete an inode by path.
     * @param {string} path - The canonical path
     * @returns {Promise<boolean>} True if successful
     */
    async deleteInode(path) {
        if (!this.db) {
            throw new Error('ZosStorage not initialized');
        }

        // Update caches synchronously (optimistic)
        this.pathCache.delete(path);
        this.inodeCache.delete(path);

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readwrite');
            const store = tx.objectStore(this.INODES_STORE);
            const request = store.delete(path);

            request.onsuccess = () => resolve(true);
            request.onerror = (event) => {
                console.error('[ZosStorage] deleteInode failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * List all children of a directory.
     * @param {string} parentPath - The parent directory path
     * @returns {Promise<object[]>} Array of child inodes
     */
    async listChildren(parentPath) {
        if (!this.db) {
            throw new Error('ZosStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readonly');
            const store = tx.objectStore(this.INODES_STORE);
            const index = store.index('parent_path');
            const request = index.getAll(parentPath);

            request.onsuccess = () => resolve(request.result || []);
            request.onerror = (event) => {
                console.error('[ZosStorage] listChildren failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Get all inodes.
     * @returns {Promise<object[]>} Array of all inodes
     */
    async getAllInodes() {
        if (!this.db) {
            throw new Error('ZosStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readonly');
            const store = tx.objectStore(this.INODES_STORE);
            const request = store.getAll();

            request.onsuccess = () => resolve(request.result || []);
            request.onerror = (event) => {
                console.error('[ZosStorage] getAllInodes failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Get all content records.
     * @returns {Promise<object[]>} Array of all content records
     */
    async getAllContent() {
        if (!this.db) {
            throw new Error('ZosStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.CONTENT_STORE], 'readonly');
            const store = tx.objectStore(this.CONTENT_STORE);
            const request = store.getAll();

            request.onsuccess = () => resolve(request.result || []);
            request.onerror = (event) => {
                console.error('[ZosStorage] getAllContent failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Store file content.
     * @param {string} path - The file path
     * @param {Uint8Array} data - The content bytes
     * @returns {Promise<boolean>} True if successful
     */
    async putContent(path, data) {
        if (!this.db) {
            throw new Error('ZosStorage not initialized');
        }

        // Update cache synchronously (optimistic)
        this.contentCache.set(path, data);

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.CONTENT_STORE], 'readwrite');
            const store = tx.objectStore(this.CONTENT_STORE);

            // Store as object with path key
            const record = { path, data };
            const request = store.put(record);

            request.onsuccess = () => resolve(true);
            request.onerror = (event) => {
                console.error('[ZosStorage] putContent failed:', event.target.error);
                // Revert cache on failure
                this.contentCache.delete(path);
                reject(event.target.error);
            };
        });
    },

    /**
     * Get file content.
     * @param {string} path - The file path
     * @returns {Promise<Uint8Array|null>} The content or null if not found
     */
    async getContent(path) {
        if (!this.db) {
            throw new Error('ZosStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.CONTENT_STORE], 'readonly');
            const store = tx.objectStore(this.CONTENT_STORE);
            const request = store.get(path);

            request.onsuccess = () => {
                const result = request.result;
                resolve(result ? result.data : null);
            };
            request.onerror = (event) => {
                console.error('[ZosStorage] getContent failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Delete file content.
     * @param {string} path - The file path
     * @returns {Promise<boolean>} True if successful
     */
    async deleteContent(path) {
        if (!this.db) {
            throw new Error('ZosStorage not initialized');
        }

        // Update cache synchronously (optimistic)
        this.contentCache.delete(path);

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.CONTENT_STORE], 'readwrite');
            const store = tx.objectStore(this.CONTENT_STORE);
            const request = store.delete(path);

            request.onsuccess = () => resolve(true);
            request.onerror = (event) => {
                console.error('[ZosStorage] deleteContent failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Get the count of inodes.
     * @returns {Promise<number>} The count
     */
    async getInodeCount() {
        if (!this.db) {
            throw new Error('ZosStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readonly');
            const store = tx.objectStore(this.INODES_STORE);
            const request = store.count();

            request.onsuccess = () => resolve(request.result);
            request.onerror = (event) => {
                console.error('[ZosStorage] getInodeCount failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Check if a path exists (async version).
     * @param {string} path - The path to check
     * @returns {Promise<boolean>} True if exists
     */
    async exists(path) {
        const inode = await this.getInode(path);
        return inode !== null;
    },

    /**
     * Clear all data.
     * @returns {Promise<boolean>} True if successful
     */
    async clear() {
        if (!this.db) {
            throw new Error('ZosStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE, this.CONTENT_STORE], 'readwrite');

            tx.objectStore(this.INODES_STORE).clear();
            tx.objectStore(this.CONTENT_STORE).clear();

            tx.oncomplete = () => {
                // Clear all caches
                this.pathCache.clear();
                this.inodeCache.clear();
                this.contentCache.clear();
                console.log('[ZosStorage] All data cleared');
                resolve(true);
            };

            tx.onerror = (event) => {
                console.error('[ZosStorage] clear failed:', event.target.error);
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
        this.pathCache.clear();
        this.inodeCache.clear();
        this.contentCache.clear();

        return new Promise((resolve, reject) => {
            const request = indexedDB.deleteDatabase(this.DB_NAME);

            request.onsuccess = () => {
                console.log('[ZosStorage] Database deleted');
                resolve(true);
            };

            request.onerror = (event) => {
                console.error('[ZosStorage] deleteDatabase failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Batch put multiple inodes (for bootstrap).
     * @param {object[]} inodes - Array of inode objects
     * @returns {Promise<number>} Number of inodes stored
     */
    async putInodes(inodes) {
        if (!this.db) {
            throw new Error('ZosStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readwrite');
            const store = tx.objectStore(this.INODES_STORE);
            let count = 0;

            for (const inode of inodes) {
                const request = store.put(inode);
                request.onsuccess = () => count++;
                // Update caches synchronously
                if (inode.path) {
                    this.pathCache.add(inode.path);
                    this.inodeCache.set(inode.path, inode);
                }
            }

            tx.oncomplete = () => {
                console.log(`[ZosStorage] Stored ${count} inodes`);
                resolve(count);
            };

            tx.onerror = (event) => {
                console.error('[ZosStorage] putInodes failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    // ==========================================================================
    // Supervisor Async API (HAL callbacks)
    // These methods are called by HAL and notify the supervisor when complete.
    // ==========================================================================

    /**
     * Start async read operation.
     * Calls supervisor.notify_storage_read_complete or notify_storage_not_found when done.
     * @param {number} requestId - Unique request ID
     * @param {string} key - Storage key to read
     */
    async startRead(requestId, key) {
        console.log(`[ZosStorage] startRead: request_id=${requestId}, key=${key}`);

        if (!this.supervisor) {
            console.error('[ZosStorage] startRead: supervisor not initialized');
            return;
        }

        try {
            await this.init();

            // Determine if this is an inode or content read based on key prefix
            let data = null;
            if (key.startsWith('content:')) {
                const path = key.substring(8); // Remove 'content:' prefix
                const content = await this.getContent(path);
                if (content) {
                    data = content;
                }
            } else if (key.startsWith('inode:')) {
                const path = key.substring(6); // Remove 'inode:' prefix
                const inode = await this.getInode(path);
                if (inode) {
                    data = new TextEncoder().encode(JSON.stringify(inode));
                }
            } else {
                // Default: treat as inode lookup
                const inode = await this.getInode(key);
                if (inode) {
                    data = new TextEncoder().encode(JSON.stringify(inode));
                }
            }

            if (data) {
                this.supervisor.notify_storage_read_complete(requestId, data);
            } else {
                this.supervisor.notify_storage_not_found(requestId);
            }
        } catch (e) {
            console.error(`[ZosStorage] startRead error: ${e.message}`);
            this.supervisor.notify_storage_error(requestId, e.message);
        }
    },

    /**
     * Start async write operation.
     * Calls supervisor.notify_storage_write_complete when done.
     * @param {number} requestId - Unique request ID
     * @param {string} key - Storage key to write
     * @param {Uint8Array} value - Data to store
     */
    async startWrite(requestId, key, value) {
        console.log(`[ZosStorage] startWrite: request_id=${requestId}, key=${key}, len=${value.length}`);

        if (!this.supervisor) {
            console.error('[ZosStorage] startWrite: supervisor not initialized');
            return;
        }

        try {
            await this.init();

            if (key.startsWith('content:')) {
                const path = key.substring(8);
                await this.putContent(path, value);
            } else if (key.startsWith('inode:')) {
                const path = key.substring(6);
                const inodeJson = new TextDecoder().decode(value);
                const inode = JSON.parse(inodeJson);
                await this.putInode(path, inode);
            } else {
                // Default: treat as inode write
                const inodeJson = new TextDecoder().decode(value);
                const inode = JSON.parse(inodeJson);
                await this.putInode(key, inode);
            }

            this.supervisor.notify_storage_write_complete(requestId);
        } catch (e) {
            console.error(`[ZosStorage] startWrite error: ${e.message}`);
            this.supervisor.notify_storage_error(requestId, e.message);
        }
    },

    /**
     * Start async delete operation.
     * Calls supervisor.notify_storage_write_complete when done.
     * @param {number} requestId - Unique request ID
     * @param {string} key - Storage key to delete
     */
    async startDelete(requestId, key) {
        console.log(`[ZosStorage] startDelete: request_id=${requestId}, key=${key}`);

        if (!this.supervisor) {
            console.error('[ZosStorage] startDelete: supervisor not initialized');
            return;
        }

        try {
            await this.init();

            if (key.startsWith('content:')) {
                const path = key.substring(8);
                await this.deleteContent(path);
            } else if (key.startsWith('inode:')) {
                const path = key.substring(6);
                await this.deleteInode(path);
            } else {
                await this.deleteInode(key);
            }

            this.supervisor.notify_storage_write_complete(requestId);
        } catch (e) {
            console.error(`[ZosStorage] startDelete error: ${e.message}`);
            this.supervisor.notify_storage_error(requestId, e.message);
        }
    },

    /**
     * Start async list operation.
     * Calls supervisor.notify_storage_list_complete with JSON array of keys.
     * @param {number} requestId - Unique request ID
     * @param {string} prefix - Key prefix to match (e.g., "inode:/home/")
     */
    async startList(requestId, prefix) {
        console.log(`[ZosStorage] startList: request_id=${requestId}, prefix=${prefix}`);

        if (!this.supervisor) {
            console.error('[ZosStorage] startList: supervisor not initialized');
            return;
        }

        try {
            await this.init();

            // List children of a path (for directory listings)
            let path = prefix;
            if (prefix.startsWith('inode:')) {
                path = prefix.substring(6);
            }

            const children = await this.listChildren(path);
            const keys = children.map(inode => inode.path);
            const keysJson = JSON.stringify(keys);

            this.supervisor.notify_storage_list_complete(requestId, keysJson);
        } catch (e) {
            console.error(`[ZosStorage] startList error: ${e.message}`);
            this.supervisor.notify_storage_error(requestId, e.message);
        }
    },

    /**
     * Start async exists check.
     * Calls supervisor.notify_storage_exists_complete with boolean.
     * @param {number} requestId - Unique request ID
     * @param {string} key - Storage key to check
     */
    async startExists(requestId, key) {
        console.log(`[ZosStorage] startExists: request_id=${requestId}, key=${key}`);

        if (!this.supervisor) {
            console.error('[ZosStorage] startExists: supervisor not initialized');
            return;
        }

        try {
            await this.init();

            let exists = false;
            if (key.startsWith('content:')) {
                // Check content - use cache first (matches what React UI sees), then IndexedDB
                const path = key.substring(8);
                exists = this.contentCache.has(path);
                if (!exists) {
                    const content = await this.getContent(path);
                    exists = content !== null;
                }
            } else if (key.startsWith('inode:')) {
                // Check inode - use cache first, then IndexedDB
                const path = key.substring(6);
                exists = this.inodeCache.has(path);
                if (!exists) {
                    const inode = await this.getInode(path);
                    exists = inode !== null;
                }
            } else {
                // Default: check inode
                exists = this.inodeCache.has(key);
                if (!exists) {
                    const inode = await this.getInode(key);
                    exists = inode !== null;
                }
            }

            this.supervisor.notify_storage_exists_complete(requestId, exists);
        } catch (e) {
            console.error(`[ZosStorage] startExists error: ${e.message}`);
            this.supervisor.notify_storage_error(requestId, e.message);
        }
    },
};

// Make ZosStorage available globally (single consolidated object)
if (typeof window !== 'undefined') {
    window.ZosStorage = ZosStorage;
}
