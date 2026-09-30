import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const workerSource = await readFile(
  new URL('../assets/resume-worker.js', import.meta.url),
  'utf8',
);

const DB_NAME = 'oxfer-resume';
const STORE = 'groups';
const ROOT = 'oxfer-resume';

const domError = name => new DOMException(name, name);
const later = callback => setTimeout(callback, 0);

// In-memory IndexedDB with the semantics the worker relies on. Opening a
// missing database runs a version-change transaction with oldVersion 0; if it
// is aborted, the open fails with AbortError and databases() does not list it,
// as in browsers. Browsers still keep the name on disk after such an open
// (Firefox an empty version-0 file, Chromium a LevelDB log entry), so `opened`
// records every name passed to open(). Without `enumerable`, databases() is
// missing, as in Firefox before 126. Records are stored by reference when
// seeded, so a test can change a "durable" group the way another tab would.
class FakeIndexedDB {
  stored = new Map();
  opened = [];
  unhandledErrors = 0;

  constructor({ enumerable = true } = {}) {
    if (enumerable) {
      this.databases = async () =>
        [...this.stored].map(([name, { version }]) => ({ name, version }));
    }
  }

  open(name, version) {
    this.opened.push(name);
    const request = { result: undefined, error: null, transaction: null };
    later(() => this.#open(request, name, version));
    return request;
  }

  seed(group) {
    const database = { version: 1, stores: new Map([[STORE, { keyPath: 'id', records: new Map() }]]) };
    database.stores.get(STORE).records.set(group.id, group);
    this.stored.set(DB_NAME, database);
  }

  records() {
    return this.stored.get(DB_NAME)?.stores.get(STORE)?.records;
  }

  #open(request, name, version) {
    const existing = this.stored.get(name);
    const oldVersion = existing?.version ?? 0;
    const connection = new FakeConnection(existing);
    request.result = connection;
    if (version > oldVersion) {
      // Upgrade a copy, so an aborted upgrade changes and creates nothing.
      const staged = { version, stores: new Map(existing?.stores ?? []) };
      connection.database = staged;
      connection.upgrading = true;
      let aborted = false;
      request.transaction = { mode: 'versionchange', abort: () => { aborted = true; } };
      request.onupgradeneeded?.({ oldVersion, newVersion: version, target: request });
      connection.upgrading = false;
      request.transaction = null;
      if (aborted) {
        connection.close();
        request.result = undefined;
        request.error = domError('AbortError');
        let prevented = false;
        request.onerror?.({ target: request, preventDefault: () => { prevented = true; } });
        if (!prevented) this.unhandledErrors += 1;
        return;
      }
      this.stored.set(name, staged);
    }
    request.onsuccess?.({ target: request });
  }
}

class FakeConnection {
  upgrading = false;
  closed = false;

  constructor(database) {
    this.database = database;
  }

  get objectStoreNames() {
    const stores = this.database?.stores ?? new Map();
    return { contains: name => stores.has(name) };
  }

  createObjectStore(name, { keyPath }) {
    if (!this.upgrading) throw domError('InvalidStateError');
    this.database.stores.set(name, { keyPath, records: new Map() });
  }

  transaction(name, mode) {
    if (this.closed) throw domError('InvalidStateError');
    const store = this.database?.stores.get(name);
    if (!store) throw domError('NotFoundError');
    return new FakeTransaction(store, mode);
  }

  close() {
    this.closed = true;
  }
}

class FakeTransaction {
  #requests = [];
  #aborted = false;
  error = null;

  constructor(store, mode) {
    this.store = store;
    this.mode = mode;
    later(() => this.#run());
  }

  objectStore() {
    const operation = (run, writes = false) => {
      if (writes && this.mode !== 'readwrite') throw domError('ReadOnlyError');
      const request = {};
      this.#requests.push([request, run]);
      return request;
    };
    const { records, keyPath } = this.store;
    return {
      get: id => operation(() => structuredClone(records.get(id))),
      getAll: () => operation(() => [...records.values()].map(value => structuredClone(value))),
      put: value => operation(() => { records.set(value[keyPath], structuredClone(value)); }, true),
      delete: id => operation(() => { records.delete(id); }, true),
    };
  }

  abort() {
    this.#aborted = true;
  }

  #run() {
    if (this.#aborted) {
      this.onabort?.();
      return;
    }
    for (const [request, run] of this.#requests) {
      request.result = run();
      request.onsuccess?.({ target: request });
    }
    this.oncomplete?.();
  }
}

// Origin-private file system. getDirectoryHandle and getFileHandle create an
// entry only with { create: true }, otherwise a missing one is NotFoundError.
class FakeDirectory {
  entries = new Map();

  constructor(openFile) {
    this.openFile = openFile;
  }

  async getDirectoryHandle(name, { create = false } = {}) {
    let entry = this.entries.get(name);
    if (!entry) {
      if (!create) throw domError('NotFoundError');
      entry = new FakeDirectory(this.openFile);
      this.entries.set(name, entry);
    }
    if (!(entry instanceof FakeDirectory)) throw domError('TypeMismatchError');
    return entry;
  }

  async getFileHandle(name, { create = false } = {}) {
    let entry = this.entries.get(name);
    if (!entry) {
      if (!create) throw domError('NotFoundError');
      const index = Number.parseInt(name, 10);
      const openFile = this.openFile;
      entry = { createSyncAccessHandle: async () => openFile(index) };
      this.entries.set(name, entry);
    }
    if (entry instanceof FakeDirectory) throw domError('TypeMismatchError');
    return entry;
  }

  async removeEntry(name, { recursive = false } = {}) {
    const entry = this.entries.get(name);
    if (!entry) throw domError('NotFoundError');
    if (entry instanceof FakeDirectory && entry.entries.size > 0 && !recursive) {
      throw domError('InvalidModificationError');
    }
    this.entries.delete(name);
  }
}

// An in-memory synchronous access handle.
function memoryAccess() {
  let bytes = new Uint8Array(0);
  return {
    getSize: () => bytes.byteLength,
    truncate: size => { bytes = bytes.slice(0, size); },
    read: (target, { at }) => {
      const part = bytes.subarray(at, at + target.byteLength);
      target.set(part);
      return part.byteLength;
    },
    write: (source, { at }) => {
      if (at + source.byteLength > bytes.byteLength) {
        const grown = new Uint8Array(at + source.byteLength);
        grown.set(bytes);
        bytes = grown;
      }
      bytes.set(source, at);
      return source.byteLength;
    },
    flush() {},
    close() {},
  };
}

function workerContext({ group, access = memoryAccess, enumerable = true } = {}) {
  const idb = new FakeIndexedDB({ enumerable });
  if (group) idb.seed(group);
  const opfs = new FakeDirectory(access);
  const context = vm.createContext({
    console,
    self: { postMessage() {} },
    structuredClone,
    indexedDB: idb,
    navigator: { storage: { getDirectory: async () => opfs } },
  });
  vm.runInContext(workerSource, context, { filename: 'resume-worker.js' });
  const fn = name => vm.runInContext(name, context);
  return {
    idb,
    opfs,
    prepare: fn('prepare'),
    write: fn('write'),
    finish: fn('finish'),
    release: fn('release'),
    list: fn('list'),
    discard: fn('discard'),
    exportFile: fn('exportFile'),
    // The worker checks `instanceof Uint8Array` in its own realm.
    bytes: (length, value) => vm.runInContext(`new Uint8Array(${length}).fill(${value})`, context),
    openCount: () => vm.runInContext('openFiles.size', context),
  };
}

function manifest(written = '10') {
  return {
    id: 'a'.repeat(64),
    version: 1,
    files: [{
      name: 'fixture.bin',
      size: '100',
      hash: 'b'.repeat(64),
      written,
      verified: false,
    }],
    updated: 1,
  };
}

function prepareMessage(group) {
  return {
    id: group.id,
    manifest: group.files.map(({ name, size, hash }) => ({ name, size, hash })),
  };
}

test('prepare rereads the checkpoint after acquiring every exclusive handle', async () => {
  const durable = manifest('10');
  let diskSize = 10;
  let closed = false;
  const worker = workerContext({
    group: durable,
    access() {
      // Another tab advances and commits while this worker is waiting for the OPFS lock.
      durable.files[0].written = '20';
      diskSize = 20;
      return {
        getSize: () => diskSize,
        truncate: size => { diskSize = size; },
        close: () => { closed = true; },
      };
    },
  });

  const result = await worker.prepare(prepareMessage(durable));
  assert.deepEqual(structuredClone(result), [{ written: '20' }]);
  assert.equal(diskSize, 20, 'acknowledged bytes were truncated by a stale snapshot');
  assert.equal(durable.files[0].written, '20');
  assert.equal(worker.openCount(), 1);
  await worker.release({ id: durable.id, index: 0 });
  assert.equal(closed, true);
  assert.equal(worker.openCount(), 0);
});

test('prepare truncates only bytes beyond the authoritative durable checkpoint', async () => {
  const durable = manifest('10');
  let diskSize = 20;
  const worker = workerContext({
    group: durable,
    access() {
      return {
        getSize: () => diskSize,
        truncate: size => { diskSize = size; },
        close() {},
      };
    },
  });

  const result = await worker.prepare(prepareMessage(durable));
  assert.deepEqual(structuredClone(result), [{ written: '10' }]);
  assert.equal(diskSize, 10);
  await worker.release({ id: durable.id, index: 0 });
});

test('prepare rejects a file shorter than its checkpoint and releases its lock', async () => {
  const durable = manifest('20');
  let closed = false;
  const worker = workerContext({
    group: durable,
    access() {
      return {
        getSize: () => 10,
        truncate() {},
        close: () => { closed = true; },
      };
    },
  });

  await assert.rejects(
    worker.prepare(prepareMessage(durable)),
    /shorter than its durable checkpoint/,
  );
  assert.equal(closed, true);
  assert.equal(worker.openCount(), 0);
});

test('prepare rejects metadata changed while locks were acquired', async () => {
  const durable = manifest('10');
  const request = prepareMessage(durable);
  let closed = false;
  const worker = workerContext({
    group: durable,
    access() {
      durable.files[0].hash = 'c'.repeat(64);
      return {
        getSize: () => 10,
        truncate() {},
        close: () => { closed = true; },
      };
    },
  });

  await assert.rejects(worker.prepare(request), /changed while local files were being locked/);
  assert.equal(closed, true);
  assert.equal(worker.openCount(), 0);
});

for (const fault of ['getSize', 'truncate']) {
  test(`prepare closes and untracks a handle when ${fault} fails`, async () => {
    const durable = manifest('10');
    let closed = false;
    const worker = workerContext({
      group: durable,
      access() {
        return {
          getSize: () => {
            if (fault === 'getSize') throw new Error('I/O error');
            return 20;
          },
          truncate: () => {
            if (fault === 'truncate') throw new Error('I/O error');
          },
          close: () => { closed = true; },
        };
      },
    });

    await assert.rejects(worker.prepare(prepareMessage(durable)), /I\/O error/);
    assert.equal(closed, true, 'exclusive OPFS handle leaked after recovery failed');
    assert.equal(worker.openCount(), 0, 'failed handle remained in worker tracking');
  });
}

for (const fault of ['acquire', 'initialize']) {
  test(`prepare cleans up all files when the second handle fails to ${fault}`, async () => {
    const durable = manifest('10');
    durable.files.push({
      name: 'second.bin',
      size: '100',
      hash: 'c'.repeat(64),
      written: '10',
      verified: false,
    });
    const closed = [false, false];
    const worker = workerContext({
      group: durable,
      access(index) {
        if (index === 1 && fault === 'acquire') throw new Error('I/O error');
        return {
          getSize: () => {
            if (index === 1 && fault === 'initialize') throw new Error('I/O error');
            return 10;
          },
          truncate() {},
          close: () => { closed[index] = true; },
        };
      },
    });

    await assert.rejects(worker.prepare(prepareMessage(durable)), /I\/O error/);
    assert.equal(closed[0], true, 'the first file remained locked');
    assert.equal(closed[1], fault === 'initialize');
    assert.equal(worker.openCount(), 0);
  });
}

// Browser storage (privacy.html, docs/compliance/ropa.md): the oxfer-resume
// database and directory exist only after the receiver chose "Keep a copy" and
// started saving. The app lists saved copies at start-up, so listing must not
// create them.
function assertNothingCreated(worker, { enumerable }) {
  assert.deepEqual([...worker.idb.stored.keys()], [], 'an IndexedDB database was created');
  assert.deepEqual([...worker.opfs.entries.keys()], [], 'an OPFS entry was created');
  assert.equal(worker.idb.unhandledErrors, 0, 'the aborted open was reported as an error');
  if (enumerable) {
    assert.deepEqual(worker.idb.opened, [], 'a missing database was opened, which leaves its name on disk');
  }
}

for (const enumerable of [true, false]) {
  const how = enumerable ? '' : ' (no indexedDB.databases())';
  test(`listing and exporting on a fresh profile create no database and no directory${how}`, async () => {
    const worker = workerContext({ enumerable });
    assert.deepEqual(structuredClone(await worker.list()), []);
    assertNothingCreated(worker, { enumerable });
    await assert.rejects(
      worker.exportFile({ id: 'a'.repeat(64), index: 0 }),
      /only a complete verified local copy/,
    );
    assertNothingCreated(worker, { enumerable });
  });

  test(`discarding a missing copy succeeds and creates nothing${how}`, async () => {
    const worker = workerContext({ enumerable });
    await worker.discard({ id: 'a'.repeat(64) });
    assertNothingCreated(worker, { enumerable });
  });
}

for (const enumerable of [true, false]) {
  test(`prepare creates the database and the directory; list, export and discard then use them${enumerable ? '' : ' (no indexedDB.databases())'}`, async () => {
    const worker = workerContext({ enumerable });
    const group = manifest('0');
    const result = await worker.prepare(prepareMessage(group));
    assert.deepEqual(structuredClone(result), [{ written: '0' }]);
    assert.deepEqual([...worker.idb.stored.keys()], [DB_NAME]);
    assert.equal(worker.idb.records().get(group.id).files[0].written, '0');
    const directory = worker.opfs.entries.get(ROOT);
    assert.deepEqual([...worker.opfs.entries.keys()], [ROOT]);
    assert.deepEqual([...directory.entries.get(group.id).entries.keys()], ['0.part']);

    await worker.write({ id: group.id, index: 0, bytes: worker.bytes(100, 7) });
    await worker.finish({ id: group.id, index: 0 });
    const listed = structuredClone(await worker.list());
    assert.equal(listed.length, 1);
    assert.equal(listed[0].id, group.id);
    assert.deepEqual(listed[0].files.map(({ written, verified }) => ({ written, verified })), [
      { written: '100', verified: true },
    ]);
    assert.deepEqual(structuredClone(await worker.exportFile({ id: group.id, index: 0 })), {
      name: 'fixture.bin',
      size: '100',
    });

    // Deleting another, missing copy leaves this one alone.
    await worker.discard({ id: 'c'.repeat(64) });
    assert.equal(worker.idb.records().size, 1);
    assert.deepEqual([...directory.entries.keys()], [group.id]);

    await worker.discard({ id: group.id });
    assert.equal(worker.idb.records().size, 0);
    assert.deepEqual([...directory.entries.keys()], []);
    assert.deepEqual(structuredClone(await worker.list()), []);
    assert.equal(worker.idb.unhandledErrors, 0);
  });
}

test('a database deleted by another tab after databases() listed it is not recreated', async () => {
  const worker = workerContext();
  await worker.prepare(prepareMessage(manifest('0')));
  await worker.release({ id: 'a'.repeat(64), index: 0 });
  const { databases } = worker.idb;
  worker.idb.databases = async () => {
    const listed = await databases();
    worker.idb.stored.clear();
    return listed;
  };
  assert.deepEqual(structuredClone(await worker.list()), []);
  assert.deepEqual([...worker.idb.stored.keys()], []);
  assert.equal(worker.idb.unhandledErrors, 0);
});

test('the IndexedDB fake drops a database whose creation is aborted, as browsers do', async () => {
  const idb = new FakeIndexedDB();
  const open = (onupgradeneeded) => new Promise(resolve => {
    const request = idb.open(DB_NAME, 1);
    request.onupgradeneeded = event => onupgradeneeded(request, event);
    request.onsuccess = () => resolve('success');
    request.onerror = event => {
      event.preventDefault();
      resolve(request.error.name);
    };
  });
  const oldVersions = [];
  assert.equal(await open((request, event) => {
    oldVersions.push(event.oldVersion);
    request.transaction.abort();
  }), 'AbortError');
  assert.equal(idb.stored.size, 0);
  assert.deepEqual(await idb.databases(), []);
  assert.equal(await open((request, event) => {
    oldVersions.push(event.oldVersion);
    request.result.createObjectStore(STORE, { keyPath: 'id' });
  }), 'success');
  assert.equal(idb.stored.get(DB_NAME).stores.has(STORE), true);
  assert.deepEqual(await idb.databases(), [{ name: DB_NAME, version: 1 }]);
  assert.equal(await open(() => assert.fail('an existing database was upgraded again')), 'success');
  assert.deepEqual(oldVersions, [0, 0]);
});
