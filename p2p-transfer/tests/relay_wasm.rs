#![cfg(target_arch = "wasm32")]

use p2p_transfer::node::RelayChoice;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn browser_test_default_relay_names_do_not_include_dns_trailing_dots() {
    // Deployments with an explicit custom relay intentionally retain that
    // choice; the n0 fallback in browser builds must use the normalized map.
    if option_env!("P2P_RELAY_URL").is_none() {
        assert_eq!(RelayChoice::from_env(), RelayChoice::N0WithoutTrailingDots);
    }
}
