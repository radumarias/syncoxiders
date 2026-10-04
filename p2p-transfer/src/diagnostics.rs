//! Browser-only, user-initiated reachability report. No tickets, capabilities,
//! filenames, IP addresses or application logs are included in the copied report.

use crate::node::{relay_probe_urls, DiagnosticNode, RelayChoice};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/assets/diagnostics.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = browserDiagnostics)]
    async fn browser_diagnostics(relays: &str) -> Result<JsValue, JsValue>;
}

pub async fn collect() -> String {
    // n0's presets probe the iroh 1.1 production relays; custom relays probe each
    // configured relay, rebuilt without credentials, path or query so a copied report
    // never carries a private relay token. This tests browser WSS access, not peer dial,
    // relay authentication or the sender's network.
    let relay = RelayChoice::from_env();
    let urls = relay_probe_urls(&relay).join(",");
    let checks = match browser_diagnostics(&urls).await {
        Ok(value) => value.as_string().unwrap_or_else(|| "No result".into()),
        Err(_) => "Browser diagnostics failed to start".into(),
    };
    // Unlike an unauthenticated WebSocket handshake, this exercises the same
    // iroh endpoint startup and relay registration as a real share. Never put
    // the generated ticket/key in the report.
    let compare_default = relay == RelayChoice::N0WithoutTrailingDots;
    let endpoint_label = match &relay {
        RelayChoice::Custom(_) => "browser, configured relays",
        RelayChoice::None => "browser, no relay",
        RelayChoice::N0 | RelayChoice::N0WithoutTrailingDots => "browser, DNS dots removed",
    };
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
    // The browser's active configuration is now the no-dot n0 preset. If it
    // fails, compare with the upstream dotted preset to distinguish a new
    // relay problem from the observed WebKit DNS-dot incompatibility.
    let alternate = if compare_default && endpoint != "registered with relay" {
        match DiagnosticNode::bind(RelayChoice::N0).await {
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
        "not run (browser relay succeeded or a custom relay is configured)"
    };
    format!(
        "Oxfer {}\n{checks}\nIroh endpoint ({endpoint_label}): {endpoint}\nIroh endpoint (legacy dotted DNS): {alternate}",
        crate::BUILD_LABEL
    )
}
