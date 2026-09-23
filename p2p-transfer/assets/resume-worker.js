// Dedicated OPFS worker for resumable Oxfer receives.
//
// Data becomes acknowledged only after:
//   write -> flush OPFS -> commit IndexedDB checkpoint.
// Recovery truncates any tail beyond that checkpoint before hashing it.

'use strict';

const DB_NAME = 'oxfer-resume';
const STORE = 'groups';
const ROOT = 'oxfer-resume';
const MAX_CHUNK = 4 * 1024 * 1024;
const openFiles = new Map();
let queue = Promise.resolve();

function key(id, index) {
  return `${id}:${index}`;
}

function validId(id) {
  return typeof id === 'string' && /^[0-9a-f]{64}$/.test(id);
}

function validateManifest(manifest) {
  if (!Array.isArray(manifest) || manifest.length > 1024) {
    throw new Error('invalid resumable file manifest');
  }
  return manifest.map(file => {
    if (!file || typeof file.name !== 'string' || file.name.length > 1024 ||
        typeof file.size !== 'string' || !/^(0|[1-9][0-9]*)$/.test(file.size) ||
        typeof file.hash !== 'string' || !/^[0-9a-f]{64}$/.test(file.hash)) {
      throw new Error('invalid resumable file metadata');
    }
    const size = Number(file.size);
    if (!Number.isSafeInteger(size) || size < 0) {
      throw new Error('this browser cannot checkpoint a file of that size safely');
    }
    return {
      name: file.name,
      size: file.size,
      hash: file.hash,
      written: '0',
      verified: false,
    };
  });
}

function sameManifest(a, b) {
  return a.length === b.length && a.every((file, index) =>
    file.name === b[index].name &&
    file.size === b[index].size &&
    file.hash === b[index].hash
  );
}

function validateGroup(group, requested) {
  if (!group || group.version !== 1 || !sameManifest(group.files, requested)) {
    throw new Error('saved progress changed while local files were being locked');
  }
  if (group.files.some(file =>
    !/^(0|[1-9][0-9]*)$/.test(file.written) ||
    Number(file.written) > Number(file.size)
  )) {
    throw new Error('saved progress metadata is corrupt');
  }
}

function database() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(STORE)) {
        request.result.createObjectStore(STORE, { keyPath: 'id' });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error || new Error('could not open resume metadata'));
  });
}

async function transact(mode, body) {
  const db = await database();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, mode);
      const store = tx.objectStore(STORE);
      let result;
      try {
        result = body(store);
      } catch (error) {
        tx.abort();
        reject(error);
        return;
      }
      tx.oncomplete = () => resolve(result);
      tx.onerror = () => reject(tx.error || new Error('resume metadata transaction failed'));
      tx.onabort = () => reject(tx.error || new Error('resume metadata transaction was aborted'));
    });
  } finally {
    db.close();
  }
}

async function getGroup(id) {
  const db = await database();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, 'readonly');
      const request = tx.objectStore(STORE).get(id);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error || new Error('could not read resume metadata'));
    });
  } finally {
    db.close();
  }
}

async function putGroup(group) {
  group.updated = Date.now();
  await transact('readwrite', store => store.put(group));
}

async function rootDirectory() {
  if (!navigator.storage?.getDirectory) {
    throw new Error('this browser does not support resumable file storage');
  }
  const root = await navigator.storage.getDirectory();
  return root.getDirectoryHandle(ROOT, { create: true });
}

async function call(handle, method, ...args) {
  return Promise.resolve(handle[method](...args));
}

async function closeEntry(entry) {
  if (!entry) return;
  try {
    await call(entry.access, 'close');
  } finally {
    openFiles.delete(key(entry.id, entry.index));
  }
}

async function prepare(message) {
  if (!validId(message.id)) throw new Error('invalid resumable transfer id');
  const requested = validateManifest(message.manifest);
  let group = await getGroup(message.id);
  if (group && !sameManifest(group.files, requested)) {
    throw new Error('saved progress belongs to a different file manifest');
  }
  if (!group) {
    group = { id: message.id, version: 1, files: requested, updated: Date.now() };
    await putGroup(group);
  }
  validateGroup(group, requested);
  if (group.files.some((_, index) => openFiles.has(key(message.id, index)))) {
    throw new Error('this local copy is already open in another transfer');
  }

  const opened = [];
  try {
    const base = await rootDirectory();
    const directory = await base.getDirectoryHandle(message.id, { create: true });
    for (let index = 0; index < group.files.length; index++) {
      const file = await directory.getFileHandle(`${index}.part`, { create: true });
      if (typeof file.createSyncAccessHandle !== 'function') {
        throw new Error('this browser does not support resumable file storage');
      }
      // Default exclusive lock. Avoid Chrome-only lock mode options.
      const access = await file.createSyncAccessHandle();
      // Cleanup owns the handle immediately. Initialization failures must not leak an
      // exclusive lock that can only be cleared by terminating this worker.
      const entry = { id: message.id, index, access, written: 0, size: 0 };
      openFiles.set(key(message.id, index), entry);
      opened.push(entry);
    }

    // Acquiring every file handle excludes another receiver for this entire manifest. The
    // checkpoint read before lock acquisition was only sufficient to locate those files:
    // another tab may have advanced it while this worker waited. Reread it now, then perform
    // recovery from this authoritative snapshot.
    group = await getGroup(message.id);
    validateGroup(group, requested);
    for (const entry of opened) {
      const meta = group.files[entry.index];
      const size = Number(await call(entry.access, 'getSize'));
      const checkpoint = Number(meta.written);
      if (!Number.isSafeInteger(size) || size < checkpoint) {
        throw new Error('saved file is shorter than its durable checkpoint');
      }
      // Discard bytes flushed before a metadata transaction failed or the page was killed.
      if (size !== checkpoint) await call(entry.access, 'truncate', checkpoint);
      entry.written = checkpoint;
      entry.size = Number(meta.size);
    }
    return group.files.map(file => ({ written: file.written }));
  } catch (error) {
    for (const entry of opened) {
      try { await closeEntry(entry); } catch (_) {}
    }
    throw error;
  }
}

function entryFor(message) {
  const entry = openFiles.get(key(message.id, message.index));
  if (!entry) throw new Error('the resumable file is not open');
  return entry;
}

async function read(message) {
  const entry = entryFor(message);
  const offset = Number(message.offset);
  if (!Number.isSafeInteger(offset) || offset < 0 ||
      !Number.isInteger(message.len) || message.len < 0 || message.len > MAX_CHUNK ||
      offset + message.len > entry.written) {
    throw new Error('invalid resumable read range');
  }
  const bytes = new Uint8Array(message.len);
  const read = Number(await call(entry.access, 'read', bytes, { at: offset }));
  if (read !== message.len) throw new Error('saved file ended before its checkpoint');
  return bytes.buffer;
}

async function write(message) {
  const entry = entryFor(message);
  if (!(message.bytes instanceof Uint8Array) || message.bytes.byteLength > MAX_CHUNK) {
    throw new Error('invalid resumable write');
  }
  if (entry.written + message.bytes.byteLength > entry.size) {
    throw new Error('download exceeded its declared size');
  }
  let consumed = 0;
  while (consumed < message.bytes.byteLength) {
    const part = message.bytes.subarray(consumed);
    const count = Number(await call(entry.access, 'write', part, {
      at: entry.written + consumed,
    }));
    if (!Number.isInteger(count) || count <= 0 || count > part.byteLength) {
      throw new Error('local storage made no progress');
    }
    consumed += count;
  }
  await call(entry.access, 'flush');
  const next = entry.written + consumed;
  const group = await getGroup(entry.id);
  if (!group || group.files[entry.index].written !== String(entry.written)) {
    throw new Error('resume checkpoint changed unexpectedly');
  }
  group.files[entry.index].written = String(next);
  group.files[entry.index].verified = false;
  await putGroup(group);
  // Update memory only after the durable metadata transaction commits.
  entry.written = next;
}

async function finish(message) {
  const entry = entryFor(message);
  if (entry.written !== entry.size) throw new Error('download is incomplete');
  await call(entry.access, 'flush');
  await closeEntry(entry);
  const group = await getGroup(message.id);
  if (!group || group.files[message.index].written !== group.files[message.index].size) {
    throw new Error('resume checkpoint changed unexpectedly');
  }
  group.files[message.index].verified = true;
  await putGroup(group);
}

async function release(message) {
  await closeEntry(openFiles.get(key(message.id, message.index)));
}

async function list() {
  const db = await database();
  try {
    const groups = await new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, 'readonly');
      const request = tx.objectStore(STORE).getAll();
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error || new Error('could not list resume metadata'));
    });
    return groups.sort((a, b) => b.updated - a.updated);
  } finally {
    db.close();
  }
}

async function discard(message) {
  if (!validId(message.id)) throw new Error('invalid resumable transfer id');
  if ([...openFiles.values()].some(entry => entry.id === message.id)) {
    throw new Error('cancel or close the active transfer before deleting this copy');
  }
  const base = await rootDirectory();
  try {
    await base.removeEntry(message.id, { recursive: true });
  } catch (error) {
    if (error?.name !== 'NotFoundError') throw error;
  }
  await transact('readwrite', store => store.delete(message.id));
}

async function exportFile(message) {
  const group = await getGroup(message.id);
  const file = group?.files?.[message.index];
  if (!file?.verified || file.written !== file.size) {
    throw new Error('only a complete verified local copy can be downloaded');
  }
  return { name: file.name, size: file.size };
}

async function dispatch(message) {
  switch (message.op) {
    case 'prepare': return prepare(message);
    case 'read': return read(message);
    case 'write': return write(message);
    case 'finish': return finish(message);
    case 'release': return release(message);
    case 'list': return list();
    case 'discard': return discard(message);
    case 'export': return exportFile(message);
    default: throw new Error('unknown resume operation');
  }
}

self.onmessage = event => {
  const message = event.data;
  // Serialize every operation. This bounds outstanding storage work and makes
  // checkpoint transitions easy to reason about.
  queue = queue
    .then(async () => {
      const value = await dispatch(message);
      if (value instanceof ArrayBuffer) {
        self.postMessage({ request: message.request, ok: true, value }, [value]);
      } else {
        self.postMessage({ request: message.request, ok: true, value });
      }
    })
    .catch(error => {
      self.postMessage({
        request: message?.request,
        ok: false,
        error: error?.message || String(error),
      });
    });
};
