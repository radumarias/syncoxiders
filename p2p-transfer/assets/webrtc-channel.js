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

export function createPeer(role, iceServers, onFrame, onIce, onOpen, onState) {
  const pc = new RTCPeerConnection({
    iceServers: iceServers.map(url => ({ urls: [url] })),
  });
  const peer = {
    pc,
    channel: null,
    closed: false,
    callbacks: { onFrame, onIce, onOpen, onState },
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
      peer.callbacks.onFrame(new Uint8Array(event.data));
    } else if (ArrayBuffer.isView(event.data)) {
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
  await waitForDrain(peer);
  if (peer.closed || peer.channel?.readyState !== 'open') {
    throw failure('data channel is closed');
  }
  peer.channel.send(bytes);
}

export function maxMessageSize(peer) {
  const value = peer.pc.sctp?.maxMessageSize;
  return Number.isSafeInteger(value) && value > 0 ? value : DEFAULT_MAX_MESSAGE;
}

export async function pathKind(peer) {
  try {
    const stats = await peer.pc.getStats();
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
    if (!selected) return 'direct';
    const local = stats.get(selected.localCandidateId);
    const remote = stats.get(selected.remoteCandidateId);
    return local?.candidateType === 'relay' || remote?.candidateType === 'relay'
      ? 'relayed'
      : 'direct';
  } catch (_) {
    return 'direct';
  }
}

export function peerConnection(peer) {
  return peer.pc;
}

export function closePeer(peer) {
  if (peer.closed) return;
  peer.closed = true;
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
