//! Browser-only, user-initiated reachability report. No tickets, capabilities,
//! filenames, IP addresses or application logs are included in the copied report.

use crate::node::{DiagnosticNode, RelayChoice};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/assets/diagnostics.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = browserDiagnostics)]
    async fn browser_diagnostics(relays: &str) -> Result<JsValue, JsValue>;
}

pub async fn collect() -> String {
    // The default relay hosts are the iroh 1.1 N0 preset. For a custom relay,
    // probe only that configured endpoint instead. This tests browser WSS access,
    // not peer dial, relay authentication or the sender's network.
    let relay = RelayChoice::from_env();
    let urls = match &relay {
        RelayChoice::Custom(url) => {
            // Reconstruct without credentials, path or query parameters: a
            // copied report must never include a private relay auth token.
            let scheme = if url.scheme() == "http" { "ws" } else { "wss" };
            let host = url.host_str().unwrap_or_default();
            let port = url
                .port()
                .map(|port| format!(":{port}"))
                .unwrap_or_default();
            format!("{scheme}://{host}{port}/relay")
        }
        RelayChoice::N0 | RelayChoice::N0WithoutTrailingDots => [
            "wss://euc1-1.relay.n0.iroh.link/relay",
            "wss://use1-1.relay.n0.iroh.link/relay",
            "wss://usw1-1.relay.n0.iroh.link/relay",
            "wss://aps1-1.relay.n0.iroh.link/relay",
            "wss://euc1-1.relay.n0.iroh.link./relay",
        ]
        .join(","),
        RelayChoice::None => String::new(),
    };
    let checks = match browser_diagnostics(&urls).await {
        Ok(value) => value.as_string().unwrap_or_else(|| "No result".into()),
        Err(_) => "Browser diagnostics failed to start".into(),
    };
    // Unlike an unauthenticated WebSocket handshake, this exercises the same
    // iroh endpoint startup and relay registration as a real share. Never put
    // the generated ticket/key in the report.
    let compare_default = relay == RelayChoice::N0;
    let endpoint = match DiagnosticNode::bind(relay).await {
        Ok(node) => {
            let online = node.ticket().await.is_ok();
            node.shutdown().await;
            if online {
                "registered with relay"
            } else {
                "could not register with relay within 15 seconds"
            }
        }
        Err(_) => "endpoint could not start",
    };
    // An iOS failure behind an otherwise successful WebSocket open can come
    // from the trailing DNS dot in iroh's preset, or from later relay
    // negotiation. Keep the n0 lookup preset identical and change only its
    // relay DNS names to distinguish those cases without changing transfers.
    let alternate = if compare_default && endpoint != "registered with relay" {
        match DiagnosticNode::bind(RelayChoice::N0WithoutTrailingDots).await {
            Ok(node) => {
                let online = node.ticket().await.is_ok();
                node.shutdown().await;
                if online {
                    "registered with relay"
                } else {
                    "could not register with relay within 15 seconds"
                }
            }
            Err(_) => "endpoint could not start",
        }
    } else {
        "not run (default relay succeeded or a custom relay is configured)"
    };
    format!(
        "Oxfer {}\n{checks}\nIroh endpoint: {endpoint}\nIroh endpoint (same preset, DNS dots removed): {alternate}",
        crate::BUILD_LABEL
    )
}
