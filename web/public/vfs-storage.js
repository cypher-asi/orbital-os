/**
 * VfsStorage - IndexedDB persistence for Zero OS Virtual Filesystem
 * 
 * Database: zos-userspace
 * Object Stores:
 *   - inodes: Filesystem metadata (path -> Inode)
 *   - content: File content blobs (path -> Uint8Array)
 * 
 * This follows the same pattern as AxiomStorage for consistency.
 */

const VfsStorage = {
    /** @type {IDBDatabase|null} */
    db: null,

    /** Database name */
    DB_NAME: 'zos-userspace',
    
    /** Database version */
    DB_VERSION: 1,

    /** Object store names */
    INODES_STORE: 'inodes',
    CONTENT_STORE: 'content',

    /**
     * Initialize the VfsStorage database
     * @returns {Promise<boolean>} True if successful
     */
    async init() {
        if (this.db) {
            console.log('[VfsStorage] Already initialized');
            return true;
        }

        return new Promise((resolve, reject) => {
            const request = indexedDB.open(this.DB_NAME, this.DB_VERSION);

            request.onupgradeneeded = (event) => {
                const db = event.target.result;
                console.log('[VfsStorage] Creating object stores...');

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

            request.onsuccess = (event) => {
                this.db = event.target.result;
                console.log('[VfsStorage] Database initialized');
                resolve(true);
            };

            request.onerror = (event) => {
                console.error('[VfsStorage] Failed to open database:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Store an inode
     * @param {string} path - The canonical path
     * @param {object} inode - The inode object
     * @returns {Promise<boolean>} True if successful
     */
    async putInode(path, inode) {
        if (!this.db) {
            throw new Error('VfsStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readwrite');
            const store = tx.objectStore(this.INODES_STORE);
            
            // Ensure path is set as the key
            const record = { ...inode, path };
            const request = store.put(record);

            request.onsuccess = () => resolve(true);
            request.onerror = (event) => {
                console.error('[VfsStorage] putInode failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Get an inode by path
     * @param {string} path - The canonical path
     * @returns {Promise<object|null>} The inode or null if not found
     */
    async getInode(path) {
        if (!this.db) {
            throw new Error('VfsStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readonly');
            const store = tx.objectStore(this.INODES_STORE);
            const request = store.get(path);

            request.onsuccess = () => resolve(request.result || null);
            request.onerror = (event) => {
                console.error('[VfsStorage] getInode failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Delete an inode by path
     * @param {string} path - The canonical path
     * @returns {Promise<boolean>} True if successful
     */
    async deleteInode(path) {
        if (!this.db) {
            throw new Error('VfsStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readwrite');
            const store = tx.objectStore(this.INODES_STORE);
            const request = store.delete(path);

            request.onsuccess = () => resolve(true);
            request.onerror = (event) => {
                console.error('[VfsStorage] deleteInode failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * List all children of a directory
     * @param {string} parentPath - The parent directory path
     * @returns {Promise<object[]>} Array of child inodes
     */
    async listChildren(parentPath) {
        if (!this.db) {
            throw new Error('VfsStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readonly');
            const store = tx.objectStore(this.INODES_STORE);
            const index = store.index('parent_path');
            const request = index.getAll(parentPath);

            request.onsuccess = () => resolve(request.result || []);
            request.onerror = (event) => {
                console.error('[VfsStorage] listChildren failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Get all inodes (for debugging/export)
     * @returns {Promise<object[]>} Array of all inodes
     */
    async getAllInodes() {
        if (!this.db) {
            throw new Error('VfsStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readonly');
            const store = tx.objectStore(this.INODES_STORE);
            const request = store.getAll();

            request.onsuccess = () => resolve(request.result || []);
            request.onerror = (event) => {
                console.error('[VfsStorage] getAllInodes failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Store file content
     * @param {string} path - The file path
     * @param {Uint8Array} data - The content bytes
     * @returns {Promise<boolean>} True if successful
     */
    async putContent(path, data) {
        if (!this.db) {
            throw new Error('VfsStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.CONTENT_STORE], 'readwrite');
            const store = tx.objectStore(this.CONTENT_STORE);
            
            // Store as object with path key
            const record = { path, data };
            const request = store.put(record);

            request.onsuccess = () => resolve(true);
            request.onerror = (event) => {
                console.error('[VfsStorage] putContent failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Get file content
     * @param {string} path - The file path
     * @returns {Promise<Uint8Array|null>} The content or null if not found
     */
    async getContent(path) {
        if (!this.db) {
            throw new Error('VfsStorage not initialized');
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
                console.error('[VfsStorage] getContent failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Delete file content
     * @param {string} path - The file path
     * @returns {Promise<boolean>} True if successful
     */
    async deleteContent(path) {
        if (!this.db) {
            throw new Error('VfsStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.CONTENT_STORE], 'readwrite');
            const store = tx.objectStore(this.CONTENT_STORE);
            const request = store.delete(path);

            request.onsuccess = () => resolve(true);
            request.onerror = (event) => {
                console.error('[VfsStorage] deleteContent failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Get the count of inodes
     * @returns {Promise<number>} The count
     */
    async getInodeCount() {
        if (!this.db) {
            throw new Error('VfsStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readonly');
            const store = tx.objectStore(this.INODES_STORE);
            const request = store.count();

            request.onsuccess = () => resolve(request.result);
            request.onerror = (event) => {
                console.error('[VfsStorage] getInodeCount failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Check if a path exists
     * @param {string} path - The path to check
     * @returns {Promise<boolean>} True if exists
     */
    async exists(path) {
        const inode = await this.getInode(path);
        return inode !== null;
    },

    /**
     * Clear all data (for testing)
     * @returns {Promise<boolean>} True if successful
     */
    async clear() {
        if (!this.db) {
            throw new Error('VfsStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE, this.CONTENT_STORE], 'readwrite');
            
            const inodesClear = tx.objectStore(this.INODES_STORE).clear();
            const contentClear = tx.objectStore(this.CONTENT_STORE).clear();

            tx.oncomplete = () => {
                console.log('[VfsStorage] All data cleared');
                resolve(true);
            };

            tx.onerror = (event) => {
                console.error('[VfsStorage] clear failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Delete the entire database (for testing/reset)
     * @returns {Promise<boolean>} True if successful
     */
    async deleteDatabase() {
        if (this.db) {
            this.db.close();
            this.db = null;
        }

        return new Promise((resolve, reject) => {
            const request = indexedDB.deleteDatabase(this.DB_NAME);

            request.onsuccess = () => {
                console.log('[VfsStorage] Database deleted');
                resolve(true);
            };

            request.onerror = (event) => {
                console.error('[VfsStorage] deleteDatabase failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },

    /**
     * Batch put multiple inodes (for bootstrap)
     * @param {object[]} inodes - Array of inode objects
     * @returns {Promise<number>} Number of inodes stored
     */
    async putInodes(inodes) {
        if (!this.db) {
            throw new Error('VfsStorage not initialized');
        }

        return new Promise((resolve, reject) => {
            const tx = this.db.transaction([this.INODES_STORE], 'readwrite');
            const store = tx.objectStore(this.INODES_STORE);
            let count = 0;

            for (const inode of inodes) {
                const request = store.put(inode);
                request.onsuccess = () => count++;
            }

            tx.oncomplete = () => {
                console.log(`[VfsStorage] Stored ${count} inodes`);
                resolve(count);
            };

            tx.onerror = (event) => {
                console.error('[VfsStorage] putInodes failed:', event.target.error);
                reject(event.target.error);
            };
        });
    },
};

// Make available globally for WASM access
if (typeof window !== 'undefined') {
    window.VfsStorage = VfsStorage;
}
