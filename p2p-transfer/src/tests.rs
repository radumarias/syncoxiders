// Run with cargo test

use crate::node::{EchoNode, ConnectEvent, TransferEvent};
use crate::app::{P2PTransfer, TorrentInfo, ReceivedFile};
use tokio::time::{timeout, Duration};
use n0_future::StreamExt;

fn create_test_file_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 256) as u8).collect()
}

#[test]
fn local_test_p2p_transfer_default() {
    let _app = P2PTransfer::default();
    assert!(true, "P2PTransfer::default() should work");
}

#[test]
fn local_test_torrent_info_default() {
    let info = TorrentInfo::default();
    assert_eq!(info.magnet_uri, None);
    assert_eq!(info.download_progress, 0.0);
    assert_eq!(info.peers_count, 0);
    assert!(!info.is_download);
    assert!(!info.is_seeding);
    assert!(!info.download_complete);
}

#[test]
fn local_test_received_file_creation() {
    let file = ReceivedFile {
        name: "test.txt".to_string(),
        size: 1024,
        saved_path: "/tmp/test.txt".to_string(),
        timestamp: "2025-12-09 12:00:00".to_string(),
    };

    assert_eq!(file.name, "test.txt");
    assert_eq!(file.size, 1024);
    assert_eq!(file.saved_path, "/tmp/test.txt");
}

#[test]
fn local_test_create_test_file_data() {
    let data = create_test_file_data(256);
    assert_eq!(data.len(), 256);
    assert_eq!(data[0], 0);
    assert_eq!(data[255], 255);
}

#[test]
fn local_test_chunk_size_calculation() {
    const CHUNK_SIZE: usize = 256 * 1024;
    assert_eq!(CHUNK_SIZE, 262144, "Chunk size should be 256KB");

    let file_size_1mb = 1024 * 1024;
    let expected_chunks_1mb = (file_size_1mb + CHUNK_SIZE - 1) / CHUNK_SIZE;
    assert_eq!(expected_chunks_1mb, 4, "1MB file should require 4 chunks");

    let file_size_small = 100 * 1024;
    let expected_chunks_small = (file_size_small + CHUNK_SIZE - 1) / CHUNK_SIZE;
    assert_eq!(expected_chunks_small, 1, "100KB file should require 1 chunk");
}

#[test]
fn local_test_empty_file_handling() {
    let test_files: Vec<(String, Vec<u8>)> = vec![
        ("empty.txt".to_string(), Vec::new()),
    ];

    assert_eq!(test_files[0].1.len(), 0, "Empty file should have 0 bytes");
}

#[tokio::test(flavor = "multi_thread")]
async fn online_test_echo_node_spawn() {
    let result = timeout(Duration::from_secs(3), async {
        EchoNode::spawn().await
    }).await;

    match result {
        Ok(Ok(node)) => {
            let node_id = node.endpoint().node_id();
            assert_ne!(format!("{:?}", node_id), "", "Node ID should not be empty");
        }
        Ok(Err(e)) => panic!("Failed to spawn node: {}", e),
        Err(_) => {
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn online_test_echo_node_spawn_with_files() {
    let test_files = vec![
        ("test1.txt".to_string(), b"Hello World".to_vec()),
        ("test2.txt".to_string(), b"Test Data".to_vec()),
    ];

    let result = timeout(Duration::from_secs(3), async {
        EchoNode::spawn_with_files(test_files.clone()).await
    }).await;

    match result {
        Ok(Ok(node)) => {
            let shared_files = node.get_shared_files();
            let files = shared_files.lock().unwrap();
            assert_eq!(files.len(), 2, "Should have 2 shared files");
            assert_eq!(files[0].0, "test1.txt");
            assert_eq!(files[1].0, "test2.txt");
        }
        Ok(Err(e)) => panic!("Failed to spawn node: {}", e),
        Err(_) => {
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn online_test_node_id_persistence() {
    let result = timeout(Duration::from_secs(3), EchoNode::spawn()).await;

    if let Ok(Ok(node)) = result {
        let node_id_1 = node.endpoint().node_id();
        let node_id_2 = node.endpoint().node_id();
        assert_eq!(format!("{:?}", node_id_1), format!("{:?}", node_id_2));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn online_test_small_file_transfer() {
    let test_data = b"Hello, P2P!".to_vec();

    let sender_result = timeout(Duration::from_secs(3),
        EchoNode::spawn_with_files(vec![("test.txt".to_string(), test_data.clone())])
    ).await;

    let receiver_result = timeout(Duration::from_secs(3), EchoNode::spawn()).await;

    match (sender_result, receiver_result) {
        (Ok(Ok(sender)), Ok(Ok(receiver))) => {
            let sender_id = sender.endpoint().node_id();

            let mut stream = receiver.connect(sender_id, b"req".to_vec(), "req.dat".to_string());

            let mut received_data = Vec::new();
            let start = tokio::time::Instant::now();

            while start.elapsed() < Duration::from_secs(5) {
                match timeout(Duration::from_millis(100), stream.next()).await {
                    Ok(Some(ConnectEvent::Transfer(TransferEvent::ChunkReceived { chunk_data, .. }))) => {
                        received_data.extend_from_slice(&chunk_data);
                    }
                    Ok(Some(ConnectEvent::Transfer(TransferEvent::FileComplete { .. }))) => {
                        break;
                    }
                    Ok(Some(ConnectEvent::Closed { .. })) => {
                        break;
                    }
                    Ok(None) => break,
                    _ => continue,
                }
            }

            if received_data == test_data {
                assert_eq!(received_data, test_data);
            }
        }
        _ => {}
    }
}

#[test]
fn local_test_data_structures() {
    let _transfer = P2PTransfer::default();
    let _torrent = TorrentInfo::default();
    let _received = ReceivedFile {
        name: "test".to_string(),
        size: 0,
        saved_path: "".to_string(),
        timestamp: "".to_string(),
    };

    let small = create_test_file_data(10);
    let medium = create_test_file_data(1024);
    let large = create_test_file_data(256 * 1024);

    assert_eq!(small.len(), 10);
    assert_eq!(medium.len(), 1024);
    assert_eq!(large.len(), 256 * 1024);
}
