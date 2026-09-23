#![cfg(target_arch = "wasm32")]

use p2p_transfer::blob_store::BlobHash;
use p2p_transfer::file_io::web::resume;
use p2p_transfer::file_io::Sink;
use p2p_transfer::protocol::FileMeta;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test(async)]
async fn browser_test_opfs_copy_reopens_rehashes_and_finishes() {
    let data: Vec<u8> = (0..256 * 1024).map(|index| (index % 251) as u8).collect();
    let meta = FileMeta {
        name: "resume-test.bin".to_string(),
        size: data.len() as u64,
        hash: BlobHash(*blake3::hash(&data).as_bytes()),
    };
    let id = resume::manifest_id(std::slice::from_ref(&meta)).expect("manifest id");
    // Idempotent cleanup makes a previous killed test harmless.
    let _ = resume::discard(&id).await;

    let mut files = resume::prepare(std::slice::from_ref(&meta))
        .await
        .expect("create durable copy");
    assert_eq!(files[0].sink.bytes_written(), 0);
    files[0]
        .sink
        .write(&data[..96 * 1024])
        .await
        .expect("checkpoint prefix");
    files.pop().unwrap().sink.abort().await;

    let mut files = resume::prepare(std::slice::from_ref(&meta))
        .await
        .expect("reopen durable copy");
    let mut file = files.pop().unwrap();
    assert_eq!(file.sink.bytes_written(), 96 * 1024);
    assert_eq!(
        file.hasher.clone().finalize().as_bytes(),
        blake3::hash(&data[..96 * 1024]).as_bytes(),
    );
    file.sink
        .write(&data[96 * 1024..])
        .await
        .expect("append tail");
    file.hasher.update(&data[96 * 1024..]);
    assert_eq!(file.hasher.finalize().as_bytes(), meta.hash.0.as_slice());
    file.sink.finish().await.expect("mark verified");

    let library = resume::list().await.expect("list retained copies");
    let stored = library
        .iter()
        .find(|transfer| transfer.id == id)
        .expect("copy listed");
    assert_eq!(stored.files[0].written, data.len() as u64);
    assert!(stored.files[0].verified);

    resume::discard(&id).await.expect("delete retained copy");
    assert!(resume::list()
        .await
        .expect("list after delete")
        .iter()
        .all(|transfer| transfer.id != id));
}

#[wasm_bindgen_test(async)]
async fn browser_test_opfs_copy_rejects_concurrent_open_and_active_delete() {
    let data = b"exclusive durable fixture";
    let meta = FileMeta {
        name: "resume-exclusive-test.bin".to_string(),
        size: data.len() as u64,
        hash: BlobHash(*blake3::hash(data).as_bytes()),
    };
    let id = resume::manifest_id(std::slice::from_ref(&meta)).expect("manifest id");
    let _ = resume::discard(&id).await;

    let first = resume::prepare(std::slice::from_ref(&meta))
        .await
        .expect("first open");
    assert!(
        resume::prepare(std::slice::from_ref(&meta)).await.is_err(),
        "a concurrent receiver opened the same durable copy"
    );
    assert!(
        resume::discard(&id).await.is_err(),
        "an active durable copy was deleted"
    );
    for file in first {
        file.sink.abort().await;
    }

    let reopened = resume::prepare(std::slice::from_ref(&meta))
        .await
        .expect("locks released for retry");
    for file in reopened {
        file.sink.abort().await;
    }
    resume::discard(&id).await.expect("delete after release");
}
