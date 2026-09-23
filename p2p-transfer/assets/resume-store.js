// Main-thread facade for durable receive staging. File access is delegated to a
// dedicated worker because OPFS synchronous handles are intentionally unavailable
// on Window, and are the broadly supported route on iOS Safari.

let worker;
let workerSource;
let nextId = 1;
const pending = new Map();
const objectUrls = new Set();

function storageWorker() {
  if (worker) return worker;
  if (!globalThis.isSecureContext || !globalThis.Worker) {
    throw new Error('browser storage is unavailable outside a secure page');
  }
  if (!workerSource) throw new Error('resume worker was not configured');
  const workerUrl = URL.createObjectURL(new Blob([workerSource], {
    type: 'text/javascript',
  }));
  worker = new Worker(workerUrl, { type: 'classic' });
  URL.revokeObjectURL(workerUrl);
  worker.onmessage = event => {
    const request = pending.get(event.data?.request);
    if (!request) return;
    pending.delete(event.data.request);
    if (event.data.ok) request.resolve(event.data.value);
    else request.reject(new Error(event.data.error || 'resume worker failed'));
  };
  worker.onmessageerror = () => failWorker('resume worker returned an unreadable response');
  worker.onerror = event => failWorker(event.message || 'resume worker stopped');
  return worker;
}

export function configureResumeWorker(source) {
  if (typeof source !== 'string' || source.length === 0) {
    throw new Error('invalid resume worker source');
  }
  workerSource = source;
}

function failWorker(message) {
  for (const request of pending.values()) request.reject(new Error(message));
  pending.clear();
  worker?.terminate();
  worker = undefined;
}

function request(op, value = {}, transfer = []) {
  const target = storageWorker();
  const request = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(request, { resolve, reject });
    try {
      target.postMessage({ request, op, ...value }, transfer);
    } catch (error) {
      pending.delete(request);
      reject(error);
    }
  });
}

export async function prepareResume(id, manifest) {
  // Persistence is advisory and may be unavailable or declined. Correctness
  // depends on flush/checkpoint ordering, not on this browser hint.
  navigator.storage.persist?.().catch(() => {});
  return request('prepare', { id, manifest });
}

export function readResume(id, index, offset, len) {
  return request('read', { id, index, offset, len });
}

export function writeResume(id, index, bytes) {
  // The wasm binding gives this module an owned Uint8Array. Transfer its buffer
  // so there is at most one bounded chunk outstanding between page and worker.
  return request('write', { id, index, bytes }, [bytes.buffer]);
}

export function finishResume(id, index) {
  return request('finish', { id, index });
}

export function releaseResume(id, index) {
  request('release', { id, index }).catch(() => {});
}

export function listResumes() {
  return request('list');
}

export function discardResume(id) {
  return request('discard', { id });
}

export async function exportResume(id, index) {
  const meta = await request('export', { id, index });
  const root = await navigator.storage.getDirectory();
  const base = await root.getDirectoryHandle('oxfer-resume');
  const directory = await base.getDirectoryHandle(id);
  const handle = await directory.getFileHandle(`${index}.part`);
  const file = await handle.getFile();
  if (String(file.size) !== meta.size) {
    throw new Error('the retained file no longer matches its verified size');
  }
  const url = URL.createObjectURL(file);
  objectUrls.add(url);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = meta.name;
  anchor.rel = 'noopener';
  anchor.style.display = 'none';
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  // Keep the URL alive long enough for iOS to hand it to its download UI.
  setTimeout(() => {
    URL.revokeObjectURL(url);
    objectUrls.delete(url);
  }, 60_000);
}
