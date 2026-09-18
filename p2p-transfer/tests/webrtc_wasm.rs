#![cfg(target_arch = "wasm32")]

use std::sync::Arc;

use bytes::Bytes;
use n0_future::time::{timeout, Duration};
use p2p_transfer::transfer::{DataChannel, DcFactory, DcRole, FrameRx, FrameTx};
use p2p_transfer::webrtc::{WebRtcFactory, CONNECT_TIMEOUT};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test(async)]
async fn browser_test_real_peer_connection_sends_a_frame() {
    let factory = WebRtcFactory::default();
    let mut offerer = factory.create(DcRole::Offerer).expect("offerer");
    let mut answerer = factory.create(DcRole::Answerer).expect("answerer");

    let offer = offerer.create_offer().await.expect("offer");
    let answer = answerer.accept_offer(&offer).await.expect("answer");
    offerer.accept_answer(&answer).await.expect("accept answer");

    let mut offerer_open = offerer.open_watch();
    let mut answerer_open = answerer.open_watch();
    let exchange = async {
        let mut offerer_gathering = true;
        let mut answerer_gathering = true;
        while !*offerer_open.borrow() || !*answerer_open.borrow() {
            tokio::select! {
                candidate = offerer.next_local_ice(), if offerer_gathering => {
                    match candidate {
                        Some(candidate) => answerer.add_ice(&candidate).await.expect("offer ICE"),
                        None => offerer_gathering = false,
                    }
                }
                candidate = answerer.next_local_ice(), if answerer_gathering => {
                    match candidate {
                        Some(candidate) => offerer.add_ice(&candidate).await.expect("answer ICE"),
                        None => answerer_gathering = false,
                    }
                }
                changed = offerer_open.changed(), if !*offerer_open.borrow() => {
                    changed.expect("offerer open watch");
                }
                changed = answerer_open.changed(), if !*answerer_open.borrow() => {
                    changed.expect("answerer open watch");
                }
            }
        }
    };
    timeout(CONNECT_TIMEOUT, exchange)
        .await
        .expect("data channel did not open");

    let cancel = CancellationToken::new();
    let relay = Arc::new(Notify::new());
    let (offer_tx, _) = offerer
        .split(cancel.clone(), relay.clone(), 1024 * 1024)
        .expect("offer split");
    let (_, mut answer_rx) = answerer
        .split(cancel, relay, 1024 * 1024)
        .expect("answer split");
    let expected = Bytes::from_static(b"direct-web-rtc");
    offer_tx.send(expected.clone()).await.expect("send");
    let actual = timeout(Duration::from_secs(5), answer_rx.recv())
        .await
        .expect("receive timeout")
        .expect("receive")
        .expect("frame");
    assert_eq!(actual, expected);
}
