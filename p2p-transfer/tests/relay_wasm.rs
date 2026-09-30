#![cfg(target_arch = "wasm32")]

use p2p_transfer::node::RelayChoice;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn browser_test_default_relay_names_do_not_include_dns_trailing_dots() {
    // The browser's n0 fallback must use the normalized map. CI passes an empty
    // P2P_RELAY_URL when the repository variable is unset, which also means n0.
    assert_eq!(
        RelayChoice::from_setting(None),
        RelayChoice::N0WithoutTrailingDots
    );
    assert_eq!(
        RelayChoice::from_setting(Some(" ")),
        RelayChoice::N0WithoutTrailingDots
    );
    // Deployments with an explicit custom relay list intentionally retain that choice.
    if option_env!("P2P_RELAY_URL").is_none_or(|setting| setting.trim().is_empty()) {
        assert_eq!(RelayChoice::from_env(), RelayChoice::N0WithoutTrailingDots);
    }
}
