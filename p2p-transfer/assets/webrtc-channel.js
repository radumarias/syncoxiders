const DEFAULT_MAX_MESSAGE = 64 * 1024;
const BUFFER_LOW = 1 * 1024 * 1024;
const BUFFER_HIGH = 4 * 1024 * 1024;
const SEND_TIMEOUT = 30_000;

function failure(message) {
  return new Error(message);
}

function description(type, sdp) {
  return { type, sdp };
}

export function createPeer(role, iceServers, onFrame, onIce, onOpen, onState, onStats) {
  const pc = new RTCPeerConnection({
    iceServers: iceServers.map(url => ({ urls: [url] })),
  });
  const peer = {
    pc,
    channel: null,
    closed: false,
    callbacks: { onFrame, onIce, onOpen, onState },
    role,
    sentBytes: 0,
    receivedBytes: 0,
    maxSentFrame: 0,
    maxReceivedFrame: 0,
    drainWaitMs: 0,
    drainStartedAt: null,
    lastStats: null,
    statsPending: false,
  };

  pc.onicecandidate = event => {
    const candidate = event.candidate;
    onIce(candidate ? {
      candidate: candidate.candidate,
      sdpMid: candidate.sdpMid,
      sdpMLineIndex: candidate.sdpMLineIndex,
    } : null);
  };
  pc.onconnectionstatechange = () => onState(pc.connectionState);
  pc.oniceconnectionstatechange = () => {
    if (pc.iceConnectionState === 'failed') onState('failed');
  };
  if (role === 'answerer') {
    pc.ondatachannel = event => attach(peer, event.channel);
  } else {
    attach(peer, pc.createDataChannel('syncoxiders-file-v1', { ordered: true }));
  }
  peer.statsTimer = setInterval(async () => {
    if (peer.closed || peer.channel?.readyState !== 'open' || peer.statsPending) return;
    peer.statsPending = true;
    try {
      const line = await performanceSample(peer);
      if (!peer.closed) onStats(line);
    } catch (_) {
      // getStats is optional on older WebKit implementations. Do not let a
      // diagnostics failure affect transfer or repeatedly log browser errors.
    } finally {
      peer.statsPending = false;
    }
  }, 5_000);
  return peer;
}

function attach(peer, channel) {
  if (peer.closed) {
    channel.close();
    return;
  }
  if (peer.channel && peer.channel !== channel) {
    channel.close();
    return;
  }
  peer.channel = channel;
  channel.binaryType = 'arraybuffer';
  channel.bufferedAmountLowThreshold = BUFFER_LOW;
  channel.onopen = () => peer.callbacks.onOpen(true);
  channel.onclose = () => peer.callbacks.onOpen(false);
  channel.onerror = () => peer.callbacks.onState('failed');
  channel.onmessage = event => {
    if (event.data instanceof ArrayBuffer) {
      peer.receivedBytes += event.data.byteLength;
      peer.maxReceivedFrame = Math.max(peer.maxReceivedFrame, event.data.byteLength);
      peer.callbacks.onFrame(new Uint8Array(event.data));
    } else if (ArrayBuffer.isView(event.data)) {
      peer.receivedBytes += event.data.byteLength;
      peer.maxReceivedFrame = Math.max(peer.maxReceivedFrame, event.data.byteLength);
      peer.callbacks.onFrame(
        new Uint8Array(event.data.buffer, event.data.byteOffset, event.data.byteLength),
      );
    } else {
      peer.callbacks.onState('failed');
    }
  };
}

export async function createOffer(peer) {
  const offer = await peer.pc.createOffer();
  await peer.pc.setLocalDescription(offer);
  return peer.pc.localDescription.sdp;
}

export async function acceptOffer(peer, sdp) {
  await peer.pc.setRemoteDescription(description('offer', sdp));
  const answer = await peer.pc.createAnswer();
  await peer.pc.setLocalDescription(answer);
  return peer.pc.localDescription.sdp;
}

export async function acceptAnswer(peer, sdp) {
  await peer.pc.setRemoteDescription(description('answer', sdp));
}

export async function addIce(peer, candidate, sdpMid, sdpMLineIndex) {
  await peer.pc.addIceCandidate({
    candidate,
    sdpMid: sdpMid ?? null,
    sdpMLineIndex: sdpMLineIndex ?? null,
  });
}

function waitForDrain(peer) {
  const channel = peer.channel;
  if (!channel || channel.readyState !== 'open') {
    return Promise.reject(failure('data channel is not open'));
  }
  if (channel.bufferedAmount <= BUFFER_HIGH) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      cleanup();
      reject(failure('data channel remained backpressured'));
    }, SEND_TIMEOUT);
    const low = () => {
      cleanup();
      resolve();
    };
    const closed = () => {
      cleanup();
      reject(failure('data channel closed while waiting for backpressure'));
    };
    const cleanup = () => {
      clearTimeout(timer);
      channel.removeEventListener('bufferedamountlow', low);
      channel.removeEventListener('close', closed);
      channel.removeEventListener('error', closed);
    };
    channel.addEventListener('bufferedamountlow', low, { once: true });
    channel.addEventListener('close', closed, { once: true });
    channel.addEventListener('error', closed, { once: true });
  });
}

export async function sendFrame(peer, bytes) {
  const waited = peer.channel?.bufferedAmount > BUFFER_HIGH ? performance.now() : null;
  if (waited !== null) peer.drainStartedAt = waited;
  try {
    await waitForDrain(peer);
  } finally {
    if (waited !== null) {
      peer.drainWaitMs += performance.now() - waited;
      peer.drainStartedAt = null;
    }
  }
  if (peer.closed || peer.channel?.readyState !== 'open') {
    throw failure('data channel is closed');
  }
  peer.channel.send(bytes);
  peer.sentBytes += bytes.byteLength;
  peer.maxSentFrame = Math.max(peer.maxSentFrame, bytes.byteLength);
}

export function maxMessageSize(peer) {
  const value = peer.pc.sctp?.maxMessageSize;
  const advertised = Number.isSafeInteger(value) && value > 0 ? value : DEFAULT_MAX_MESSAGE;
  // A public sender-side QA knob. Only the sender's page needs it: the
  // receiver gets the smaller frames through the same ordered data channel.
  // Keep this out of share links, whose fragment contains a bearer capability.
  const requested = new URLSearchParams(globalThis.location?.search ?? '').get('dcframe');
  return /^(16|32|64|128|256)$/.test(requested ?? '')
    ? Math.min(advertised, Number(requested) * 1024)
    : advertised;
}

function selectedPair(stats) {
  let selected;
  stats.forEach(report => {
    if (report.type === 'transport' && report.selectedCandidatePairId) {
      selected = stats.get(report.selectedCandidatePairId);
    }
    if (!selected && report.type === 'candidate-pair' && report.state === 'succeeded' &&
        (report.nominated || report.selected)) {
      selected = report;
    }
  });
  return selected;
}

function candidateType(type) {
  return ['host', 'srflx', 'prflx', 'relay'].includes(type) ? type : 'unknown';
}

function candidatePath(stats, pair) {
  if (!pair) return 'unknown';
  const local = candidateType(stats.get(pair.localCandidateId)?.candidateType);
  const remote = candidateType(stats.get(pair.remoteCandidateId)?.candidateType);
  if (local === 'relay' || remote === 'relay') return 'relayed';
  if (local === 'unknown' || remote === 'unknown') return 'unknown';
  return 'direct';
}

export async function pathKind(peer) {
  try {
    const stats = await peer.pc.getStats();
    return candidatePath(stats, selectedPair(stats));
  } catch (_) {
    return 'unknown';
  }
}

function rate(now, previous, current, old) {
  if (!previous || !Number.isFinite(current) || !Number.isFinite(old) || current < old) {
    return '-';
  }
  const seconds = (now - previous.at) / 1000;
  return seconds > 0 ? String(Math.round((current - old) / seconds)) : '-';
}

// Contains only counters and candidate *types*: never log ICE addresses,
// SDP, tickets, capabilities, file names or browser identifiers.
export async function performanceSample(peer) {
  const stats = await peer.pc.getStats();
  const pair = selectedPair(stats);
  const now = performance.now();
  const previous = peer.lastStats;
  const samePair = previous?.pairId && previous.pairId === pair?.id ? previous : null;
  const safeType = id => candidateType(stats.get(id)?.candidateType);
  const local = pair ? safeType(pair.localCandidateId) : 'unknown';
  const remote = pair ? safeType(pair.remoteCandidateId) : 'unknown';
  const rtt = Number.isFinite(pair?.currentRoundTripTime)
    ? `${Math.round(pair.currentRoundTripTime * 1000)}ms` : '-';
  const available = Number.isFinite(pair?.availableOutgoingBitrate)
    ? `${Math.round(pair.availableOutgoingBitrate / 1000)}kbps` : '-';
  const pairTx = rate(now, samePair, pair?.bytesSent, samePair?.pairTx);
  const pairRx = rate(now, samePair, pair?.bytesReceived, samePair?.pairRx);
  const appTx = rate(now, previous, peer.sentBytes, previous?.appTx);
  const appRx = rate(now, previous, peer.receivedBytes, previous?.appRx);
  const totalDrain = peer.drainWaitMs +
    (peer.drainStartedAt == null ? 0 : now - peer.drainStartedAt);
  const drain = previous ? Math.max(0, Math.round(totalDrain - previous.drainWaitMs)) : 0;
  peer.lastStats = {
    at: now,
    pairId: pair?.id,
    pairTx: pair?.bytesSent,
    pairRx: pair?.bytesReceived,
    appTx: peer.sentBytes,
    appRx: peer.receivedBytes,
    drainWaitMs: totalDrain,
  };
  return `WebRTC perf role=${peer.role} path=${candidatePath(stats, pair)} ` +
    `candidates=${local}/${remote} rtt=${rtt} availableUp=${available} ` +
    `pairTx=${pairTx}B/s pairRx=${pairRx}B/s ` +
    `queuedTx=${appTx}B/s deliveredRx=${appRx}B/s ` +
    `buffered=${peer.channel?.bufferedAmount ?? 0}B drainWait=${drain}ms/interval ` +
    `maxFrameTx=${peer.maxSentFrame}B maxFrameRx=${peer.maxReceivedFrame}B`;
}

export function peerConnection(peer) {
  return peer.pc;
}

export function closePeer(peer) {
  if (peer.closed) return;
  peer.closed = true;
  clearInterval(peer.statsTimer);
  peer.callbacks.onOpen(false);
  if (peer.channel) {
    peer.channel.onopen = null;
    peer.channel.onclose = null;
    peer.channel.onerror = null;
    peer.channel.onmessage = null;
    peer.channel.close();
  }
  peer.pc.onicecandidate = null;
  peer.pc.ondatachannel = null;
  peer.pc.onconnectionstatechange = null;
  peer.pc.oniceconnectionstatechange = null;
  peer.pc.close();
  peer.callbacks.onState('closed');
}
