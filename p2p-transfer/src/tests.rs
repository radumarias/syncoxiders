// Run with cargo test.
//
// Naming (see CLAUDE.md): `local_test_*` is pure and in-process and must pass in any
// environment; `online_test_*` touches real iroh endpoints and skips when the network or the
// relay is unavailable.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use iroh::{EndpointAddr, SecretKey};
use iroh_tickets::endpoint::EndpointTicket;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::app::P2PTransfer;
use crate::blob_store::BlobHash;
use crate::file_io::{
    hash_source, read_length, sanitize_name, AnySink, FileOrigin, FileSnapshot, MemSink, MemSource,
    SharedFile, Sink, SlowSink, Source, StuckSink, MAX_SOURCE_READ,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::file_io::{FsSink, FsSource};
use crate::node::{
    n0_relays_without_trailing_dots, without_trailing_relay_dots, DiagnosticNode, Node, Peers,
    RelayChoice, SinkPref,
};
use crate::protocol::{
    cap_eq, cap_from_hex, cap_to_hex, decode, encode_chunk, encode_control, max_payload,
    negotiate_chunk, ChunkHeader, ChunkPlan, Control, FileMeta, Frame, ProtocolError, CAP_LEN,
    DEFAULT_CHUNK, INITIAL_WINDOW, LEN_PREFIX, MAX_FRAME, MIN_USABLE_FRAME, PROTOCOL_VERSION,
};
use crate::transfer::{
    run_receiver_on, run_receiver_reconnecting, run_sender_on, AuthFailure, ByteBudget, CloseSeam,
    DataChannel, DcFactory, DcRole, FrameRx, FrameTx, MaybeDc, MemDcFactory, MemRx, MemTx,
    NoWebRtc, Path, PcState, Phase, ReceiveCommand, ReceiveOptions, ResumeFile, SenderOptions,
    SessionIo, TransferError, TransferHandle, TransferProgress, TransportError,
};

#[tokio::test]
async fn local_test_diagnostics_peer_ping_has_a_separate_protocol() {
    timeout(Duration::from_secs(20), async {
        let sender = DiagnosticNode::bind(RelayChoice::None).await.unwrap();
        let ticket = sender.ticket().await.unwrap();
        let parsed: EndpointTicket = ticket.to_string().parse().unwrap();
        assert_eq!(
            DiagnosticNode::session_id(&ticket),
            DiagnosticNode::session_id(&parsed)
        );
        let receiver = DiagnosticNode::bind(RelayChoice::None).await.unwrap();
        receiver.probe(&ticket).await.unwrap();
        assert_eq!(sender.completed_probes(), 1);
        assert_eq!(receiver.completed_probes(), 0);
        receiver.shutdown().await;
        sender.shutdown().await;
    })
    .await
    .expect("local diagnostic ping timed out");
}

#[test]
fn local_test_normalized_relay_map_only_removes_final_dns_dots() {
    let original: Vec<iroh::RelayUrl> = iroh::endpoint::default_relay_mode().relay_map().urls();
    let normalized: Vec<iroh::RelayUrl> = n0_relays_without_trailing_dots().unwrap().urls();
    assert_eq!(original.len(), normalized.len());
    for relay in original {
        let host = relay.host_str().unwrap();
        assert!(host.ends_with('.'));
        let expected = host.trim_end_matches('.');
        assert!(normalized.iter().any(|url| {
            url.host_str() == Some(expected)
                && url.scheme() == relay.scheme()
                && url.port() == relay.port()
        }));
    }
}

#[test]
fn local_test_old_tickets_keep_peer_and_ip_when_normalizing_relay() {
    let id = SecretKey::generate().public();
    let addr = EndpointAddr::new(id)
        .with_ip_addr("127.0.0.1:4433".parse().unwrap())
        .with_relay_url(
            "https://euc1-1.relay.n0.iroh.link./relay?x=y"
                .parse()
                .unwrap(),
        )
        .with_relay_url("https://other.example/".parse().unwrap());
    let old_ticket: EndpointTicket = EndpointTicket::new(addr.clone())
        .to_string()
        .parse()
        .unwrap();
    let normalized = without_trailing_relay_dots(old_ticket.endpoint_addr().clone()).unwrap();
    assert_eq!(normalized.id, id);
    assert_eq!(
        normalized.ip_addrs().copied().collect::<Vec<_>>(),
        addr.ip_addrs().copied().collect::<Vec<_>>()
    );
    assert!(normalized.relay_urls().any(|url| {
        url.host_str() == Some("euc1-1.relay.n0.iroh.link")
            && url.path() == "/relay"
            && url.query() == Some("x=y")
    }));
    assert!(normalized
        .relay_urls()
        .any(|url| url.host_str() == Some("other.example")));
    assert_eq!(normalized.addrs.len(), addr.addrs.len());
    assert_eq!(
        without_trailing_relay_dots(normalized.clone()).unwrap(),
        normalized
    );
    // This is a local dial copy; do not rewrite the sender's original ticket.
    assert!(old_ticket
        .endpoint_addr()
        .relay_urls()
        .any(|url| { url.host_str() == Some("euc1-1.relay.n0.iroh.link.") }));
}

// ---------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------

/// Deterministic bytes, so a failure is reproducible.
fn filler(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

fn test_cap(seed: u8) -> [u8; CAP_LEN] {
    let mut cap = [0u8; CAP_LEN];
    for (i, b) in cap.iter_mut().enumerate() {
        *b = seed.wrapping_add(i as u8);
    }
    cap
}

#[cfg(not(target_arch = "wasm32"))]
struct TestDir(std::path::PathBuf);

#[cfg(not(target_arch = "wasm32"))]
impl TestDir {
    fn new(label: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "syncoxiders-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("creates test directory");
        Self(path)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A ticket built offline, with both an IP and a relay address.
fn test_ticket() -> EndpointTicket {
    let id = SecretKey::generate().public();
    let addr = EndpointAddr::new(id)
        .with_ip_addr("127.0.0.1:4433".parse().expect("socket addr"))
        .with_relay_url("https://relay.example".parse().expect("relay url"));
    EndpointTicket::new(addr)
}

fn meta(name: &str, size: u64) -> FileMeta {
    FileMeta {
        name: name.to_string(),
        size,
        hash: BlobHash::from_bytes(name.as_bytes()),
    }
}

// ---------------------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------------------

#[test]
fn local_test_p2p_transfer_default() {
    // Constructing the default app must not panic.
    let _app = P2PTransfer::default();
}

// ---------------------------------------------------------------------------------------
// Protocol framing
// ---------------------------------------------------------------------------------------

#[test]
fn local_test_protocol_control_roundtrip() {
    let frames = vec![
        Control::Hello {
            version: PROTOCOL_VERSION,
            webrtc: true,
            cap: test_cap(7),
        },
        Control::Manifest {
            files: vec![meta("hello.txt", 11), meta("δοκιμή 🚀.bin", 1 << 30)],
        },
        Control::Offer {
            sdp: "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\n".to_string(),
        },
        Control::Answer {
            sdp: "v=0\r\n".to_string(),
        },
        Control::Ice {
            candidate: "candidate:1 1 udp 2113937151 127.0.0.1 50000 typ host".to_string(),
            sdp_mid: Some("0".to_string()),
            sdp_mline_index: Some(0),
        },
        Control::Ice {
            candidate: String::new(),
            sdp_mid: None,
            sdp_mline_index: None,
        },
        Control::Request {
            file: 3,
            offset: 1 << 40,
            epoch: 9,
        },
        Control::Credit {
            epoch: 9,
            bytes: INITIAL_WINDOW,
        },
        Control::UseRelay,
        Control::Done { file: 3, epoch: 9 },
        Control::Error {
            file: None,
            epoch: None,
            message: "unauthorized".to_string(),
        },
        Control::Error {
            file: Some(2),
            epoch: Some(4),
            message: "file changed since it was shared".to_string(),
        },
        Control::Verified {
            files: 2,
            bytes: 1 << 20,
        },
        Control::VerifiedAck,
    ];

    for control in frames {
        let encoded = encode_control(&control).expect("encodes");
        match decode(&encoded).expect("decodes") {
            Frame::Control(decoded) => assert_eq!(decoded, control),
            Frame::Chunk { .. } => panic!("control frame decoded as a chunk"),
        }
    }
}

#[test]
fn local_test_protocol_hello_debug_redacts_cap() {
    // The terminal buffer is user-visible and copyable, so no `Debug` output may carry a cap.
    let hello = Control::Hello {
        version: PROTOCOL_VERSION,
        webrtc: false,
        cap: test_cap(0x5a),
    };
    let rendered = format!("{hello:?}");
    assert!(rendered.contains("<redacted>"), "{rendered}");
    assert!(!rendered.contains("5a"), "{rendered}");
}

#[test]
fn local_test_protocol_chunk_roundtrip() {
    let payload = filler(4096);
    let header = ChunkHeader {
        file: 2,
        epoch: 5,
        offset: 8192,
        len: payload.len() as u32,
    };
    let encoded = encode_chunk(header, &payload);
    match decode(&encoded).expect("decodes") {
        Frame::Chunk {
            header: got,
            payload: got_payload,
        } => {
            assert_eq!(got, header);
            assert_eq!(got_payload, &payload[..]);
        }
        Frame::Control(_) => panic!("chunk frame decoded as control"),
    }

    // A zero-length chunk is still a well-formed frame.
    let empty = encode_chunk(
        ChunkHeader {
            file: 0,
            epoch: 0,
            offset: 0,
            len: 0,
        },
        &[],
    );
    assert!(matches!(
        decode(&empty).expect("decodes"),
        Frame::Chunk { payload, .. } if payload.is_empty()
    ));
}

#[test]
fn local_test_protocol_rejects_bad_tag() {
    assert!(matches!(decode(&[]), Err(ProtocolError::Empty)));
    assert!(matches!(
        decode(&[7, 1, 2, 3]),
        Err(ProtocolError::UnknownTag(7))
    ));
}

#[test]
fn local_test_protocol_rejects_length_mismatch() {
    let payload = filler(64);
    let header = ChunkHeader {
        file: 0,
        epoch: 0,
        offset: 0,
        len: payload.len() as u32,
    };
    let mut encoded = encode_chunk(header, &payload).to_vec();
    encoded.truncate(encoded.len() - 8);
    match decode(&encoded) {
        Err(ProtocolError::LengthMismatch { declared, actual }) => {
            assert_eq!(declared, 64);
            assert_eq!(actual, 56);
        }
        other => panic!("expected a length mismatch, got {other:?}"),
    }
}

#[tokio::test]
async fn local_test_protocol_rejects_oversize() {
    // Over-long frames are refused by the decoder...
    let oversize = vec![0u8; MAX_FRAME + 1];
    assert!(matches!(
        decode(&oversize),
        Err(ProtocolError::TooLarge(n)) if n == MAX_FRAME + 1
    ));

    // ...and by the reader, from the length prefix alone, before anything is allocated.
    let mut wire = ((MAX_FRAME + 1) as u32).to_le_bytes().to_vec();
    wire.extend_from_slice(b"not actually there");
    let mut cursor = std::io::Cursor::new(wire);
    let err = crate::protocol::read_frame(&mut cursor)
        .await
        .expect_err("must refuse the declared length");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("too large"), "{err}");

    // A manifest above the cap never reaches the wire either.
    let files = (0..crate::protocol::MAX_MANIFEST_FILES + 1)
        .map(|i| meta(&format!("f{i}"), 1))
        .collect();
    assert!(matches!(
        encode_control(&Control::Manifest { files }),
        Err(ProtocolError::TooLarge(_))
    ));
}

#[test]
fn local_test_negotiate_chunk() {
    assert_eq!(negotiate_chunk(None), ChunkPlan::Frame(DEFAULT_CHUNK));
    assert_eq!(
        negotiate_chunk(Some(256 * 1024)),
        ChunkPlan::Frame(DEFAULT_CHUNK)
    );
    assert_eq!(
        negotiate_chunk(Some(20 * 1024)),
        ChunkPlan::Frame(20 * 1024)
    );
    // An advertised limit is clamped down, never up: there is no floor.
    assert_eq!(negotiate_chunk(Some(8 * 1024)), ChunkPlan::Frame(8 * 1024));
    assert_eq!(
        negotiate_chunk(Some(MIN_USABLE_FRAME)),
        ChunkPlan::Frame(MIN_USABLE_FRAME)
    );
    assert_eq!(negotiate_chunk(Some(512)), ChunkPlan::TooSmall);
    assert_eq!(negotiate_chunk(Some(0)), ChunkPlan::TooSmall);
}

#[test]
fn local_test_chunk_budget_boundary() {
    for limit in [
        1024,
        8 * 1024,
        16 * 1024,
        16 * 1024 - 1,
        64 * 1024,
        256 * 1024,
    ] {
        for prefixed in [true, false] {
            let payload_len = max_payload(limit, prefixed);
            assert!(payload_len > 0, "limit {limit} should carry a payload");
            // Worst-case varint widths for every header field.
            let header = ChunkHeader {
                file: u32::MAX,
                epoch: u32::MAX,
                offset: u64::MAX - payload_len as u64,
                len: payload_len as u32,
            };
            let payload = vec![0u8; payload_len];
            let encoded = encode_chunk(header, &payload);
            let complete = encoded.len() + if prefixed { LEN_PREFIX } else { 0 };
            assert!(
                complete <= limit,
                "limit {limit} prefixed {prefixed}: complete frame {complete} exceeds it"
            );
            assert!(
                complete > limit - 8,
                "limit {limit} prefixed {prefixed}: budget {complete} wastes too much"
            );
        }
    }

    // Below the framing cost the budget saturates instead of wrapping.
    assert_eq!(max_payload(10, true), 0);
    assert_eq!(max_payload(0, false), 0);
}

// ---------------------------------------------------------------------------------------
// Capability encoding
// ---------------------------------------------------------------------------------------

#[test]
fn local_test_cap_hex_roundtrip_and_compare() {
    let cap = test_cap(0x11);
    let hex = cap_to_hex(&cap);
    assert_eq!(hex.len(), CAP_LEN * 2);
    assert!(hex
        .chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    assert_eq!(cap_from_hex(&hex), Some(cap));

    assert_eq!(cap_from_hex(&hex.to_uppercase()), None);
    assert_eq!(cap_from_hex(&hex[..30]), None);
    assert_eq!(cap_from_hex(&format!("{hex}00")), None);
    assert_eq!(cap_from_hex("zz00112233445566778899aabbccddee"), None);

    assert!(cap_eq(&cap, &cap));
    // One flipped bit in any position must be rejected.
    for i in 0..CAP_LEN {
        let mut other = cap;
        other[i] ^= 0x01;
        assert!(!cap_eq(&cap, &other), "byte {i} was not compared");
    }
}

// ---------------------------------------------------------------------------------------
// Sender transfer receipts
// ---------------------------------------------------------------------------------------

#[test]
fn local_test_completed_peer_receipt_survives_session_close() {
    let peers = Peers::default();
    let id = SecretKey::generate().public();
    let mut completed = TransferProgress::connecting();
    completed.phase = Phase::Complete { saved: Vec::new() };
    completed.path = Path::Direct;
    completed.file_name = Some("kept-after-close.bin".to_string());
    completed.bytes_done = 42;
    completed.bytes_total = 42;
    let (tx, rx) = watch::channel(completed);
    peers.register(id, rx);
    drop(tx);

    for _ in 0..2 {
        let snapshot = peers.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert!(matches!(snapshot[0].1.phase, Phase::Complete { .. }));
        assert_eq!(snapshot[0].1.path, Path::Direct);
    }

    let (tx, rx) = watch::channel(TransferProgress::connecting());
    peers.register(SecretKey::generate().public(), rx);
    drop(tx);
    assert_eq!(
        peers.snapshot().len(),
        1,
        "an unfinished closed session must not become a receipt"
    );
}

// ---------------------------------------------------------------------------------------
// Links and fragments
// ---------------------------------------------------------------------------------------

#[test]
fn local_test_ticket_link_roundtrip() {
    let ticket = test_ticket();
    let cap = test_cap(3);
    let link = Node::link("https://syncoxiders.app/", &ticket, &cap, false);
    assert!(link.starts_with("https://syncoxiders.app/#"));
    assert!(link.contains("endpoint"));

    let fragment = link.split_once('#').expect("fragment").1;
    let params = Node::parse_fragment(fragment);
    assert_eq!(params.ticket.as_ref(), Some(&ticket));
    assert_eq!(params.cap, Some(cap));
    assert!(!params.dev);
    assert_eq!(params.error, None);

    // An existing fragment on the base URL is replaced, never appended to.
    let relinked = Node::link("https://syncoxiders.app/#stale", &ticket, &cap, false);
    assert_eq!(relinked, link);
}

#[test]
fn local_test_fragment_dev_only() {
    let params = Node::parse_fragment("dev");
    assert!(params.dev);
    assert!(params.ticket.is_none());
    assert!(params.cap.is_none());
    assert_eq!(params.error, None);
}

#[test]
fn local_test_fragment_dev_and_ticket() {
    let ticket = test_ticket();
    let cap = test_cap(9);
    let link = Node::link("https://syncoxiders.app/", &ticket, &cap, true);
    let fragment = link.split_once('#').expect("fragment").1;
    assert!(fragment.starts_with("dev&"));

    let params = Node::parse_fragment(fragment);
    assert!(params.dev);
    assert_eq!(params.ticket.as_ref(), Some(&ticket));
    assert_eq!(params.cap, Some(cap));

    // Order does not matter, and a leading '#' is accepted.
    let reordered = format!("#cap={}&{}&dev", cap_to_hex(&cap), ticket);
    let params = Node::parse_fragment(&reordered);
    assert!(params.dev);
    assert_eq!(params.ticket.as_ref(), Some(&ticket));
    assert_eq!(params.cap, Some(cap));
}

#[test]
fn local_test_fragment_relay_flag() {
    let params = Node::parse_fragment("relay&sink=sw&killdc=1048576&win=16");
    assert!(params.force_relay);
    assert_eq!(params.sink_pref, SinkPref::Sw);
    assert_eq!(params.kill_dc_after, Some(1024 * 1024));
    assert_eq!(params.window, Some(16 * 1024 * 1024));
    assert_eq!(params.error, None);

    // `win=` is clamped to the tunable range; nonsense values are ignored.
    assert_eq!(
        Node::parse_fragment("win=9999").window,
        Some(crate::protocol::MAX_WINDOW_MIB * 1024 * 1024)
    );
    assert_eq!(Node::parse_fragment("win=0").window, Some(1024 * 1024));
    assert_eq!(Node::parse_fragment("win=big").window, None);
    assert_eq!(Node::parse_fragment("killdc=soon").kill_dc_after, None);
    assert_eq!(Node::parse_fragment("sink=nope").sink_pref, SinkPref::Auto);
}

#[test]
fn local_test_fragment_garbage() {
    let params = Node::parse_fragment("");
    assert_eq!(params, Default::default());

    // Something was there, and none of it was a ticket or a known flag: say the link is
    // damaged rather than let it look like a connection failure later.
    let params = Node::parse_fragment("&&nonsense&x=y&&");
    assert!(!params.dev);
    assert!(params.ticket.is_none());
    assert!(params.error.is_some());

    let params = Node::parse_fragment("endpointnotreallyaticket");
    assert!(params.ticket.is_none());
    assert!(params.error.is_some());

    // Only known flags is not an error — it is simply not a receive link.
    let params = Node::parse_fragment("dev&relay&sink=mem");
    assert!(params.dev);
    assert!(params.ticket.is_none());
    assert_eq!(params.error, None);
}

#[test]
fn local_test_fragment_damaged_ticket() {
    // A paste that lost the head of its ticket is damaged, with or without the access code —
    // and the message never echoes what was pasted, because a mangled cap is still a secret.
    let ticket = test_ticket().to_string();
    let damaged = &ticket[3..];
    let cap = test_cap(0xd1);

    for fragment in [
        damaged.to_string(),
        format!("{damaged}&cap={}", cap_to_hex(&cap)),
        format!("dev&{damaged}"),
    ] {
        let params = Node::parse_fragment(&fragment);
        assert!(params.ticket.is_none(), "{fragment}");
        let error = params.error.expect("a damaged link must say so");
        assert!(error.contains("damaged"), "{error}");
        assert!(!error.contains(damaged), "the error echoed the link");
        assert!(
            !error.contains(&cap_to_hex(&cap)),
            "the error echoed the cap"
        );
    }

    // An access code with no ticket at all is incomplete rather than damaged: the user copied
    // half of a link, not a broken one.
    let params = Node::parse_fragment(&format!("cap={}", cap_to_hex(&cap)));
    let error = params.error.expect("an incomplete link must say so");
    assert!(error.contains("incomplete"), "{error}");
    assert!(
        !error.contains(&cap_to_hex(&cap)),
        "the error echoed the cap"
    );
}

#[test]
fn local_test_fragment_unknown_flag_with_ticket() {
    // Once a ticket has parsed, tokens this version does not know are ignored, so a link
    // written by a later version still opens.
    let ticket = test_ticket();
    let cap = test_cap(0xd2);
    let params = Node::parse_fragment(&format!(
        "{ticket}&cap={}&future=1&whatever",
        cap_to_hex(&cap)
    ));
    assert_eq!(params.ticket.as_ref(), Some(&ticket));
    assert_eq!(params.cap, Some(cap));
    assert_eq!(params.error, None);
}

#[test]
fn local_test_fragment_cap() {
    let cap = test_cap(0xa0);
    // The code itself round-trips out of the fragment...
    let params = Node::parse_fragment(&format!("cap={}", cap_to_hex(&cap)));
    assert_eq!(params.cap, Some(cap));
    // ...but on its own it is half a link, and the UI has to say which half is missing.
    assert!(params
        .error
        .as_deref()
        .is_some_and(|error| error.contains("incomplete")));

    // With its ticket, it is a complete link and nothing is wrong with it.
    let ticket = test_ticket();
    let params = Node::parse_fragment(&format!("{ticket}&cap={}", cap_to_hex(&cap)));
    assert_eq!(params.cap, Some(cap));
    assert_eq!(params.ticket.as_ref(), Some(&ticket));
    assert_eq!(params.error, None);
}

#[test]
fn local_test_fragment_missing_cap() {
    // A bare ticket is parsed, but without a capability — the receive flow refuses it.
    let ticket = test_ticket();
    let params = Node::parse_fragment(&ticket.to_string());
    assert_eq!(params.ticket.as_ref(), Some(&ticket));
    assert_eq!(params.cap, None);
    assert_eq!(params.error, None);
}

#[test]
fn local_test_fragment_bad_cap() {
    for bad in [
        "cap=",
        "cap=zz",
        "cap=0123",
        "cap=ABCDEF0123456789ABCDEF0123456789",
    ] {
        let params = Node::parse_fragment(bad);
        assert_eq!(params.cap, None, "{bad}");
        assert!(params.error.is_some(), "{bad} must report an error");
    }
}

// ---------------------------------------------------------------------------------------
// File I/O
// ---------------------------------------------------------------------------------------

#[test]
fn local_test_sanitize_name() {
    assert_eq!(sanitize_name("../x"), "x");
    assert_eq!(sanitize_name("../../etc/passwd"), "passwd");
    assert_eq!(sanitize_name("a/b"), "b");
    assert_eq!(sanitize_name("C:\\windows\\system32\\evil.dll"), "evil.dll");
    assert_eq!(sanitize_name(""), "file.bin");
    assert_eq!(sanitize_name("."), "file.bin");
    assert_eq!(sanitize_name(".."), "file.bin");
    assert_eq!(sanitize_name("   "), "file.bin");
    assert_eq!(sanitize_name("a\u{0}b\nc"), "abc");
    assert_eq!(sanitize_name("report:final?.txt"), "report_final_.txt");
    assert_eq!(sanitize_name("CON"), "file.bin");
    assert_eq!(sanitize_name("com1.txt"), "file.bin");
    assert_eq!(sanitize_name("LPT9.backup"), "file.bin");
    assert_eq!(sanitize_name("normal name. "), "normal name");
    assert_eq!(sanitize_name("δοκιμή.txt"), "δοκιμή.txt");

    let long = sanitize_name(&"x".repeat(300));
    assert_eq!(long.len(), 255);
    // Truncation never splits a character.
    let wide = sanitize_name(&"é".repeat(300));
    assert!(wide.len() <= 255);
    assert!(wide.chars().all(|c| c == 'é'));
}

#[tokio::test]
async fn local_test_incremental_hash_matches_whole() {
    let data = filler(5 * 1024 * 1024);
    let mut source = MemSource::new(Bytes::from(data.clone()));
    let (progress, watcher) = watch::channel(0.0f32);

    let hashed = hash_source(&mut source, &progress).await.expect("hashes");
    assert_eq!(hashed, BlobHash::from_bytes(&data));
    assert_eq!(*watcher.borrow(), 1.0);

    // The empty file is the boundary case the manifest still has to describe.
    let mut empty = MemSource::new(Bytes::new());
    let hashed = hash_source(&mut empty, &progress).await.expect("hashes");
    assert_eq!(hashed, BlobHash::from_bytes(&[]));
}

#[tokio::test]
async fn local_test_mem_sink_writes_and_caps() {
    let mut sink = AnySink::Mem(MemSink::new("out.bin".to_string(), 16));
    sink.write(&filler(8)).await.expect("first write fits");
    assert_eq!(sink.bytes_written(), 8);
    sink.write(&filler(16)).await.expect_err("cap is enforced");
    assert_eq!(sink.bytes_written(), 8, "a refused write stores nothing");

    let saved = sink.finish().await.expect("finishes");
    assert_eq!(saved.name, "out.bin");
    assert_eq!(saved.size, 8);
    assert!(!saved.location.is_empty());
}

#[test]
fn local_test_source_read_length_clamps_in_u64() {
    assert_eq!(read_length(1_u64 << 34, 1_u64 << 33, 1024).unwrap(), 1024);
    assert_eq!(read_length(100, 90, 50).unwrap(), 10);
    assert_eq!(read_length(100, 100, 50).unwrap(), 0);
    assert_eq!(read_length(100, 101, 50).unwrap(), 0);
    assert!(read_length(u64::MAX, 0, MAX_SOURCE_READ + 1).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn local_test_fs_source_range_and_truncation() {
    let dir = TestDir::new("source");
    let path = dir.0.join("source.bin");
    let data = filler(2 * 1024 * 1024 + 37);
    std::fs::write(&path, &data).unwrap();

    let mut source = FsSource::open(&path).unwrap();
    let range = source.read((1024 * 1024 - 17) as u64, 91).await.unwrap();
    assert_eq!(&range[..], &data[1024 * 1024 - 17..1024 * 1024 + 74]);
    assert_eq!(
        source.read(data.len() as u64, 128).await.unwrap(),
        Bytes::new()
    );
    assert!(source.read(0, MAX_SOURCE_READ + 1).await.is_err());

    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(1024)
        .unwrap();
    let error = source
        .read(0, 2048)
        .await
        .expect_err("captured file was truncated");
    assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn local_test_hash_source_rejects_premature_eof() {
    let dir = TestDir::new("hash-eof");
    let path = dir.0.join("source.bin");
    std::fs::write(&path, filler(2 * 1024 * 1024)).unwrap();
    let mut source = FsSource::open(&path).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(1024)
        .unwrap();
    let (progress, _) = watch::channel(0.0);
    let error = hash_source(&mut source, &progress)
        .await
        .expect_err("must reject short source");
    assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn local_test_fs_sink_never_overwrites_and_cleans_staging() {
    let dir = TestDir::new("sink");
    let destination = dir.0.join("report.txt");
    std::fs::write(&destination, b"keep me").unwrap();

    let mut sink = FsSink::create(&dir.0, "../report.txt").unwrap();
    sink.write(b"replacement").await.unwrap();
    let saved = sink.finish().await.unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), b"keep me");
    assert_eq!(std::fs::read(&saved.location).unwrap(), b"replacement");
    assert_ne!(std::path::Path::new(&saved.location), destination);

    let mut aborted = FsSink::create(&dir.0, "aborted.bin").unwrap();
    aborted.write(b"partial").await.unwrap();
    aborted.abort().await;

    let mut dropped = FsSink::create(&dir.0, "dropped.bin").unwrap();
    dropped.write(b"partial").await.unwrap();
    drop(dropped);

    let staging = std::fs::read_dir(&dir.0)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".p2p-"))
        .collect::<Vec<_>>();
    assert!(staging.is_empty(), "staging files leaked: {staging:?}");
    assert!(!dir.0.join("aborted.bin").exists());
    assert!(!dir.0.join("dropped.bin").exists());
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_test_native_nodes_transfer_file_over_loopback() {
    let source_dir = TestDir::new("node-source");
    let receive_dir = TestDir::new("node-receive");
    let source_path = source_dir.0.join("source.bin");
    let data = filler(2 * 1024 * 1024 + 113);
    std::fs::write(&source_path, &data).unwrap();
    let snapshot = crate::file_io::snapshot_path(&source_path).unwrap();
    let meta = FileMeta {
        name: "source.bin".to_string(),
        size: data.len() as u64,
        hash: BlobHash::from_bytes(&data),
    };
    let files = Arc::new(Mutex::new(vec![SharedFile {
        meta: meta.clone(),
        origin: FileOrigin::Path(source_path),
        snapshot,
    }]));

    timeout(Duration::from_secs(30), async {
        let sender = Node::bind(files, RelayChoice::None).await.unwrap();
        let ticket = sender.ticket().await.unwrap();
        let receiver = Arc::new(
            Node::bind(Arc::new(Mutex::new(Vec::new())), RelayChoice::None)
                .await
                .unwrap(),
        );
        let options = ReceiveOptions {
            cap: Some(sender.cap()),
            ..ReceiveOptions::default()
        };
        let mut handle = TransferHandle::start_receive(receiver, ticket, options);

        loop {
            match handle.latest().phase {
                Phase::AwaitingSave { manifest } => {
                    assert_eq!(manifest, vec![meta.clone()]);
                    let sink = FsSink::create(&receive_dir.0, &manifest[0].name).unwrap();
                    handle
                        .commands
                        .send(ReceiveCommand::Save(vec![AnySink::Fs(sink)]))
                        .await
                        .unwrap();
                    break;
                }
                Phase::Failed => panic!("receive failed before Save: {:?}", handle.latest().error),
                phase => {
                    assert!(!phase.is_terminal(), "unexpected terminal phase: {phase:?}");
                    handle.progress.changed().await.unwrap();
                }
            }
        }

        let saved = loop {
            match handle.latest().phase {
                Phase::Complete { saved } => break saved,
                Phase::Failed => panic!("receive failed: {:?}", handle.latest().error),
                phase => {
                    assert!(!phase.is_terminal(), "unexpected terminal phase: {phase:?}");
                    handle.progress.changed().await.unwrap();
                }
            }
        };
        assert_eq!(saved.len(), 1);
        assert_eq!(std::fs::read(&saved[0].location).unwrap(), data);
        assert_eq!(saved[0].size, meta.size);
        sender.shutdown().await;
    })
    .await
    .expect("loopback transfer timed out");
}

// ---------------------------------------------------------------------------------------
// Engine plumbing
// ---------------------------------------------------------------------------------------

#[test]
fn local_test_inbound_budget_admits_full_window() {
    let budget_bytes = INITIAL_WINDOW as usize + MAX_FRAME;
    for frame in [8 * 1024usize, 16 * 1024, 64 * 1024] {
        let payload = max_payload(frame, false);
        let needed = (INITIAL_WINDOW as usize).div_ceil(payload);
        let budget = ByteBudget::new(budget_bytes);

        // A conforming sender fills the whole credit window without being refused, whatever
        // frame size it negotiated.
        for i in 0..needed {
            assert!(
                budget.try_admit(frame),
                "frame {frame}: refused admission {i} of {needed}"
            );
        }

        // Beyond the budget it does refuse, and space comes back on release.
        let mut refused = false;
        for _ in 0..(budget_bytes / frame + 2) {
            if !budget.try_admit(frame) {
                refused = true;
                break;
            }
        }
        assert!(refused, "frame {frame}: the budget never refused");
        budget.release(frame);
        assert!(
            budget.try_admit(frame),
            "frame {frame}: no space after release"
        );
        assert!(budget.in_queue() <= budget.budget());
    }
}

#[tokio::test]
async fn local_test_mem_transports_roundtrip() {
    // The in-memory transports are what every engine test drives the cores with, so they get
    // their own smoke test rather than being exercised for the first time by a session.
    let (tx, mut rx) = MemTx::pair();
    let tx = tx
        .with_max_frame(20 * 1024)
        .with_prefixed(false)
        .with_path(Path::Direct);
    assert_eq!(tx.max_frame(), 20 * 1024);
    assert!(!tx.is_length_prefixed());
    assert_eq!(tx.path(), Path::Direct);

    let frame = encode_control(&Control::UseRelay).expect("encodes");
    tx.send(frame.clone()).await.expect("sends");
    assert_eq!(rx.recv().await.expect("recv"), Some(frame.clone()));
    assert_eq!(tx.recorded(), vec![frame]);

    let (factory, handle) = MemDcFactory::new(Some(64 * 1024));
    assert!(factory.available());
    let mut dc = factory.create(DcRole::Offerer).expect("creates");
    assert_eq!(dc.max_message_size(), Some(64 * 1024));
    assert_eq!(dc.create_offer().await.expect("offer"), "mem-offer");

    let mut open = dc.open_watch();
    assert!(!*open.borrow());
    handle.set_open(true);
    assert!(open.changed().await.is_ok());
    assert!(*open.borrow());

    let mut state = dc.state_watch();
    handle.set_state(PcState::Connected);
    assert!(state.changed().await.is_ok());
    assert_eq!(*state.borrow(), PcState::Connected);
    assert!(!PcState::Connected.is_terminal());
    assert!(PcState::Failed.is_terminal());

    // The halves are handed out exactly once.
    let cancel = tokio_util::sync::CancellationToken::new();
    let notify = Arc::new(tokio::sync::Notify::new());
    assert!(dc
        .split(cancel.clone(), notify.clone(), INITIAL_WINDOW as usize)
        .is_some());
    assert!(dc.split(cancel, notify, INITIAL_WINDOW as usize).is_none());

    dc.close();
    assert_eq!(*dc.state_watch().borrow(), PcState::Closed);
    assert_eq!(dc.path_kind().await, Path::Direct);
}

#[tokio::test]
async fn local_test_test_sinks_behave() {
    // The slow and stuck sinks stand in for a lagging and a hung download in the engine tests.
    let mut slow = AnySink::Slow(SlowSink::new(
        "slow.bin".to_string(),
        1024,
        Duration::from_millis(1),
    ));
    slow.write(&filler(64)).await.expect("writes");
    assert_eq!(slow.bytes_written(), 64);

    let stuck = StuckSink::new("stuck.bin".to_string());
    let flag = stuck.abort_flag();
    let mut stuck = AnySink::Stuck(stuck);
    assert!(
        timeout(Duration::from_millis(50), stuck.write(&filler(8)))
            .await
            .is_err(),
        "a stuck write must never return on its own"
    );
    stuck.abort().await;
    assert!(flag.load(std::sync::atomic::Ordering::SeqCst));
}

// ---------------------------------------------------------------------------------------
// Engine harness
// ---------------------------------------------------------------------------------------

/// A shared file backed by memory, hashed the way the app would hash it.
fn shared_file(name: &str, data: &[u8]) -> SharedFile {
    SharedFile {
        meta: FileMeta {
            name: name.to_string(),
            size: data.len() as u64,
            hash: BlobHash::from_bytes(data),
        },
        origin: FileOrigin::Memory(Bytes::copy_from_slice(data)),
        snapshot: FileSnapshot {
            size: data.len() as u64,
            modified_ms: None,
        },
    }
}

/// One in-memory sink per manifest entry.
fn sinks_for(manifest: &[FileMeta]) -> Vec<AnySink> {
    manifest
        .iter()
        .map(|meta| AnySink::Mem(MemSink::new(meta.name.clone(), 1 << 30)))
        .collect()
}

/// A connection-close seam that records what it was called with.
type CloseRecord = Arc<Mutex<Option<(u32, Vec<u8>)>>>;

fn close_seam() -> (CloseSeam, CloseRecord) {
    let record: CloseRecord = Arc::new(Mutex::new(None));
    let sink = record.clone();
    (
        Box::new(move |code, reason| {
            *sink.lock().expect("close record") = Some((code, reason.to_vec()));
        }),
        record,
    )
}

fn sender_opts(cap: [u8; CAP_LEN]) -> SenderOptions {
    let mut opts = SenderOptions::new(cap);
    opts.hello_deadline = Duration::from_secs(5);
    opts.dcep_deadline = Duration::from_millis(100);
    opts.aux_deadline = Duration::from_millis(200);
    opts
}

fn receive_opts(cap: [u8; CAP_LEN]) -> ReceiveOptions {
    ReceiveOptions {
        cap: Some(cap),
        inactivity: Duration::from_secs(5),
        write_deadline: Duration::from_secs(5),
        aux_deadline: Duration::from_millis(200),
        webrtc_open: Duration::from_millis(200),
        ..Default::default()
    }
}

/// Both halves of one session pair, wired to each other.
/// A wired session pair, with a handle on each side's connection-close seam.
type Duplex<A, B> = (
    SessionIo<MemTx, MemRx, A>,
    SessionIo<MemTx, MemRx, B>,
    CloseRecord,
    CloseRecord,
);

fn duplex<A: DcFactory, B: DcFactory>(sender_dc: A, receiver_dc: B) -> Duplex<A, B> {
    let (s2r_tx, s2r_rx) = MemTx::pair();
    let (r2s_tx, r2s_rx) = MemTx::pair();
    let (sender_close, sender_record) = close_seam();
    let (receiver_close, receiver_record) = close_seam();
    (
        SessionIo {
            ctrl_tx: s2r_tx,
            ctrl_rx: r2s_rx,
            close: sender_close,
            dc: sender_dc,
        },
        SessionIo {
            ctrl_tx: r2s_tx,
            ctrl_rx: s2r_rx,
            close: receiver_close,
            dc: receiver_dc,
        },
        sender_record,
        receiver_record,
    )
}

/// Everything a scripted peer needs to drive one core by hand.
struct Scripted {
    io: SessionIo<MemTx, MemRx, NoWebRtc>,
    /// Frames the core wrote.
    out: MemRx,
    /// Frames delivered to the core.
    inject: MemTx,
    close: CloseRecord,
}

/// Model an iroh control stream that reports a lost connection rather than
/// a clean EOF when the receiver closes immediately after saving.
struct LostOnCloseRx(MemRx);

impl FrameRx for LostOnCloseRx {
    async fn recv(&mut self) -> Result<Option<Bytes>, TransportError> {
        match self.0.recv().await {
            Ok(None) => Err(TransportError::Io("connection lost".into())),
            other => other,
        }
    }
}

fn scripted() -> Scripted {
    let (core_tx, out) = MemTx::pair();
    let (inject, core_rx) = MemTx::pair();
    let (close, record) = close_seam();
    Scripted {
        io: SessionIo {
            ctrl_tx: core_tx,
            ctrl_rx: core_rx,
            close,
            dc: NoWebRtc,
        },
        out,
        inject,
        close: record,
    }
}

/// Deliver one control frame to a core under test.
async fn inject_control(tx: &MemTx, control: Control) {
    tx.send(encode_control(&control).expect("encodes"))
        .await
        .expect("injects");
}

/// The sender may report Complete only after the receiver confirms every
/// destination was verified and saved. Closing a stream is not confirmation.
async fn acknowledge_verified(inject: &MemTx, out: &mut MemRx, files: u32, bytes: u64) {
    inject_control(inject, Control::Verified { files, bytes }).await;
    assert_eq!(next_control(out).await, Control::VerifiedAck);
}

async fn next_verified(out: &mut MemRx, files: u32, bytes: u64) {
    loop {
        match next_control(out).await {
            Control::Credit { .. } => {}
            Control::Verified {
                files: count,
                bytes: total,
            } => {
                assert_eq!((count, total), (files, bytes));
                break;
            }
            other => panic!("expected credit or verified receipt, got {other:?}"),
        }
    }
}

/// Deliver one chunk to a core under test.
async fn inject_chunk(tx: &MemTx, file: u32, epoch: u32, offset: u64, payload: &[u8]) {
    let header = ChunkHeader {
        file,
        epoch,
        offset,
        len: payload.len() as u32,
    };
    tx.send(encode_chunk(header, payload))
        .await
        .expect("injects");
}

/// Read the next frame the core wrote, failing the test if it never comes.
async fn next_frame(out: &mut MemRx) -> Bytes {
    timeout(Duration::from_secs(5), out.recv())
        .await
        .expect("the core went quiet")
        .expect("transport error")
        .expect("the core closed its transport")
}

async fn next_control(out: &mut MemRx) -> Control {
    let frame = next_frame(out).await;
    match decode(&frame).expect("decodes") {
        Frame::Control(control) => control,
        Frame::Chunk { header, .. } => panic!("expected a control frame, got a chunk {header:?}"),
    }
}

/// Everything the core wrote, once it has finished and dropped its transport.
async fn drain(out: &mut MemRx) -> Vec<Bytes> {
    let mut frames = Vec::new();
    while let Ok(Some(frame)) = out.recv().await {
        frames.push(frame);
    }
    frames
}

fn hello(cap: [u8; CAP_LEN], webrtc: bool) -> Control {
    Control::Hello {
        version: PROTOCOL_VERSION,
        webrtc,
        cap,
    }
}

// ---------------------------------------------------------------------------------------
// Engine sessions
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn local_test_sender_requires_verified_receipt_not_a_full_bar_or_clean_close() {
    let cap = test_cap(0x31);
    let data = filler(128 * 1024);
    for verified in [false, true] {
        let Scripted {
            io,
            mut out,
            inject,
            ..
        } = scripted();
        let (progress, watch) = watch::channel(TransferProgress::connecting());
        let io = SessionIo {
            ctrl_tx: io.ctrl_tx,
            ctrl_rx: LostOnCloseRx(io.ctrl_rx),
            close: io.close,
            dc: io.dc,
        };
        let files = Arc::new(vec![shared_file("received.bin", &data)]);
        let session = tokio::spawn(run_sender_on(
            io,
            files,
            progress,
            CancellationToken::new(),
            sender_opts(cap),
        ));

        inject_control(&inject, hello(cap, false)).await;
        assert!(matches!(
            next_control(&mut out).await,
            Control::Hello { .. }
        ));
        assert!(matches!(
            next_control(&mut out).await,
            Control::Manifest { .. }
        ));
        inject_control(
            &inject,
            Control::Request {
                file: 0,
                offset: 0,
                epoch: 1,
            },
        )
        .await;
        inject_control(
            &inject,
            Control::Credit {
                epoch: 1,
                bytes: 1 << 20,
            },
        )
        .await;
        loop {
            match decode(&next_frame(&mut out).await).expect("decodes") {
                Frame::Chunk { .. } => {}
                Frame::Control(Control::Done { .. }) => break,
                other => panic!("unexpected frame before Done: {other:?}"),
            }
        }
        // The sender has queued every byte, but the receiver has not
        // acknowledged a destination write yet.
        assert_eq!(watch.borrow().bytes_done, 0);
        assert_eq!(watch.borrow().bytes_per_sec, 0.0);

        if verified {
            inject_control(
                &inject,
                Control::Credit {
                    epoch: 1,
                    bytes: data.len() as u64,
                },
            )
            .await;
            acknowledge_verified(&inject, &mut out, 1, data.len() as u64).await;
        }
        drop(inject);
        let result = session.await.expect("sender task");
        if verified {
            assert!(result.is_ok(), "{result:?}");
            assert!(matches!(watch.borrow().phase, Phase::Complete { .. }));
        } else {
            assert!(matches!(
                result,
                Err(TransferError::Transport(TransportError::Io(_)))
            ));
            assert_eq!(watch.borrow().phase, Phase::Failed);
        }
    }
}

#[tokio::test]
async fn local_test_sender_rate_uses_committed_credits_not_queued_or_resumed_bytes() {
    let cap = test_cap(0x34);
    let data = filler(384 * 1024);
    let offset = 64 * 1024_u64;
    let Scripted {
        io,
        mut out,
        inject,
        ..
    } = scripted();
    let files = Arc::new(vec![shared_file("resumed.bin", &data)]);
    let (progress, mut watch) = watch::channel(TransferProgress::connecting());
    let session = tokio::spawn(run_sender_on(
        io,
        files,
        progress,
        CancellationToken::new(),
        sender_opts(cap),
    ));
    inject_control(&inject, hello(cap, false)).await;
    assert!(matches!(
        next_control(&mut out).await,
        Control::Hello { .. }
    ));
    assert!(matches!(
        next_control(&mut out).await,
        Control::Manifest { .. }
    ));
    inject_control(
        &inject,
        Control::Request {
            file: 0,
            offset,
            epoch: 1,
        },
    )
    .await;
    inject_control(
        &inject,
        Control::Credit {
            epoch: 1,
            bytes: INITIAL_WINDOW,
        },
    )
    .await;
    loop {
        match decode(&next_frame(&mut out).await).expect("decodes") {
            Frame::Chunk { .. } => {}
            Frame::Control(Control::Done { .. }) => break,
            other => panic!("unexpected frame before Done: {other:?}"),
        }
    }
    assert_eq!(watch.borrow().bytes_done, offset);
    assert_eq!(watch.borrow().bytes_per_sec, 0.0);

    tokio::time::sleep(Duration::from_millis(300)).await;
    watch.borrow_and_update();
    inject_control(
        &inject,
        Control::Credit {
            epoch: 1,
            bytes: 128 * 1024,
        },
    )
    .await;
    timeout(Duration::from_secs(2), watch.changed())
        .await
        .expect("sender publishes committed progress")
        .expect("sender still running");
    let after_credit = watch.borrow();
    assert_eq!(after_credit.bytes_done, offset + 128 * 1024);
    assert!(after_credit.bytes_per_sec > 0.0);
    assert!(
        after_credit.bytes_per_sec < 1024.0 * 1024.0,
        "the resumed prefix or queued bytes inflated the measured rate"
    );
    drop(after_credit);

    watch.borrow_and_update();
    timeout(Duration::from_secs(5), watch.changed())
        .await
        .expect("idle rate expires")
        .expect("sender still running");
    assert_eq!(watch.borrow().bytes_done, offset + 128 * 1024);
    assert_eq!(watch.borrow().bytes_per_sec, 0.0);

    acknowledge_verified(&inject, &mut out, 1, data.len() as u64).await;
    drop(inject);
    session.await.expect("sender task").expect("verified");
    assert_eq!(watch.borrow().bytes_done, data.len() as u64);
}

#[tokio::test]
async fn local_test_sender_rate_spans_small_files() {
    let cap = test_cap(0x35);
    let contents = filler(32 * 1024);
    let files = Arc::new(
        (0..6)
            .map(|index| shared_file(&format!("{index}.bin"), &contents))
            .collect::<Vec<_>>(),
    );
    let total = files.len() as u64 * contents.len() as u64;
    let Scripted {
        io,
        mut out,
        inject,
        ..
    } = scripted();
    let (progress, mut watch) = watch::channel(TransferProgress::connecting());
    let session = tokio::spawn(run_sender_on(
        io,
        files,
        progress,
        CancellationToken::new(),
        sender_opts(cap),
    ));
    inject_control(&inject, hello(cap, false)).await;
    assert!(matches!(
        next_control(&mut out).await,
        Control::Hello { .. }
    ));
    assert!(matches!(
        next_control(&mut out).await,
        Control::Manifest { .. }
    ));

    for index in 0..6 {
        let epoch = index + 1;
        inject_control(
            &inject,
            Control::Request {
                file: index,
                offset: 0,
                epoch,
            },
        )
        .await;
        inject_control(
            &inject,
            Control::Credit {
                epoch,
                bytes: INITIAL_WINDOW,
            },
        )
        .await;
        loop {
            match decode(&next_frame(&mut out).await).expect("decodes") {
                Frame::Chunk { .. } => {}
                Frame::Control(Control::Done { .. }) => break,
                other => panic!("unexpected frame before Done: {other:?}"),
            }
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
        watch.borrow_and_update();
        inject_control(
            &inject,
            Control::Credit {
                epoch,
                bytes: contents.len() as u64,
            },
        )
        .await;
        timeout(Duration::from_secs(2), watch.changed())
            .await
            .expect("acknowledged file is published")
            .expect("sender still running");
        assert_eq!(
            watch.borrow().bytes_done,
            (index as u64 + 1) * contents.len() as u64
        );
    }
    assert!(
        watch.borrow().bytes_per_sec > 0.0,
        "resetting the meter for each short file hid the session throughput"
    );

    acknowledge_verified(&inject, &mut out, 6, total).await;
    drop(inject);
    session.await.expect("sender task").expect("verified");
    let completed = watch.borrow();
    assert_eq!(completed.file_done, contents.len() as u64);
    assert_eq!(completed.bytes_done, total);
}

#[tokio::test]
async fn local_test_sender_rejects_unverified_or_wrong_size_receipt() {
    let cap = test_cap(0x32);
    let Scripted {
        io,
        mut out,
        inject,
        ..
    } = scripted();
    let files = Arc::new(vec![shared_file("x.bin", b"contents")]);
    let (progress, watch) = watch::channel(TransferProgress::connecting());
    let session = tokio::spawn(run_sender_on(
        io,
        files,
        progress,
        CancellationToken::new(),
        sender_opts(cap),
    ));
    inject_control(&inject, hello(cap, false)).await;
    assert!(matches!(
        next_control(&mut out).await,
        Control::Hello { .. }
    ));
    assert!(matches!(
        next_control(&mut out).await,
        Control::Manifest { .. }
    ));
    inject_control(
        &inject,
        Control::Verified {
            files: 1,
            bytes: 99,
        },
    )
    .await;
    assert!(matches!(
        session.await.expect("sender task"),
        Err(TransferError::Protocol(ProtocolError::InvalidReceipt))
    ));
    assert_eq!(watch.borrow().phase, Phase::Failed);
}

#[tokio::test]
async fn local_test_engine_roundtrip_in_memory() {
    let cap = test_cap(0x11);
    let big = filler(3 * 1024 * 1024);
    let files = Arc::new(vec![
        shared_file("big.bin", &big),
        shared_file("empty.bin", &[]),
    ]);
    let manifest: Vec<FileMeta> = files.iter().map(|f| f.meta.clone()).collect();

    let (sender_io, receiver_io, sender_close, _receiver_close) = duplex(NoWebRtc, NoWebRtc);
    let (sender_progress, _sender_watch) = watch::channel(TransferProgress::connecting());
    let (receiver_progress, receiver_watch) = watch::channel(TransferProgress::connecting());
    let (commands, command_rx) = mpsc::channel(1);
    commands
        .send(ReceiveCommand::Save(sinks_for(&manifest)))
        .await
        .expect("save accepted");

    let cancel = CancellationToken::new();
    let (sent, received) = tokio::join!(
        run_sender_on(
            sender_io,
            files.clone(),
            sender_progress,
            cancel.clone(),
            sender_opts(cap)
        ),
        run_receiver_on(
            receiver_io,
            command_rx,
            receiver_progress,
            cancel.clone(),
            receive_opts(cap)
        ),
    );

    sent.expect("the sender session ended cleanly");
    let saved = received.expect("the receiver session completed");
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0].name, "big.bin");
    assert_eq!(saved[0].size, big.len() as u64);
    // The empty file is a real manifest entry and must round-trip like any other.
    assert_eq!(saved[1].name, "empty.bin");
    assert_eq!(saved[1].size, 0);

    let progress = receiver_watch.borrow().clone();
    match &progress.phase {
        Phase::Complete { saved } => assert_eq!(saved.len(), 2),
        other => panic!("expected Complete, got {other:?}"),
    }
    assert_eq!(progress.bytes_done, big.len() as u64);
    assert_eq!(progress.path, Path::Relayed);
    assert!(sender_close.lock().expect("close record").is_some());
}

#[tokio::test]
async fn local_test_receiver_reconnects_and_resumes_from_committed_offset() {
    let cap = test_cap(0x2a);
    let data = filler(512 * 1024);
    let data_len = data.len();
    let meta = shared_file("resume.bin", &data).meta;
    let Scripted {
        io: first_io,
        out: first_out,
        inject: first_inject,
        ..
    } = scripted();
    let Scripted {
        io: second_io,
        out: second_out,
        inject: second_inject,
        ..
    } = scripted();
    let sessions = Arc::new(Mutex::new(VecDeque::from([first_io, second_io])));
    let connect_sessions = sessions.clone();
    let connect = move || {
        std::future::ready(
            connect_sessions
                .lock()
                .expect("session queue")
                .pop_front()
                .map(|io| (io, ()))
                .ok_or(TransferError::Transport(
                    crate::transfer::TransportError::Closed,
                )),
        )
    };
    let (commands, command_rx) = mpsc::channel(1);
    commands
        .send(ReceiveCommand::Save(sinks_for(std::slice::from_ref(&meta))))
        .await
        .expect("save accepted");
    let (progress, watch) = watch::channel(TransferProgress::connecting());
    let cancel = CancellationToken::new();
    let mut opts = receive_opts(cap);
    opts.reconnect_backoff = Duration::from_millis(1);
    opts.max_reconnect_backoff = Duration::from_millis(2);
    opts.max_reconnect_attempts = 2;

    let receiver = tokio::spawn(run_receiver_reconnecting(
        connect, command_rx, progress, cancel, opts,
    ));
    let driver = tokio::spawn(async move {
        let mut out = first_out;
        let inject = first_inject;
        assert!(matches!(
            next_control(&mut out).await,
            Control::Hello { .. }
        ));
        inject_control(
            &inject,
            Control::Hello {
                version: PROTOCOL_VERSION,
                webrtc: false,
                cap: [0; CAP_LEN],
            },
        )
        .await;
        inject_control(
            &inject,
            Control::Manifest {
                files: vec![meta.clone()],
            },
        )
        .await;
        let request = next_control(&mut out).await;
        let epoch = match request {
            Control::Request {
                file: 0,
                offset: 0,
                epoch,
            } => epoch,
            other => panic!("expected initial request, got {other:?}"),
        };
        assert!(matches!(
            next_control(&mut out).await,
            Control::Credit { epoch: e, .. } if e == epoch
        ));
        let split = data.len() / 2;
        inject_chunk(&inject, 0, epoch, 0, &data[..split]).await;
        drop(inject);

        let mut out = second_out;
        let inject = second_inject;
        assert!(matches!(
            next_control(&mut out).await,
            Control::Hello { .. }
        ));
        inject_control(
            &inject,
            Control::Hello {
                version: PROTOCOL_VERSION,
                webrtc: false,
                cap: [0; CAP_LEN],
            },
        )
        .await;
        inject_control(
            &inject,
            Control::Manifest {
                files: vec![meta.clone()],
            },
        )
        .await;
        assert!(matches!(next_control(&mut out).await, Control::UseRelay));
        let resumed_epoch = match next_control(&mut out).await {
            Control::Request {
                file: 0,
                offset,
                epoch,
            } => {
                assert_eq!(offset, split as u64);
                epoch
            }
            other => panic!("expected resumed request, got {other:?}"),
        };
        assert!(resumed_epoch > epoch);
        assert!(matches!(
            next_control(&mut out).await,
            Control::Credit { epoch: e, .. } if e == resumed_epoch
        ));
        inject_chunk(&inject, 0, resumed_epoch, split as u64, &data[split..]).await;
        inject_control(
            &inject,
            Control::Done {
                file: 0,
                epoch: resumed_epoch,
            },
        )
        .await;
        next_verified(&mut out, 1, data.len() as u64).await;
        inject_control(&inject, Control::VerifiedAck).await;
    });

    driver.await.expect("driver");
    let saved = receiver
        .await
        .expect("receiver task")
        .expect("receiver resumed");
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].size, data_len as u64);
    let final_progress = watch.borrow().clone();
    assert!(matches!(final_progress.phase, Phase::Complete { .. }));
    assert_eq!(final_progress.bytes_done, data_len as u64);
}

#[tokio::test]
async fn local_test_durable_resume_rehashes_prefix_and_requests_only_the_tail() {
    let cap = test_cap(0x2d);
    let data = filler(128 * 1024);
    let split = data.len() / 3;
    let meta = shared_file("durable.bin", &data).meta;
    let mut sink = MemSink::new(meta.name.clone(), data.len());
    sink.write(&data[..split]).await.expect("seed prefix");
    let mut hasher = blake3::Hasher::new();
    hasher.update(&data[..split]);
    let Scripted {
        io,
        mut out,
        inject,
        ..
    } = scripted();
    let (commands, command_rx) = mpsc::channel(1);
    commands
        .send(ReceiveCommand::Resume(vec![ResumeFile {
            meta: meta.clone(),
            sink: AnySink::Mem(sink),
            hasher,
        }]))
        .await
        .expect("resume accepted");
    let (progress, watch) = watch::channel(TransferProgress::connecting());
    let receiver = tokio::spawn(run_receiver_on(
        io,
        command_rx,
        progress,
        CancellationToken::new(),
        receive_opts(cap),
    ));
    assert!(matches!(
        next_control(&mut out).await,
        Control::Hello { .. }
    ));
    inject_control(&inject, hello([0; CAP_LEN], false)).await;
    inject_control(&inject, Control::Manifest { files: vec![meta] }).await;
    let epoch = match next_control(&mut out).await {
        Control::Request {
            file: 0,
            offset,
            epoch,
        } => {
            assert_eq!(offset, split as u64);
            epoch
        }
        other => panic!("expected tail request, got {other:?}"),
    };
    let _ = next_control(&mut out).await;
    inject_chunk(&inject, 0, epoch, split as u64, &data[split..]).await;
    inject_control(&inject, Control::Done { file: 0, epoch }).await;
    next_verified(&mut out, 1, data.len() as u64).await;
    inject_control(&inject, Control::VerifiedAck).await;
    let saved = receiver.await.expect("receiver task").expect("resumed");
    assert_eq!(saved[0].size, data.len() as u64);
    assert_eq!(watch.borrow().bytes_done, data.len() as u64);
}

#[tokio::test]
async fn local_test_reconnect_rejects_a_changed_manifest() {
    let cap = test_cap(0x2b);
    let original = shared_file("same.bin", b"original").meta;
    let changed = shared_file("same.bin", b"changed!").meta;
    let Scripted {
        io: first_io,
        out: first_out,
        inject: first_inject,
        ..
    } = scripted();
    let Scripted {
        io: second_io,
        out: second_out,
        inject: second_inject,
        ..
    } = scripted();
    let sessions = Arc::new(Mutex::new(VecDeque::from([first_io, second_io])));
    let connect_sessions = sessions.clone();
    let connect = move || {
        std::future::ready(
            connect_sessions
                .lock()
                .expect("session queue")
                .pop_front()
                .map(|io| (io, ()))
                .ok_or(TransferError::Transport(
                    crate::transfer::TransportError::Closed,
                )),
        )
    };
    let (_commands, command_rx) = mpsc::channel(1);
    let (progress, _watch) = watch::channel(TransferProgress::connecting());
    let mut opts = receive_opts(cap);
    opts.reconnect_backoff = Duration::from_millis(1);
    let receiver = tokio::spawn(run_receiver_reconnecting(
        connect,
        command_rx,
        progress,
        CancellationToken::new(),
        opts,
    ));
    let driver = tokio::spawn(async move {
        let mut out = first_out;
        let inject = first_inject;
        let _ = next_control(&mut out).await;
        inject_control(&inject, hello([0; CAP_LEN], false)).await;
        inject_control(
            &inject,
            Control::Manifest {
                files: vec![original],
            },
        )
        .await;
        drop(inject);

        let mut out = second_out;
        let inject = second_inject;
        let _ = next_control(&mut out).await;
        inject_control(&inject, hello([0; CAP_LEN], false)).await;
        inject_control(
            &inject,
            Control::Manifest {
                files: vec![changed],
            },
        )
        .await;
    });
    driver.await.expect("driver");
    assert!(matches!(
        receiver.await.expect("receiver task"),
        Err(TransferError::ManifestChanged)
    ));
}

#[tokio::test]
async fn local_test_reconnect_attempts_are_bounded() {
    let (_commands, command_rx) = mpsc::channel(1);
    let (progress, watch) = watch::channel(TransferProgress::connecting());
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = attempts.clone();
    let connect = move || {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::future::ready(Err(TransferError::Transport(
            crate::transfer::TransportError::Closed,
        ))
            as Result<(SessionIo<MemTx, MemRx, NoWebRtc>, ()), TransferError>)
    };
    let mut opts = receive_opts(test_cap(0x2c));
    opts.max_reconnect_attempts = 2;
    opts.reconnect_backoff = Duration::from_millis(1);
    opts.max_reconnect_backoff = Duration::from_millis(1);
    let result = run_receiver_reconnecting(
        connect,
        command_rx,
        progress,
        CancellationToken::new(),
        opts,
    )
    .await;
    assert!(matches!(
        result,
        Err(TransferError::Transport(
            crate::transfer::TransportError::Closed
        ))
    ));
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
    assert!(matches!(watch.borrow().phase, Phase::Failed));
}

#[tokio::test]
async fn local_test_cancel_interrupts_reconnect_backoff() {
    let (_commands, command_rx) = mpsc::channel(1);
    let (progress, mut watch) = watch::channel(TransferProgress::connecting());
    let connect = || {
        std::future::ready(Err(TransferError::Transport(
            crate::transfer::TransportError::Closed,
        ))
            as Result<(SessionIo<MemTx, MemRx, NoWebRtc>, ()), TransferError>)
    };
    let cancel = CancellationToken::new();
    let mut opts = receive_opts(test_cap(0x2e));
    opts.max_reconnect_attempts = 4;
    opts.reconnect_backoff = Duration::from_secs(60);
    let session = tokio::spawn(run_receiver_reconnecting(
        connect,
        command_rx,
        progress,
        cancel.clone(),
        opts,
    ));
    timeout(Duration::from_secs(1), async {
        loop {
            if matches!(watch.borrow().phase, Phase::Reconnecting { .. }) {
                break;
            }
            watch.changed().await.expect("progress sender");
        }
    })
    .await
    .expect("entered reconnect backoff");
    cancel.cancel();
    assert!(matches!(
        timeout(Duration::from_secs(1), session)
            .await
            .expect("cancel completed")
            .expect("receiver task"),
        Err(TransferError::Cancelled)
    ));
}

#[tokio::test]
async fn local_test_wrong_cap_rejected_before_manifest() {
    let ours = test_cap(0xa1);
    let theirs = test_cap(0xb2);

    // A mismatch buys exactly one frame: the error. No Hello, no Manifest, no Offer.
    let rejected = run_gate(ours, theirs).await;
    assert!(
        matches!(
            rejected.result,
            Err(TransferError::Unauthorized(AuthFailure::Rejected))
        ),
        "{:?}",
        rejected.result
    );
    assert_eq!(
        rejected.frames.len(),
        1,
        "the sender wrote more than the rejection"
    );
    match decode(&rejected.frames[0]).expect("decodes") {
        Frame::Control(Control::Error { file, epoch, .. }) => {
            assert_eq!(file, None);
            assert_eq!(epoch, None);
        }
        other => panic!("expected a session error, got {other:?}"),
    }
    assert!(
        rejected.closed.lock().expect("close record").is_some(),
        "the connection was left open"
    );

    // The same script with the right capability must serve the manifest — otherwise the test
    // above could pass for the wrong reason. The script then disconnects without saving, so
    // unlike an authenticated receive it must NOT claim a completed transfer.
    let accepted = run_gate(ours, ours).await;
    assert!(
        matches!(
            accepted.result,
            Err(TransferError::Transport(TransportError::Closed))
        ),
        "{:?}",
        accepted.result
    );
    let controls: Vec<Control> = accepted
        .frames
        .iter()
        .map(|frame| match decode(frame).expect("decodes") {
            Frame::Control(control) => control,
            Frame::Chunk { .. } => panic!("a chunk before any request"),
        })
        .collect();
    assert!(matches!(controls[0], Control::Hello { .. }));
    assert!(matches!(controls[1], Control::Manifest { .. }));

    // One flipped bit in any position is still a mismatch.
    for byte in 0..CAP_LEN {
        let mut wrong = ours;
        wrong[byte] ^= 0x01;
        let outcome = run_gate(ours, wrong).await;
        assert!(
            matches!(
                outcome.result,
                Err(TransferError::Unauthorized(AuthFailure::Rejected))
            ),
            "byte {byte} was not compared"
        );
        assert_eq!(outcome.frames.len(), 1, "byte {byte} leaked a frame");
    }
}

#[tokio::test]
async fn local_test_old_protocol_is_rejected_before_any_manifest() {
    let cap = test_cap(0xd0);
    let Scripted {
        io,
        mut out,
        inject,
        ..
    } = scripted();
    let files = Arc::new(vec![shared_file("x.bin", b"contents")]);
    let (progress, _watch) = watch::channel(TransferProgress::connecting());
    let session = tokio::spawn(run_sender_on(
        io,
        files,
        progress,
        CancellationToken::new(),
        sender_opts(cap),
    ));
    inject_control(
        &inject,
        Control::Hello {
            version: PROTOCOL_VERSION - 1,
            webrtc: false,
            cap,
        },
    )
    .await;
    assert!(matches!(
        next_control(&mut out).await,
        Control::Error {
            file: None,
            epoch: None,
            ..
        }
    ));
    assert!(matches!(
        session.await.expect("sender task"),
        Err(TransferError::Version { theirs }) if theirs == PROTOCOL_VERSION - 1
    ));
    assert!(matches!(out.recv().await, Ok(None)));
}

struct GateOutcome {
    result: Result<(), TransferError>,
    frames: Vec<Bytes>,
    closed: CloseRecord,
}

/// Run one sender session against a peer that says `Hello` with `offered` and nothing else.
async fn run_gate(ours: [u8; CAP_LEN], offered: [u8; CAP_LEN]) -> GateOutcome {
    let Scripted {
        io,
        mut out,
        inject,
        close,
    } = scripted();
    let files = Arc::new(vec![shared_file("secret.bin", b"top secret")]);
    let (progress, _watch) = watch::channel(TransferProgress::connecting());

    inject_control(&inject, hello(offered, false)).await;
    // Closing the peer's half ends the session once the handshake has run its course.
    drop(inject);

    let result = run_sender_on(
        io,
        files,
        progress,
        CancellationToken::new(),
        sender_opts(ours),
    )
    .await;
    GateOutcome {
        frames: drain(&mut out).await,
        result,
        closed: close,
    }
}

#[tokio::test]
async fn local_test_transport_selection_one_sided_webrtc() {
    let cap = test_cap(0x33);
    let data = filler(256 * 1024);

    // (1) only the sender can do WebRTC, (2) only the receiver: neither may produce an Offer,
    // and every chunk must ride the control stream.
    for (sender_webrtc, receiver_webrtc) in [(true, false), (false, true)] {
        let (sender_dc, _sender_handle) = MemDcFactory::new(Some(64 * 1024));
        let (receiver_dc, _receiver_handle) = MemDcFactory::new(Some(64 * 1024));
        let files = Arc::new(vec![shared_file("one.bin", &data)]);
        let manifest: Vec<FileMeta> = files.iter().map(|f| f.meta.clone()).collect();
        let (s2r_tx, mut tap_rx) = MemTx::pair();
        let (tap_tx, receiver_rx) = MemTx::pair();
        let (r2s_tx, sender_rx) = MemTx::pair();
        let (sender_close, _sc) = close_seam();
        let (receiver_close, _rc) = close_seam();

        // A tap on the sender's control stream, so the test can see every frame the sender
        // wrote while still delivering them to the receiver.
        let relay = tokio::spawn(async move {
            let mut seen = Vec::new();
            while let Ok(Some(frame)) = tap_rx.recv().await {
                seen.push(frame.clone());
                if tap_tx.send(frame).await.is_err() {
                    break;
                }
            }
            seen
        });

        let (commands, command_rx) = mpsc::channel(1);
        commands
            .send(ReceiveCommand::Save(sinks_for(&manifest)))
            .await
            .expect("save accepted");
        let (sender_progress, _sw) = watch::channel(TransferProgress::connecting());
        let (receiver_progress, receiver_watch) = watch::channel(TransferProgress::connecting());
        let cancel = CancellationToken::new();

        let sender_io = SessionIo {
            ctrl_tx: s2r_tx,
            ctrl_rx: sender_rx,
            close: sender_close,
            dc: MaybeDc::new(sender_dc, sender_webrtc),
        };
        let receiver_io = SessionIo {
            ctrl_tx: r2s_tx,
            ctrl_rx: receiver_rx,
            close: receiver_close,
            dc: MaybeDc::new(receiver_dc, receiver_webrtc),
        };

        let (sent, received) = tokio::join!(
            run_sender_on(
                sender_io,
                files,
                sender_progress,
                cancel.clone(),
                sender_opts(cap)
            ),
            run_receiver_on(
                receiver_io,
                command_rx,
                receiver_progress,
                cancel.clone(),
                receive_opts(cap)
            ),
        );
        sent.expect("sender");
        let saved = received.expect("receiver");
        assert_eq!(saved[0].size, data.len() as u64);

        let seen = relay.await.expect("tap");
        let mut offers = 0;
        let mut chunks = 0;
        for frame in &seen {
            match decode(frame).expect("decodes") {
                Frame::Control(Control::Offer { .. }) => offers += 1,
                Frame::Chunk { .. } => chunks += 1,
                _ => {}
            }
        }
        assert_eq!(
            offers, 0,
            "an Offer was sent although one side advertised no WebRTC"
        );
        assert!(chunks > 0, "no chunk rode the control stream");
        assert_eq!(receiver_watch.borrow().path, Path::Relayed);
    }
}

#[tokio::test]
async fn local_test_transport_selection_both_webrtc() {
    crate::logging::init_logging();
    // Both sides can do WebRTC and the channel opens after signalling, as a browser channel
    // does: the sender must offer, and every chunk must leave the control stream for the data
    // channel.
    let cap = test_cap(0x44);
    let data = filler(256 * 1024);
    let (sender_dc, sender_handle, receiver_dc, receiver_handle) =
        MemDcFactory::pair(Some(64 * 1024));
    let open_channels = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(25)).await;
        sender_handle.set_state(PcState::Connected);
        sender_handle.set_open(true);
        receiver_handle.set_state(PcState::Connected);
        receiver_handle.set_open(true);
    });

    let files = Arc::new(vec![shared_file("direct.bin", &data)]);
    let manifest: Vec<FileMeta> = files.iter().map(|f| f.meta.clone()).collect();
    let (s2r_tx, mut tap_rx) = MemTx::pair();
    let (tap_tx, receiver_rx) = MemTx::pair();
    let (r2s_tx, sender_rx) = MemTx::pair();
    let (sender_close, _sc) = close_seam();
    let (receiver_close, _rc) = close_seam();

    let relay = tokio::spawn(async move {
        let mut seen = Vec::new();
        while let Ok(Some(frame)) = tap_rx.recv().await {
            seen.push(frame.clone());
            if tap_tx.send(frame).await.is_err() {
                break;
            }
        }
        seen
    });

    let (commands, command_rx) = mpsc::channel(1);
    let (sender_progress, _sw) = watch::channel(TransferProgress::connecting());
    let (receiver_progress, receiver_watch) = watch::channel(TransferProgress::connecting());
    let awaiting_save = receiver_watch.clone();
    let save_after_signalling = tokio::spawn(async move {
        // Negotiation is allowed to complete in the background, but must not replace the
        // destination prompt: without sinks, no request can be sent and the UI would deadlock.
        tokio::time::sleep(Duration::from_millis(75)).await;
        let prompt_visible = matches!(awaiting_save.borrow().phase, Phase::AwaitingSave { .. });
        commands
            .send(ReceiveCommand::Save(sinks_for(&manifest)))
            .await
            .expect("save accepted");
        assert!(
            prompt_visible,
            "WebRTC negotiation replaced the destination prompt"
        );
    });
    let cancel = CancellationToken::new();

    let (sent, received) = tokio::join!(
        run_sender_on(
            SessionIo {
                ctrl_tx: s2r_tx,
                ctrl_rx: sender_rx,
                close: sender_close,
                dc: sender_dc,
            },
            files,
            sender_progress,
            cancel.clone(),
            sender_opts(cap)
        ),
        run_receiver_on(
            SessionIo {
                ctrl_tx: r2s_tx,
                ctrl_rx: receiver_rx,
                close: receiver_close,
                dc: receiver_dc,
            },
            command_rx,
            receiver_progress,
            cancel.clone(),
            receive_opts(cap)
        ),
    );
    open_channels.await.expect("channel opener");
    save_after_signalling.await.expect("delayed save");
    sent.expect("sender");
    let saved = received.expect("receiver");
    assert_eq!(saved[0].size, data.len() as u64);

    let seen = relay.await.expect("tap");
    let mut offers = 0;
    for frame in &seen {
        match decode(frame).expect("decodes") {
            Frame::Control(Control::Offer { .. }) => offers += 1,
            Frame::Chunk { .. } => panic!("a chunk rode the control stream with an open channel"),
            _ => {}
        }
    }
    assert_eq!(offers, 1, "the sender did not offer a data channel");
    assert_eq!(receiver_watch.borrow().path, Path::Direct);
}

#[tokio::test]
async fn local_test_engine_resume_from_offset() {
    // A request with an offset yields only the tail, under the epoch it asked with.
    let cap = test_cap(0x55);
    let data = filler(400 * 1024);
    let resume_at = 123 * 1024;
    let Scripted {
        io,
        mut out,
        inject,
        close: _close,
    } = scripted();
    let files = Arc::new(vec![shared_file("tail.bin", &data)]);
    let (progress, _watch) = watch::channel(TransferProgress::connecting());

    let session = tokio::spawn(run_sender_on(
        io,
        files,
        progress,
        CancellationToken::new(),
        sender_opts(cap),
    ));

    inject_control(&inject, hello(cap, false)).await;
    assert!(matches!(
        next_control(&mut out).await,
        Control::Hello { .. }
    ));
    assert!(matches!(
        next_control(&mut out).await,
        Control::Manifest { .. }
    ));

    inject_control(
        &inject,
        Control::Request {
            file: 0,
            offset: resume_at,
            epoch: 7,
        },
    )
    .await;
    inject_control(
        &inject,
        Control::Credit {
            epoch: 7,
            bytes: 1 << 20,
        },
    )
    .await;

    let mut offset = resume_at;
    let mut first = true;
    loop {
        let frame = next_frame(&mut out).await;
        match decode(&frame).expect("decodes") {
            Frame::Chunk { header, payload } => {
                assert_eq!(header.epoch, 7, "a chunk carried the wrong epoch");
                if first {
                    assert_eq!(
                        header.offset, resume_at,
                        "the tail did not start at the offset"
                    );
                    first = false;
                }
                assert_eq!(header.offset, offset);
                assert_eq!(&data[offset as usize..][..payload.len()], payload);
                offset += payload.len() as u64;
            }
            Frame::Control(Control::Done { file, epoch }) => {
                assert_eq!((file, epoch), (0, 7));
                break;
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }
    assert_eq!(offset, data.len() as u64, "the whole tail was not sent");
    acknowledge_verified(&inject, &mut out, 1, data.len() as u64).await;
    drop(inject);
    session.await.expect("joins").expect("sender");
}

#[tokio::test]
async fn local_test_credit_window_never_exceeded() {
    // The sender may never put more in flight than it has been granted for the epoch.
    let cap = test_cap(0x66);
    let data = filler(512 * 1024);
    let grant = 64 * 1024u64;
    let Scripted {
        io,
        mut out,
        inject,
        close: _close,
    } = scripted();
    let files = Arc::new(vec![shared_file("windowed.bin", &data)]);
    let (progress, _watch) = watch::channel(TransferProgress::connecting());
    let session = tokio::spawn(run_sender_on(
        io,
        files,
        progress,
        CancellationToken::new(),
        sender_opts(cap),
    ));

    inject_control(&inject, hello(cap, false)).await;
    assert!(matches!(
        next_control(&mut out).await,
        Control::Hello { .. }
    ));
    assert!(matches!(
        next_control(&mut out).await,
        Control::Manifest { .. }
    ));

    inject_control(
        &inject,
        Control::Request {
            file: 0,
            offset: 0,
            epoch: 1,
        },
    )
    .await;

    let mut received = 0u64;
    let mut granted = 0u64;
    // Grant one window at a time and prove the sender stops at exactly that line.
    while received < data.len() as u64 {
        inject_control(
            &inject,
            Control::Credit {
                epoch: 1,
                bytes: grant,
            },
        )
        .await;
        granted += grant;

        loop {
            match timeout(Duration::from_millis(150), out.recv()).await {
                Err(_) => break, // quiet: the sender is blocked on credit, as it must be
                Ok(Ok(Some(frame))) => match decode(&frame).expect("decodes") {
                    Frame::Chunk { payload, .. } => {
                        received += payload.len() as u64;
                        assert!(
                            received <= granted,
                            "sent {received} bytes against {granted} granted"
                        );
                    }
                    Frame::Control(Control::Done { .. }) => {
                        assert_eq!(received, data.len() as u64);
                        break;
                    }
                    other => panic!("unexpected frame {other:?}"),
                },
                Ok(other) => panic!("transport ended early: {other:?}"),
            }
        }
        assert!(
            received >= granted.min(data.len() as u64),
            "the sender stopped short of its window"
        );
    }
    acknowledge_verified(&inject, &mut out, 1, data.len() as u64).await;
    drop(inject);
    session.await.expect("joins").expect("sender");
}

#[tokio::test]
async fn local_test_receiver_grants_credit_after_consuming() {
    // The receiver's first frame after a Request is the explicit initial grant, and later
    // grants are coalesced to roughly one per grain actually written to the sink.
    let cap = test_cap(0x77);
    let data = filler(3 * 1024 * 1024 + 7);
    let meta = FileMeta {
        name: "granted.bin".to_string(),
        size: data.len() as u64,
        hash: BlobHash::from_bytes(&data),
    };
    let Scripted {
        io,
        mut out,
        inject,
        close: _close,
    } = scripted();
    let (progress, _watch) = watch::channel(TransferProgress::connecting());
    let (commands, command_rx) = mpsc::channel(1);
    let opts = receive_opts(cap);
    let initial_window = opts.initial_window;
    let session = tokio::spawn(run_receiver_on(
        io,
        command_rx,
        progress,
        CancellationToken::new(),
        opts,
    ));

    assert!(matches!(
        next_control(&mut out).await,
        Control::Hello { .. }
    ));
    inject_control(&inject, hello([0u8; CAP_LEN], false)).await;
    inject_control(
        &inject,
        Control::Manifest {
            files: vec![meta.clone()],
        },
    )
    .await;
    commands
        .send(ReceiveCommand::Save(sinks_for(std::slice::from_ref(&meta))))
        .await
        .expect("save accepted");

    match next_control(&mut out).await {
        Control::Request {
            file,
            offset,
            epoch,
        } => assert_eq!((file, offset, epoch), (0, 0, 1)),
        other => panic!("expected a Request, got {other:?}"),
    }
    match next_control(&mut out).await {
        Control::Credit { epoch, bytes } => {
            assert_eq!(epoch, 1);
            assert_eq!(bytes, initial_window, "the initial grant was not explicit");
        }
        other => panic!("expected the initial Credit, got {other:?}"),
    }

    // Feed the file in 64 KiB chunks and count what comes back.
    let chunk = 64 * 1024;
    let mut offset = 0usize;
    while offset < data.len() {
        let end = (offset + chunk).min(data.len());
        let payload = &data[offset..end];
        inject_chunk(&inject, 0, 1, offset as u64, payload).await;
        offset = end;
    }
    inject_control(&inject, Control::Done { file: 0, epoch: 1 }).await;
    drop(inject);

    let saved = session.await.expect("joins").expect("receiver");
    assert_eq!(saved[0].size, data.len() as u64);

    let credits: Vec<u64> = drain(&mut out)
        .await
        .iter()
        .filter_map(|frame| match decode(frame).expect("decodes") {
            Frame::Control(Control::Credit { bytes, .. }) => Some(bytes),
            _ => None,
        })
        .collect();
    let expected = (data.len() as u64).div_ceil(crate::protocol::CREDIT_GRAIN);
    assert!(
        credits.len() as u64 <= expected + 2,
        "credit was granted per chunk, not per grain: {} frames",
        credits.len()
    );
    assert!(credits.len() as u64 >= expected - 1, "too few grants");
    let granted: u64 = credits.iter().sum();
    assert!(
        granted <= data.len() as u64,
        "more was granted ({granted}) than was consumed"
    );
    assert_eq!(
        granted,
        data.len() as u64,
        "the final write was not acknowledged"
    );
}

#[tokio::test]
async fn local_test_slow_receiver_reports_subgrain_writes() {
    let cap = test_cap(0x36);
    let data = filler(128 * 1024);
    let meta = shared_file("slow.bin", &data).meta;
    let Scripted {
        io,
        mut out,
        inject,
        ..
    } = scripted();
    let (progress, _watch) = watch::channel(TransferProgress::connecting());
    let (commands, command_rx) = mpsc::channel(1);
    let receiver = tokio::spawn(run_receiver_on(
        io,
        command_rx,
        progress,
        CancellationToken::new(),
        receive_opts(cap),
    ));
    assert!(matches!(
        next_control(&mut out).await,
        Control::Hello { .. }
    ));
    inject_control(&inject, hello([0; CAP_LEN], false)).await;
    inject_control(
        &inject,
        Control::Manifest {
            files: vec![meta.clone()],
        },
    )
    .await;
    commands
        .send(ReceiveCommand::Save(vec![AnySink::Slow(SlowSink::new(
            meta.name.clone(),
            data.len(),
            Duration::from_millis(1100),
        ))]))
        .await
        .expect("save accepted");
    let epoch = match next_control(&mut out).await {
        Control::Request { epoch, .. } => epoch,
        other => panic!("expected request, got {other:?}"),
    };
    assert!(matches!(
        next_control(&mut out).await,
        Control::Credit { epoch: e, .. } if e == epoch
    ));
    inject_chunk(&inject, 0, epoch, 0, &data[..64 * 1024]).await;
    assert_eq!(
        timeout(Duration::from_secs(3), next_control(&mut out))
            .await
            .expect("slow write is acknowledged"),
        Control::Credit {
            epoch,
            bytes: 64 * 1024
        }
    );
    inject_chunk(&inject, 0, epoch, 64 * 1024, &data[64 * 1024..]).await;
    inject_control(&inject, Control::Done { file: 0, epoch }).await;
    assert_eq!(
        timeout(Duration::from_secs(3), next_control(&mut out))
            .await
            .expect("final slow write is acknowledged"),
        Control::Credit {
            epoch,
            bytes: 64 * 1024
        }
    );
    next_verified(&mut out, 1, data.len() as u64).await;
    inject_control(&inject, Control::VerifiedAck).await;
    receiver.await.expect("receiver task").expect("saved");
}

#[tokio::test]
async fn local_test_engine_hash_mismatch_fails() {
    // A manifest hash that does not match the bytes must fail the transfer, not save a file.
    let cap = test_cap(0x88);
    let data = filler(64 * 1024);
    let mut file = shared_file("corrupt.bin", &data);
    file.meta.hash = BlobHash::from_bytes(b"a different file entirely");
    let files = Arc::new(vec![file]);
    let manifest: Vec<FileMeta> = files.iter().map(|f| f.meta.clone()).collect();

    let (sender_io, receiver_io, _sc, _rc) = duplex(NoWebRtc, NoWebRtc);
    let (sender_progress, sender_watch) = watch::channel(TransferProgress::connecting());
    let (receiver_progress, receiver_watch) = watch::channel(TransferProgress::connecting());
    let (commands, command_rx) = mpsc::channel(1);
    commands
        .send(ReceiveCommand::Save(sinks_for(&manifest)))
        .await
        .expect("save accepted");
    let cancel = CancellationToken::new();

    let (sent, received) = tokio::join!(
        run_sender_on(
            sender_io,
            files,
            sender_progress,
            cancel.clone(),
            sender_opts(cap)
        ),
        run_receiver_on(
            receiver_io,
            command_rx,
            receiver_progress,
            cancel.clone(),
            receive_opts(cap)
        ),
    );
    match received {
        Err(TransferError::HashMismatch { file }) => assert_eq!(file, 0),
        other => panic!("expected a hash mismatch, got {other:?}"),
    }
    assert!(
        sent.is_err(),
        "sender claimed success without a verified receipt"
    );
    assert_eq!(sender_watch.borrow().phase, Phase::Failed);
    assert_eq!(receiver_watch.borrow().phase, Phase::Failed);
}

#[tokio::test]
async fn local_test_receiver_write_deadline_and_cancel() {
    // A sink that never returns must not be able to hold the session open, and while a write
    // is pending the loop must not consume control frames (only cancel and the write deadline
    // stay live during it).
    for cancelling in [true, false] {
        let cap = test_cap(0x99);
        let data = filler(64 * 1024);
        let meta = FileMeta {
            name: "stuck.bin".to_string(),
            size: data.len() as u64,
            hash: BlobHash::from_bytes(&data),
        };
        let Scripted {
            io,
            mut out,
            inject,
            close: _close,
        } = scripted();
        let (progress, _watch) = watch::channel(TransferProgress::connecting());
        let (commands, command_rx) = mpsc::channel(1);
        let mut opts = receive_opts(cap);
        opts.write_deadline = Duration::from_millis(200);
        let cancel = CancellationToken::new();
        let session = tokio::spawn(run_receiver_on(
            io,
            command_rx,
            progress,
            cancel.clone(),
            opts,
        ));

        assert!(matches!(
            next_control(&mut out).await,
            Control::Hello { .. }
        ));
        inject_control(&inject, hello([0u8; CAP_LEN], false)).await;
        inject_control(
            &inject,
            Control::Manifest {
                files: vec![meta.clone()],
            },
        )
        .await;

        let stuck = StuckSink::new(meta.name.clone());
        let aborted = stuck.abort_flag();
        commands
            .send(ReceiveCommand::Save(vec![AnySink::Stuck(stuck)]))
            .await
            .expect("save accepted");
        assert!(matches!(
            next_control(&mut out).await,
            Control::Request { .. }
        ));
        assert!(matches!(
            next_control(&mut out).await,
            Control::Credit { .. }
        ));

        // This write never returns.
        inject_chunk(&inject, 0, 1, 0, &data).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        // A session-fatal error queued during the write: if the loop were still reading
        // control frames it would surface as `Remote`, which is exactly what must not happen.
        inject_control(
            &inject,
            Control::Error {
                file: None,
                epoch: None,
                message: "should not be read during a write".to_string(),
            },
        )
        .await;

        let started = std::time::Instant::now();
        if cancelling {
            cancel.cancel();
        }
        let result = timeout(Duration::from_secs(2), session)
            .await
            .expect("the session hung")
            .expect("joins");
        let elapsed = started.elapsed();
        match (cancelling, result) {
            (true, Err(TransferError::Cancelled)) => {
                assert!(elapsed < Duration::from_millis(150), "{elapsed:?}");
            }
            (false, Err(TransferError::SinkTimeout)) => {
                assert!(elapsed < Duration::from_millis(400), "{elapsed:?}");
            }
            (_, other) => panic!("cancelling={cancelling} gave {other:?}"),
        }
        assert!(
            aborted.load(std::sync::atomic::Ordering::SeqCst),
            "the stuck sink was not aborted"
        );
    }
}

#[tokio::test]
async fn local_test_sender_dcep_deadline_keeps_pumping_ice() {
    // A request that arrives while the channel is still connecting is parked, not awaited:
    // the loop keeps running, applies the candidates that arrive meanwhile, and serves over
    // the relay once the deadline passes.
    let cap = test_cap(0xaa);
    let data = filler(32 * 1024);
    let (factory, handle) = MemDcFactory::new(Some(64 * 1024));
    handle.set_state(PcState::Connecting);
    let (core_tx, mut out) = MemTx::pair();
    let (inject, core_rx) = MemTx::pair();
    let (close, _record) = close_seam();
    let files = Arc::new(vec![shared_file("parked.bin", &data)]);
    let (progress, _watch) = watch::channel(TransferProgress::connecting());
    let session = tokio::spawn(run_sender_on(
        SessionIo {
            ctrl_tx: core_tx,
            ctrl_rx: core_rx,
            close,
            dc: factory,
        },
        files,
        progress,
        CancellationToken::new(),
        sender_opts(cap),
    ));

    inject_control(&inject, hello(cap, true)).await;
    assert!(matches!(
        next_control(&mut out).await,
        Control::Hello { .. }
    ));
    assert!(matches!(
        next_control(&mut out).await,
        Control::Manifest { .. }
    ));
    assert!(matches!(
        next_control(&mut out).await,
        Control::Offer { .. }
    ));

    let requested = std::time::Instant::now();
    inject_control(
        &inject,
        Control::Request {
            file: 0,
            offset: 0,
            epoch: 1,
        },
    )
    .await;
    inject_control(
        &inject,
        Control::Credit {
            epoch: 1,
            bytes: 1 << 20,
        },
    )
    .await;
    // Candidates that arrive while the request is parked are applied, not rejected.
    inject_control(
        &inject,
        Control::Ice {
            candidate: "candidate:1 1 udp 1 127.0.0.1 1 typ host".to_string(),
            sdp_mid: Some("0".to_string()),
            sdp_mline_index: Some(0),
        },
    )
    .await;

    // The first chunk must wait for the parking deadline and then ride the control stream.
    let frame = next_frame(&mut out).await;
    let waited = requested.elapsed();
    assert!(
        waited >= Duration::from_millis(90),
        "the request was not parked: {waited:?}"
    );
    assert!(matches!(
        decode(&frame).expect("decodes"),
        Frame::Chunk { .. }
    ));
    drop(inject);
    assert!(matches!(
        session.await.expect("joins"),
        Err(TransferError::Transport(TransportError::Closed))
    ));
}

#[tokio::test]
async fn local_test_sender_use_relay_starts_a_parked_request_at_once() {
    // `UseRelay` while a request is parked starts it immediately: the deadline is not waited
    // out, because the receiver has already given up on the channel.
    let cap = test_cap(0xab);
    let data = filler(32 * 1024);
    let (factory, handle) = MemDcFactory::new(Some(64 * 1024));
    handle.set_state(PcState::Connecting);
    let (core_tx, mut out) = MemTx::pair();
    let (inject, core_rx) = MemTx::pair();
    let (close, _record) = close_seam();
    let files = Arc::new(vec![shared_file("relayed.bin", &data)]);
    let (progress, _watch) = watch::channel(TransferProgress::connecting());
    let session = tokio::spawn(run_sender_on(
        SessionIo {
            ctrl_tx: core_tx,
            ctrl_rx: core_rx,
            close,
            dc: factory,
        },
        files,
        progress,
        CancellationToken::new(),
        sender_opts(cap),
    ));

    inject_control(&inject, hello(cap, true)).await;
    for _ in 0..3 {
        let _ = next_control(&mut out).await; // Hello, Manifest, Offer
    }
    let requested = std::time::Instant::now();
    inject_control(
        &inject,
        Control::Request {
            file: 0,
            offset: 0,
            epoch: 1,
        },
    )
    .await;
    inject_control(
        &inject,
        Control::Credit {
            epoch: 1,
            bytes: 1 << 20,
        },
    )
    .await;
    inject_control(&inject, Control::UseRelay).await;

    let frame = next_frame(&mut out).await;
    assert!(matches!(
        decode(&frame).expect("decodes"),
        Frame::Chunk { .. }
    ));
    assert!(
        requested.elapsed() < Duration::from_millis(90),
        "the parked request waited for its deadline anyway"
    );
    drop(inject);
    assert!(matches!(
        session.await.expect("joins"),
        Err(TransferError::Transport(TransportError::Closed))
    ));
}

#[tokio::test]
async fn local_test_sender_survives_dc_failure() {
    // A data channel that dies mid-serve must not take the session with it: the receiver's
    // `UseRelay` + `Request` has to find a sender still able to answer over the relay.
    let cap = test_cap(0xac);
    let data = filler(128 * 1024);
    let (factory, handle) = MemDcFactory::new(Some(64 * 1024));
    handle.set_open(true);
    handle.set_state(PcState::Connected);
    // Nobody is reading the channel: every send on it fails.
    drop(handle);

    let (core_tx, mut out) = MemTx::pair();
    let (inject, core_rx) = MemTx::pair();
    let (close, _record) = close_seam();
    let files = Arc::new(vec![shared_file("survivor.bin", &data)]);
    let (progress, _watch) = watch::channel(TransferProgress::connecting());
    let session = tokio::spawn(run_sender_on(
        SessionIo {
            ctrl_tx: core_tx,
            ctrl_rx: core_rx,
            close,
            dc: factory,
        },
        files,
        progress,
        CancellationToken::new(),
        sender_opts(cap),
    ));

    inject_control(&inject, hello(cap, true)).await;
    for _ in 0..3 {
        let _ = next_control(&mut out).await; // Hello, Manifest, Offer
    }
    inject_control(
        &inject,
        Control::Request {
            file: 0,
            offset: 0,
            epoch: 1,
        },
    )
    .await;
    inject_control(
        &inject,
        Control::Credit {
            epoch: 1,
            bytes: 1 << 20,
        },
    )
    .await;

    // The serve fails on the dead channel; the session must still be there for the retry.
    tokio::time::sleep(Duration::from_millis(50)).await;
    inject_control(&inject, Control::UseRelay).await;
    inject_control(
        &inject,
        Control::Request {
            file: 0,
            offset: 0,
            epoch: 2,
        },
    )
    .await;
    inject_control(
        &inject,
        Control::Credit {
            epoch: 2,
            bytes: 1 << 20,
        },
    )
    .await;

    let mut received = 0u64;
    loop {
        let frame = next_frame(&mut out).await;
        match decode(&frame).expect("decodes") {
            Frame::Chunk { header, payload } => {
                assert_eq!(header.epoch, 2, "a chunk carried the dead epoch");
                received += payload.len() as u64;
            }
            Frame::Control(Control::Done { epoch, .. }) => {
                assert_eq!(epoch, 2);
                break;
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }
    assert_eq!(received, data.len() as u64);
    acknowledge_verified(&inject, &mut out, 1, data.len() as u64).await;
    drop(inject);
    session.await.expect("joins").expect("sender");
}

#[tokio::test]
async fn local_test_engine_switch_discards_stale_epoch() {
    // Frames from a serve the receiver has already replaced are discarded, not treated as a
    // protocol error — and a stale `Done` must never advance to the next file.
    let cap = test_cap(0xad);
    let data = filler(128 * 1024);
    let meta = FileMeta {
        name: "epochs.bin".to_string(),
        size: data.len() as u64,
        hash: BlobHash::from_bytes(&data),
    };
    let Scripted {
        io,
        mut out,
        inject,
        close: _close,
    } = scripted();
    let (progress, _watch) = watch::channel(TransferProgress::connecting());
    let (commands, command_rx) = mpsc::channel(1);
    let session = tokio::spawn(run_receiver_on(
        io,
        command_rx,
        progress,
        CancellationToken::new(),
        receive_opts(cap),
    ));

    assert!(matches!(
        next_control(&mut out).await,
        Control::Hello { .. }
    ));
    inject_control(&inject, hello([0u8; CAP_LEN], false)).await;
    inject_control(
        &inject,
        Control::Manifest {
            files: vec![meta.clone()],
        },
    )
    .await;
    commands
        .send(ReceiveCommand::Save(sinks_for(std::slice::from_ref(&meta))))
        .await
        .expect("save accepted");
    assert!(matches!(
        next_control(&mut out).await,
        Control::Request { epoch: 1, .. }
    ));
    assert!(matches!(
        next_control(&mut out).await,
        Control::Credit { .. }
    ));

    let half = data.len() / 2;
    inject_chunk(&inject, 0, 1, 0, &data[..half]).await;
    // Stale frames from an aborted serve: a chunk at an offset we already passed, and a Done
    // for the whole file. Both must be dropped in silence.
    inject_chunk(&inject, 0, 0, 0, &data[..half]).await;
    inject_control(&inject, Control::Done { file: 0, epoch: 0 }).await;
    // The rest of the file under the live epoch still completes.
    inject_chunk(&inject, 0, 1, half as u64, &data[half..]).await;
    inject_control(&inject, Control::Done { file: 0, epoch: 1 }).await;
    drop(inject);

    let saved = session.await.expect("joins").expect("receiver");
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].size, data.len() as u64);
}

#[tokio::test]
async fn local_test_engine_respects_max_frame() {
    // Whatever the transport says it can carry, the *complete encoded frame* must fit.
    let cap = test_cap(0xae);
    let data = filler(200 * 1024);
    let limit = 20 * 1024;
    let (core_tx, mut out) = MemTx::pair();
    let (inject, core_rx) = MemTx::pair();
    let (close, _record) = close_seam();
    let files = Arc::new(vec![shared_file("framed.bin", &data)]);
    let (progress, _watch) = watch::channel(TransferProgress::connecting());
    let session = tokio::spawn(run_sender_on(
        SessionIo {
            ctrl_tx: core_tx.with_max_frame(limit),
            ctrl_rx: core_rx,
            close,
            dc: NoWebRtc,
        },
        files,
        progress,
        CancellationToken::new(),
        sender_opts(cap),
    ));

    inject_control(&inject, hello(cap, false)).await;
    inject_control(
        &inject,
        Control::Request {
            file: 0,
            offset: 0,
            epoch: 1,
        },
    )
    .await;
    inject_control(
        &inject,
        Control::Credit {
            epoch: 1,
            bytes: 1 << 20,
        },
    )
    .await;

    let mut chunks = 0;
    loop {
        let frame = next_frame(&mut out).await;
        // The control stream is length-prefixed, so the prefix counts against the limit too.
        assert!(
            frame.len() + LEN_PREFIX <= limit,
            "a frame of {} bytes exceeded the {limit}-byte limit",
            frame.len() + LEN_PREFIX
        );
        match decode(&frame).expect("decodes") {
            Frame::Chunk { .. } => chunks += 1,
            Frame::Control(Control::Done { .. }) => break,
            Frame::Control(_) => {}
        }
    }
    assert!(chunks >= 10, "the payload was not split into frames");
    acknowledge_verified(&inject, &mut out, 1, data.len() as u64).await;
    drop(inject);
    session.await.expect("joins").expect("sender");
}

#[tokio::test]
async fn local_test_engine_use_relay_does_not_abort_control_serve() {
    // `UseRelay` is a state flag, not a pre-emption: a serve already running on the control
    // stream finishes, because only the receiver's next `Request` may replace it.
    let cap = test_cap(0xaf);
    let data = filler(256 * 1024);
    let Scripted {
        io,
        mut out,
        inject,
        close: _close,
    } = scripted();
    let files = Arc::new(vec![shared_file("uninterrupted.bin", &data)]);
    let (progress, _watch) = watch::channel(TransferProgress::connecting());
    let session = tokio::spawn(run_sender_on(
        io,
        files,
        progress,
        CancellationToken::new(),
        sender_opts(cap),
    ));

    inject_control(&inject, hello(cap, false)).await;
    inject_control(
        &inject,
        Control::Request {
            file: 0,
            offset: 0,
            epoch: 1,
        },
    )
    .await;
    inject_control(
        &inject,
        Control::Credit {
            epoch: 1,
            bytes: 1 << 20,
        },
    )
    .await;
    inject_control(&inject, Control::UseRelay).await;

    let mut received = 0u64;
    loop {
        let frame = next_frame(&mut out).await;
        match decode(&frame).expect("decodes") {
            Frame::Chunk { header, payload } => {
                assert_eq!(header.epoch, 1);
                assert_eq!(header.offset, received);
                received += payload.len() as u64;
            }
            Frame::Control(Control::Done { epoch, .. }) => {
                assert_eq!(epoch, 1);
                break;
            }
            Frame::Control(_) => {}
        }
    }
    assert_eq!(
        received,
        data.len() as u64,
        "UseRelay truncated a serve it must not touch"
    );
    acknowledge_verified(&inject, &mut out, 1, data.len() as u64).await;
    drop(inject);
    session.await.expect("joins").expect("sender");
}

// ---------------------------------------------------------------------------------------
// Online
// ---------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the public n0 relay; run with --ignored --nocapture"]
async fn online_test_node_ticket() {
    let files = Arc::new(Mutex::new(Vec::new()));
    let node = timeout(Duration::from_secs(10), Node::bind(files, RelayChoice::N0))
        .await
        .expect("binding timed out")
        .expect("binding a node must not fail");

    let ticket = timeout(Duration::from_secs(30), node.ticket())
        .await
        .expect("relay did not bring the endpoint online")
        .expect("ticket creation failed");
    let text = ticket.to_string();
    assert!(text.starts_with("endpoint"), "{text}");
    assert!(!text.contains('#') && !text.contains('&'), "{text}");
    assert_eq!(ticket.endpoint_addr().id, node.id());
    node.shutdown().await;
}
