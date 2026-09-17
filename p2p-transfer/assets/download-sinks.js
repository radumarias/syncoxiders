// Local wasm-bindgen module. All handles stay on the browser's main JS thread.
// The SW acknowledgement means "accepted by the response stream", not disk fsync.
const MAX_CHUNK = 256 * 1024;
const TIMEOUT = 30_000;
const PROTOCOL = 'p2p-download-v1';

function failure(message, code = 'Js') {
  return Object.assign(new Error(message), { code });
}

function detail(error) {
  return `${error?.name || 'Error'}: ${error?.message || error}`;
}

function deadline(promise, message, ms = TIMEOUT) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => { timer = setTimeout(() => reject(failure(message)), ms); }),
  ]).finally(() => clearTimeout(timer));
}

// Pick ALL handles before opening writable streams: another picker should follow
// the previous picker immediately, not slow disk work. The specification refreshes
// activation on picker completion, but engines/policy may still require a new click.
export async function openFsa(names) {
  if (!globalThis.isSecureContext || typeof globalThis.showSaveFilePicker !== 'function') {
    throw failure('Save-file picker unavailable. Use HTTPS and a browser with File System Access, or select streaming download.', 'Unsupported');
  }
  const handles = [];
  for (const name of names) {
    try {
      handles.push(await globalThis.showSaveFilePicker({ suggestedName: name }));
    } catch (error) {
      if (error?.name === 'AbortError') throw failure('Save cancelled.', 'Cancelled');
      throw failure(`Could not choose a destination (${detail(error)}). Select one file and click Save again; file pickers require a fresh user gesture.`);
    }
  }
  const sinks = [];
  try {
    for (const handle of handles) {
      // createWritable uses a temporary file; only close commits its contents.
      // Do not claim abort deletes new files created by the picker itself.
      const writable = await handle.createWritable();
      sinks.push(new FsaWriter(writable));
    }
    return sinks;
  } catch (error) {
    await Promise.all(sinks.map(sink => sink.abort()));
    throw failure(`Could not open the selected destination (${detail(error)}). Check write permission and available disk space, then choose a new destination.`);
  }
}

class FsaWriter {
  constructor(writable) {
    this.writable = writable;
    this.closed = false;
    this.busy = false;
  }

  async write(bytes) {
    if (this.closed || this.busy) throw failure('Destination is closed or a previous write is uncertain. Start a new transfer.');
    if (!(bytes instanceof Uint8Array) || bytes.byteLength > MAX_CHUNK) throw failure('Invalid download chunk.');
    this.busy = true;
    try {
      await deadline(this.writable.write(bytes), 'Destination write timed out. Cancel and choose a new destination.');
      if (this.closed) throw failure('Destination was cancelled during write.');
      this.busy = false;
    } catch (error) {
      void this.abort();
      throw failure(`Could not write destination (${detail(error)}). Check disk space and permissions; restart the transfer.`);
    }
  }

  async finish() {
    if (this.closed || this.busy) throw failure('Cannot finish an interrupted destination. Start a new transfer.');
    this.busy = true;
    // No timeout/retry on close: once commit starts, its outcome cannot be undone.
    await this.writable.close();
    this.closed = true;
  }

  async abort() {
    if (this.closed) return;
    this.closed = true;
    try {
      await deadline(this.writable.abort(), 'Destination abort timed out.', 5_000);
    } catch (error) {
      console.warn('Could not confirm destination cleanup:', error);
    }
  }
}

async function controllingWorker() {
  if (!globalThis.isSecureContext || !navigator.serviceWorker ||
      typeof MessageChannel !== 'function' || typeof ReadableStream !== 'function' ||
      !globalThis.crypto?.getRandomValues) {
    throw failure('Streaming downloads require HTTPS, service workers, MessageChannel and Streams support.', 'Unsupported');
  }
  if (!globalThis.p2pServiceWorkerRegistration) {
    throw failure('Streaming downloads are unavailable while service workers are disabled (including #dev).', 'Unsupported');
  }
  let registration;
  try {
    registration = await deadline(globalThis.p2pServiceWorkerRegistration, 'Service worker registration timed out. Reload and retry.');
  } catch (error) {
    throw failure(`Could not start streaming downloads (${detail(error)}). Reload outside private browsing or choose another destination.`);
  }
  const container = navigator.serviceWorker;
  const expectedScript = new URL('sw.js', document.baseURI).href;
  const current = () => {
    const worker = container.controller;
    return worker && worker.scriptURL === expectedScript &&
      worker === registration.active && worker.state === 'activated' ? worker : null;
  };
  if (current()) return current();
  let listener;
  try {
    return await deadline(new Promise(resolve => {
      listener = () => { const worker = current(); if (worker) resolve(worker); };
      container.addEventListener('controllerchange', listener);
      // Close the check/listen race on a first visit.
      listener();
    }), 'The download worker is not controlling this page. Reload once, then click Save.');
  } finally {
    container.removeEventListener('controllerchange', listener);
  }
}

class SwWriter {
  constructor(worker, name, size) {
    this.worker = worker;
    this.error = null;
    this.closed = false;
    this.demand = false;
    this.waiter = null;
    this.busy = false;
    this.sequence = 0;
    this.pingPending = false;
    this.iframe = null;
    this.token = Array.from(crypto.getRandomValues(new Uint8Array(32)), b => b.toString(16).padStart(2, '0')).join('');
    const channel = new MessageChannel();
    this.port = channel.port1;
    this.port.onmessage = event => this.message(event.data);
    this.port.onmessageerror = () => this.fail(failure('The download worker sent an unreadable message. Restart the transfer.'));
    this.changed = () => {
      if (navigator.serviceWorker.controller !== worker || worker.state === 'redundant') {
        this.fail(failure('The download worker was replaced. Restart the transfer; partial downloads cannot be resumed.'));
      }
    };
    navigator.serviceWorker.addEventListener('controllerchange', this.changed);
    worker.addEventListener('statechange', this.changed);
    // One ping outstanding at most. This is best effort, not a lifetime promise.
    this.heartbeat = setInterval(() => {
      if (this.pingPending) {
        this.fail(failure('The download worker stopped responding. Restart the transfer.'));
        return;
      }
      this.pingPending = true;
      try { worker.postMessage({ type: PROTOCOL, op: 'ping', token: this.token }); }
      catch (error) { this.fail(failure(`Download worker unavailable (${detail(error)}). Restart the transfer.`)); }
    }, 10_000);
    this.ready = this.wait('ready');
    try {
      worker.postMessage({ type: PROTOCOL, op: 'open', token: this.token, name, size }, [channel.port2]);
    } catch (error) {
      channel.port2.close();
      this.fail(failure(`Could not contact download worker (${detail(error)}). Reload and retry.`));
    }
  }

  wait(op) {
    if (this.error) return Promise.reject(this.error);
    if (this.closed || this.waiter) return Promise.reject(failure('Invalid concurrent download operation.'));
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => this.fail(failure(`Streaming download stalled waiting for ${op}. Check the browser download prompt, then restart.`)), TIMEOUT);
      this.waiter = { op, resolve, reject, timer };
    });
  }

  resolve(op, value) {
    if (this.waiter?.op !== op) {
      this.fail(failure(`Unexpected download acknowledgement (${op}). Restart the transfer.`));
      return;
    }
    const waiter = this.waiter;
    this.waiter = null;
    clearTimeout(waiter.timer);
    waiter.resolve(value);
  }

  message(message) {
    if (this.closed) return;
    switch (message?.op) {
      case 'ready': this.resolve('ready', message.url); break;
      case 'started': this.resolve('started'); break;
      case 'demand':
        if (this.demand) { this.fail(failure('Duplicate download demand. Restart the transfer.')); break; }
        this.demand = true;
        if (this.waiter?.op === 'demand') this.resolve('demand');
        break;
      case 'ack':
        if (message.sequence !== this.sequence) { this.fail(failure('Out-of-order download acknowledgement. Restart the transfer.')); break; }
        this.resolve('ack');
        break;
      case 'ended': this.resolve('ended'); break;
      case 'pong': this.pingPending = false; break;
      case 'error': this.fail(failure(`Streaming download failed: ${message.message}. Restart the transfer or choose another destination.`)); break;
      default: this.fail(failure('Invalid download worker response. Reload the page.'));
    }
  }

  async start() {
    try {
      const url = new URL(await this.ready);
      const prefix = new URL('download/', document.baseURI);
      if (url.origin !== prefix.origin || url.pathname !== `${prefix.pathname}${this.token}` || url.search || url.hash) {
        throw failure('Invalid download route. Reload the page.');
      }
      const started = this.wait('started');
      this.iframe = document.createElement('iframe');
      this.iframe.hidden = true;
      this.iframe.src = url.href;
      document.body.appendChild(this.iframe);
      await started; // A real fetch, not just API feature detection.
      return this;
    } catch (error) {
      this.fail(error);
      throw error;
    }
  }

  async write(bytes) {
    if (this.error) throw this.error;
    if (this.closed || this.busy) throw failure('Cannot reuse an interrupted streaming download. Start a new transfer.');
    if (!(bytes instanceof Uint8Array) || !bytes.byteLength || bytes.byteLength > MAX_CHUNK) throw failure('Invalid streaming download chunk.');
    this.busy = true;
    try {
      if (!this.demand) await this.wait('demand');
      if (this.error) throw this.error;
      this.demand = false;
      this.sequence += 1;
      const ack = this.wait('ack');
      // Transfer the one owned buffer. No second write may post before its ack.
      this.port.postMessage({ op: 'chunk', sequence: this.sequence, bytes }, [bytes.buffer]);
      await ack;
      this.busy = false;
    } catch (error) {
      this.fail(error);
      throw error;
    }
  }

  async finish() {
    if (this.error) throw this.error;
    if (this.closed || this.busy) throw failure('Cannot finish an interrupted streaming download.');
    this.busy = true;
    try {
      const ended = this.wait('ended');
      this.port.postMessage({ op: 'end' });
      await ended;
      this.cleanup();
    } catch (error) {
      this.fail(error);
      throw error;
    }
  }

  async abort() {
    this.fail(failure('Download cancelled. Start a new transfer to save again.'));
  }

  fail(error) {
    if (this.closed) return;
    this.error = error;
    try { this.port.postMessage({ op: 'abort' }); } catch (_) { /* worker gone */ }
    if (this.waiter) {
      clearTimeout(this.waiter.timer);
      this.waiter.reject(error);
      this.waiter = null;
    }
    this.cleanup();
  }

  cleanup() {
    this.closed = true;
    clearInterval(this.heartbeat);
    navigator.serviceWorker.removeEventListener('controllerchange', this.changed);
    this.worker.removeEventListener('statechange', this.changed);
    this.port.onmessage = null;
    this.port.onmessageerror = null;
    this.port.close();
    this.iframe?.remove();
    this.iframe = null;
  }
}

export async function openSw(name, size) {
  const worker = await controllingWorker();
  return new SwWriter(worker, name, size).start();
}

export function writeSink(sink, bytes) { return sink.write(bytes); }
export function finishSink(sink) { return sink.finish(); }
export function abortSink(sink) { return sink.abort(); }
export function dropSink(sink) {
  // Synchronous invalidation with best-effort asynchronous cleanup only.
  void sink.abort().catch(error => console.warn('Download cleanup failed:', error));
}
