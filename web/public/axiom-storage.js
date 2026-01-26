/**
 * ZosStorageAxiom - IndexedDB-backed storage for Zero OS Axiom log.
 *
 * This is the persistence layer for the Axiom commit log.
 * All storage operations are async and use IndexedDB for durability.
 *
 * ## Storage Separation
 *
 * Zero OS uses 3 separate IndexedDBs:
 * - **zos-filesystem**: VFS inodes and content (see zos-storage.js)
 * - **zos-keystore**: Cryptographic key storage (see zos-keystore.js)
 * - **zos-axiom** (this file): Commit log
 *
 * @see docs/spec/v0.1.2/02-axiom/axiom-spec.md
 */
window.ZosStorageAxiom = {
  db: null,
  DB_NAME: 'zos-axiom',
  DB_VERSION: 2,
  STORE_NAME: 'commits',

  /**
   * Initialize the IndexedDB database.
   * @returns {Promise<boolean>} True if initialization succeeded.
   */
  async init() {
    if (this.db) return true;

    return new Promise((resolve, reject) => {
      const request = indexedDB.open(this.DB_NAME, this.DB_VERSION);

      request.onupgradeneeded = (event) => {
        const db = event.target.result;
        // Clean up old store if exists
        if (db.objectStoreNames.contains('log')) {
          db.deleteObjectStore('log');
        }
        if (!db.objectStoreNames.contains(this.STORE_NAME)) {
          const store = db.createObjectStore(this.STORE_NAME, { keyPath: 'seq' });
          store.createIndex('timestamp', 'timestamp', { unique: false });
          store.createIndex('commit_type', 'commit_type', { unique: false });
        }
      };

      request.onsuccess = () => {
        this.db = request.result;
        console.log('[axiom-storage] IndexedDB initialized');
        resolve(true);
      };

      request.onerror = () => {
        console.error('[axiom-storage] IndexedDB error:', request.error);
        reject(request.error);
      };
    });
  },

  /**
   * Persist a single commit entry.
   * @param {Object} entry - The commit entry with a 'seq' property.
   * @returns {Promise<number>} The sequence number of the persisted entry.
   */
  async persistEntry(entry) {
    if (!this.db) await this.init();
    return new Promise((resolve, reject) => {
      const tx = this.db.transaction(this.STORE_NAME, 'readwrite');
      const store = tx.objectStore(this.STORE_NAME);
      const request = store.put(entry);
      request.onsuccess = () => resolve(entry.seq);
      request.onerror = () => reject(request.error);
    });
  },

  /**
   * Persist multiple commit entries in a single transaction.
   * @param {Array<Object>} entries - Array of commit entries.
   * @returns {Promise<number>} The count of persisted entries.
   */
  async persistEntries(entries) {
    if (!this.db) await this.init();
    if (!entries || entries.length === 0) return 0;
    return new Promise((resolve, reject) => {
      const tx = this.db.transaction(this.STORE_NAME, 'readwrite');
      const store = tx.objectStore(this.STORE_NAME);
      let count = 0;
      for (const entry of entries) {
        const request = store.put(entry);
        request.onsuccess = () => count++;
      }
      tx.oncomplete = () => resolve(count);
      tx.onerror = () => reject(tx.error);
    });
  },

  /**
   * Load all commit entries, sorted by sequence number.
   * @returns {Promise<Array<Object>>} Array of commit entries.
   */
  async loadAll() {
    if (!this.db) await this.init();
    return new Promise((resolve, reject) => {
      const tx = this.db.transaction(this.STORE_NAME, 'readonly');
      const store = tx.objectStore(this.STORE_NAME);
      const request = store.getAll();
      request.onsuccess = () => {
        const entries = request.result || [];
        entries.sort((a, b) => a.seq - b.seq);
        resolve(entries);
      };
      request.onerror = () => reject(request.error);
    });
  },

  /**
   * Get the count of stored commit entries.
   * @returns {Promise<number>} The count of entries.
   */
  async getCount() {
    if (!this.db) await this.init();
    return new Promise((resolve, reject) => {
      const tx = this.db.transaction(this.STORE_NAME, 'readonly');
      const store = tx.objectStore(this.STORE_NAME);
      const request = store.count();
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  },

  /**
   * Clear all commit entries from storage.
   * @returns {Promise<void>}
   */
  async clear() {
    if (!this.db) await this.init();
    return new Promise((resolve, reject) => {
      const tx = this.db.transaction(this.STORE_NAME, 'readwrite');
      const store = tx.objectStore(this.STORE_NAME);
      const request = store.clear();
      request.onsuccess = () => resolve();
      request.onerror = () => reject(request.error);
    });
  },

  /**
   * Get the last sequence number in the log.
   * @returns {Promise<number>} The last sequence number, or -1 if empty.
   */
  async getLastSeq() {
    if (!this.db) await this.init();
    return new Promise((resolve, reject) => {
      const tx = this.db.transaction(this.STORE_NAME, 'readonly');
      const store = tx.objectStore(this.STORE_NAME);
      const request = store.openCursor(null, 'prev');
      request.onsuccess = () => {
        const cursor = request.result;
        resolve(cursor ? cursor.value.seq : -1);
      };
      request.onerror = () => reject(request.error);
    });
  },
};
