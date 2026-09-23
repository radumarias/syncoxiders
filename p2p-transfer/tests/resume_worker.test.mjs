import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const workerSource = await readFile(
  new URL('../assets/resume-worker.js', import.meta.url),
  'utf8',
);

function workerContext({ group, access }) {
  const context = vm.createContext({
    console,
    self: { postMessage() {} },
    structuredClone,
    mockGetGroup: async () => structuredClone(group),
    mockRootDirectory: async () => ({
      async getDirectoryHandle() {
        return {
          async getFileHandle(name) {
            const index = Number.parseInt(name, 10);
            return {
              async createSyncAccessHandle() {
                return access(index);
              },
            };
          },
        };
      },
    }),
  });
  vm.runInContext(workerSource, context, { filename: 'resume-worker.js' });
  vm.runInContext(
    'getGroup = mockGetGroup; rootDirectory = mockRootDirectory;',
    context,
  );
  return {
    prepare: vm.runInContext('prepare', context),
    release: vm.runInContext('release', context),
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
