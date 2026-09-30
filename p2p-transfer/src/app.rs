//! The egui layer: mode, flows and rendering.
//!
//! This module owns no bytes and no protocol. It holds handles (`SharedFiles`, `Node`,
//! `TransferHandle`), turns user gestures into engine commands, and renders whatever the
//! engine publishes on its progress watch.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use eframe::egui;
use egui::{Button, Color32, CornerRadius, RichText, Stroke, Ui};
use n0_future::task::{self, AbortOnDropHandle};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::file_io::{self, FileOrigin, FileSnapshot, SharedFile, SharedFiles};
use crate::logging;
#[cfg(target_arch = "wasm32")]
use crate::node::DiagnosticNode;
use crate::node::{FragmentParams, Node, RelayChoice};
use crate::protocol::FileMeta;
use crate::transfer::{
    Path as TransferPath, Phase, ReceiveCommand, ReceiveOptions, TransferHandle, TransferProgress,
};
#[cfg(target_arch = "wasm32")]
use iroh_tickets::endpoint::EndpointTicket;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(module = "/assets/wake-lock.js")]
extern "C" {
    #[wasm_bindgen(js_name = setTransferWakeLock)]
    fn set_transfer_wake_lock(active: bool);
    #[wasm_bindgen(js_name = retryTransferWakeLock)]
    fn retry_transfer_wake_lock();
    #[wasm_bindgen(js_name = transferWakeLockStatus)]
    fn transfer_wake_lock_status() -> String;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(module = "/assets/theme.js")]
extern "C" {
    #[wasm_bindgen(js_name = setBrowserTheme)]
    fn set_browser_theme(theme: &str, dark: bool);
}

/// Shown next to a link, because the link *is* the credential (design §2.6).
const LINK_WARNING: &str =
    "Anyone with this link can download your files while you're sharing. Send it only to people you trust.";
/// What a receiver is told when the link carries no `cap` (design §2.6, §4.8.2).
const MISSING_CAP: &str = "this link is missing its access code; ask the sender for the full link";
/// Both sentences of design §2.6 for a capability the sender rejected.
const CAP_REJECTED: &str =
    "this link is not valid for these files. Ask the sender for a fresh link — the old one stops \
     working when they restart sharing.";
/// A desktop ticket carries the endpoint's direct addresses as well as its relay, so the
/// sender is told what the link reveals besides the capability (compliance plan B5).
#[cfg(not(target_arch = "wasm32"))]
const NATIVE_LINK_ADDRESSES: &str =
    "This link also contains this device's IP addresses, so the recipient can connect directly.";

// ── Legal and source links ───────────────────────────────────────────────────

/// The public repository this app is built from.
const SOURCE_URL: &str = "https://github.com/radumarias/syncoxiders";
/// From this bottom-bar width up the links are shown inline; below it they share one menu.
const FOOTER_INLINE_LINKS_MIN_WIDTH: f32 = 760.0;

/// A page every screen links to from the bottom bar (compliance plan B6).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FooterLink {
    Privacy,
    Terms,
    Abuse,
    Source,
}

impl FooterLink {
    /// In reading order.
    const ALL: [Self; 4] = [Self::Privacy, Self::Terms, Self::Abuse, Self::Source];

    const fn label(self) -> &'static str {
        match self {
            Self::Privacy => "Privacy",
            Self::Terms => "Terms",
            Self::Abuse => "Abuse & safety",
            Self::Source => "Source",
        }
    }

    /// Where the link goes. The web app opens the legal pages of the origin it was served
    /// from, so a preview deployment shows its own copy; the desktop app has no origin and
    /// opens the public site. No form carries a query or fragment, so no share state leaves
    /// the app.
    const fn url(self, web: bool) -> &'static str {
        match (self, web) {
            (Self::Privacy, true) => "/privacy",
            (Self::Privacy, false) => "https://oxfer.app/privacy",
            (Self::Terms, true) => "/terms",
            (Self::Terms, false) => "https://oxfer.app/terms",
            (Self::Abuse, true) => "/abuse",
            (Self::Abuse, false) => "https://oxfer.app/abuse",
            (Self::Source, _) => SOURCE_URL,
        }
    }

    /// This build's destination.
    const fn target_url(self) -> &'static str {
        self.url(cfg!(target_arch = "wasm32"))
    }

    /// Open the page in a new browser tab, or in the default browser from the desktop app.
    fn open(self, ctx: &egui::Context) {
        ctx.open_url(egui::OpenUrl::new_tab(self.target_url()));
    }
}

// ── Relay disclosure ─────────────────────────────────────────────────────────

/// Who runs the relay this build uses, for the "Technical details" copy (plan C6).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RelayOperator {
    /// `P2P_RELAY_URL` named relays; the deployment operates them.
    Oxfer,
    /// The default: n0.computer's public relays and address lookup.
    N0,
    /// No relay at all (LAN and tests; never produced by `RelayChoice::from_env`).
    Disabled,
}

impl RelayOperator {
    fn of(relay: &RelayChoice) -> Self {
        match relay {
            RelayChoice::Custom(..) => Self::Oxfer,
            RelayChoice::N0 | RelayChoice::N0WithoutTrailingDots => Self::N0,
            RelayChoice::None => Self::Disabled,
        }
    }

    /// This build's operator. `RelayChoice::from_env` parses and may warn, and the copy is
    /// drawn every frame, so it is resolved once.
    fn current() -> Self {
        static CURRENT: OnceLock<RelayOperator> = OnceLock::new();
        *CURRENT.get_or_init(|| Self::of(&RelayChoice::from_env()))
    }

    /// Body of "Technical details" step 03.
    const fn peer_path(self) -> &'static str {
        match self {
            Self::Oxfer => {
                "An authenticated iroh connection carries signaling, then browsers prefer a \
                 direct WebRTC DataChannel protected by DTLS. If direct ICE fails, encrypted \
                 QUIC traffic falls back through a relay operated by Oxfer without exposing \
                 plaintext."
            }
            Self::N0 => {
                "An authenticated iroh connection carries signaling, then browsers prefer a \
                 direct WebRTC DataChannel protected by DTLS. If direct ICE fails, encrypted \
                 QUIC traffic falls back through n0.computer's public iroh relays without \
                 exposing plaintext."
            }
            Self::Disabled => {
                "An authenticated iroh connection carries signaling, then browsers prefer a \
                 direct WebRTC DataChannel protected by DTLS. This build has no relay, so \
                 peers must reach each other directly."
            }
        }
    }

    /// Body of this build's "PRIVACY BOUNDARY" panel.
    const fn privacy_boundary(self) -> &'static str {
        self.privacy_boundary_for(cfg!(target_arch = "wasm32"))
    }

    /// Body of the "PRIVACY BOUNDARY" panel in the browser app (`web`) or the desktop app.
    ///
    /// Only the browser app is served by Cloudflare and sends STUN requests to it (the wasm-only
    /// `webrtc::ICE_SERVERS`). The desktop app does neither: its transfers involve the relay
    /// operator's infrastructure (none for `Disabled`) and the peer, and its links carry the
    /// device's IP addresses (`NATIVE_LINK_ADDRESSES`).
    const fn privacy_boundary_for(self, web: bool) -> &'static str {
        match (self, web) {
            (Self::Oxfer, true) => {
                "Cloudflare serves the web app shell and answers STUN requests. The relay is \
                 operated by Oxfer. These services can observe network addresses and timing, \
                 and the relay also sees approximate traffic volume, but none of them sees file \
                 contents or the capability fragment. Peers that connect directly learn each \
                 other's network address."
            }
            (Self::N0, true) => {
                "Cloudflare serves the web app shell and answers STUN requests. n0.computer \
                 runs the public relays and address lookup this build uses. These services can \
                 observe network addresses and timing, and a relay also sees approximate \
                 traffic volume, but none of them sees file contents or the capability \
                 fragment. Peers that connect directly learn each other's network address."
            }
            (Self::Disabled, true) => {
                "Cloudflare serves the web app shell and answers STUN requests, so it can \
                 observe network addresses and timing, but not file contents or the \
                 capability fragment. This build uses no relay. Peers that connect directly \
                 learn each other's network address."
            }
            (Self::Oxfer, false) => {
                "The relay is operated by Oxfer. It can observe network addresses, timing and \
                 approximate traffic volume, but not file contents or the capability fragment. \
                 Share links from this desktop app also contain this device's IP addresses, so \
                 anyone holding a link can see them. Peers that connect directly learn each \
                 other's network address."
            }
            (Self::N0, false) => {
                "n0.computer runs the public relays and address lookup this build uses. They \
                 can observe network addresses and timing, and a relay also sees approximate \
                 traffic volume, but not file contents or the capability fragment. Share links \
                 from this desktop app also contain this device's IP addresses, so anyone \
                 holding a link can see them. Peers that connect directly learn each other's \
                 network address."
            }
            (Self::Disabled, false) => {
                "This build uses no relay: peers connect directly and learn each other's \
                 network address. The network between them carries only encrypted traffic and \
                 sees neither file contents nor the capability fragment. Share links from this \
                 desktop app contain this device's IP addresses, so anyone holding a link can \
                 see them."
            }
        }
    }
}

// ── File-messenger theme — shared colors for desktop and browser ────────────

/// A display preference, independent of the light/dark setting and transfer state.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
enum Theme {
    Rusty,
    #[default]
    Clean,
}

// ── Browser storage ──────────────────────────────────────────────────────────
//
// ePrivacy Directive art. 5(3): the browser build stores only what the user asked for. Its
// one localStorage key is `BROWSER_THEME_KEY`, written when the user picks a theme (or once,
// to carry over a theme they picked in an older build). eframe's own browser persistence is
// off: `App::save` writes nothing and `persist_egui_memory` is false there, so the eframe
// `app` key and `egui_memory_ron` are never written, and start-up removes the copies older
// builds left. Native builds keep eframe's normal on-disk settings (theme and save folder).

/// The browser's only localStorage key: the theme the user picked, as `rusty` or `clean`.
#[cfg(target_arch = "wasm32")]
const BROWSER_THEME_KEY: &str = "oxfer.theme.v1";

/// localStorage keys older browser builds wrote through eframe: its `App::save` output
/// (`eframe::APP_KEY`, holding the theme) and egui's memory. Removed at start-up.
#[cfg(target_arch = "wasm32")]
const LEGACY_BROWSER_KEYS: [&str; 2] = [eframe::APP_KEY, "egui_memory_ron"];

/// The part of an older browser build's eframe `App::save` output this build still reads.
#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Default, Deserialize)]
#[serde(default)]
struct LegacyBrowserSettings {
    theme: Theme,
}

/// The theme an older browser build saved under `eframe::APP_KEY`, if that value exists and
/// parses. Other fields are ignored, and a value without `theme` means the default.
#[cfg(any(test, target_arch = "wasm32"))]
fn legacy_browser_theme(storage: &dyn eframe::Storage) -> Option<Theme> {
    eframe::get_value::<LegacyBrowserSettings>(storage, eframe::APP_KEY)
        .map(|settings| settings.theme)
}

/// What start-up does to `BROWSER_THEME_KEY`.
#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ThemeKeyChange {
    Leave,
    Write(Theme),
    Remove,
}

/// Decide the browser's start-up theme and what happens to its key.
///
/// - `stored`: the raw `BROWSER_THEME_KEY` value.
/// - `legacy_theme`: the theme in an older build's eframe `app` value (see
///   [`legacy_browser_theme`]).
/// - `legacy_present`: whether any of `LEGACY_BROWSER_KEYS` exists.
///
/// Older builds saved their settings periodically whether or not the user chose anything,
/// and the build before this one also wrote the theme key on every start. So a stored
/// default theme cannot be told apart from no choice, while a non-default one was picked.
/// Only a picked theme is carried over, and a default-valued theme key found next to legacy
/// data is removed. Either way the user sees the same theme as before.
#[cfg(any(test, target_arch = "wasm32"))]
fn browser_theme_start(
    stored: Option<&str>,
    legacy_theme: Option<Theme>,
    legacy_present: bool,
) -> (Theme, ThemeKeyChange) {
    match stored.and_then(Theme::from_storage_value) {
        Some(theme) if legacy_present && theme == Theme::default() => {
            (theme, ThemeKeyChange::Remove)
        }
        Some(theme) => (theme, ThemeKeyChange::Leave),
        None => match legacy_theme {
            Some(theme) if theme != Theme::default() => (theme, ThemeKeyChange::Write(theme)),
            _ => (Theme::default(), ThemeKeyChange::Leave),
        },
    }
}

/// Start-up theme for the browser build: apply [`browser_theme_start`] to localStorage, then
/// remove the legacy eframe keys. Missing or refused storage (private browsing, blocked site
/// data) reads as empty, so nothing is written or removed and the default theme applies.
/// Failed writes and removals are logged by eframe, never raised.
#[cfg(target_arch = "wasm32")]
fn restore_browser_theme(storage: Option<&dyn eframe::Storage>) -> Theme {
    use eframe::web::storage::{local_storage_get, local_storage_remove, local_storage_set};

    let legacy: Vec<&str> = LEGACY_BROWSER_KEYS
        .into_iter()
        .filter(|key| local_storage_get(key).is_some())
        .collect();
    let stored = local_storage_get(BROWSER_THEME_KEY);
    let (theme, change) = browser_theme_start(
        stored.as_deref(),
        storage.and_then(legacy_browser_theme),
        !legacy.is_empty(),
    );
    match change {
        ThemeKeyChange::Leave => {}
        ThemeKeyChange::Write(theme) => {
            local_storage_set(BROWSER_THEME_KEY, theme.storage_value());
        }
        ThemeKeyChange::Remove => local_storage_remove(BROWSER_THEME_KEY),
    }
    for key in legacy {
        local_storage_remove(key);
    }
    theme
}

// Keep header and footer controls clear of the shared content column's edges.
const CONTROL_EDGE_INSET: f32 = 16.0;

impl Theme {
    #[cfg(any(test, target_arch = "wasm32"))]
    const fn storage_value(self) -> &'static str {
        match self {
            Self::Rusty => "rusty",
            Self::Clean => "clean",
        }
    }

    #[cfg(any(test, target_arch = "wasm32"))]
    fn from_storage_value(value: &str) -> Option<Self> {
        match value {
            "rusty" => Some(Self::Rusty),
            "clean" => Some(Self::Clean),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct Tc {
    theme: Theme,
    bg: Color32,
    surface_lowest: Color32,
    surface_low: Color32,
    surface: Color32,
    surface_high: Color32,
    primary: Color32,
    on_primary: Color32,
    secondary: Color32,
    on_surface: Color32,
    on_surface_var: Color32,
    outline: Color32,
    outline_var: Color32,
    error: Color32,
}

impl Tc {
    const fn clean_dark() -> Self {
        Self {
            theme: Theme::Clean,
            bg: Color32::from_rgb(21, 23, 36), // #151724
            surface_lowest: Color32::from_rgb(14, 16, 24), // #0e1018
            surface_low: Color32::from_rgb(28, 31, 46), // #1c1f2e
            surface: Color32::from_rgb(35, 38, 58), // #23263a
            surface_high: Color32::from_rgb(46, 50, 80), // #2e3250
            primary: Color32::from_rgb(26, 136, 254), // #1a88fe
            on_primary: Color32::from_rgb(6, 16, 28), // #06101c
            secondary: Color32::from_rgb(4, 242, 162), // #04f2a2
            on_surface: Color32::from_rgb(245, 246, 248), // #f5f6f8
            on_surface_var: Color32::from_rgb(177, 178, 184), // #b1b2b8
            outline: Color32::from_rgb(154, 160, 176), // #9aa0b0
            outline_var: Color32::from_rgb(58, 62, 82), // #3a3e52
            error: Color32::from_rgb(255, 139, 154), // #ff8b9a
        }
    }

    const fn clean_light() -> Self {
        Self {
            theme: Theme::Clean,
            bg: Color32::from_rgb(228, 228, 228), // #e4e4e4
            surface_lowest: Color32::from_rgb(254, 254, 254), // #fefefe
            surface_low: Color32::from_rgb(255, 255, 255), // #ffffff
            surface: Color32::from_rgb(244, 245, 247), // #f4f5f7
            surface_high: Color32::from_rgb(220, 232, 252), // #dce8fc
            primary: Color32::from_rgb(22, 116, 216), // #1674d8
            on_primary: Color32::from_rgb(255, 255, 255), // #ffffff
            secondary: Color32::from_rgb(0, 121, 81), // #007951
            on_surface: Color32::from_rgb(21, 23, 36), // #151724
            on_surface_var: Color32::from_rgb(92, 96, 112), // #5c6070
            outline: Color32::from_rgb(78, 82, 96), // #4e5260
            outline_var: Color32::from_rgb(197, 198, 203), // #c5c6cb
            error: Color32::from_rgb(196, 55, 74), // #c4374a
        }
    }

    const fn rusty_dark() -> Self {
        Self {
            theme: Theme::Rusty,
            bg: Color32::from_rgb(16, 13, 12),
            surface_lowest: Color32::from_rgb(10, 8, 7),
            surface_low: Color32::from_rgb(28, 22, 19),
            surface: Color32::from_rgb(35, 27, 23),
            surface_high: Color32::from_rgb(54, 39, 31),
            primary: Color32::from_rgb(239, 112, 56),
            on_primary: Color32::from_rgb(34, 15, 7),
            secondary: Color32::from_rgb(68, 218, 181),
            on_surface: Color32::from_rgb(247, 235, 226),
            on_surface_var: Color32::from_rgb(213, 190, 176),
            outline: Color32::from_rgb(168, 137, 120),
            outline_var: Color32::from_rgb(81, 57, 46),
            error: Color32::from_rgb(255, 181, 164),
        }
    }

    const fn rusty_light() -> Self {
        Self {
            theme: Theme::Rusty,
            bg: Color32::from_rgb(250, 246, 242),
            surface_lowest: Color32::from_rgb(255, 253, 251),
            surface_low: Color32::from_rgb(244, 235, 228),
            surface: Color32::from_rgb(237, 222, 212),
            surface_high: Color32::from_rgb(226, 202, 187),
            primary: Color32::from_rgb(176, 66, 22),
            on_primary: Color32::from_rgb(255, 249, 245),
            secondary: Color32::from_rgb(0, 113, 88),
            on_surface: Color32::from_rgb(48, 28, 20),
            on_surface_var: Color32::from_rgb(94, 67, 55),
            outline: Color32::from_rgb(132, 99, 82),
            outline_var: Color32::from_rgb(211, 187, 173),
            error: Color32::from_rgb(177, 46, 30),
        }
    }

    fn of(theme: Theme, dark: bool) -> Self {
        match (theme, dark) {
            (Theme::Rusty, true) => Self::rusty_dark(),
            (Theme::Rusty, false) => Self::rusty_light(),
            (Theme::Clean, true) => Self::clean_dark(),
            (Theme::Clean, false) => Self::clean_light(),
        }
    }
}

// ── App state ────────────────────────────────────────────────────────────────

/// One file this node received and saved, as the UI lists it.
#[derive(Clone, Debug)]
pub struct ReceivedFile {
    pub name: String,
    pub size: u64,
    pub location: String,
    pub when: String,
    pub path: TransferPath,
    /// Newly received bytes per second in this session; absent for an empty/already saved file.
    pub average_bytes_per_sec: Option<f64>,
}

/// A picked file waiting for `logic()` to turn it into a [`PrepareHandle`].
///
/// The wasm file picker completes in a JS callback that cannot hold `&mut self`, so both
/// targets hand picks over through this queue and one code path drains it.
struct PendingPick {
    name: String,
    origin: FileOrigin,
    snapshot: FileSnapshot,
}

/// Hashing one picked file. The link is withheld until every one of these is gone
/// (design §4.8.2, plan amendment 14), so a receiver never sees a non-final hash.
struct PrepareHandle {
    name: String,
    progress: watch::Receiver<f32>,
    /// Abort-on-drop is right here: hashing owns nothing that needs cleanup.
    _task: AbortOnDropHandle<()>,
}

impl PrepareHandle {
    /// The hashing task drops its `watch::Sender` when it ends, whatever the outcome.
    fn finished(&self) -> bool {
        self.progress.has_changed().is_err()
    }
}

type ReceiveStartup = Arc<Mutex<Option<Result<TransferHandle, String>>>>;

/// Everything one receive needs.
///
/// Boxed inside [`Mode`] because a parsed `FragmentParams` (which carries an
/// `EndpointTicket`) makes this variant ~230 bytes against ~24 for the next largest, and
/// every `Mode` value — including `Home` — would otherwise be that wide.
struct ReceiveState {
    input: String,
    params: FragmentParams,
    handle: Option<TransferHandle>,
    error: Option<String>,
    opening: bool,
    startup: ReceiveStartup,
    save_pending: Arc<AtomicBool>,
}

/// One destination selection per receive session. Errors and dismissal allow a retry;
/// a submitted Save remains pending until the engine leaves AwaitingSave.
struct SaveAttempt {
    pending: Arc<AtomicBool>,
    submitted: bool,
}

impl SaveAttempt {
    fn begin(pending: &Arc<AtomicBool>) -> Option<Self> {
        if pending.swap(true, Ordering::AcqRel) {
            return None;
        }
        Some(Self {
            pending: pending.clone(),
            submitted: false,
        })
    }

    fn submitted(mut self) {
        self.submitted = true;
    }
}

impl Drop for SaveAttempt {
    fn drop(&mut self) {
        if !self.submitted {
            self.pending.store(false, Ordering::Release);
        }
    }
}

/// What the app is doing. One value replaces the six booleans the pre-migration app used.
#[derive(Default)]
enum Mode {
    #[default]
    Home,
    #[cfg(target_arch = "wasm32")]
    Diagnostics,
    Send {
        preparing: Vec<PrepareHandle>,
    },
    Receive(Box<ReceiveState>),
}

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
struct DiagnosticPeer {
    node: Option<Arc<DiagnosticNode>>,
    target: Option<EndpointTicket>,
    link: Option<String>,
    session: Option<String>,
    outcome: Option<String>,
    busy: bool,
    generation: u64,
}

/// Browser-only library of opt-in local copies. Neither this state nor any share link is
/// serialized by eframe; the storage module owns versioned file checkpoints.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Default)]
struct LocalCopies {
    entries: Arc<Mutex<Vec<file_io::web::resume::StoredTransfer>>>,
    error: Arc<Mutex<Option<String>>>,
    busy: Arc<AtomicBool>,
}

#[cfg(target_arch = "wasm32")]
enum LocalCopyAction {
    Refresh,
    Discard(String),
    Export(String, usize),
}

#[cfg(target_arch = "wasm32")]
impl LocalCopies {
    fn run(&self, action: LocalCopyAction, repaint: egui::Context) {
        if self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let library = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = match action {
                LocalCopyAction::Refresh => Ok(()),
                LocalCopyAction::Discard(id) => file_io::web::resume::discard(&id).await,
                LocalCopyAction::Export(id, index) => {
                    file_io::web::resume::export(&id, index).await
                }
            };
            let result = match result {
                Ok(()) => file_io::web::resume::list().await,
                Err(error) => Err(error),
            };
            match result {
                Ok(entries) => {
                    *library.entries.lock().unwrap() = entries;
                    *library.error.lock().unwrap() = None;
                }
                Err(error) => *library.error.lock().unwrap() = Some(error.to_string()),
            }
            library.busy.store(false, Ordering::Release);
            repaint.request_repaint();
        });
    }
}

struct HeaderLogos {
    light: egui::TextureHandle,
    dark: egui::TextureHandle,
}

fn bundled_png_texture(
    ctx: &egui::Context,
    name: &'static str,
    bytes: &'static [u8],
) -> egui::TextureHandle {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .expect("bundled Oxfer logo must be a valid PNG")
        .into_rgba8();
    ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    )
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct P2PTransfer {
    /// Only native receives persist a destination; links and file bytes never do.
    #[cfg(not(target_arch = "wasm32"))]
    save_directory: Option<std::path::PathBuf>,

    /// Native: saved by eframe with `save_directory`. Browser: only in `BROWSER_THEME_KEY`,
    /// written when the user picks a theme; eframe saves nothing there.
    theme: Theme,
    #[serde(skip)]
    mode: Mode,
    /// Wake the real UI when browser callbacks or endpoint startup finish.
    #[serde(skip)]
    repaint: egui::Context,
    /// Handles and metadata for everything this node offers — never bytes.
    #[serde(skip)]
    shared_files: SharedFiles,
    /// `Some` while sharing; written by the bind task.
    #[serde(skip)]
    node: Arc<Mutex<Option<Arc<Node>>>>,
    #[serde(skip)]
    link: Arc<Mutex<Option<String>>>,
    /// Where every spawned task reports a failure. `logic()` drains it to the panel that is
    /// showing, so a task never needs to know which mode the app is in.
    #[serde(skip)]
    node_error: Arc<Mutex<Option<String>>>,
    #[serde(skip)]
    received_files: Arc<Mutex<Vec<ReceivedFile>>>,
    /// Picks waiting to become `PrepareHandle`s (see [`PendingPick`]).
    #[serde(skip)]
    pending_picks: Arc<Mutex<Vec<PendingPick>>>,
    /// Set the moment a bind task is spawned, not when it finishes: `node` stays `None` for
    /// the whole bind, so two picks in quick succession would otherwise start two nodes and
    /// the second would silently replace the first one's link.
    #[serde(skip)]
    sharing: Arc<AtomicBool>,
    #[serde(skip)]
    show_terminal_view: bool,
    /// Stays visible until sharing stops, so copying has an unmistakable result.
    #[serde(skip)]
    link_copied: bool,
    #[serde(skip)]
    header_logos: Option<HeaderLogos>,
    #[serde(skip)]
    last_dark_mode: Option<bool>,
    #[serde(skip)]
    last_theme: Option<Theme>,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    file_input_closure: Option<wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>>,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    hashchange_closure: Option<HashChangeListener>,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    last_fragment: Option<String>,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    local_copies: LocalCopies,
    /// Retaining plaintext on this device is always an explicit choice, never a persisted
    /// preference silently applied to a later private transfer.
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    keep_local_copy: bool,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    confirm_discard: Option<String>,
    /// Set only when the user selected a specific partial copy to resume.
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    resume_target: Option<String>,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    diagnostics_report: Arc<Mutex<Option<String>>>,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    diagnostics_running: Arc<AtomicBool>,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    diagnostic_peer: Arc<Mutex<DiagnosticPeer>>,
}

impl Default for P2PTransfer {
    fn default() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            save_directory: None,
            theme: Theme::default(),
            mode: Mode::Home,
            repaint: egui::Context::default(),
            shared_files: Arc::new(Mutex::new(Vec::new())),
            node: Arc::new(Mutex::new(None)),
            link: Arc::new(Mutex::new(None)),
            node_error: Arc::new(Mutex::new(None)),
            received_files: Arc::new(Mutex::new(Vec::new())),
            pending_picks: Arc::new(Mutex::new(Vec::new())),
            sharing: Arc::new(AtomicBool::new(false)),
            show_terminal_view: false,
            link_copied: false,
            header_logos: None,
            last_dark_mode: None,
            last_theme: None,
            #[cfg(target_arch = "wasm32")]
            file_input_closure: None,
            #[cfg(target_arch = "wasm32")]
            hashchange_closure: None,
            #[cfg(target_arch = "wasm32")]
            last_fragment: None,
            #[cfg(target_arch = "wasm32")]
            local_copies: LocalCopies::default(),
            #[cfg(target_arch = "wasm32")]
            keep_local_copy: false,
            #[cfg(target_arch = "wasm32")]
            confirm_discard: None,
            #[cfg(target_arch = "wasm32")]
            resume_target: None,
            #[cfg(target_arch = "wasm32")]
            diagnostics_report: Arc::new(Mutex::new(None)),
            #[cfg(target_arch = "wasm32")]
            diagnostics_running: Arc::new(AtomicBool::new(false)),
            #[cfg(target_arch = "wasm32")]
            diagnostic_peer: Arc::new(Mutex::new(DiagnosticPeer::default())),
        }
    }
}

#[cfg(target_arch = "wasm32")]
struct HashChangeListener(wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>);

#[cfg(target_arch = "wasm32")]
impl Drop for HashChangeListener {
    fn drop(&mut self) {
        use wasm_bindgen::JsCast as _;

        if let Some(window) = web_sys::window() {
            let _ = window
                .remove_event_listener_with_callback("hashchange", self.0.as_ref().unchecked_ref());
            let _ = window
                .remove_event_listener_with_callback("popstate", self.0.as_ref().unchecked_ref());
        }
    }
}

// ── Flows ────────────────────────────────────────────────────────────────────

impl P2PTransfer {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        #[cfg(target_arch = "wasm32")]
        cc.egui_ctx
            .options_mut(|options| options.sync_window_theme = false);
        // Native: eframe's saved settings. The browser saves none (see `BROWSER_THEME_KEY`).
        #[cfg(not(target_arch = "wasm32"))]
        let mut app: Self = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, eframe::APP_KEY))
            .unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        let mut app = Self {
            theme: restore_browser_theme(cc.storage),
            ..Self::default()
        };
        app.repaint = cc.egui_ctx.clone();
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast as _;

            let ctx = cc.egui_ctx.clone();
            let closure =
                wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
                    ctx.request_repaint();
                })
                    as Box<dyn FnMut(web_sys::Event)>);
            if let Some(window) = web_sys::window() {
                let _ = window.add_event_listener_with_callback(
                    "hashchange",
                    closure.as_ref().unchecked_ref(),
                );
                let _ = window
                    .add_event_listener_with_callback("popstate", closure.as_ref().unchecked_ref());
                app.hashchange_closure = Some(HashChangeListener(closure));
            }
            app.local_copies
                .run(LocalCopyAction::Refresh, cc.egui_ctx.clone());
        }
        app
    }

    /// Record a failure for whichever panel is showing.
    fn report(slot: &Arc<Mutex<Option<String>>>, message: String) {
        log::error!("{message}");
        if let Ok(mut slot) = slot.lock() {
            *slot = Some(message);
        }
    }

    // ── Pick ─────────────────────────────────────────────────────────────

    #[cfg(not(target_arch = "wasm32"))]
    fn pick_file(&mut self) {
        let Some(path) = rfd::FileDialog::new().pick_file() else {
            return;
        };
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        // The snapshot is taken *before* hashing, so `open_source` can tell later whether the
        // bytes still match the hash the manifest advertises.
        match file_io::snapshot_path(&path) {
            Ok(snapshot) => {
                if let Ok(mut picks) = self.pending_picks.lock() {
                    picks.push(PendingPick {
                        name,
                        origin: FileOrigin::Path(path),
                        snapshot,
                    });
                }
            }
            Err(e) => Self::report(&self.node_error, format!("could not read {name}: {e}")),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn pick_file(&mut self) {
        use wasm_bindgen::JsCast as _;

        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let input = match document
            .create_element("input")
            .ok()
            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        {
            Some(input) => input,
            None => {
                Self::report(
                    &self.node_error,
                    "this browser would not open a file picker".to_string(),
                );
                return;
            }
        };
        input.set_type("file");

        let picks = self.pending_picks.clone();
        let repaint = self.repaint.clone();
        let closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::Event| {
            let Some(input) = event
                .target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
            else {
                return;
            };
            let Some(file) = input.files().and_then(|files| files.get(0)) else {
                return;
            };
            // A file-picker change is still a user action on WebKit. Try the
            // first screen wake lock here, before async hashing/binding.
            set_transfer_wake_lock(true);
            // `File` freezes name, size and mtime at pick time, so this snapshot is the
            // one the browser will still report later.
            let snapshot = file_io::snapshot_web(&file);
            let name = file.name();
            if let Ok(mut picks) = picks.lock() {
                picks.push(PendingPick {
                    name,
                    origin: FileOrigin::Web(send_wrapper::SendWrapper::new(file)),
                    snapshot,
                });
            }
            repaint.request_repaint();
        })
            as Box<dyn FnMut(web_sys::Event)>);

        input.set_onchange(Some(closure.as_ref().unchecked_ref()));
        input.click();
        // Keep the closure alive for as long as the input can fire it.
        self.file_input_closure = Some(closure);
    }

    /// Hash a picked file, then publish it as a `SharedFile`.
    fn begin_prepare(&mut self, pick: PendingPick) -> PrepareHandle {
        let (progress_tx, progress_rx) = watch::channel(0.0f32);
        let files = self.shared_files.clone();
        let errors = self.node_error.clone();
        let label = pick.name.clone();

        let task = task::spawn(async move {
            let PendingPick {
                name,
                origin,
                snapshot,
            } = pick;
            let mut source = match file_io::open_source(&origin, &snapshot).await {
                Ok(source) => source,
                Err(e) => {
                    Self::report(&errors, format!("could not open {name}: {e}"));
                    return;
                }
            };
            let size = file_io::Source::size(&source);
            match file_io::hash_source(&mut source, &progress_tx).await {
                Ok(hash) => {
                    let meta = FileMeta {
                        name: file_io::sanitize_name(&name),
                        size,
                        hash,
                    };
                    if let Ok(mut files) = files.lock() {
                        files.push(SharedFile {
                            meta,
                            origin,
                            snapshot,
                        });
                    }
                    log::info!("ready to share: {name} ({size} bytes)");
                }
                Err(e) => Self::report(&errors, format!("could not hash {name}: {e}")),
            }
            // Dropping `progress_tx` here is what tells the UI this handle is done.
        });

        PrepareHandle {
            name: label,
            progress: progress_rx,
            _task: AbortOnDropHandle::new(task),
        }
    }

    // ── Share ────────────────────────────────────────────────────────────

    /// The page this app is served from, without private fragments or sender-only
    /// diagnostic options — the base of a share link.
    fn base_url() -> String {
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::window()
                .and_then(|w| w.location().href().ok())
                .map(|href| Self::share_base_url(&href))
                .unwrap_or_else(|| "https://oxfer.app/".to_string())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            "https://oxfer.app/".to_string()
        }
    }

    #[cfg(any(test, target_arch = "wasm32"))]
    fn share_base_url(href: &str) -> String {
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

    /// Whether a share link should carry `#dev` — only if this page was opened with it.
    fn dev_flag(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::window()
                .and_then(|w| w.location().hash().ok())
                .map(|hash| Node::parse_fragment(&hash).dev)
                .unwrap_or(false)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }

    fn start_sharing(&mut self) {
        if self.sharing.swap(true, Ordering::AcqRel) {
            return;
        }
        self.link_copied = false;
        let files = self.shared_files.clone();
        let node_slot = self.node.clone();
        let link_slot = self.link.clone();
        let errors = self.node_error.clone();
        let sharing = self.sharing.clone();
        let repaint = self.repaint.clone();
        let base = Self::base_url();
        let dev = self.dev_flag();

        task::spawn(async move {
            let node = match Node::bind(files, RelayChoice::from_env()).await {
                Ok(node) => Arc::new(node),
                Err(e) => {
                    sharing.store(false, Ordering::Release);
                    Self::report(&errors, e.to_string());
                    repaint.request_repaint();
                    return;
                }
            };
            match node.ticket().await {
                Ok(ticket) => {
                    let link = Node::link(&base, &ticket, &node.cap(), dev);
                    if let Ok(mut slot) = link_slot.lock() {
                        *slot = Some(link);
                    }
                }
                Err(e) => {
                    sharing.store(false, Ordering::Release);
                    Self::report(&errors, e.to_string());
                    repaint.request_repaint();
                    node.shutdown().await;
                    return;
                }
            }
            if let Ok(mut slot) = node_slot.lock() {
                *slot = Some(node);
            }
            repaint.request_repaint();
        });
    }

    /// Stop serving. The next share draws a fresh key, so the old link stops working.
    fn stop_sharing(&mut self) {
        self.sharing.store(false, Ordering::Release);
        #[cfg(target_arch = "wasm32")]
        set_transfer_wake_lock(false);
        self.link_copied = false;
        let node = self.node.lock().ok().and_then(|mut n| n.take());
        if let Some(node) = node {
            task::spawn(async move { node.shutdown().await });
        }
        if let Ok(mut link) = self.link.lock() {
            *link = None;
        }
    }

    // ── Receive ──────────────────────────────────────────────────────────

    /// Turn pasted text — a whole link or a bare fragment — into receive parameters.
    fn start_receive(&mut self, input: &str) {
        let fragment = input.split_once('#').map(|(_, f)| f).unwrap_or(input);
        let params = Node::parse_fragment(fragment);

        if let Some(error) = params.error.clone() {
            self.set_receive(params, Some(input.to_string()), None, Some(error), false);
            return;
        }
        let Some(ticket) = params.ticket.clone() else {
            self.set_receive(
                params,
                Some(input.to_string()),
                None,
                Some("this is not a share link".to_string()),
                false,
            );
            return;
        };
        // A bare ticket authorizes nothing (design §2.6): refuse before dialling, and say why,
        // so a truncated link never looks like a connectivity failure.
        if params.cap.is_none() {
            self.set_receive(
                params,
                Some(input.to_string()),
                None,
                Some(MISSING_CAP.to_string()),
                false,
            );
            return;
        }

        let opts = ReceiveOptions::from_fragment(&params);
        let relay = RelayChoice::from_env();
        let repaint = self.repaint.clone();
        self.set_receive(params, Some(input.to_string()), None, None, true);
        let Mode::Receive(r) = &self.mode else {
            return;
        };
        // Each receive owns its startup result. Leaving, cancelling, or opening another
        // link drops this slot, so a late completion cannot replace the new session.
        let slot = Arc::downgrade(&r.startup);

        task::spawn(async move {
            // A receiver binds its own node on demand; `run_receiver` shuts it down again on
            // every exit path, so a cancelled receive leaks nothing.
            let result = match Node::bind(Arc::new(Mutex::new(Vec::new())), relay).await {
                Ok(node) if slot.strong_count() == 0 => {
                    node.shutdown().await;
                    return;
                }
                Ok(node) => Ok(TransferHandle::start_receive(Arc::new(node), ticket, opts)),
                Err(e) => Err(e.to_string()),
            };
            if let Some(slot) = slot.upgrade() {
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(result);
                }
            }
            repaint.request_repaint();
        });
    }

    /// Enter (or update) receive mode, keeping whatever the user typed.
    fn set_receive(
        &mut self,
        params: FragmentParams,
        input: Option<String>,
        handle: Option<TransferHandle>,
        error: Option<String>,
        opening: bool,
    ) {
        let input = input.unwrap_or_else(|| match &self.mode {
            Mode::Receive(r) => r.input.clone(),
            _ => String::new(),
        });
        self.mode = Mode::Receive(Box::new(ReceiveState {
            input,
            params,
            handle,
            error,
            opening,
            startup: Arc::default(),
            save_pending: Arc::new(AtomicBool::new(false)),
        }));
    }

    /// Build one sink per manifest entry and hand them to the session.
    ///
    /// Invoke the browser picker promptly after the Save gesture, without preceding
    /// asynchronous work. Transient activation is browser-controlled, not an await count.
    fn save_click(&mut self, manifest: Vec<FileMeta>) {
        let Mode::Receive(r) = &mut self.mode else {
            return;
        };
        let Some(handle) = &r.handle else { return };
        if !matches!(handle.latest().phase, Phase::AwaitingSave { .. }) {
            return;
        }
        let Some(attempt) = SaveAttempt::begin(&r.save_pending) else {
            return;
        };
        #[cfg(target_arch = "wasm32")]
        retry_transfer_wake_lock();
        r.error = None;
        let commands = handle.commands.clone();
        let errors = self.node_error.clone();

        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = match self.save_directory.clone() {
                Some(dir) => Some(dir),
                None => rfd::FileDialog::new().pick_folder(),
            };
            let Some(dir) = dir else {
                return; // the user dismissed the folder picker: stay in AwaitingSave
            };
            self.save_directory = Some(dir.clone());

            let mut sinks = Vec::with_capacity(manifest.len());
            for meta in &manifest {
                match file_io::FsSink::create(&dir, &file_io::sanitize_name(&meta.name)) {
                    Ok(sink) => sinks.push(file_io::AnySink::Fs(sink)),
                    Err(e) => {
                        Self::report(&errors, format!("could not create {}: {e}", meta.name));
                        return;
                    }
                }
            }
            if commands.try_send(ReceiveCommand::Save(sinks)).is_err() {
                Self::report(&errors, "the transfer is no longer running".to_string());
            } else {
                attempt.submitted();
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            if self.keep_local_copy {
                if let Some(expected) = &self.resume_target {
                    match file_io::web::resume::manifest_id(&manifest) {
                        Ok(id) if &id == expected => {}
                        Ok(_) => {
                            Self::report(
                                &errors,
                                "this link offers different files than the selected local copy; \
                                 ask the sender for the same files, names and ordering"
                                    .to_string(),
                            );
                            return;
                        }
                        Err(error) => {
                            Self::report(&errors, error.to_string());
                            return;
                        }
                    }
                }
            }
            let pref = r.params.sink_pref;
            let cancel = handle.cancel.clone();
            let keep_local_copy = self.keep_local_copy;
            let repaint = self.repaint.clone();
            let library = self.local_copies.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if cancel.is_cancelled() {
                    return;
                }
                if keep_local_copy {
                    // Persistent copies are keyed by the complete manifest, not the link.
                    // Reopening a fresh sender link can therefore resume the same bytes
                    // without storing a bearer capability on this device.
                    match file_io::web::resume::prepare(&manifest).await {
                        Ok(files) => {
                            let permit = tokio::select! {
                                biased;
                                _ = cancel.cancelled() => None,
                                result = commands.reserve() => result.ok(),
                            };
                            if let Some(permit) = permit.filter(|_| !cancel.is_cancelled()) {
                                permit.send(ReceiveCommand::Resume(files));
                                attempt.submitted();
                            } else {
                                for file in files {
                                    file_io::Sink::abort(file.sink).await;
                                }
                            }
                        }
                        Err(_) if cancel.is_cancelled() => {}
                        Err(error) => Self::report(&errors, error.to_string()),
                    }
                    library.run(LocalCopyAction::Refresh, repaint.clone());
                    repaint.request_repaint();
                    return;
                }
                // Keep awaiting the picker even if the receive is cancelled: a native dialog
                // cannot be closed by dropping its Promise, and returned sinks need cleanup.
                match file_io::web::pick_sinks(&manifest, pref).await {
                    Ok(sinks) => {
                        let permit = tokio::select! {
                            biased;
                            _ = cancel.cancelled() => None,
                            result = commands.reserve() => result.ok(),
                        };
                        if let Some(permit) = permit.filter(|_| !cancel.is_cancelled()) {
                            permit.send(ReceiveCommand::Save(sinks));
                            attempt.submitted();
                            return;
                        }
                        for sink in sinks {
                            file_io::Sink::abort(sink).await;
                        }
                        if !cancel.is_cancelled() {
                            Self::report(&errors, "the transfer is no longer running".to_string());
                        }
                    }
                    // Dismissing a picker is not an error: stay in AwaitingSave so Save can be
                    // clicked again.
                    Err(file_io::SinkError::Cancelled) => {}
                    Err(_) if cancel.is_cancelled() => {}
                    Err(e) => Self::report(&errors, e.to_string()),
                }
            });
        }
    }

    /// Accept each new URL fragment, including navigation within the already-open page.
    ///
    /// A fragment never reaches a server, but it does stay on the machine — URL bar, history,
    /// session restore, a pasted screenshot. The ticket and the capability are removed
    /// immediately; the QA flags stay so a reload keeps them (design §4.7.1).
    #[cfg(target_arch = "wasm32")]
    fn check_fragment_change(&mut self) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Ok(hash) = window.location().hash() else {
            return;
        };
        let path = window.location().pathname().unwrap_or_default();
        let route = format!("{path}{hash}");
        if self.last_fragment.as_ref() == Some(&route) {
            return;
        }
        self.last_fragment = Some(route);
        if path.trim_end_matches('/') == "/diags" && hash.is_empty() {
            if !matches!(self.mode, Mode::Diagnostics) {
                self.reset_peer_diagnostics();
                self.mode = Mode::Diagnostics;
                self.run_diagnostics();
            }
            return;
        }
        if path != "/diags" && hash.is_empty() && matches!(self.mode, Mode::Diagnostics) {
            self.stop_peer_diagnostics();
            self.mode = Mode::Home;
            return;
        }
        self.check_fragment(&hash);
    }

    #[cfg(target_arch = "wasm32")]
    fn diagnostics_url() -> Option<String> {
        Some(format!(
            "{}/diags",
            web_sys::window()?.location().origin().ok()?
        ))
    }

    #[cfg(target_arch = "wasm32")]
    fn open_diagnostics(&mut self) {
        if matches!(self.mode, Mode::Diagnostics) || self.is_preparing_share() {
            return;
        }
        if let (Some(window), Some(url)) = (web_sys::window(), Self::diagnostics_url()) {
            if let Ok(history) = window.history() {
                let _ = history.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&url));
            }
            self.last_fragment = None;
        }
        self.reset_peer_diagnostics();
        self.mode = Mode::Diagnostics;
        self.run_diagnostics();
    }

    #[cfg(target_arch = "wasm32")]
    fn check_fragment(&mut self, hash: &str) {
        use crate::node::SinkPref;
        use wasm_bindgen::JsValue;

        let Some(window) = web_sys::window() else {
            return;
        };
        if let Some(raw) = hash.strip_prefix("#diagnostics=") {
            // Remove the ephemeral peer ticket from browser history immediately.
            if let Ok(history) = window.history() {
                let href = window.location().href().unwrap_or_default();
                let base = href.split('#').next().unwrap_or(&href);
                let scrubbed = if window.location().pathname().ok().as_deref() == Some("/diags") {
                    base.to_string()
                } else {
                    format!("{base}#diagnostics")
                };
                let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&scrubbed));
            }
            self.last_fragment = Some(format!(
                "{}{}",
                window.location().pathname().unwrap_or_default(),
                window.location().hash().unwrap_or_default()
            ));
            self.reset_peer_diagnostics();
            let mut peer = self.diagnostic_peer.lock().unwrap();
            match raw.parse::<EndpointTicket>() {
                Ok(ticket) => {
                    peer.session = Some(DiagnosticNode::session_id(&ticket));
                    peer.target = Some(ticket);
                }
                Err(_) => peer.outcome = Some("Invalid diagnostics link".into()),
            }
            drop(peer);
            self.mode = Mode::Diagnostics;
            self.run_diagnostics();
            return;
        }
        if hash == "#diagnostics" {
            self.reset_peer_diagnostics();
            self.mode = Mode::Diagnostics;
            self.run_diagnostics();
            return;
        }
        if window.location().pathname().ok().as_deref() == Some("/diags") {
            // A transfer fragment on the diagnostics path is neither a peer
            // diagnostic ticket nor a valid diagnostics URL. Discard it rather
            // than starting a receive which the path router would cancel.
            if let Ok(history) = window.history() {
                let _ = history.replace_state_with_url(&JsValue::NULL, "", Some("/diags"));
            }
            self.last_fragment = Some("/diags".into());
            self.reset_peer_diagnostics();
            self.diagnostic_peer.lock().unwrap().outcome =
                Some("This page runs diagnostics only; open file links on the home page".into());
            self.mode = Mode::Diagnostics;
            self.run_diagnostics();
            return;
        }
        let params = Node::parse_fragment(hash);
        if params.ticket.is_none() && params.cap.is_none() && params.error.is_none() {
            return;
        }

        let mut keep: Vec<String> = Vec::new();
        if params.dev {
            keep.push("dev".to_string());
        }
        if params.force_relay {
            keep.push("relay".to_string());
        }
        match params.sink_pref {
            SinkPref::Auto => {}
            SinkPref::Fsa => keep.push("sink=fsa".to_string()),
            SinkPref::Sw => keep.push("sink=sw".to_string()),
            SinkPref::Mem => keep.push("sink=mem".to_string()),
        }
        if let Some(n) = params.kill_dc_after {
            keep.push(format!("killdc={n}"));
        }
        if let Some(bytes) = params.window {
            keep.push(format!("win={}", bytes / (1024 * 1024)));
        }

        let href = window.location().href().unwrap_or_default();
        let base = href.split('#').next().unwrap_or(&href).to_string();
        let scrubbed = if keep.is_empty() {
            base
        } else {
            format!("{base}#{}", keep.join("&"))
        };
        if let Ok(history) = window.history() {
            let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&scrubbed));
        }
        // replaceState does not fire hashchange. Remember the scrubbed value now so opening
        // the same original link again is still treated as a new navigation.
        self.last_fragment = Some(format!(
            "{}{}",
            window.location().pathname().unwrap_or_default(),
            window.location().hash().unwrap_or_default()
        ));

        self.reset_peer_diagnostics();
        self.start_receive(&href);
    }

    #[cfg(target_arch = "wasm32")]
    fn run_diagnostics(&mut self) {
        if self.diagnostics_running.swap(true, Ordering::AcqRel) {
            return;
        }
        *self.diagnostics_report.lock().unwrap() = None;
        let report = self.diagnostics_report.clone();
        let running = self.diagnostics_running.clone();
        let repaint = self.repaint.clone();
        wasm_bindgen_futures::spawn_local(async move {
            *report.lock().unwrap() = Some(crate::diagnostics::collect().await);
            running.store(false, Ordering::Release);
            repaint.request_repaint();
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn run_peer_diagnostics(&mut self) {
        let (target, generation) = {
            let mut peer = self.diagnostic_peer.lock().unwrap();
            if peer.busy || peer.node.is_some() {
                return;
            }
            peer.busy = true;
            peer.outcome = None;
            (peer.target.clone(), peer.generation)
        };
        let state = self.diagnostic_peer.clone();
        let repaint = self.repaint.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let outcome = match DiagnosticNode::bind(RelayChoice::from_env()).await {
                Ok(node) => {
                    if let Some(ticket) = target {
                        let outcome = if node.probe(&ticket).await.is_ok() {
                            "Peer ping succeeded (iroh dial, stream and reply)".to_string()
                        } else {
                            "Peer ping failed or timed out (see terminal for network warnings)"
                                .to_string()
                        };
                        node.shutdown().await;
                        outcome
                    } else {
                        match node.ticket().await {
                            Ok(ticket) => {
                                let base = P2PTransfer::diagnostics_url().unwrap_or_default();
                                let link = format!("{base}#diagnostics={ticket}");
                                let node = Arc::new(node);
                                let stale = {
                                    let mut peer = state.lock().unwrap();
                                    if peer.generation == generation {
                                        peer.session = Some(DiagnosticNode::session_id(&ticket));
                                        peer.link = Some(link);
                                        peer.node = Some(node.clone());
                                        false
                                    } else {
                                        true
                                    }
                                };
                                if stale {
                                    node.shutdown().await;
                                    return;
                                }
                                "Listening for a diagnostic peer; keep this tab open".to_string()
                            }
                            Err(_) => {
                                node.shutdown().await;
                                "Could not register diagnostic endpoint with relay".to_string()
                            }
                        }
                    }
                }
                Err(_) => "Could not start diagnostic endpoint".to_string(),
            };
            let mut peer = state.lock().unwrap();
            if peer.generation == generation {
                peer.outcome = Some(outcome);
                peer.busy = false;
            }
            repaint.request_repaint();
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn stop_peer_diagnostics(&mut self) {
        let mut peer = self.diagnostic_peer.lock().unwrap();
        peer.generation = peer.generation.wrapping_add(1);
        peer.busy = false;
        if let Some(node) = peer.node.take() {
            wasm_bindgen_futures::spawn_local(async move { node.shutdown().await });
        }
        peer.link = None;
        peer.outcome = Some("Stopped listening for diagnostic peers".into());
    }

    #[cfg(target_arch = "wasm32")]
    fn reset_peer_diagnostics(&mut self) {
        self.stop_peer_diagnostics();
        let mut peer = self.diagnostic_peer.lock().unwrap();
        *peer = DiagnosticPeer {
            generation: peer.generation,
            ..DiagnosticPeer::default()
        };
    }

    // ── Per-frame bookkeeping (called from `logic`) ───────────────────────

    /// Turn queued picks into hashing tasks, entering Send mode on the first one.
    fn drain_picks(&mut self) {
        let picks: Vec<PendingPick> = match self.pending_picks.lock() {
            Ok(mut picks) if !picks.is_empty() => picks.drain(..).collect(),
            _ => return,
        };
        for pick in picks {
            let handle = self.begin_prepare(pick);
            match &mut self.mode {
                Mode::Send { preparing } => preparing.push(handle),
                _ => {
                    self.mode = Mode::Send {
                        preparing: vec![handle],
                    }
                }
            }
        }
        // Sharing starts as soon as there is something to share; the link stays hidden until
        // every hash is final (design §4.8.2).
        self.start_sharing();
    }

    /// Drop hashing handles whose task has ended.
    fn prune_preparing(&mut self) {
        if let Mode::Send { preparing } = &mut self.mode {
            preparing.retain(|h| !h.finished());
        }
    }

    /// Engine watches have no egui callback: poll while starting, serving, or receiving.
    fn needs_progress_poll(&self) -> bool {
        self.sharing.load(Ordering::Acquire)
            || match &self.mode {
                Mode::Send { preparing } => !preparing.is_empty(),
                Mode::Receive(r) => r.opening || r.handle.is_some(),
                Mode::Home => false,
                #[cfg(target_arch = "wasm32")]
                Mode::Diagnostics => false,
            }
    }

    #[cfg(target_arch = "wasm32")]
    fn needs_wake_lock(&self) -> bool {
        self.sharing.load(Ordering::Acquire)
            || match &self.mode {
                Mode::Send { preparing } => !preparing.is_empty(),
                Mode::Receive(r) => {
                    let progress = r.handle.as_ref().map(|h| h.latest());
                    Self::receive_needs_wake_lock(
                        r.opening,
                        r.save_pending.load(Ordering::Acquire),
                        progress.as_ref().map(|p| &p.phase),
                    )
                }
                Mode::Home | Mode::Diagnostics => false,
            }
    }

    #[cfg(any(test, target_arch = "wasm32"))]
    fn receive_needs_wake_lock(opening: bool, save_pending: bool, phase: Option<&Phase>) -> bool {
        opening
            || phase.is_some_and(|phase| {
                !phase.is_terminal()
                    && (save_pending || !matches!(phase, Phase::AwaitingSave { .. }))
            })
    }

    #[cfg(target_arch = "wasm32")]
    fn show_transfer_attention(ui: &mut Ui, tc: &Tc, sender: bool, awaiting_save: bool) {
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(
                tc.primary.r(),
                tc.primary.g(),
                tc.primary.b(),
                24,
            ))
            .stroke(Stroke::new(1.0, tc.primary))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(if sender {
                            "Keep this tab open while they save your file."
                        } else {
                            "Keep this tab open until your file is saved."
                        })
                        .color(tc.on_surface)
                        .size(16.0)
                        .strong(),
                    )
                    .wrap(),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "Keep this page visible and your screen on. Locking your device or \
                             switching apps may interrupt the transfer.",
                        )
                        .color(tc.on_surface_var)
                        .size(13.0),
                    )
                    .wrap(),
                );
                let (status, color) = if awaiting_save {
                    (
                        "We'll ask your browser to keep the screen on when you start saving.",
                        tc.on_surface_var,
                    )
                } else {
                    match transfer_wake_lock_status().as_str() {
                        "active" => (
                            "Your browser is helping keep the screen on (uses more battery).",
                            tc.secondary,
                        ),
                        "requesting" => {
                            ("Asking your browser to keep the screen on…", tc.outline)
                        }
                        _ => (
                            "Your browser can't keep the screen on automatically. Keep it on yourself.",
                            tc.error,
                        ),
                    }
                };
                ui.add(egui::Label::new(RichText::new(status).color(color).size(12.0)).wrap());
            });
    }

    /// Adopt a receive handle built by the on-demand bind task.
    fn adopt_pending_handle(&mut self) {
        let Mode::Receive(r) = &mut self.mode else {
            return;
        };
        let pending = r.startup.lock().ok().and_then(|mut h| h.take());
        if let Some(result) = pending {
            r.opening = false;
            match result {
                Ok(handle) => r.handle = Some(handle),
                Err(error) => r.error = Some(error),
            }
        }
    }

    /// Move any reported failure into the panel that is showing.
    fn drain_error(&mut self) {
        if let Mode::Receive(r) = &mut self.mode {
            if r.error.is_some() {
                return;
            }
            let taken = self.node_error.lock().ok().and_then(|mut e| e.take());
            if taken.is_some() {
                r.opening = false;
                r.error = taken;
            }
        }
    }

    /// Record a finished receive exactly once, then drop the handle.
    ///
    /// The terminal phase arrives on the same watch the panel already reads, so there is no
    /// second source of truth; dropping the handle afterwards also stops cloning the terminal
    /// progress value on every later frame.
    fn poll_receive(&mut self) {
        let Mode::Receive(r) = &mut self.mode else {
            return;
        };
        let Some(h) = &r.handle else { return };
        let progress = h.latest();
        if !progress.phase.is_terminal() {
            return;
        }
        let path = progress.path;
        match progress.phase {
            Phase::Complete { saved } => {
                let when = Self::timestamp();
                if let Ok(mut files) = self.received_files.lock() {
                    files.extend(saved.into_iter().enumerate().map(|(index, f)| {
                        ReceivedFile {
                            name: f.name,
                            size: f.size,
                            location: f.location,
                            when: when.clone(),
                            path,
                            average_bytes_per_sec: progress
                                .file_average_bytes_per_sec
                                .get(index)
                                .copied()
                                .flatten(),
                        }
                    }));
                }
                // The URL was scrubbed as soon as it was accepted; after success, do not put the
                // bearer credential back on screen.
                r.input.clear();
            }
            Phase::Failed => {
                r.error = Some(
                    progress
                        .error
                        .clone()
                        .map(|e| Self::humanize(&e))
                        .unwrap_or_else(|| "the transfer failed".to_string()),
                );
            }
            Phase::Cancelled => r.error = Some("the transfer was cancelled".to_string()),
            _ => {}
        }
        r.handle = None;
        #[cfg(target_arch = "wasm32")]
        {
            self.keep_local_copy = false;
            self.resume_target = None;
            self.local_copies
                .run(LocalCopyAction::Refresh, self.repaint.clone());
        }
    }

    /// Turn an engine error string into something a user can act on.
    fn humanize(error: &str) -> String {
        if error.contains("access code is missing") {
            MISSING_CAP.to_string()
        } else if error.contains("access code") {
            CAP_REJECTED.to_string()
        } else {
            error.to_string()
        }
    }

    fn timestamp() -> String {
        #[cfg(target_arch = "wasm32")]
        {
            js_sys::Date::new_0()
                .to_locale_time_string("en-GB")
                .as_string()
                .unwrap_or_default()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
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

    fn format_size(size_bytes: u64) -> String {
        const KB: f64 = 1024.0;
        const MB: f64 = KB * 1024.0;
        const GB: f64 = MB * 1024.0;
        let size = size_bytes as f64;
        if size >= GB {
            format!("{:.2} GB", size / GB)
        } else if size >= MB {
            format!("{:.2} MB", size / MB)
        } else if size >= KB {
            format!("{:.1} KB", size / KB)
        } else {
            format!("{size_bytes} B")
        }
    }

    fn transfer_speed(progress: &TransferProgress) -> Option<String> {
        (matches!(progress.phase, Phase::Transferring)
            && progress.bytes_per_sec.is_finite()
            && progress.bytes_per_sec > 0.0)
            .then(|| format!("{}/s", Self::format_size(progress.bytes_per_sec as u64)))
    }

    /// The QA flags this receive is running with (design §7.1), or empty for a plain run.
    fn flag_summary(params: &FragmentParams) -> String {
        use crate::node::SinkPref;
        let mut flags: Vec<String> = Vec::new();
        if params.force_relay {
            flags.push("relay forced".to_string());
        }
        match params.sink_pref {
            SinkPref::Auto => {}
            SinkPref::Fsa => flags.push("sink=fsa".to_string()),
            SinkPref::Sw => flags.push("sink=sw".to_string()),
            SinkPref::Mem => flags.push("sink=mem".to_string()),
        }
        if let Some(n) = params.kill_dc_after {
            flags.push(format!("killdc={n}"));
        }
        if let Some(bytes) = params.window {
            flags.push(format!("win={} MiB", bytes / (1024 * 1024)));
        }
        flags.join(" · ")
    }

    fn path_badge(phase: &Phase, path: TransferPath) -> &'static str {
        if matches!(phase, Phase::Reconnecting { .. }) {
            return "Connecting again";
        }
        if matches!(phase, Phase::Signaling) {
            return "Finding a connection";
        }
        Self::transfer_path_text(path)
    }

    fn transfer_path_text(path: TransferPath) -> &'static str {
        match path {
            TransferPath::Direct => "Direct connection",
            TransferPath::Relayed => "Encrypted via a helper server",
            TransferPath::Unknown => "Connecting",
        }
    }

    fn phase_text(phase: &Phase) -> &'static str {
        match phase {
            Phase::Connecting => "Connecting…",
            Phase::Reconnecting { .. } => "Connecting again…",
            Phase::Handshake => "Checking the link…",
            Phase::AwaitingSave { .. } => "Ready: choose where to save",
            Phase::Signaling => "Finding a connection…",
            Phase::Transferring => "Transfer in progress…",
            Phase::Switching => "Trying another connection…",
            Phase::Verifying => "Checking the file…",
            Phase::Complete { .. } => "Transfer complete and checked",
            Phase::Failed => "Failed",
            Phase::Cancelled => "Cancelled",
        }
    }

    fn apply_theme(ctx: &egui::Context, ui: &mut Ui, theme: Theme, dark: bool) {
        let tc = Tc::of(theme, dark);
        #[cfg(target_arch = "wasm32")]
        set_browser_theme(
            if theme == Theme::Clean {
                "clean"
            } else {
                "rusty"
            },
            dark,
        );
        let mut v = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.panel_fill = tc.bg;
        v.window_fill = tc.surface;
        v.faint_bg_color = tc.surface_low;
        v.extreme_bg_color = tc.surface_lowest;
        v.code_bg_color = tc.surface_lowest;
        v.widgets.noninteractive.bg_fill = tc.surface_low;
        v.widgets.noninteractive.weak_bg_fill = tc.surface_low;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, tc.outline_var);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, tc.outline);
        v.widgets.inactive.bg_fill = tc.surface;
        v.widgets.inactive.weak_bg_fill = tc.surface;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, tc.outline_var);
        v.widgets.hovered.bg_fill = tc.surface_high;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, tc.outline);
        v.widgets.active.bg_fill = tc.surface_high;
        v.widgets.active.bg_stroke = Stroke::new(1.0_f32, tc.primary);
        v.selection.bg_fill =
            Color32::from_rgba_unmultiplied(tc.primary.r(), tc.primary.g(), tc.primary.b(), 60);
        v.override_text_color = Some(tc.on_surface);
        let radius = if theme == Theme::Clean { 18 } else { 10 };
        v.widgets.inactive.corner_radius = CornerRadius::same(radius);
        v.widgets.hovered.corner_radius = CornerRadius::same(radius);
        v.widgets.active.corner_radius = CornerRadius::same(radius);
        // Both, and in this order: the context so later frames start correct, and the live
        // `Ui` so *this* frame is already themed. Setting only the context would leave the
        // root `Ui` — built before `logic()` ran — one frame behind on every toggle.
        ctx.set_visuals(v.clone());
        ctx.global_style_mut(|style| {
            style.spacing.button_padding = egui::vec2(18.0, 11.0);
            style.spacing.item_spacing = egui::vec2(10.0, 10.0);
            style.spacing.interact_size.y = 44.0;
        });
        *ui.visuals_mut() = v;
        ui.spacing_mut().button_padding = egui::vec2(18.0, 11.0);
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        ui.spacing_mut().interact_size.y = 44.0;
    }
}

// ── Rendering ────────────────────────────────────────────────────────────────

/// A card: the one container every panel below is built from.
fn card(tc: &Tc) -> egui::Frame {
    egui::Frame::new()
        .fill(tc.surface_low)
        .corner_radius(CornerRadius::same(if tc.theme == Theme::Clean {
            20
        } else {
            16
        }))
        .stroke(Stroke::new(1.0_f32, tc.outline_var))
        .inner_margin(egui::Margin::same(22))
}

fn primary_button(tc: &Tc, label: &str) -> Button<'static> {
    Button::new(
        RichText::new(label.to_string())
            .color(tc.on_primary)
            .strong()
            .size(15.0),
    )
    .fill(tc.primary)
    .stroke(Stroke::new(1.0, tc.primary))
    .corner_radius(CornerRadius::same(if tc.theme == Theme::Clean {
        18
    } else {
        10
    }))
    .min_size(egui::vec2(0.0, 46.0))
}

fn copy_button(tc: &Tc, copied: bool) -> Button<'static> {
    if copied {
        primary_button(tc, "✓ Copied to clipboard")
            .fill(tc.secondary)
            .stroke(Stroke::new(1.0, tc.secondary))
    } else {
        primary_button(tc, "Copy link")
    }
}

fn outline_button(label: &str, color: Color32) -> Button<'static> {
    Button::new(
        RichText::new(label.to_string())
            .color(color)
            .strong()
            .size(14.0),
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::new(1.0_f32, color))
    .corner_radius(CornerRadius::same(10))
    .min_size(egui::vec2(0.0, 46.0))
}

/// Width of a bottom-bar button: "Diags" and "Legal" share the bar with the terminal toggle.
const fn bar_button_width(narrow: bool) -> f32 {
    if narrow {
        60.0
    } else {
        72.0
    }
}

/// An outline button sized for the 50px bottom bar, which fits nothing taller than 44px.
fn bar_button(label: &str, tc: &Tc) -> Button<'static> {
    outline_button(label, tc.outline).min_size(egui::vec2(0.0, 44.0))
}

fn compact(ui: &Ui) -> bool {
    ui.available_width() < 620.0
}

/// Privacy, Terms, Abuse & safety and Source, for a right-to-left bottom-bar layout: inline
/// buttons when there is room, otherwise one "Legal" menu that opens upwards.
fn show_footer_links(ui: &mut Ui, tc: &Tc, inline: bool, menu_width: f32) {
    if inline {
        ui.scope(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(8.0, 6.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            // Right to left: add the last link first so they read in order.
            for link in FooterLink::ALL.into_iter().rev() {
                let response = ui
                    .add(
                        Button::new(
                            RichText::new(link.label())
                                .color(tc.on_surface_var)
                                .size(13.0),
                        )
                        .fill(Color32::TRANSPARENT)
                        .frame_when_inactive(false),
                    )
                    .on_hover_text(link.target_url());
                if response.clicked() {
                    link.open(ui.ctx());
                }
            }
        });
        return;
    }
    let menu = ui.add_sized([menu_width, 44.0], bar_button("Legal", tc));
    egui::Popup::menu(&menu)
        .align(egui::RectAlign::TOP_END)
        .show(|ui| {
            for link in FooterLink::ALL {
                if ui
                    .button(link.label())
                    .on_hover_text(link.target_url())
                    .clicked()
                {
                    link.open(ui.ctx());
                    ui.close();
                }
            }
        });
}

/// The visible text of the receive screens' reporting link.
const REPORT_ABUSE_LABEL: &str = "Report abuse";

/// A reporting link wherever received files are listed, so reporting is easy to find at the
/// moment a recipient sees what they were sent (Ofcom ICU D1/D2). It opens the Abuse & safety
/// page exactly as the bottom bar does and carries nothing about the transfer: no share link,
/// capability or file name.
fn show_report_abuse(ui: &mut Ui, tc: &Tc) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new("Received something illegal or harmful?")
                .color(tc.on_surface_var)
                .size(12.0),
        );
        // For the link only (this row has its own `Ui`, and the prompt is already laid out).
        // egui gives a selectable link the Label role, so screen readers would not announce
        // it as a link. Unwrapped, it moves whole to the next row on a narrow screen.
        ui.style_mut().interaction.selectable_labels = false;
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        let link = FooterLink::Abuse;
        if ui
            .add(egui::Link::new(
                RichText::new(REPORT_ABUSE_LABEL)
                    .color(tc.primary)
                    .underline()
                    .size(13.0),
            ))
            .on_hover_text(link.target_url())
            .clicked()
        {
            link.open(ui.ctx());
        }
    });
}

fn pill(ui: &mut Ui, tc: &Tc, text: &str, accent: bool) {
    let (fill, stroke, color) = if accent {
        (
            Color32::from_rgba_unmultiplied(
                tc.secondary.r(),
                tc.secondary.g(),
                tc.secondary.b(),
                18,
            ),
            tc.secondary,
            tc.secondary,
        )
    } else {
        (tc.surface_high, tc.outline_var, tc.on_surface_var)
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke))
        .corner_radius(CornerRadius::same(20))
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            // Keep badge text on one line even when a row wraps on a phone.
            ui.add(
                egui::Label::new(
                    RichText::new(text)
                        .monospace()
                        .strong()
                        .size(11.0)
                        .color(color),
                )
                .extend(),
            );
        });
}

/// A file-shaped message used for outgoing shares, incoming manifests, and history.
fn file_attachment(ui: &mut Ui, tc: &Tc, name: &str, detail: &str, outgoing: bool) {
    egui::Frame::new()
        .fill(if outgoing {
            tc.surface_high
        } else {
            tc.surface
        })
        .stroke(Stroke::new(1.0, tc.outline_var))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                egui::Frame::new()
                    .fill(tc.primary)
                    .corner_radius(CornerRadius::same(18))
                    .inner_margin(egui::Margin::same(9))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(if outgoing { "↑" } else { "↓" })
                                .color(tc.on_primary)
                                .strong()
                                .size(17.0),
                        );
                    });
                ui.vertical(|ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(name).color(tc.on_surface).size(14.0).strong(),
                        )
                        .wrap(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(detail)
                                .color(tc.on_surface_var)
                                .monospace()
                                .size(11.0),
                        )
                        .wrap(),
                    );
                });
            });
        });
}

#[derive(Clone, Copy)]
enum HomeIcon {
    File,
    Link,
    Device,
}

fn home_story_icon(ui: &mut Ui, tc: &Tc, icon: HomeIcon) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 12.0, tc.surface_high);
    let center = rect.center();
    let stroke = Stroke::new(2.0, tc.secondary);
    match icon {
        HomeIcon::File => {
            let page = egui::Rect::from_center_size(center, egui::vec2(22.0, 28.0));
            painter.rect_stroke(page, 2.0, stroke, egui::StrokeKind::Inside);
            for y in [-5.0, 1.0, 7.0] {
                painter.line_segment(
                    [
                        egui::pos2(center.x - 6.0, center.y + y),
                        egui::pos2(center.x + 6.0, center.y + y),
                    ],
                    stroke,
                );
            }
        }
        HomeIcon::Link => {
            painter.circle_stroke(center + egui::vec2(-6.0, 5.0), 8.0, stroke);
            painter.circle_stroke(center + egui::vec2(6.0, -5.0), 8.0, stroke);
        }
        HomeIcon::Device => {
            let screen = egui::Rect::from_center_size(center, egui::vec2(22.0, 30.0));
            painter.rect_stroke(screen, 4.0, stroke, egui::StrokeKind::Inside);
            painter.circle_filled(center + egui::vec2(0.0, 11.0), 1.5, tc.secondary);
        }
    }
}

fn home_story_step(ui: &mut Ui, tc: &Tc, icon: HomeIcon, title: &str, body: &str) {
    egui::Frame::new()
        .fill(tc.surface_low)
        .stroke(Stroke::new(1.0, tc.outline_var))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                home_story_icon(ui, tc, icon);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(title)
                            .color(tc.on_surface)
                            .size(16.0)
                            .strong(),
                    );
                    ui.add(egui::Label::new(RichText::new(body).color(tc.on_surface_var)).wrap());
                });
            });
        });
}

fn process_step(ui: &mut Ui, tc: &Tc, number: &str, title: &str, body: &str) {
    egui::Frame::new()
        .fill(tc.surface_low)
        .stroke(Stroke::new(1.0, tc.outline_var))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_min_height(142.0);
            ui.horizontal(|ui| {
                pill(ui, tc, number, number == "03");
                ui.label(
                    RichText::new(title)
                        .color(tc.on_surface)
                        .size(17.0)
                        .strong(),
                );
            });
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(RichText::new(body).color(tc.on_surface_var).size(13.0)).wrap(),
            );
        });
}

fn show_how_it_works(ui: &mut Ui, tc: &Tc) {
    // The relay sentences depend on which relays this build was compiled for (plan C6).
    let relay = RelayOperator::current();
    let steps: [(&str, &str, &str); 4] = [
        (
            "01",
            "Hash on your device",
            "Oxfer reads the selected file in bounded slices and computes its BLAKE3 digest locally. \
             The file is not stored on Cloudflare, an Oxfer server, or cloud storage.",
        ),
        (
            "02",
            "Authorize with one private link",
            "The URL fragment contains the endpoint ticket and a fresh bearer capability. Fragments \
             are not sent to the web host, but anyone holding the complete link can receive while \
             the sender keeps sharing.",
        ),
        ("03", "Open an encrypted peer path", relay.peer_path()),
        (
            "04",
            "Stream, save, and verify",
            "Credit-based backpressure keeps memory bounded while chunks stream to the chosen \
             destination. Oxfer reports success only after the received bytes match the sender's \
             BLAKE3 digest.",
        ),
    ];

    let compact = compact(ui);
    ui.add_space(if compact { 22.0 } else { 30.0 });
    egui::Frame::new()
        .fill(tc.surface_lowest)
        .stroke(Stroke::new(1.0, tc.outline_var))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(egui::Margin::same(if compact { 14 } else { 18 }))
        .show(ui, |ui| {
            egui::CollapsingHeader::new(
                RichText::new("Technical details")
                    .color(tc.on_surface)
                    .size(if compact { 20.0 } else { 22.0 })
                    .strong(),
            )
            .id_salt("oxfer_technical_details")
            .default_open(true)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "No accounts, no server-side file storage, and no plaintext relay.",
                        )
                        .color(tc.on_surface_var)
                        .size(14.0),
                    )
                    .wrap(),
                );
                ui.add_space(16.0);

                if compact {
                    for (number, title, body) in steps {
                        process_step(ui, tc, number, title, body);
                        ui.add_space(10.0);
                    }
                } else {
                    for pair in steps.chunks(2) {
                        ui.columns(2, |cols| {
                            for (column, (number, title, body)) in pair.iter().enumerate() {
                                process_step(&mut cols[column], tc, number, title, body);
                            }
                        });
                        ui.add_space(10.0);
                    }
                }

                ui.add_space(8.0);
                egui::Frame::new()
                    .fill(tc.bg)
                    .stroke(Stroke::new(1.0, tc.secondary))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(18))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            pill(ui, tc, "PRIVACY BOUNDARY", true);
                            ui.label(
                                RichText::new("Encrypted content, visible connection metadata")
                                    .color(tc.on_surface)
                                    .size(15.0)
                                    .strong(),
                            );
                        });
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(relay.privacy_boundary())
                                    .color(tc.on_surface_var)
                                    .size(13.0),
                            )
                            .wrap(),
                        );
                    });
            });
        });
    ui.add_space(18.0);
}

impl P2PTransfer {
    fn is_preparing_share(&self) -> bool {
        matches!(&self.mode, Mode::Send { preparing } if !preparing.is_empty())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_new_receive_link(&mut self) {
        if self.is_preparing_share() {
            return;
        }
        self.set_receive(
            FragmentParams::default(),
            Some(String::new()),
            None,
            None,
            false,
        );
    }

    /// Home and diagnostics do not revoke a share. Preserve its status and return path.
    fn show_active_share_overview(&mut self, ui: &mut Ui, tc: &Tc) {
        if !self.sharing.load(Ordering::Acquire) {
            return;
        }
        let mut return_to_share = false;
        card(tc).show(ui, |ui| {
            ui.label(
                RichText::new("Your files are still available")
                    .color(tc.on_surface)
                    .size(19.0)
                    .strong(),
            );
            #[cfg(target_arch = "wasm32")]
            Self::show_transfer_attention(ui, tc, true, false);
            return_to_share = ui.add(primary_button(tc, "Return to sharing")).clicked();
        });
        if return_to_share {
            self.return_to_sharing();
        }
        self.show_peers(ui);
        ui.add_space(12.0);
    }

    fn return_to_sharing(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if matches!(self.mode, Mode::Diagnostics) {
            if let Some(window) = web_sys::window() {
                if let Ok(history) = window.history() {
                    let _ =
                        history.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some("/"));
                }
            }
            self.last_fragment = None;
            self.stop_peer_diagnostics();
        }
        self.mode = Mode::Send {
            preparing: Vec::new(),
        };
    }

    fn show_home(&mut self, ui: &mut Ui) {
        const STEPS: [(HomeIcon, &str, &str); 3] = [
            (
                HomeIcon::File,
                "1. Pick a file",
                "Choose a photo, video, or document.",
            ),
            (
                HomeIcon::Link,
                "2. Send the link",
                "Send it to the person you want to share with.",
            ),
            (
                HomeIcon::Device,
                "3. They save it",
                "They open your link and choose where to save it.",
            ),
        ];

        let tc = Tc::of(self.theme, ui.visuals().dark_mode);
        let compact = compact(ui);
        self.show_active_share_overview(ui, &tc);
        ui.add_space(if compact { 4.0 } else { 16.0 });
        egui::Frame::new()
            .fill(tc.surface_low)
            .stroke(Stroke::new(1.0, tc.outline_var))
            .corner_radius(CornerRadius::same(20))
            .inner_margin(egui::Margin::same(if compact { 16 } else { 24 }))
            .show(ui, |ui| {
                ui.horizontal(|ui| pill(ui, &tc, "NO ACCOUNT NEEDED", true));
                ui.add_space(12.0);
                ui.add(
                    egui::Label::new(
                        RichText::new("Send files without cloud storage.")
                            .color(tc.on_surface)
                            .size(if compact { 26.0 } else { 34.0 })
                            .strong(),
                    )
                    .wrap(),
                );
                ui.add_space(10.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "Pick a file, send them a link, and they save it on their device.",
                        )
                        .color(tc.on_surface_var)
                        .size(if compact { 15.0 } else { 17.0 }),
                    )
                    .wrap(),
                );
                ui.add_space(12.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "Your file is encrypted while it travels. Oxfer doesn't store a copy \
                             on a server or in the cloud.",
                        )
                        .color(tc.on_surface)
                        .size(if compact { 14.0 } else { 16.0 }),
                    )
                    .wrap(),
                );
                ui.add_space(18.0);
                let width = if compact {
                    ui.available_width()
                } else {
                    ui.available_width().min(280.0)
                };
                if ui
                    .add_sized([width, 52.0], primary_button(&tc, "Send files"))
                    .clicked()
                {
                    self.pick_file();
                }
                ui.add_space(12.0);
                ui.add(
                    egui::Label::new(
                        RichText::new({
                            #[cfg(target_arch = "wasm32")]
                            {
                                "Got a link? Open it to see the file and choose where to save it."
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                "Got a link? Choose File → Open transfer link to paste it \
                                 and save the file."
                            }
                        })
                        .color(tc.on_surface_var)
                        .size(14.0),
                    )
                    .wrap(),
                );
            });
        ui.add_space(if compact { 16.0 } else { 24.0 });
        ui.label(
            RichText::new("From your device to theirs")
                .color(tc.on_surface)
                .size(21.0)
                .strong(),
        );
        ui.add_space(10.0);
        if compact {
            for (index, (icon, title, body)) in STEPS.into_iter().enumerate() {
                if index > 0 {
                    ui.add_space(8.0);
                }
                home_story_step(ui, &tc, icon, title, body);
            }
        } else {
            ui.columns(STEPS.len(), |cols| {
                for (column, (icon, title, body)) in STEPS.into_iter().enumerate() {
                    home_story_step(&mut cols[column], &tc, icon, title, body);
                }
            });
        }

        show_how_it_works(ui, &tc);
    }

    #[cfg(target_arch = "wasm32")]
    fn show_diagnostics(&mut self, ui: &mut Ui) {
        let tc = Tc::of(self.theme, ui.visuals().dark_mode);
        self.show_active_share_overview(ui, &tc);
        let link = Self::diagnostics_url().unwrap_or_default();
        let report = self.diagnostics_report.lock().unwrap().clone();
        let cancelled_websockets = logging::terminal_buffer()
            .lock()
            .map(|logs| {
                logs.iter()
                    .filter(|line| line.contains("WsMeta::connect future was dropped"))
                    .count()
            })
            .unwrap_or(0);
        let (target, peer_link, session, outcome, busy, completed) = {
            let peer = self.diagnostic_peer.lock().unwrap();
            (
                peer.target.is_some(),
                peer.link.clone(),
                peer.session.clone(),
                peer.outcome.clone(),
                peer.busy,
                peer.node.as_ref().map(|node| node.completed_probes()),
            )
        };
        card(&tc).show(ui, |ui| {
            ui.heading("Network diagnostics");
            ui.label("Use the peer test on both devices, then copy each report. Compare their session IDs and timestamps.");
            ui.add_space(8.0);
            if let Some(session) = &session {
                ui.label(format!("Session ID: {session}"));
            }
            if target {
                if ui.add_enabled(!busy, Button::new("Test peer connection")).clicked() {
                    self.run_peer_diagnostics();
                }
            } else if peer_link.is_none()
                && completed.is_none()
                && ui.add_enabled(!busy, Button::new("Start peer test")).clicked()
            {
                self.run_peer_diagnostics();
            }
            if let Some(peer_link) = &peer_link {
                ui.label("Keep this page open and send this test-only link to the other device. It cannot download files.");
                if ui.button("Copy peer test link").clicked() {
                    ui.ctx().copy_text(peer_link.clone());
                }
                ui.add(egui::Label::new(RichText::new(peer_link).monospace().size(12.0)).wrap());
                if ui.button("Stop peer test").clicked() {
                    self.stop_peer_diagnostics();
                }
            }
            if busy {
                ui.spinner();
                ui.label("Starting or dialling the diagnostic peer…");
            }
            if let Some(outcome) = &outcome {
                ui.label(outcome);
            }
            if let Some(completed) = completed {
                ui.label(format!("Successful incoming peer pings: {completed}"));
            }
            ui.add_space(10.0);
            ui.label("For local checks only, share this link. It contains no peer address or file access code.");
            if ui.button("Copy local checks link").clicked() && !link.is_empty() {
                ui.ctx().copy_text(link.clone());
            }
            ui.add(egui::Label::new(RichText::new(&link).monospace().size(12.0)).wrap());
            ui.add_space(12.0);
            if self.diagnostics_running.load(Ordering::Acquire) {
                ui.spinner();
                ui.label("Testing relay WebSockets and iroh registration (up to 60 seconds if the default fails)…");
            } else if ui.button("Run checks again").clicked() {
                self.run_diagnostics();
            }
            if let Some(report) = report {
                ui.add_space(12.0);
                ui.label("Review before sharing: the report contains your browser version, time, relay checks and peer test result. It does not include tickets or files.");
                let peer_report = format!(
                    "\nSession ID: {}\nPeer test: {}\nSuccessful incoming pings: {}\nCancelled relay WebSocket attempts logged: {}",
                    session.as_deref().unwrap_or("none"),
                    outcome.as_deref().unwrap_or("not run"),
                    completed.unwrap_or(0),
                    cancelled_websockets,
                );
                if ui.button("Copy report").clicked() {
                    ui.ctx().copy_text(format!("{report}{peer_report}"));
                }
                for line in format!("{report}{peer_report}").lines() {
                    ui.add(egui::Label::new(RichText::new(line).monospace().size(12.0)).wrap());
                }
            }
        });
    }

    fn show_send(&mut self, ui: &mut Ui) {
        let tc = Tc::of(self.theme, ui.visuals().dark_mode);
        let compact = compact(ui);
        let preparing_names: Vec<(String, f32)> = match &self.mode {
            Mode::Send { preparing } => preparing
                .iter()
                .map(|h| (h.name.clone(), *h.progress.borrow()))
                .collect(),
            _ => Vec::new(),
        };
        let ready = self.shared_files.lock().map(|f| f.len()).unwrap_or(0);
        let link = self.link.lock().ok().and_then(|l| l.clone());
        let node_error = self.node_error.lock().ok().and_then(|e| e.clone());
        let mut retry_share = false;

        card(&tc).show(ui, |ui| {
            if compact {
                ui.label(
                    RichText::new("Send a file")
                        .color(tc.on_surface)
                        .size(21.0)
                        .strong(),
                );
                pill(ui, &tc, "ENCRYPTED", true);
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Send a file")
                            .color(tc.on_surface)
                            .size(21.0)
                            .strong(),
                    );
                    pill(ui, &tc, "ENCRYPTED", true);
                });
            }
            ui.add_space(10.0);

            #[cfg(target_arch = "wasm32")]
            if self.sharing.load(Ordering::Acquire) || link.is_some() || !preparing_names.is_empty()
            {
                Self::show_transfer_attention(ui, &tc, true, false);
                ui.add_space(10.0);
            }

            if let Ok(files) = self.shared_files.lock() {
                for f in files.iter() {
                    if tc.theme == Theme::Rusty {
                        let file_row = |ui: &mut Ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&f.meta.name)
                                        .color(tc.on_surface)
                                        .size(15.0)
                                        .strong(),
                                )
                                .wrap(),
                            );
                            ui.label(
                                RichText::new(Self::format_size(f.meta.size))
                                    .color(tc.outline)
                                    .monospace()
                                    .size(12.0),
                            );
                        };
                        if compact {
                            ui.vertical(file_row);
                        } else {
                            ui.horizontal_wrapped(file_row);
                        }
                    } else {
                        file_attachment(
                            ui,
                            &tc,
                            &f.meta.name,
                            &Self::format_size(f.meta.size),
                            true,
                        );
                        ui.add_space(6.0);
                    }
                }
            }

            // The prepare phase (plan amendment 14): no link while any hash is provisional.
            for (name, pct) in &preparing_names {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("Preparing {name}… {:.0} %", pct * 100.0))
                        .color(tc.on_surface_var)
                        .size(14.0),
                );
                ui.add(egui::ProgressBar::new(*pct).desired_height(10.0));
            }

            ui.add_space(14.0);
            match (&link, preparing_names.is_empty()) {
                (Some(link), true) => {
                    ui.label(RichText::new("Send this link").color(tc.outline).size(12.0));
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .fill(tc.surface_lowest)
                        .corner_radius(CornerRadius::same(8))
                        .stroke(Stroke::new(1.0, tc.outline_var))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(link)
                                        .color(tc.on_surface)
                                        .monospace()
                                        .size(12.0),
                                )
                                .wrap()
                                .selectable(true),
                            );
                        });
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(NATIVE_LINK_ADDRESSES)
                                    .color(tc.on_surface_var)
                                    .size(12.0),
                            )
                            .wrap(),
                        );
                    }
                    ui.add_space(8.0);
                    if compact {
                        let width = ui.available_width();
                        if ui
                            .add_sized([width, 48.0], copy_button(&tc, self.link_copied))
                            .clicked()
                        {
                            #[cfg(target_arch = "wasm32")]
                            retry_transfer_wake_lock();
                            ui.ctx().copy_text(link.clone());
                            self.link_copied = true;
                        }
                        if ui
                            .add_sized([width, 48.0], outline_button("Stop sharing", tc.outline))
                            .clicked()
                        {
                            self.stop_sharing();
                        }
                    } else {
                        ui.horizontal(|ui| {
                            let copy_width = 212.0;
                            let stop_width = 140.0;
                            let row_width = copy_width + stop_width + ui.spacing().item_spacing.x;
                            ui.add_space(((ui.available_width() - row_width) / 2.0).max(0.0));
                            if ui
                                .add_sized([copy_width, 48.0], copy_button(&tc, self.link_copied))
                                .clicked()
                            {
                                #[cfg(target_arch = "wasm32")]
                                retry_transfer_wake_lock();
                                ui.ctx().copy_text(link.clone());
                                self.link_copied = true;
                            }
                            if ui
                                .add_sized(
                                    [stop_width, 48.0],
                                    outline_button("Stop sharing", tc.outline),
                                )
                                .clicked()
                            {
                                self.stop_sharing();
                            }
                        });
                    }
                    if self.link_copied {
                        ui.horizontal_wrapped(|ui| {
                            pill(ui, &tc, "COPIED", true);
                            ui.label(
                                RichText::new("Send this link to the person you chose.")
                                    .color(tc.secondary)
                                    .size(13.0)
                                    .strong(),
                            );
                        });
                    }
                    ui.add_space(10.0);
                    egui::Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(
                            tc.primary.r(),
                            tc.primary.g(),
                            tc.primary.b(),
                            14,
                        ))
                        .stroke(Stroke::new(1.0, tc.outline_var))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(LINK_WARNING).color(tc.error).size(13.0),
                                )
                                .wrap(),
                            );
                        });
                }
                (_, false) => {
                    ui.label(
                        RichText::new("Getting your link ready…")
                            .color(tc.outline)
                            .size(12.0),
                    );
                }
                (None, true) if ready > 0 && self.sharing.load(Ordering::Acquire) => {
                    ui.label(
                        RichText::new("Getting your link ready…")
                            .color(tc.outline)
                            .size(12.0),
                    );
                    ui.add(egui::Spinner::new().size(20.0).color(tc.secondary));
                }
                (None, true) if ready > 0 => {
                    retry_share = if compact {
                        let width = ui.available_width();
                        ui.add_sized([width, 48.0], outline_button("Try again", tc.secondary))
                            .clicked()
                    } else {
                        ui.add(outline_button("Try again", tc.secondary)).clicked()
                    };
                }
                _ => {}
            }

            if let Some(err) = &node_error {
                ui.add_space(8.0);
                ui.label(RichText::new(err).color(tc.error).size(12.0));
            }
        });

        if retry_share {
            if let Ok(mut error) = self.node_error.lock() {
                *error = None;
            }
            self.start_sharing();
        }
        self.show_peers(ui);
    }

    /// Per-peer progress for whatever this node is currently serving.
    fn show_peers(&mut self, ui: &mut Ui) {
        let tc = Tc::of(self.theme, ui.visuals().dark_mode);
        let peers = self
            .node
            .lock()
            .ok()
            .and_then(|n| n.as_ref().map(|n| n.peers()))
            .unwrap_or_default();
        if peers.is_empty() {
            return;
        }
        ui.add_space(12.0);
        card(&tc).show(ui, |ui| {
            ui.label(
                RichText::new(format!("File activity ({})", peers.len()))
                    .color(tc.on_surface)
                    .size(15.0)
                    .strong(),
            );
            #[cfg(target_arch = "wasm32")]
            if peers.iter().any(|(_, p)| !p.phase.is_terminal()) {
                ui.add_space(8.0);
                Self::show_transfer_attention(ui, &tc, true, false);
            }
            for (id, p) in peers {
                ui.add_space(8.0);
                let short = id.to_string();
                let short = short.get(..12).unwrap_or(&short).to_string();
                let peer_row = |ui: &mut Ui| {
                    ui.label(
                        RichText::new(&short)
                            .color(tc.on_surface_var)
                            .monospace()
                            .size(12.0),
                    );
                    ui.label(
                        RichText::new(Self::phase_text(&p.phase))
                            .color(tc.outline)
                            .size(12.0),
                    );
                    ui.label(
                        RichText::new(Self::path_badge(&p.phase, p.path))
                            .color(tc.secondary)
                            .size(12.0),
                    );
                    if let Some(speed) = Self::transfer_speed(&p) {
                        ui.label(RichText::new(speed).color(tc.outline).size(12.0));
                    }
                };
                if compact(ui) {
                    ui.vertical(peer_row);
                } else {
                    ui.horizontal_wrapped(peer_row);
                }
                if p.bytes_total > 0 {
                    let frac = p.bytes_done as f32 / p.bytes_total as f32;
                    ui.add(egui::ProgressBar::new(frac).desired_height(10.0));
                    ui.horizontal_wrapped(|ui| {
                        if let Some(name) = &p.file_name {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(name).color(tc.on_surface_var).size(12.0),
                                )
                                .wrap(),
                            );
                        }
                        ui.label(
                            RichText::new(format!(
                                "{} / {}",
                                Self::format_size(p.bytes_done),
                                Self::format_size(p.bytes_total)
                            ))
                            .color(tc.outline)
                            .monospace()
                            .size(11.0),
                        );
                    });
                }
                if let Some(err) = &p.error {
                    ui.label(RichText::new(err).color(tc.error).size(11.0));
                    // The engine refuses a file whose bytes no longer match the hash it
                    // published; only the sender can resolve that, so say how.
                    if err.contains("changed since it was shared") {
                        ui.label(
                            RichText::new("Remove and re-add the file to share the new version.")
                                .color(tc.outline)
                                .size(11.0),
                        );
                    }
                }
            }
        });
    }

    fn show_receive(&mut self, ui: &mut Ui) {
        let tc = Tc::of(self.theme, ui.visuals().dark_mode);
        let compact = compact(ui);
        let progress = match &self.mode {
            Mode::Receive(r) => r.handle.as_ref().map(|h| h.latest()),
            _ => None,
        };
        let error = match &self.mode {
            Mode::Receive(r) => r.error.clone(),
            _ => None,
        };
        let flags = match &self.mode {
            Mode::Receive(r) => Self::flag_summary(&r.params),
            _ => String::new(),
        };
        let save_pending = match &self.mode {
            Mode::Receive(r) => r.save_pending.load(Ordering::Acquire),
            _ => false,
        };
        let opening = matches!(&self.mode, Mode::Receive(r) if r.opening);

        let mut submit = false;
        let mut save_manifest: Option<Vec<FileMeta>> = None;
        let mut cancel = false;

        card(&tc).show(ui, |ui| {
            if compact {
                ui.label(
                    RichText::new("Receive on this device")
                        .color(tc.on_surface)
                        .size(21.0)
                        .strong(),
                );
                pill(ui, &tc, "ENCRYPTED", true);
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Receive on this device")
                            .color(tc.on_surface)
                            .size(21.0)
                            .strong(),
                    );
                    pill(ui, &tc, "ENCRYPTED", true);
                });
            }
            ui.add_space(10.0);

            #[cfg(target_arch = "wasm32")]
            if opening || progress.as_ref().is_some_and(|p| !p.phase.is_terminal()) {
                let awaiting_save = matches!(
                    progress.as_ref().map(|p| &p.phase),
                    Some(Phase::AwaitingSave { .. })
                ) && !save_pending;
                Self::show_transfer_attention(ui, &tc, false, awaiting_save);
                ui.add_space(10.0);
            }

            if progress.is_none() && opening {
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::Spinner::new().size(24.0).color(tc.secondary));
                    ui.label(
                        RichText::new("Connecting to the sender…")
                            .color(tc.on_surface)
                            .size(15.0)
                            .strong(),
                    );
                });
                ui.label(
                    RichText::new(
                        "Looking for the device sharing this file. This can take a moment.",
                    )
                    .color(tc.on_surface_var)
                    .size(13.0),
                );
                ui.add_space(10.0);
                cancel = if compact {
                    let width = ui.available_width();
                    ui.add_sized([width, 48.0], outline_button("Cancel", tc.outline))
                        .clicked()
                } else {
                    ui.add(outline_button("Cancel", tc.outline))
                        .clicked()
                };
            } else if progress.is_none() {
                ui.add(
                    egui::Label::new(
                        RichText::new("Paste the full link someone sent you.")
                            .color(tc.on_surface_var)
                            .size(14.0),
                    )
                    .wrap(),
                );
                ui.add_space(6.0);
                if let Mode::Receive(r) = &mut self.mode {
                    egui::Frame::new()
                        .fill(tc.surface_lowest)
                        .corner_radius(CornerRadius::same(10))
                        .stroke(Stroke::new(1.0, tc.outline_var))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.add_sized(
                                [ui.available_width(), 48.0],
                                egui::TextEdit::singleline(&mut r.input)
                                    .hint_text("Paste a link here")
                                    .frame(egui::Frame::new())
                                    .font(egui::FontId::monospace(14.0))
                                    .text_color(tc.on_surface)
                                    .desired_width(f32::INFINITY),
                            );
                        });
                }
                ui.add_space(10.0);
                submit = if compact {
                    let width = ui.available_width();
                    ui.add_sized([width, 48.0], primary_button(&tc, "Open link"))
                        .clicked()
                } else {
                    ui.add(primary_button(&tc, "Open link"))
                        .clicked()
                };
            }

            if let Some(p) = &progress {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(Self::phase_text(&p.phase))
                            .color(tc.on_surface)
                            .size(15.0)
                            .strong(),
                    );
                    pill(ui, &tc, Self::path_badge(&p.phase, p.path), true);
                    if let Some(speed) = Self::transfer_speed(p) {
                        ui.label(RichText::new(speed).color(tc.outline).size(12.0));
                    }
                });
                if let Phase::Reconnecting {
                    attempt,
                    max_attempts,
                } = p.phase
                {
                    ui.label(
                        RichText::new(format!(
                            "Trying again ({attempt} of {max_attempts}). Keep the sender's tab open. We'll try to continue where we stopped."
                        ))
                        .color(tc.on_surface_var)
                        .size(13.0),
                    );
                }

                if let Phase::AwaitingSave { manifest } = &p.phase {
                    ui.add_space(10.0);
                    for meta in manifest {
                        if tc.theme == Theme::Rusty {
                            let manifest_row = |ui: &mut Ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&meta.name)
                                            .color(tc.on_surface)
                                            .size(14.0)
                                            .strong(),
                                    )
                                    .wrap(),
                                );
                                ui.label(
                                    RichText::new(Self::format_size(meta.size))
                                        .color(tc.outline)
                                        .monospace()
                                        .size(12.0),
                                );
                            };
                            if compact {
                                ui.vertical(manifest_row);
                            } else {
                                ui.horizontal_wrapped(manifest_row);
                            }
                        } else {
                            file_attachment(
                                ui,
                                &tc,
                                &meta.name,
                                &Self::format_size(meta.size),
                                false,
                            );
                            ui.add_space(6.0);
                        }
                    }
                    show_report_abuse(ui, &tc);
                    ui.add_space(12.0);
                    ui.label("Oxfer checks the file after it's saved.");
                    #[cfg(target_arch = "wasm32")]
                    ui.add_enabled_ui(!save_pending, |ui| {
                        ui.checkbox(
                            &mut self.keep_local_copy,
                            "Keep a copy here so I can continue later",
                        );
                        if self.keep_local_copy {
                            ui.label(
                                RichText::new(
                                    "This copy is stored in your browser, not in Downloads yet. \
                                     If the transfer stops, reopen the sender's link to continue. \
                                     Download the finished file below. Clearing browser data, \
                                     private browsing, or running low on storage may remove it.",
                                )
                                .color(tc.on_surface_var)
                                .size(12.0),
                            );
                        }
                    });
                    #[cfg(target_arch = "wasm32")]
                    let save_label = if self.keep_local_copy {
                        "Save a copy in this browser"
                    } else {
                        "Choose where to save"
                    };
                    #[cfg(not(target_arch = "wasm32"))]
                    let save_label = "Choose where to save";
                    let save = if compact {
                        let width = ui.available_width();
                        ui.add_enabled_ui(!save_pending, |ui| {
                            ui.add_sized(
                                [width, 48.0],
                                primary_button(&tc, save_label),
                            )
                            .clicked()
                        })
                        .inner
                    } else {
                        ui.add_enabled(
                            !save_pending,
                            primary_button(&tc, save_label),
                        )
                        .clicked()
                    };
                    if save {
                        save_manifest = Some(manifest.clone());
                    }
                    if save_pending {
                        #[cfg(target_arch = "wasm32")]
                        let preparing = if self.keep_local_copy {
                            "Checking your saved copy…"
                        } else {
                            "Opening your save location…"
                        };
                        #[cfg(not(target_arch = "wasm32"))]
                        let preparing = "Opening your save location…";
                        ui.label(
                            RichText::new(preparing)
                                .color(tc.outline)
                                .size(12.0),
                        );
                    }
                } else if p.bytes_total > 0 {
                    ui.add_space(10.0);
                    let frac = p.bytes_done as f32 / p.bytes_total as f32;
                    ui.add(egui::ProgressBar::new(frac).desired_height(12.0));
                    ui.label(
                        RichText::new(format!(
                            "{} of {}",
                            Self::format_size(p.bytes_done),
                            Self::format_size(p.bytes_total)
                        ))
                        .color(tc.outline)
                        .size(12.0),
                    );
                }

                if !p.phase.is_terminal() {
                    ui.add_space(10.0);
                    cancel = if compact {
                        let width = ui.available_width();
                        ui.add_sized([width, 48.0], outline_button("Cancel", tc.outline))
                            .clicked()
                    } else {
                        ui.add(outline_button("Cancel", tc.outline)).clicked()
                    };
                }
            }

            if !flags.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new(&flags).color(tc.outline_var).size(11.0));
            }
            if let Some(err) = &error {
                ui.add_space(10.0);
                ui.label(RichText::new(err).color(tc.error).size(13.0));
            }
        });

        if submit {
            let input = match &self.mode {
                Mode::Receive(r) => r.input.clone(),
                _ => String::new(),
            };
            self.start_receive(&input);
        }
        if let Some(manifest) = save_manifest {
            self.save_click(manifest);
        }
        if cancel {
            if let Mode::Receive(r) = &mut self.mode {
                // Dropping the handle cancels the session, which then aborts its sinks,
                // closes the connection and shuts its node down on its own.
                r.opening = false;
                r.startup = Arc::default();
                r.handle = None;
            }
            #[cfg(target_arch = "wasm32")]
            {
                self.keep_local_copy = false;
                self.resume_target = None;
                self.local_copies
                    .run(LocalCopyAction::Refresh, self.repaint.clone());
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn show_local_copies(&mut self, ui: &mut Ui) {
        let entries = self.local_copies.entries.lock().unwrap().clone();
        let error = self.local_copies.error.lock().unwrap().clone();
        if entries.is_empty() && error.is_none() {
            return;
        }
        let tc = Tc::of(self.theme, ui.visuals().dark_mode);
        let busy = self.local_copies.busy.load(Ordering::Acquire);
        let receiving = matches!(&self.mode, Mode::Receive(r) if r.opening || r.handle.is_some());
        let mut action = None;
        let mut resume = None;
        ui.add_space(12.0);
        card(&tc).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("Saved files in this browser")
                        .color(tc.on_surface)
                        .size(21.0)
                        .strong(),
                );
                if ui
                    .add_enabled(!busy, outline_button("Refresh", tc.outline))
                    .clicked()
                {
                    action = Some(LocalCopyAction::Refresh);
                }
                if busy {
                    ui.spinner();
                }
            });
            ui.label(
                RichText::new(
                    "To continue an unfinished download, open the sender's link again. \
                     If they closed the app, ask for a new link to the same files. These copies \
                     stay in this browser, not on a server. They may disappear if browser data \
                     is cleared or space runs low. Links aren't saved here.",
                )
                .color(tc.on_surface_var)
                .size(13.0),
            );
            for entry in &entries {
                ui.separator();
                for (index, file) in entry.files.iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&file.name).strong());
                        ui.label(format!(
                            "{} / {}",
                            Self::format_size(file.written),
                            Self::format_size(file.size),
                        ));
                        if file.verified {
                            pill(ui, &tc, "READY TO SAVE", true);
                            if ui
                                .add_enabled(
                                    !busy && !receiving,
                                    outline_button("Download file", tc.secondary),
                                )
                                .clicked()
                            {
                                action = Some(LocalCopyAction::Export(entry.id.clone(), index));
                            }
                        } else {
                            pill(ui, &tc, "UNFINISHED", false);
                        }
                    });
                }
                ui.add_enabled_ui(!busy && !receiving, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if entry.files.iter().any(|f| !f.verified)
                            && ui
                                .add(outline_button("Continue download", tc.secondary))
                                .clicked()
                        {
                            resume = Some(entry.id.clone());
                        }
                        if self.confirm_discard.as_ref() == Some(&entry.id) {
                            ui.label("Delete this copy and its saved progress?");
                            if ui
                                .add(outline_button("Delete permanently", tc.error))
                                .clicked()
                            {
                                action = Some(LocalCopyAction::Discard(entry.id.clone()));
                                self.confirm_discard = None;
                            }
                            if ui.add(outline_button("Keep", tc.outline)).clicked() {
                                self.confirm_discard = None;
                            }
                        } else if ui
                            .add(outline_button("Delete saved copy", tc.outline))
                            .clicked()
                        {
                            self.confirm_discard = Some(entry.id.clone());
                        }
                    });
                });
            }
            if !entries.is_empty() {
                ui.separator();
                show_report_abuse(ui, &tc);
            }
            if let Some(error) = &error {
                ui.label(RichText::new(error).color(tc.error).size(13.0));
            }
        });
        if let Some(id) = resume {
            self.set_receive(
                FragmentParams::default(),
                Some(String::new()),
                None,
                None,
                false,
            );
            self.keep_local_copy = true;
            self.resume_target = Some(id);
        }
        if let Some(action) = action {
            self.local_copies.run(action, self.repaint.clone());
        }
    }

    fn show_received_files(&mut self, ui: &mut Ui) {
        let tc = Tc::of(self.theme, ui.visuals().dark_mode);
        let compact = compact(ui);
        let Ok(files) = self.received_files.lock() else {
            return;
        };
        if files.is_empty() {
            return;
        }
        ui.add_space(12.0);
        card(&tc).show(ui, |ui| {
            ui.label(
                RichText::new(format!("Received ({})", files.len()))
                    .color(tc.on_surface)
                    .size(15.0)
                    .strong(),
            );
            for f in files.iter() {
                ui.add_space(10.0);
                if tc.theme == Theme::Rusty {
                    let file_row = |ui: &mut Ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&f.name)
                                    .color(tc.on_surface)
                                    .size(14.0)
                                    .strong(),
                            )
                            .wrap(),
                        );
                        ui.label(
                            RichText::new(Self::format_size(f.size))
                                .color(tc.outline)
                                .monospace()
                                .size(12.0),
                        );
                        ui.label(RichText::new(&f.when).color(tc.outline_var).size(11.0));
                    };
                    if compact {
                        ui.vertical(file_row);
                    } else {
                        ui.horizontal_wrapped(file_row);
                    }
                } else {
                    file_attachment(
                        ui,
                        &tc,
                        &f.name,
                        &format!("{} · {}", Self::format_size(f.size), f.when),
                        false,
                    );
                }
                ui.label(
                    RichText::new(&f.location)
                        .color(tc.on_surface_var)
                        .size(11.0),
                );
                if let Some(rate) = f.average_bytes_per_sec {
                    let speed = if rate < 1.0 {
                        format!("{rate:.2} B/s")
                    } else {
                        format!("{}/s", Self::format_size(rate as u64))
                    };
                    ui.label(
                        RichText::new(format!("Avg download (this session): {speed}"))
                            .color(tc.on_surface_var)
                            .size(12.0),
                    );
                }
                pill(ui, &tc, Self::transfer_path_text(f.path), true);
            }
            ui.add_space(10.0);
            show_report_abuse(ui, &tc);
        });
    }

    fn header_logos(&mut self, ctx: &egui::Context) -> &HeaderLogos {
        // PNGs are 256px-tall rasters of `assets/oxfer-wordmark-{light,dark}.svg`.
        self.header_logos.get_or_insert_with(|| HeaderLogos {
            light: bundled_png_texture(
                ctx,
                "oxfer-wordmark-light",
                include_bytes!("../assets/oxfer-wordmark-light.png"),
            ),
            dark: bundled_png_texture(
                ctx,
                "oxfer-wordmark-dark",
                include_bytes!("../assets/oxfer-wordmark-dark.png"),
            ),
        })
    }

    fn show_header(&mut self, ui: &mut Ui, ctx: &egui::Context, tc: &Tc) {
        let compact = ui.available_width() < 680.0;
        ui.set_height(64.0);
        ui.horizontal_centered(|ui| {
            {
                let dark = ui.visuals().dark_mode;
                let logos = self.header_logos(ctx);
                let logo = if dark { &logos.dark } else { &logos.light };
                let height = if compact { 20.0 } else { 22.0 };
                let max_width = if compact { 120.0 } else { 148.0 };
                let width = (height * logo.aspect_ratio()).min(max_width);
                ui.add(
                    egui::Image::from_texture(logo)
                        .fit_to_exact_size(egui::vec2(width, width / logo.aspect_ratio()))
                        .alt_text("Oxfer"),
                );
            }
            if !compact
                && ui
                    .add(
                        Button::new(
                            RichText::new("i")
                                .color(tc.secondary)
                                .monospace()
                                .strong()
                                .size(16.0),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::new(1.0, tc.outline))
                        .corner_radius(CornerRadius::same(16))
                        .min_size(egui::vec2(32.0, 44.0)),
                    )
                    .on_hover_text("Show this build's version in Terminal Output")
                    .clicked()
            {
                self.show_terminal_view = true;
            }
            let at_home = matches!(self.mode, Mode::Home);
            let label = match self.mode {
                Mode::Home if self.sharing.load(Ordering::Acquire) => "SHARING",
                Mode::Home => "READY",
                #[cfg(target_arch = "wasm32")]
                Mode::Diagnostics => "DIAG",
                Mode::Send { .. } => "TX",
                Mode::Receive(_) => "RX",
            };
            if !compact {
                ui.add_space(8.0);
                pill(ui, tc, label, matches!(self.mode, Mode::Receive(_)));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(CONTROL_EDGE_INSET);
                let dark = ui.visuals().dark_mode;
                if !compact
                    && ui
                        .add_sized(
                            [108.0, 44.0],
                            outline_button(
                                if dark { "Light mode" } else { "Dark mode" },
                                tc.outline,
                            ),
                        )
                        .clicked()
                {
                    let next_dark = !dark;
                    Self::apply_theme(ctx, ui, self.theme, next_dark);
                    self.last_dark_mode = Some(next_dark);
                }
                ui.menu_button("Theme", |ui| {
                    ui.label(RichText::new("Appearance").strong());
                    let rusty = ui
                        .selectable_value(&mut self.theme, Theme::Rusty, "Rusty")
                        .clicked();
                    let clean = ui
                        .selectable_value(&mut self.theme, Theme::Clean, "Clean")
                        .clicked();
                    if rusty || clean {
                        // The user's own choice: the only write the browser build makes
                        // (ePrivacy art. 5(3)). Written now, as eframe saves nothing there.
                        #[cfg(target_arch = "wasm32")]
                        eframe::web::storage::local_storage_set(
                            BROWSER_THEME_KEY,
                            self.theme.storage_value(),
                        );
                        self.last_theme = None;
                        ctx.request_repaint();
                        ui.close();
                    }
                    if compact
                        && ui
                            .button(if dark { "Light mode" } else { "Dark mode" })
                            .clicked()
                    {
                        let next_dark = !dark;
                        Self::apply_theme(ctx, ui, self.theme, next_dark);
                        self.last_dark_mode = Some(next_dark);
                        ui.close();
                    }
                    if compact && ui.button("Show build version").clicked() {
                        self.show_terminal_view = true;
                        ui.close();
                    }
                });
                if !compact
                    && !at_home
                    && !matches!(self.mode, Mode::Receive(_))
                    && ui.add(primary_button(tc, "Choose File")).clicked()
                {
                    self.pick_file();
                }
                let preparing = self.is_preparing_share();
                if !at_home {
                    // Dropping Mode::Send also drops in-flight hashing tasks. Do not let Home
                    // silently abort file preparation while the endpoint remains live.
                    let home = ui
                        .add_enabled_ui(!preparing, |ui| {
                            ui.add_sized(
                                [if compact { 52.0 } else { 84.0 }, 44.0],
                                outline_button("Home", tc.outline),
                            )
                        })
                        .inner
                        .on_disabled_hover_text("Wait for files to finish preparing");
                    if home.clicked() {
                        #[cfg(target_arch = "wasm32")]
                        {
                            if matches!(self.mode, Mode::Diagnostics) {
                                if let Some(window) = web_sys::window() {
                                    if let Ok(history) = window.history() {
                                        let _ = history.push_state_with_url(
                                            &wasm_bindgen::JsValue::NULL,
                                            "",
                                            Some("/"),
                                        );
                                    }
                                    self.last_fragment = None;
                                }
                            }
                            self.stop_peer_diagnostics();
                        }
                        self.mode = Mode::Home;
                        #[cfg(target_arch = "wasm32")]
                        {
                            self.keep_local_copy = false;
                            self.resume_target = None;
                        }
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.menu_button("File", |ui| {
                        if ui
                            .add_enabled(!preparing, Button::new("Open transfer link"))
                            .on_disabled_hover_text("Wait for files to finish preparing")
                            .clicked()
                        {
                            self.open_new_receive_link();
                            ui.close();
                        }
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                }
                #[cfg(target_arch = "wasm32")]
                let _ = ctx;
            });
        });
    }

    fn show_terminal(&mut self, ui: &mut Ui, tc: &Tc) {
        let compact = ui.available_width() < 620.0;
        let narrow_controls = ui.available_width() < 340.0;
        let inline_links = ui.available_width() >= FOOTER_INLINE_LINKS_MIN_WIDTH;
        ui.horizontal(|ui| {
            ui.set_height(50.0);
            ui.add_space(CONTROL_EDGE_INSET);
            let chevron = if self.show_terminal_view {
                "▼"
            } else {
                "▲"
            };
            if ui
                .add(
                    Button::new(
                        RichText::new(if narrow_controls {
                            format!("{chevron} Logs >_")
                        } else {
                            format!("{chevron} Terminal Output >_")
                        })
                        .color(tc.secondary)
                        .monospace()
                        .size(13.0),
                    )
                    .fill(Color32::TRANSPARENT),
                )
                .on_hover_text("Terminal Output")
                .clicked()
            {
                self.show_terminal_view = !self.show_terminal_view;
            }
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(CONTROL_EDGE_INSET);
                #[cfg(target_arch = "wasm32")]
                {
                    let diagnostics = ui
                        .add_enabled_ui(!self.is_preparing_share(), |ui| {
                            ui.add_sized(
                                [bar_button_width(narrow_controls), 44.0],
                                bar_button("Diags", tc),
                            )
                        })
                        .inner
                        .on_disabled_hover_text("Wait for files to finish preparing");
                    if diagnostics.clicked() {
                        self.open_diagnostics();
                    }
                }
                // Every screen keeps this bar, so the legal and source links are always
                // one tap away (compliance plan B6).
                show_footer_links(ui, tc, inline_links, bar_button_width(narrow_controls));
                if !compact && !self.show_terminal_view {
                    ui.add_space(12.0);
                    if let Ok(logs) = logging::terminal_buffer().lock() {
                        let msg = logs
                            .back()
                            .cloned()
                            .unwrap_or_else(|| "No logs yet…".into());
                        ui.add(
                            egui::Label::new(
                                RichText::new(msg).color(tc.outline).monospace().size(12.0),
                            )
                            .truncate(),
                        );
                    }
                }
            });
        });

        if self.show_terminal_view {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("Build {}", crate::BUILD_LABEL))
                            .color(tc.secondary)
                            .monospace()
                            .size(12.0),
                    );
                    ui.label(
                        RichText::new(format!("Commit time: {}", crate::BUILD_COMMITTED_AT))
                            .color(tc.secondary)
                            .monospace()
                            .size(12.0),
                    );
                    match logging::terminal_buffer().lock() {
                        Ok(logs) if !logs.is_empty() => {
                            for line in logs.iter() {
                                ui.label(
                                    RichText::new(line)
                                        .color(tc.on_surface_var)
                                        .monospace()
                                        .size(12.0),
                                );
                            }
                        }
                        _ => {
                            ui.label(
                                RichText::new("No logs yet…")
                                    .color(tc.outline_var)
                                    .monospace()
                                    .size(12.0),
                            );
                        }
                    }
                });
        }
    }
}

// ── eframe glue ──────────────────────────────────────────────────────────────

impl eframe::App for P2PTransfer {
    /// Native: the theme and save folder, in eframe's settings file.
    #[cfg(not(target_arch = "wasm32"))]
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Browser: nothing. eframe's storage there is localStorage, and under ePrivacy art. 5(3)
    /// the browser build stores only what the user asked for: the theme key, written when
    /// they pick one (see `BROWSER_THEME_KEY`).
    #[cfg(target_arch = "wasm32")]
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}

    /// Native keeps eframe's default (window and widget state on disk). The browser would
    /// write `egui_memory_ron` to localStorage, which the user never asked for.
    fn persist_egui_memory(&self) -> bool {
        cfg!(not(target_arch = "wasm32"))
    }

    /// Non-drawing per-frame work. Runs before `ui`, and may show nothing itself.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_arch = "wasm32")]
        self.check_fragment_change();

        self.drain_picks();
        self.prune_preparing();
        self.adopt_pending_handle();
        self.poll_receive();
        self.drain_error();

        #[cfg(target_arch = "wasm32")]
        set_transfer_wake_lock(self.needs_wake_lock());

        if self.needs_progress_poll() || {
            #[cfg(target_arch = "wasm32")]
            {
                matches!(self.mode, Mode::Diagnostics)
                    && self.diagnostic_peer.lock().unwrap().node.is_some()
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                false
            }
        } {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;

        // Apply the theme here, not in `logic()`: the root `Ui` is built before `logic()`
        // runs, so a context-only update would leave this frame with the previous palette.
        let dark = ctx.global_style().visuals.dark_mode;
        if self.last_dark_mode != Some(dark) || self.last_theme != Some(self.theme) {
            Self::apply_theme(ctx, ui, self.theme, dark);
            self.last_dark_mode = Some(dark);
            self.last_theme = Some(self.theme);
        }
        let tc = Tc::of(self.theme, dark);
        let compact = ui.available_width() < 680.0;

        let header_frame = egui::Frame::new()
            .fill(tc.surface)
            .stroke(Stroke::new(1.0_f32, tc.outline_var))
            .inner_margin(egui::Margin {
                left: if compact { 14 } else { 24 },
                right: if compact { 14 } else { 24 },
                top: 0,
                bottom: 0,
            });
        egui::Panel::top("header")
            .exact_size(64.0)
            .frame(header_frame)
            .show(ui, |ui| {
                let content_width = ui.available_width().min(1040.0);
                ui.vertical_centered(|ui| {
                    ui.set_width(content_width);
                    self.show_header(ui, ctx, &tc);
                });
            });

        let terminal_frame = egui::Frame::new()
            .fill(tc.surface_lowest)
            .stroke(Stroke::new(1.0_f32, tc.outline_var))
            .inner_margin(egui::Margin {
                left: if compact { 12 } else { 20 },
                right: if compact { 12 } else { 20 },
                top: 0,
                bottom: 0,
            });
        let terminal_height = if self.show_terminal_view {
            if compact {
                180.0
            } else {
                220.0
            }
        } else {
            50.0
        };
        egui::Panel::bottom("terminal_bar")
            .exact_size(terminal_height)
            .frame(terminal_frame)
            .show(ui, |ui| {
                let content_width = ui.available_width().min(1040.0);
                ui.vertical_centered(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(content_width, ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| self.show_terminal(ui, &tc),
                    );
                });
            });

        let content_frame = egui::Frame::new().fill(tc.bg).inner_margin(egui::Margin {
            left: if compact { 14 } else { 28 },
            right: if compact { 14 } else { 28 },
            top: if compact { 12 } else { 20 },
            bottom: if compact { 16 } else { 24 },
        });
        egui::CentralPanel::default()
            .frame(content_frame)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let content_width = ui.available_width().min(1040.0);
                    ui.vertical_centered(|ui| {
                        ui.set_width(content_width);
                        match self.mode {
                            Mode::Home => self.show_home(ui),
                            #[cfg(target_arch = "wasm32")]
                            Mode::Diagnostics => self.show_diagnostics(ui),
                            Mode::Send { .. } => self.show_send(ui),
                            Mode::Receive(_) => self.show_receive(ui),
                        }
                        #[cfg(target_arch = "wasm32")]
                        if !matches!(self.mode, Mode::Diagnostics) {
                            self.show_received_files(ui);
                            self.show_local_copies(ui);
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        self.show_received_files(ui);
                    });
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_test_theme_choice_is_independent_of_light_and_dark_mode() {
        assert_eq!(P2PTransfer::default().theme, Theme::Clean);
        assert_eq!(Theme::from_storage_value("rusty"), Some(Theme::Rusty));
        assert_eq!(Theme::from_storage_value("clean"), Some(Theme::Clean));
        assert_eq!(Theme::from_storage_value("unknown"), None);
        let encoded = postcard::to_stdvec(&Theme::Clean).unwrap();
        let restored: Theme = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(restored, Theme::Clean);
        for dark in [false, true] {
            let rusty = Tc::of(Theme::Rusty, dark);
            let clean = Tc::of(Theme::Clean, dark);
            assert_ne!(rusty.primary, clean.primary);
            assert_ne!(rusty.bg, clean.bg);
        }
    }

    /// eframe settings held in memory: the browser's localStorage or the native file.
    #[derive(Default)]
    struct TestStorage(std::collections::HashMap<String, String>);

    impl eframe::Storage for TestStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn local_test_browser_theme_key_is_written_only_for_a_picked_theme() {
        use ThemeKeyChange::{Leave, Remove, Write};
        let default = Theme::default();
        assert_eq!(default, Theme::Clean);
        for (stored, legacy_theme, legacy_present, expected) in [
            // A first visit stores nothing.
            (None, None, false, (default, Leave)),
            // A theme the user picked in this build is used as is.
            (Some("rusty"), None, false, (Theme::Rusty, Leave)),
            (Some("clean"), None, false, (Theme::Clean, Leave)),
            // A pick from an older build wins over its periodically saved settings.
            (
                Some("rusty"),
                Some(Theme::Clean),
                true,
                (Theme::Rusty, Leave),
            ),
            // The build before this one wrote the key on every start, so a default value
            // next to legacy data may not be a choice: drop it (same theme either way).
            (
                Some("clean"),
                Some(Theme::Rusty),
                true,
                (Theme::Clean, Remove),
            ),
            (Some("clean"), None, true, (Theme::Clean, Remove)),
            // Only a non-default legacy theme was certainly picked, so only that migrates.
            (
                None,
                Some(Theme::Rusty),
                true,
                (Theme::Rusty, Write(Theme::Rusty)),
            ),
            (None, Some(Theme::Clean), true, (default, Leave)),
            // egui memory alone, or unreadable app settings, carry no theme.
            (None, None, true, (default, Leave)),
            // An unknown value is ignored, not deleted.
            (Some("neon"), None, false, (default, Leave)),
            (
                Some("neon"),
                Some(Theme::Rusty),
                true,
                (Theme::Rusty, Write(Theme::Rusty)),
            ),
        ] {
            assert_eq!(
                browser_theme_start(stored, legacy_theme, legacy_present),
                expected,
                "stored={stored:?} legacy={legacy_theme:?} present={legacy_present}"
            );
        }
        for theme in [Theme::Rusty, Theme::Clean] {
            assert_eq!(
                Theme::from_storage_value(theme.storage_value()),
                Some(theme)
            );
        }
    }

    #[test]
    fn local_test_legacy_browser_theme_reads_old_eframe_settings() {
        let mut storage = TestStorage::default();
        assert_eq!(legacy_browser_theme(&storage), None);
        for (saved, expected) in [
            // What older browser builds wrote: the wasm struct had only `theme`.
            ("(theme:Rusty)", Some(Theme::Rusty)),
            ("(theme:Clean)", Some(Theme::Clean)),
            // Settings from before themes existed mean the default.
            ("()", Some(Theme::Clean)),
            // Fields this build no longer knows are ignored.
            (
                r#"(save_directory:Some("/tmp/x"),theme:Rusty)"#,
                Some(Theme::Rusty),
            ),
            ("not ron", None),
            ("(theme:Neon)", None),
        ] {
            eframe::Storage::set_string(&mut storage, eframe::APP_KEY, saved.to_owned());
            assert_eq!(legacy_browser_theme(&storage), expected, "{saved}");
        }
        // A value eframe itself wrote for this app reads back.
        let app = P2PTransfer {
            theme: Theme::Rusty,
            ..P2PTransfer::default()
        };
        eframe::set_value(&mut storage, eframe::APP_KEY, &app);
        assert_eq!(legacy_browser_theme(&storage), Some(Theme::Rusty));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn local_test_native_keeps_eframe_persistence() {
        let mut app = P2PTransfer {
            theme: Theme::Rusty,
            save_directory: Some(std::path::PathBuf::from("/tmp/oxfer-receives")),
            ..P2PTransfer::default()
        };
        assert!(eframe::App::persist_egui_memory(&app));
        let mut storage = TestStorage::default();
        eframe::App::save(&mut app, &mut storage);
        let restored: P2PTransfer = eframe::get_value(&storage, eframe::APP_KEY).unwrap();
        assert_eq!(restored.theme, Theme::Rusty);
        assert_eq!(restored.save_directory, app.save_directory);
        assert_eq!(storage.0.len(), 1, "{:?}", storage.0.keys());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn local_test_theme_storage_restores_old_settings_and_new_choice() {
        let mut storage = TestStorage::default();
        eframe::Storage::set_string(
            &mut storage,
            eframe::APP_KEY,
            r#"(save_directory:Some("/tmp/oxfer-receives"))"#.to_owned(),
        );
        let old: P2PTransfer = eframe::get_value(&storage, eframe::APP_KEY).unwrap();
        assert_eq!(old.theme, Theme::Clean);
        assert_eq!(
            old.save_directory,
            Some(std::path::PathBuf::from("/tmp/oxfer-receives"))
        );

        let app = P2PTransfer {
            theme: Theme::Rusty,
            save_directory: old.save_directory,
            ..P2PTransfer::default()
        };
        eframe::set_value(&mut storage, eframe::APP_KEY, &app);
        let restored: P2PTransfer = eframe::get_value(&storage, eframe::APP_KEY).unwrap();
        assert_eq!(restored.theme, Theme::Rusty);
        assert_eq!(restored.save_directory, app.save_directory);
    }

    #[test]
    fn local_test_narrow_header_fits_send_and_receive_modes() {
        let width = 320.0;
        for theme in [Theme::Rusty, Theme::Clean] {
            for mode in [
                Mode::Send {
                    preparing: Vec::new(),
                },
                Mode::Receive(Box::new(ReceiveState {
                    input: String::new(),
                    params: FragmentParams::default(),
                    handle: None,
                    error: None,
                    opening: false,
                    startup: Arc::default(),
                    save_pending: Arc::new(AtomicBool::new(false)),
                })),
            ] {
                let ctx = egui::Context::default();
                let mut app = P2PTransfer {
                    theme,
                    mode,
                    ..P2PTransfer::default()
                };
                let mut measured = None;
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 640.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        ui.set_width(width - 28.0);
                        P2PTransfer::apply_theme(&ctx, ui, theme, false);
                        let tc = Tc::of(theme, false);
                        app.show_header(ui, &ctx, &tc);
                        measured = Some(ui.min_rect());
                    },
                );
                output.drop_without_applying_deltas();
                let rect = measured.unwrap();
                assert!(
                    rect.max.x <= width,
                    "{theme:?} header overflowed by {}px",
                    rect.max.x - width
                );
            }
        }
    }

    #[test]
    fn local_test_bundled_wordmarks_are_wide_rgba_pngs() {
        for (label, bytes) in [
            (
                "light",
                include_bytes!("../assets/oxfer-wordmark-light.png").as_slice(),
            ),
            (
                "dark",
                include_bytes!("../assets/oxfer-wordmark-dark.png").as_slice(),
            ),
        ] {
            let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
                .unwrap_or_else(|error| panic!("{label} wordmark must decode: {error}"))
                .into_rgba8();
            assert!(
                image.width() > image.height() * 5,
                "{label} is not a wordmark"
            );
            assert_eq!(
                image.height(),
                256,
                "{label} should be rasterized at 256px tall"
            );
        }
    }

    #[test]
    fn local_test_theme_spacing_persists_across_frames() {
        for theme in [Theme::Rusty, Theme::Clean] {
            let ctx = egui::Context::default();
            let first = ctx.run_ui(egui::RawInput::default(), |ui| {
                P2PTransfer::apply_theme(&ctx, ui, theme, false);
            });
            first.drop_without_applying_deltas();
            let second = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert_eq!(ui.spacing().button_padding, egui::vec2(18.0, 11.0));
                assert_eq!(ui.spacing().item_spacing, egui::vec2(10.0, 10.0));
                assert_eq!(ui.spacing().interact_size.y, 44.0);
            });
            second.drop_without_applying_deltas();
        }
    }

    #[test]
    fn local_test_build_label_identifies_compiled_revision() {
        assert_eq!(
            crate::BUILD_LABEL,
            format!("v{} · {}", env!("CARGO_PKG_VERSION"), crate::BUILD_REVISION)
        );
        assert!(
            crate::BUILD_REVISION == "unknown"
                || (crate::BUILD_REVISION.len() == 12
                    && crate::BUILD_REVISION
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit()))
        );
        assert!(
            crate::BUILD_COMMITTED_AT == "unknown"
                || (crate::BUILD_COMMITTED_AT.len() == 25
                    && crate::BUILD_COMMITTED_AT.as_bytes()[10] == b'T')
        );
    }

    #[test]
    fn local_test_share_link_excludes_sender_frame_diagnostics() {
        assert_eq!(
            P2PTransfer::share_base_url(
                "https://oxfer.pages.dev/?dcframe=64&theme=dark#ticket&cap=private"
            ),
            "https://oxfer.pages.dev/?theme=dark"
        );
        assert_eq!(
            P2PTransfer::share_base_url("https://oxfer.pages.dev/?dcframe=64#private"),
            "https://oxfer.pages.dev/"
        );
        assert_eq!(
            P2PTransfer::share_base_url(
                "https://oxfer.pages.dev/?tag=kept&d%63frame=64&other=also-kept#private"
            ),
            "https://oxfer.pages.dev/?tag=kept&other=also-kept"
        );
        assert_eq!(
            P2PTransfer::share_base_url("https://oxfer.pages.dev/#private"),
            "https://oxfer.pages.dev/"
        );
    }

    #[test]
    fn local_test_receiver_wake_lock_requires_active_session_or_save_attempt() {
        let preview = Phase::AwaitingSave { manifest: vec![] };
        let done = Phase::Complete { saved: vec![] };
        assert!(P2PTransfer::receive_needs_wake_lock(true, false, None));
        assert!(!P2PTransfer::receive_needs_wake_lock(false, true, None));
        assert!(!P2PTransfer::receive_needs_wake_lock(
            false,
            false,
            Some(&preview)
        ));
        assert!(P2PTransfer::receive_needs_wake_lock(
            false,
            true,
            Some(&preview)
        ));
        assert!(P2PTransfer::receive_needs_wake_lock(
            false,
            true,
            Some(&Phase::Transferring)
        ));
        assert!(!P2PTransfer::receive_needs_wake_lock(
            false,
            true,
            Some(&done)
        ));
    }

    #[test]
    fn local_test_transfer_speed_only_appears_while_transferring() {
        let mut progress = TransferProgress::connecting();
        progress.bytes_per_sec = 1_572_864.0;
        assert_eq!(P2PTransfer::transfer_speed(&progress), None);

        progress.phase = Phase::Transferring;
        assert_eq!(
            P2PTransfer::transfer_speed(&progress).as_deref(),
            Some("1.50 MB/s")
        );
        progress.bytes_per_sec = 0.0;
        assert_eq!(P2PTransfer::transfer_speed(&progress), None);
        progress.bytes_per_sec = f64::NAN;
        assert_eq!(P2PTransfer::transfer_speed(&progress), None);

        progress.bytes_per_sec = 1_572_864.0;
        progress.phase = Phase::Verifying;
        assert_eq!(P2PTransfer::transfer_speed(&progress), None);
        progress.phase = Phase::Complete { saved: Vec::new() };
        assert_eq!(P2PTransfer::transfer_speed(&progress), None);
    }

    #[test]
    fn local_test_reconnect_ui_does_not_claim_the_old_path_is_active() {
        let phase = Phase::Reconnecting {
            attempt: 2,
            max_attempts: 5,
        };
        assert_eq!(P2PTransfer::phase_text(&phase), "Connecting again…");
        assert_eq!(
            P2PTransfer::path_badge(&phase, TransferPath::Direct),
            "Connecting again",
        );
    }

    #[test]
    fn local_test_connection_labels_explain_paths_without_protocol_names() {
        assert_eq!(
            P2PTransfer::transfer_path_text(TransferPath::Direct),
            "Direct connection",
        );
        assert_eq!(
            P2PTransfer::transfer_path_text(TransferPath::Relayed),
            "Encrypted via a helper server",
        );
        assert_eq!(
            P2PTransfer::phase_text(&Phase::Signaling),
            "Finding a connection…",
        );
        assert_eq!(
            P2PTransfer::phase_text(&Phase::Complete { saved: Vec::new() }),
            "Transfer complete and checked",
        );
    }

    #[test]
    fn local_test_startup_keeps_polling_without_user_input() {
        let mut app = P2PTransfer::default();
        assert!(!app.needs_progress_poll());

        // Hashing may already be done while the endpoint is still opening.
        app.mode = Mode::Send { preparing: vec![] };
        app.sharing.store(true, Ordering::Release);
        assert!(app.needs_progress_poll());
        app.sharing.store(false, Ordering::Release);
        assert!(!app.needs_progress_poll());

        // The receiver has no TransferHandle until its bind task finishes.
        app.set_receive(FragmentParams::default(), None, None, None, true);
        assert!(app.needs_progress_poll());
    }

    #[test]
    fn local_test_receive_preserves_the_link_that_was_opened() {
        let mut app = P2PTransfer::default();
        let link = "https://oxfer.app/#ticket=invalid";
        app.start_receive(link);
        let Mode::Receive(r) = &app.mode else {
            panic!("expected receive panel");
        };
        assert_eq!(r.input, link);
        assert!(r.error.is_some());
        assert!(!r.opening);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn local_test_open_new_receive_link_clears_previous_input() {
        let mut app = P2PTransfer::default();
        app.set_receive(
            FragmentParams::default(),
            Some("previous private link".into()),
            None,
            None,
            false,
        );
        app.open_new_receive_link();
        let Mode::Receive(r) = &app.mode else {
            panic!("expected receive panel");
        };
        assert!(r.input.is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn local_test_open_new_receive_link_keeps_preparing_share() {
        let mut app = P2PTransfer::default();
        let (_, progress) = watch::channel(0.0);
        app.mode = Mode::Send {
            preparing: vec![PrepareHandle {
                name: "file".into(),
                progress,
                _task: AbortOnDropHandle::new(task::spawn(async {
                    std::future::pending::<()>().await;
                })),
            }],
        };

        app.open_new_receive_link();
        assert!(matches!(&app.mode, Mode::Send { preparing } if preparing.len() == 1));
    }

    #[test]
    fn local_test_receive_startup_failure_stops_spinner_and_keeps_input() {
        let mut app = P2PTransfer::default();
        let input = "a private link".to_string();
        app.set_receive(
            FragmentParams::default(),
            Some(input.clone()),
            None,
            None,
            true,
        );
        if let Mode::Receive(r) = &app.mode {
            *r.startup.lock().unwrap() = Some(Err("endpoint startup timed out".to_string()));
        }
        app.adopt_pending_handle();
        let Mode::Receive(r) = &app.mode else {
            panic!("expected receive panel");
        };
        assert_eq!(r.input, input);
        assert_eq!(r.error.as_deref(), Some("endpoint startup timed out"));
        assert!(!app.needs_progress_poll());
    }

    #[test]
    fn local_test_old_startup_cannot_replace_a_new_receive() {
        let mut app = P2PTransfer::default();
        app.set_receive(FragmentParams::default(), None, None, None, true);
        let old_slot = match &app.mode {
            Mode::Receive(r) => Arc::downgrade(&r.startup),
            _ => unreachable!(),
        };
        app.set_receive(FragmentParams::default(), None, None, None, true);
        assert!(old_slot.upgrade().is_none());
        app.adopt_pending_handle();
        assert!(app.needs_progress_poll());
    }

    #[test]
    fn local_test_save_attempt_blocks_duplicates_and_allows_retry() {
        let pending = Arc::new(AtomicBool::new(false));
        let attempt = SaveAttempt::begin(&pending).unwrap();
        assert!(pending.load(Ordering::Acquire));
        assert!(SaveAttempt::begin(&pending).is_none());

        // A picker error or dismissal drops the attempt without submitting a command.
        drop(attempt);
        assert!(!pending.load(Ordering::Acquire));
        assert!(SaveAttempt::begin(&pending).is_some());
    }

    #[test]
    fn local_test_submitted_save_stays_blocked_until_phase_changes() {
        let pending = Arc::new(AtomicBool::new(false));
        SaveAttempt::begin(&pending).unwrap().submitted();
        // There can still be a frame showing AwaitingSave before the engine consumes Save.
        assert!(SaveAttempt::begin(&pending).is_none());
    }

    #[test]
    fn local_test_old_save_attempt_cannot_change_new_receive_state() {
        let old_pending = Arc::new(AtomicBool::new(false));
        let old_attempt = SaveAttempt::begin(&old_pending).unwrap();
        let new_pending = Arc::new(AtomicBool::new(false));
        let new_attempt = SaveAttempt::begin(&new_pending).unwrap();

        drop(old_attempt);
        assert!(new_pending.load(Ordering::Acquire));
        drop(new_attempt);
        assert!(!new_pending.load(Ordering::Acquire));
    }

    #[test]
    fn local_test_footer_links_open_legal_pages_and_source() {
        let expected = [
            (FooterLink::Privacy, "/privacy", "https://oxfer.app/privacy"),
            (FooterLink::Terms, "/terms", "https://oxfer.app/terms"),
            (FooterLink::Abuse, "/abuse", "https://oxfer.app/abuse"),
            (
                FooterLink::Source,
                "https://github.com/radumarias/syncoxiders",
                "https://github.com/radumarias/syncoxiders",
            ),
        ];
        assert_eq!(
            FooterLink::ALL.to_vec(),
            expected.iter().map(|(link, ..)| *link).collect::<Vec<_>>()
        );
        for (link, web, native) in expected {
            assert_eq!(link.url(true), web, "{link:?} on the web");
            assert_eq!(link.url(false), native, "{link:?} in the desktop app");
            for url in [link.url(true), link.url(false)] {
                assert!(
                    !url.contains('#') && !url.contains('?'),
                    "{link:?} must not carry share state: {url}"
                );
            }
        }
        // The web app stays on the origin that served it; the desktop app has none.
        for link in [FooterLink::Privacy, FooterLink::Terms, FooterLink::Abuse] {
            assert!(link.url(true).starts_with('/') && !link.url(true).starts_with("//"));
            assert_eq!(
                link.url(false),
                format!("https://oxfer.app{}", link.url(true))
            );
        }
        assert_eq!(
            FooterLink::Privacy.target_url(),
            FooterLink::Privacy.url(cfg!(target_arch = "wasm32"))
        );
        let labels: Vec<&str> = FooterLink::ALL.iter().map(|link| link.label()).collect();
        assert_eq!(labels, ["Privacy", "Terms", "Abuse & safety", "Source"]);
    }

    /// WCAG 2 contrast ratio of two opaque colours.
    fn contrast_ratio(a: Color32, b: Color32) -> f32 {
        fn luminance(color: Color32) -> f32 {
            let channel = |value: u8| {
                let value = f32::from(value) / 255.0;
                if value <= 0.040_45 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
        }
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn local_test_footer_links_are_readable_in_every_theme() {
        for theme in [Theme::Rusty, Theme::Clean] {
            for dark in [false, true] {
                let tc = Tc::of(theme, dark);
                // Inline links and the "Legal" menu button sit on the bottom bar's fill.
                for (what, color) in [("link", tc.on_surface_var), ("menu", tc.outline)] {
                    let ratio = contrast_ratio(color, tc.surface_lowest);
                    assert!(
                        ratio >= 4.5,
                        "{theme:?} dark={dark} {what} contrast {ratio:.2} is below WCAG AA"
                    );
                }
            }
        }
    }

    #[test]
    fn local_test_relay_copy_names_who_runs_the_relay() {
        let custom = |urls: &[&str]| {
            RelayChoice::Custom(
                urls.iter()
                    .map(|url| url.parse::<iroh::RelayUrl>().unwrap())
                    .collect(),
            )
        };
        for relay in [
            custom(&["https://relay.example.test"]),
            custom(&[
                "https://relay.example.test",
                "https://relay2.example.test:8443",
            ]),
            RelayChoice::from_setting(Some("https://relay.example.test")),
        ] {
            assert_eq!(RelayOperator::of(&relay), RelayOperator::Oxfer, "{relay:?}");
        }
        for relay in [
            RelayChoice::N0,
            RelayChoice::N0WithoutTrailingDots,
            RelayChoice::from_setting(None),
            RelayChoice::from_setting(Some("  ")),
        ] {
            assert_eq!(RelayOperator::of(&relay), RelayOperator::N0, "{relay:?}");
        }
        assert_eq!(
            RelayOperator::of(&RelayChoice::None),
            RelayOperator::Disabled
        );
        assert_eq!(
            RelayOperator::current(),
            RelayOperator::of(&RelayChoice::from_env())
        );

        for operator in [
            RelayOperator::Oxfer,
            RelayOperator::N0,
            RelayOperator::Disabled,
        ] {
            assert_eq!(
                operator.privacy_boundary(),
                operator.privacy_boundary_for(cfg!(target_arch = "wasm32")),
                "{operator:?}: this build shows its own target's copy"
            );
            // Only the browser app is served by Cloudflare and uses its STUN server.
            let web = operator.privacy_boundary_for(true);
            assert!(
                web.starts_with("Cloudflare serves the web app shell and answers STUN requests"),
                "{operator:?}: {web}"
            );
            // The desktop app contacts neither, and its links carry the device's addresses
            // (compliance plan C6, B5).
            let desktop = operator.privacy_boundary_for(false);
            for absent in ["Cloudflare", "STUN", "web app shell"] {
                assert!(!desktop.contains(absent), "{operator:?}: {desktop}");
            }
            assert!(
                desktop.contains("desktop app") && desktop.contains("IP addresses"),
                "{operator:?}: {desktop}"
            );
            for boundary in [web, desktop] {
                assert!(boundary.contains("capability fragment"), "{operator:?}");
                assert!(
                    boundary.contains("learn each other's network address"),
                    "{operator:?}: the peer is named: {boundary}"
                );
            }
            assert!(operator.peer_path().contains("DTLS"), "{operator:?}");
        }
        let oxfer = [
            RelayOperator::Oxfer.peer_path(),
            RelayOperator::Oxfer.privacy_boundary_for(true),
            RelayOperator::Oxfer.privacy_boundary_for(false),
        ];
        for text in oxfer {
            assert!(text.contains("operated by Oxfer"), "{text}");
            assert!(
                !text.contains("n0"),
                "a custom relay build must not credit n0: {text}"
            );
        }
        for text in [
            RelayOperator::N0.peer_path(),
            RelayOperator::N0.privacy_boundary_for(true),
            RelayOperator::N0.privacy_boundary_for(false),
        ] {
            assert!(text.contains("n0.computer"), "{text}");
            assert!(
                !text.contains("Oxfer"),
                "n0's relays are not Oxfer's: {text}"
            );
        }
        for text in [
            RelayOperator::Disabled.peer_path(),
            RelayOperator::Disabled.privacy_boundary_for(true),
            RelayOperator::Disabled.privacy_boundary_for(false),
        ] {
            assert!(text.contains("no relay"), "{text}");
            assert!(!text.contains("n0") && !text.contains("operated by Oxfer"));
        }
    }

    /// Drives the whole app UI headlessly, frame by frame, reading widgets back from the
    /// accessibility tree the way a screen reader or a tester's pointer would find them.
    #[cfg(not(target_arch = "wasm32"))]
    struct UiHarness {
        ctx: egui::Context,
        app: P2PTransfer,
        frame: eframe::Frame,
        size: egui::Vec2,
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[derive(Debug)]
    struct Widget {
        role: egui::accesskit::Role,
        text: String,
        rect: egui::Rect,
    }

    #[cfg(not(target_arch = "wasm32"))]
    impl UiHarness {
        fn new(app: P2PTransfer, width: f32, dark: bool) -> Self {
            Self::sharing(&egui::Context::default(), app, width, dark)
        }

        /// A harness on an existing context, which keeps its fonts: building them dominates
        /// a short test.
        fn sharing(ctx: &egui::Context, app: P2PTransfer, width: f32, dark: bool) -> Self {
            let ctx = ctx.clone();
            ctx.enable_accesskit();
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            Self {
                ctx,
                app,
                frame: eframe::Frame::_new_kittest(),
                size: egui::vec2(width, 640.0),
            }
        }

        /// Run one frame; return its widgets and the URLs it asked the platform to open.
        fn step(&mut self, events: Vec<egui::Event>) -> (Vec<Widget>, Vec<egui::OpenUrl>) {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                events,
                ..Default::default()
            };
            let (app, frame) = (&mut self.app, &mut self.frame);
            let mut output = self.ctx.run_ui(input, |ui| eframe::App::ui(app, ui, frame));
            let widgets = output
                .platform_output
                .accesskit_update
                .take()
                .expect("accesskit is enabled")
                .nodes
                .into_iter()
                .filter_map(|(_, node)| {
                    let text = node.label().or(node.value())?.to_owned();
                    let bounds = node.bounds()?;
                    Some(Widget {
                        role: node.role(),
                        text,
                        rect: egui::Rect::from_min_max(
                            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                        ),
                    })
                })
                .collect();
            let opened = output
                .platform_output
                .commands
                .drain(..)
                .filter_map(|command| match command {
                    egui::OutputCommand::OpenUrl(open) => Some(open),
                    _ => None,
                })
                .collect();
            output.drop_without_applying_deltas();
            (widgets, opened)
        }

        fn click(&mut self, at: egui::Pos2) -> (Vec<Widget>, Vec<egui::OpenUrl>) {
            let press = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            };
            self.step(vec![egui::Event::PointerMoved(at), press(true)]);
            self.step(vec![press(false)])
        }

        fn screen(&self) -> egui::Rect {
            egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn find<'a>(
        widgets: &'a [Widget],
        role: egui::accesskit::Role,
        text: &str,
    ) -> Option<&'a Widget> {
        widgets
            .iter()
            .find(|widget| widget.role == role && widget.text == text)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn every_screen() -> Vec<(&'static str, P2PTransfer)> {
        let receive = Mode::Receive(Box::new(ReceiveState {
            input: String::new(),
            params: FragmentParams::default(),
            handle: None,
            error: None,
            opening: false,
            startup: Arc::default(),
            save_pending: Arc::new(AtomicBool::new(false)),
        }));
        let sharing = P2PTransfer {
            mode: Mode::Send {
                preparing: Vec::new(),
            },
            ..P2PTransfer::default()
        };
        *sharing.link.lock().unwrap() = Some("https://oxfer.app/#ticket&cap=0".to_string());
        vec![
            ("home", P2PTransfer::default()),
            ("send", sharing),
            (
                "receive",
                P2PTransfer {
                    mode: receive,
                    ..P2PTransfer::default()
                },
            ),
        ]
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn local_test_legal_links_are_reachable_from_every_screen() {
        use egui::accesskit::Role;

        // Light and dark share one layout, so each pairs with one terminal state.
        let mut cases = Vec::new();
        for width in [320.0, 360.0, 640.0, 900.0, 1040.0] {
            for theme in [Theme::Rusty, Theme::Clean] {
                for (dark, terminal_open) in [(false, false), (true, true)] {
                    cases.push((width, theme, dark, terminal_open));
                }
            }
        }
        let ctx = egui::Context::default();
        for (width, theme, dark, terminal_open) in cases {
            for (screen, mut app) in every_screen() {
                let case = format!(
                    "{screen} at {width}px, {theme:?}, dark={dark}, terminal={terminal_open}"
                );
                app.theme = theme;
                app.show_terminal_view = terminal_open;
                let mut ui = UiHarness::sharing(&ctx, app, width, dark);
                // The first frame applies the theme; lay out with it settled.
                ui.step(Vec::new());
                let (widgets, _) = ui.step(Vec::new());
                let toggle = widgets
                    .iter()
                    .find(|w| w.role == Role::Button && w.text.contains(">_"))
                    .unwrap_or_else(|| panic!("{case}: no terminal toggle"));
                let labels: Vec<&str> = if width >= FOOTER_INLINE_LINKS_MIN_WIDTH {
                    FooterLink::ALL.iter().map(|link| link.label()).collect()
                } else {
                    vec!["Legal"]
                };
                let controls: Vec<&Widget> = labels
                    .iter()
                    .map(|label| {
                        find(&widgets, Role::Button, label)
                            .unwrap_or_else(|| panic!("{case}: no {label} button"))
                    })
                    .collect();
                for control in &controls {
                    assert!(
                        ui.screen().contains_rect(control.rect),
                        "{case}: {control:?} is off screen"
                    );
                    assert!(
                        !control.rect.intersects(toggle.rect),
                        "{case}: {control:?} overlaps the terminal toggle"
                    );
                    // With the terminal closed the bottom bar is the last 50px.
                    assert!(
                        terminal_open || control.rect.min.y >= ui.size.y - 50.0,
                        "{case}: {control:?} is not in the bottom bar"
                    );
                }
                for pair in controls.windows(2) {
                    assert!(
                        pair[0].rect.max.x <= pair[1].rect.min.x,
                        "{case}: links overlap or are out of order: {pair:?}"
                    );
                }
                // The browser build adds its "Diags" button beside these; this native build
                // must leave that much room after the toggle's 12px gap. The short toggle
                // label marks the narrow bar, where both buttons shrink.
                let diags = bar_button_width(toggle.text.contains("Logs")) + 10.0;
                let spare = controls[0].rect.min.x - toggle.rect.max.x - 12.0;
                assert!(
                    spare >= diags,
                    "{case}: only {spare}px left for the {diags}px Diags button"
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn local_test_legal_menu_opens_each_page_at_phone_width() {
        use egui::accesskit::Role;

        for link in FooterLink::ALL {
            for dark in [false, true] {
                let mut ui = UiHarness::new(P2PTransfer::default(), 360.0, dark);
                ui.step(Vec::new());
                let (widgets, _) = ui.step(Vec::new());
                let legal = find(&widgets, Role::Button, "Legal")
                    .expect("Legal menu")
                    .rect;
                ui.click(legal.center());
                // The menu's first frame is an invisible sizing pass.
                ui.step(Vec::new());
                let (widgets, _) = ui.step(Vec::new());
                let item = find(&widgets, Role::Button, link.label())
                    .unwrap_or_else(|| panic!("{link:?} is missing from the open menu"))
                    .rect;
                assert!(
                    ui.screen().contains_rect(item),
                    "{link:?} menu item is off screen at 360px: {item:?}"
                );
                assert!(
                    item.height() >= 24.0,
                    "{link:?} menu item is too small to tap: {item:?}"
                );
                assert!(
                    item.max.y <= legal.min.y,
                    "the menu must open above the bar"
                );
                let (_, opened) = ui.click(item.center());
                assert_eq!(
                    opened,
                    vec![egui::OpenUrl::new_tab(link.url(false))],
                    "{link:?}"
                );
                // Choosing an entry closes the menu.
                let (widgets, _) = ui.step(Vec::new());
                assert!(find(&widgets, Role::Button, link.label()).is_none());
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn local_test_inline_legal_links_open_each_page_on_wide_screens() {
        use egui::accesskit::Role;

        for link in FooterLink::ALL {
            let mut ui = UiHarness::new(P2PTransfer::default(), 1040.0, false);
            ui.step(Vec::new());
            let (widgets, _) = ui.step(Vec::new());
            let rect = find(&widgets, Role::Button, link.label())
                .unwrap_or_else(|| panic!("no inline {link:?} link"))
                .rect;
            let (_, opened) = ui.click(rect.center());
            assert_eq!(opened, vec![egui::OpenUrl::new_tab(link.url(false))]);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn local_test_received_files_offer_a_report_abuse_link() {
        use egui::accesskit::Role;

        let ctx = egui::Context::default();
        for (width, theme, dark) in [
            (320.0, Theme::Clean, false),
            (360.0, Theme::Rusty, true),
            (640.0, Theme::Rusty, false),
            (1040.0, Theme::Clean, true),
        ] {
            for received in [false, true] {
                for (screen, mut app) in every_screen() {
                    let case = format!(
                        "{screen} at {width}px, {theme:?}, dark={dark}, received={received}"
                    );
                    app.theme = theme;
                    if received {
                        app.received_files.lock().unwrap().push(ReceivedFile {
                            name: "holiday.jpg".to_owned(),
                            size: 2048,
                            location: "/tmp/holiday.jpg".to_owned(),
                            when: "12:00".to_owned(),
                            path: TransferPath::Direct,
                            average_bytes_per_sec: None,
                        });
                    }
                    let mut ui = UiHarness::sharing(&ctx, app, width, dark);
                    // Tall enough that the received card is on screen below any panel.
                    ui.size.y = 4000.0;
                    ui.step(Vec::new());
                    let (widgets, _) = ui.step(Vec::new());
                    let link = find(&widgets, Role::Link, REPORT_ABUSE_LABEL);
                    if !received {
                        assert!(link.is_none(), "{case}: nothing received to report");
                        continue;
                    }
                    let card = find(&widgets, Role::Label, "Received (1)")
                        .unwrap_or_else(|| panic!("{case}: no received card"))
                        .rect;
                    let link = link
                        .unwrap_or_else(|| panic!("{case}: no {REPORT_ABUSE_LABEL} link"))
                        .rect;
                    // Announced as a link (see `show_report_abuse`), one row high, on screen
                    // and below the received card's title.
                    assert!(link.height() < 24.0, "{case}: link text wrapped: {link:?}");
                    assert!(link.max.y <= ui.size.y - 50.0, "{case}: {link:?}");
                    if screen == "home" {
                        // Home's technical details are already wider than a 320px screen;
                        // the link must not add to that.
                        let widest = widgets
                            .iter()
                            .filter(|w| w.text != REPORT_ABUSE_LABEL)
                            .fold(width, |widest, w| widest.max(w.rect.max.x));
                        assert!(link.max.x <= widest, "{case}: {link:?}");
                    } else {
                        assert!(ui.screen().contains_rect(link), "{case}: {link:?}");
                    }
                    assert!(link.min.y > card.max.y, "{case}: link above the card title");
                    let (_, opened) = ui.click(link.center());
                    assert_eq!(
                        opened,
                        vec![egui::OpenUrl::new_tab(FooterLink::Abuse.url(false))],
                        "{case}"
                    );
                }
            }
        }
        // The browser build opens its own origin's page, like the bottom bar.
        assert_eq!(FooterLink::Abuse.url(true), "/abuse");
        assert_eq!(FooterLink::Abuse.url(false), "https://oxfer.app/abuse");
    }

    /// Before saving: a real loopback receive waiting at the Save prompt.
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_test_save_prompt_offers_a_report_abuse_link() {
        use egui::accesskit::Role;

        let dir = std::env::temp_dir().join(format!(
            "oxfer-report-link-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("holiday.jpg");
        let data = b"received bytes".to_vec();
        std::fs::write(&path, &data).unwrap();
        let files = Arc::new(Mutex::new(vec![SharedFile {
            meta: FileMeta {
                name: "holiday.jpg".to_owned(),
                size: data.len() as u64,
                hash: crate::blob_store::BlobHash::from_bytes(&data),
            },
            snapshot: file_io::snapshot_path(&path).unwrap(),
            origin: FileOrigin::Path(path),
        }]));
        let sender = Node::bind(files, RelayChoice::None).await.unwrap();
        let ticket = sender.ticket().await.unwrap();
        let receiver = Node::bind(Arc::new(Mutex::new(Vec::new())), RelayChoice::None)
            .await
            .unwrap();
        let mut handle = TransferHandle::start_receive(
            Arc::new(receiver),
            ticket,
            ReceiveOptions {
                cap: Some(sender.cap()),
                ..ReceiveOptions::default()
            },
        );
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while !matches!(handle.latest().phase, Phase::AwaitingSave { .. }) {
                let phase = handle.latest().phase;
                assert!(
                    !phase.is_terminal(),
                    "{phase:?}: {:?}",
                    handle.latest().error
                );
                handle.progress.changed().await.unwrap();
            }
        })
        .await
        .expect("no Save prompt over loopback");

        let save_pending = Arc::new(AtomicBool::new(false));
        let app = P2PTransfer {
            mode: Mode::Receive(Box::new(ReceiveState {
                input: String::new(),
                params: FragmentParams::default(),
                handle: Some(handle),
                error: None,
                opening: false,
                startup: Arc::default(),
                save_pending: save_pending.clone(),
            })),
            ..P2PTransfer::default()
        };
        let mut ui = UiHarness::new(app, 360.0, false);
        ui.size.y = 1600.0;
        for (width, theme) in [
            (320.0, Theme::Clean),
            (360.0, Theme::Rusty),
            (1040.0, Theme::Clean),
        ] {
            let case = format!("{width}px, {theme:?}");
            ui.size.x = width;
            ui.app.theme = theme;
            ui.step(Vec::new());
            let (widgets, _) = ui.step(Vec::new());
            let file = find(&widgets, Role::Label, "holiday.jpg")
                .unwrap_or_else(|| panic!("{case}: the manifest is not shown"))
                .rect;
            let save = find(&widgets, Role::Button, "Choose where to save")
                .unwrap_or_else(|| panic!("{case}: no Save button"))
                .rect;
            let link = find(&widgets, Role::Link, REPORT_ABUSE_LABEL)
                .unwrap_or_else(|| panic!("{case}: no {REPORT_ABUSE_LABEL} link"))
                .rect;
            assert!(ui.screen().contains_rect(link), "{case}: {link:?}");
            assert!(link.height() < 24.0, "{case}: link text wrapped: {link:?}");
            assert!(
                file.max.y <= link.min.y && link.max.y <= save.min.y,
                "{case}: the link belongs between the files and Save"
            );
            let (_, opened) = ui.click(link.center());
            assert_eq!(
                opened,
                vec![egui::OpenUrl::new_tab(FooterLink::Abuse.url(false))],
                "{case}"
            );
            assert!(
                !save_pending.load(Ordering::Acquire),
                "{case}: reporting must not start a save"
            );
        }
        drop(ui);
        sender.shutdown().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn local_test_native_share_link_discloses_ip_addresses() {
        use egui::accesskit::Role;

        assert!(NATIVE_LINK_ADDRESSES.contains("IP addresses"));
        for width in [360.0, 1040.0] {
            let (_, app) = every_screen()
                .into_iter()
                .find(|(screen, _)| *screen == "send")
                .unwrap();
            let mut ui = UiHarness::new(app, width, true);
            ui.step(Vec::new());
            let (widgets, _) = ui.step(Vec::new());
            let link = widgets
                .iter()
                .find(|w| w.role == Role::Label && w.text.starts_with("https://oxfer.app/#"))
                .expect("the share link is shown");
            let note = find(&widgets, Role::Label, NATIVE_LINK_ADDRESSES)
                .unwrap_or_else(|| panic!("no IP address note at {width}px"));
            assert!(
                note.rect.min.y >= link.rect.max.y,
                "the note follows the link"
            );
            assert!(
                note.rect.min.y - link.rect.max.y < 40.0,
                "the note sits next to the link"
            );
        }
        // Home and Receive have no link, so they show no note.
        for (screen, app) in every_screen()
            .into_iter()
            .filter(|(screen, _)| *screen != "send")
        {
            let mut ui = UiHarness::new(app, 360.0, true);
            ui.step(Vec::new());
            let (widgets, _) = ui.step(Vec::new());
            assert!(
                find(&widgets, Role::Label, NATIVE_LINK_ADDRESSES).is_none(),
                "{screen}"
            );
        }
    }

    #[test]
    fn local_test_technical_details_show_this_builds_relay_copy() {
        let operator = RelayOperator::current();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1040.0, 2000.0),
                )),
                ..Default::default()
            },
            |ui| show_how_it_works(ui, &Tc::of(Theme::Clean, true)),
        );
        let texts: Vec<String> = output
            .platform_output
            .accesskit_update
            .take()
            .expect("accesskit is enabled")
            .nodes
            .into_iter()
            .filter_map(|(_, node)| node.value().map(str::to_owned))
            .collect();
        output.drop_without_applying_deltas();
        for expected in [operator.peer_path(), operator.privacy_boundary()] {
            assert!(
                texts.iter().any(|text| text == expected),
                "missing {expected:?}"
            );
        }
        assert!(
            !texts
                .iter()
                .any(|text| text.contains("through the iroh relay")),
            "step 03 must say who runs the relay"
        );
        if !cfg!(target_arch = "wasm32") {
            // The desktop app is not served by Cloudflare and sends no STUN requests.
            for text in &texts {
                assert!(
                    !text.contains("STUN") && !text.contains("web app shell"),
                    "desktop Technical details: {text}"
                );
            }
        }
    }
}
