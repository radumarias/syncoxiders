//! What the sessions need from the place they run in.
//!
//! Everything browser- or OS-specific that a session *decides with* lives behind
//! [`Platform`], so the session code itself is the same on every target and a test can
//! substitute a [`TestPlatform`]. Rendering-side concerns (clipboard, pickers, layout) stay
//! in the UI adapters.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The browser's screen wake lock, as `assets/wake-lock.js` reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WakeLock {
    /// Nothing asked for it.
    #[default]
    Off,
    /// A request is in flight.
    Requesting,
    /// The screen is being kept on.
    Active,
    /// The browser or OS took it back (tab hidden, battery saver); a gesture can retry.
    Released,
    /// The request was refused.
    Denied,
    /// No Screen Wake Lock API here (or an insecure context).
    Unsupported,
}

impl WakeLock {
    /// Parse the JS status string; unknown values read as [`WakeLock::Unsupported`].
    pub fn from_js(status: &str) -> Self {
        match status {
            "off" => Self::Off,
            "requesting" => Self::Requesting,
            "active" => Self::Active,
            "released" => Self::Released,
            "denied" => Self::Denied,
            _ => Self::Unsupported,
        }
    }
}

/// Whether the page (or window) is in front of the user.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
}

/// Host services a session calls into. Implementations must be cheap: sessions call
/// [`Platform::set_wake_lock`] on every [`super::Oxfer::tick`].
///
/// Planned additions (phase 3, #71/#72): Web Share out, storage estimate. They join this
/// trait as provided methods so existing implementations keep compiling.
pub trait Platform: Send + Sync + 'static {
    /// The page a share link is built on: no fragment, no sender-only diagnostic query
    /// parameters (see [`share_base_url`]).
    fn base_url(&self) -> String;

    /// The page was opened with `#dev`, so share links should carry it too.
    fn dev_flag(&self) -> bool {
        false
    }

    /// A short local time label for the received-files list.
    fn now_label(&self) -> String;

    /// Keep the screen on while a transfer needs the page awake. Idempotent.
    fn set_wake_lock(&self, _active: bool) {}

    /// Retry the wake lock from inside a user gesture (WebKit grants it only then).
    fn retry_wake_lock(&self) {}

    /// The current wake-lock state, for the "keep this tab open" card.
    fn wake_lock(&self) -> WakeLock {
        WakeLock::Unsupported
    }

    /// Whether the page is visible. Native windows always report visible.
    fn visibility(&self) -> Visibility {
        Visibility::Visible
    }
}

/// The page a share link is built on: `href` without its fragment or the sender-only
/// `dcframe` diagnostic parameter.
pub fn share_base_url(href: &str) -> String {
    let without_fragment = href.split('#').next().unwrap_or(href);
    let Some((path, query)) = without_fragment.split_once('?') else {
        return without_fragment.to_string();
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|part| {
            !url::form_urlencoded::parse(part.as_bytes()).any(|(key, _)| key == "dcframe")
        })
        .collect();
    if kept.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", kept.join("&"))
    }
}

/// The origin every share link is built on outside a browser.
pub const DEFAULT_BASE_URL: &str = "https://oxfer.app/";

/// The native desktop: no wake lock, always visible, links on [`DEFAULT_BASE_URL`].
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Default)]
pub struct NativePlatform;

#[cfg(not(target_arch = "wasm32"))]
impl Platform for NativePlatform {
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }

    fn now_label(&self) -> String {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            % 86_400;
        format!(
            "{:02}:{:02}:{:02} UTC",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        )
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::WebPlatform;

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{share_base_url, Platform, Visibility, WakeLock, DEFAULT_BASE_URL};

    #[wasm_bindgen::prelude::wasm_bindgen(module = "/assets/wake-lock.js")]
    extern "C" {
        #[wasm_bindgen(js_name = setTransferWakeLock)]
        fn set_transfer_wake_lock(active: bool);
        #[wasm_bindgen(js_name = retryTransferWakeLock)]
        fn retry_transfer_wake_lock();
        #[wasm_bindgen(js_name = transferWakeLockStatus)]
        fn transfer_wake_lock_status() -> String;
    }

    /// The browser page: wake lock through `assets/wake-lock.js`, links on this page.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct WebPlatform;

    impl Platform for WebPlatform {
        fn base_url(&self) -> String {
            web_sys::window()
                .and_then(|w| w.location().href().ok())
                .map(|href| share_base_url(&href))
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
        }

        fn dev_flag(&self) -> bool {
            web_sys::window()
                .and_then(|w| w.location().hash().ok())
                .map(|hash| crate::node::Node::parse_fragment(&hash).dev)
                .unwrap_or(false)
        }

        fn now_label(&self) -> String {
            js_sys::Date::new_0()
                .to_locale_time_string("en-GB")
                .as_string()
                .unwrap_or_default()
        }

        fn set_wake_lock(&self, active: bool) {
            set_transfer_wake_lock(active);
        }

        fn retry_wake_lock(&self) {
            retry_transfer_wake_lock();
        }

        fn wake_lock(&self) -> WakeLock {
            WakeLock::from_js(&transfer_wake_lock_status())
        }

        fn visibility(&self) -> Visibility {
            match web_sys::window()
                .and_then(|w| w.document())
                .map(|d| d.visibility_state())
            {
                Some(web_sys::VisibilityState::Hidden) => Visibility::Hidden,
                _ => Visibility::Visible,
            }
        }
    }
}

/// A deterministic platform for tests: records wake-lock requests, fixed clock and URL.
#[derive(Debug, Default)]
pub struct TestPlatform {
    wake_lock: AtomicBool,
    retries: Mutex<u32>,
}

impl TestPlatform {
    /// Whether the last [`Platform::set_wake_lock`] call asked for the lock.
    pub fn wake_lock_wanted(&self) -> bool {
        self.wake_lock.load(Ordering::Acquire)
    }

    /// How many times a gesture retried the lock.
    pub fn retries(&self) -> u32 {
        *self.retries.lock().unwrap()
    }
}

impl Platform for TestPlatform {
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }

    fn now_label(&self) -> String {
        "12:00:00".to_string()
    }

    fn set_wake_lock(&self, active: bool) {
        self.wake_lock.store(active, Ordering::Release);
    }

    fn retry_wake_lock(&self) {
        *self.retries.lock().unwrap() += 1;
    }

    fn wake_lock(&self) -> WakeLock {
        if self.wake_lock_wanted() {
            WakeLock::Active
        } else {
            WakeLock::Off
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_test_share_link_excludes_sender_frame_diagnostics() {
        assert_eq!(
            share_base_url("https://oxfer.pages.dev/?dcframe=64&theme=dark#ticket&cap=private"),
            "https://oxfer.pages.dev/?theme=dark"
        );
        assert_eq!(
            share_base_url("https://oxfer.pages.dev/?dcframe=64#private"),
            "https://oxfer.pages.dev/"
        );
        assert_eq!(
            share_base_url(
                "https://oxfer.pages.dev/?tag=kept&d%63frame=64&other=also-kept#private"
            ),
            "https://oxfer.pages.dev/?tag=kept&other=also-kept"
        );
        assert_eq!(
            share_base_url("https://oxfer.pages.dev/#private"),
            "https://oxfer.pages.dev/"
        );
    }

    #[test]
    fn local_test_wake_lock_status_parses_every_js_value() {
        for (js, status) in [
            ("off", WakeLock::Off),
            ("requesting", WakeLock::Requesting),
            ("active", WakeLock::Active),
            ("released", WakeLock::Released),
            ("denied", WakeLock::Denied),
            ("unsupported", WakeLock::Unsupported),
            ("something new", WakeLock::Unsupported),
        ] {
            assert_eq!(WakeLock::from_js(js), status);
        }
    }
}
