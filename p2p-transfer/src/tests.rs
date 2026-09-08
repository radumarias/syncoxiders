// Run with cargo test.
//
// Naming (see CLAUDE.md): `local_test_*` is pure and in-process and must pass in any
// environment; `online_test_*` touches real iroh endpoints and skips when the network or the
// relay is unavailable.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use iroh::{EndpointAddr, SecretKey};
use iroh_tickets::endpoint::EndpointTicket;
use tokio::sync::watch;
use tokio::time::timeout;

use crate::app::P2PTransfer;
use crate::blob_store::BlobHash;
use crate::file_io::{
    hash_source, sanitize_name, AnySink, MemSink, MemSource, Sink, SlowSink, StuckSink,
};
use crate::node::{Node, RelayChoice, SinkPref};
use crate::protocol::{
    cap_eq, cap_from_hex, cap_to_hex, decode, encode_chunk, encode_control, max_payload,
    negotiate_chunk, ChunkHeader, ChunkPlan, Control, FileMeta, Frame, ProtocolError, CAP_LEN,
    DEFAULT_CHUNK, INITIAL_WINDOW, LEN_PREFIX, MAX_FRAME, MIN_USABLE_FRAME,
};
use crate::transfer::{
    ByteBudget, DataChannel, DcFactory, DcRole, FrameRx, FrameTx, MemDcFactory, MemTx, Path,
    PcState,
};

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
            version: 1,
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
        version: 1,
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

    let params = Node::parse_fragment("&&nonsense&x=y&&");
    assert!(!params.dev);
    assert!(params.ticket.is_none());
    assert_eq!(params.error, None);

    // A token that looks like a ticket but is not reports an error rather than vanishing.
    let params = Node::parse_fragment("endpointnotreallyaticket");
    assert!(params.ticket.is_none());
    assert!(params.error.is_some());
}

#[test]
fn local_test_fragment_cap() {
    let cap = test_cap(0xa0);
    let params = Node::parse_fragment(&format!("cap={}", cap_to_hex(&cap)));
    assert_eq!(params.cap, Some(cap));
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
    assert!(dc.split(cancel.clone(), notify.clone()).is_some());
    assert!(dc.split(cancel, notify).is_none());

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
// Online
// ---------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn online_test_node_ticket() {
    let files = Arc::new(Mutex::new(Vec::new()));
    let Ok(bound) = timeout(Duration::from_secs(5), Node::bind(files, RelayChoice::N0)).await
    else {
        return; // offline: nothing to assert
    };
    let node = bound.expect("binding a node must not fail");

    if let Ok(Ok(ticket)) = timeout(Duration::from_secs(10), node.ticket()).await {
        let text = ticket.to_string();
        assert!(text.starts_with("endpoint"), "{text}");
        assert!(!text.contains('#') && !text.contains('&'), "{text}");
        assert_eq!(ticket.endpoint_addr().id, node.id());
    }
    node.shutdown().await;
}
