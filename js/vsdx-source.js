// IndexedDB cache for the original OPC package.
//
// Keeping the package out of the document JSON means autosave and undo stay
// small and serialisable. A small side cache lets "Save VSDX copy" keep
// working after a browser reload as well.

const DB_NAME = 'layoutcraft-vsdx';
const STORE = 'packages';

function openDb() {
  return new Promise((resolve, reject) => {
    if (typeof indexedDB === 'undefined') { resolve(null); return; }
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE, { keyPath: 'key' });
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error || new Error('IndexedDB unavailable'));
  });
}

export async function persistVisioSource(key, source) {
  if (!key || !source || !source.package) return false;
  try {
    const db = await openDb();
    if (!db) return false;
    const record = {
      key,
      name: source.name,
      pageParts: source.pageParts || [],
      entries: [...source.package.entries()],
    };
    await new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, 'readwrite');
      tx.objectStore(STORE).put(record);
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error || new Error('Could not store package'));
    });
    db.close();
    return true;
  } catch (err) {
    console.warn('Could not persist VSDX package', err);
    return false;
  }
}

export async function loadVisioSource(key) {
  if (!key) return null;
  try {
    const db = await openDb();
    if (!db) return null;
    const record = await new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, 'readonly');
      const req = tx.objectStore(STORE).get(key);
      req.onsuccess = () => resolve(req.result || null);
      req.onerror = () => reject(req.error || new Error('Could not read package'));
    });
    db.close();
    if (!record) return null;
    const entries = new Map();
    for (const pair of record.entries || []) entries.set(pair[0], pair[1]);
    return {
      package: entries,
      name: record.name,
      pageParts: record.pageParts || [],
      key: record.key,
    };
  } catch (err) {
    console.warn('Could not restore VSDX package', err);
    return null;
  }
}

export async function deleteVisioSource(key) {
  if (!key) return;
  try {
    const db = await openDb();
    if (!db) return;
    await new Promise((resolve) => {
      const tx = db.transaction(STORE, 'readwrite');
      tx.objectStore(STORE).delete(key);
      tx.oncomplete = resolve;
      tx.onerror = resolve;
    });
    db.close();
  } catch (err) {
    console.warn('Could not remove VSDX package', err);
  }
}
