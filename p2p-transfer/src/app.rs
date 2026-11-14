//! The egui adapter: theme, layout and rendering over the session core.
//!
//! This module holds no transfer, retry or session logic. Each frame it ticks
//! [`crate::sessions::Oxfer`], renders the [`Snapshot`] it publishes, and turns gestures
//! into [`Intent`]s. What it does own is view state: the theme, which panels are open,
//! "Copied" flags, text-box focus, and the native file and folder dialogs.

use std::sync::Arc;

use eframe::egui;
use egui::{Button, Color32, CornerRadius, RichText, Stroke, Ui};
use serde::{Deserialize, Serialize};

use crate::logging;
use crate::sessions::text::{self, format_size, LINK_WARNING};
#[cfg(target_arch = "wasm32")]
use crate::sessions::CopiesIntent;
#[cfg(target_arch = "wasm32")]
use crate::sessions::DiagnosticsIntent;
use crate::sessions::{
    chat::agent_prompt, Activity, ChatIntent, ChatRole, ChatSnapshot, ChatStatus, Intent,
    LinkState, Oxfer, ReceiveIntent, SaveTarget, SendIntent, Snapshot,
};
#[cfg(target_arch = "wasm32")]
use crate::sessions::{PickedFile, WakeLock};
use crate::transfer::Phase;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(module = "/assets/theme.js")]
extern "C" {
    #[wasm_bindgen(js_name = setBrowserTheme)]
    fn set_browser_theme(theme: &str, dark: bool);
}

// ── File-messenger theme — shared colors for desktop and browser ────────────

/// A display preference, independent of the light/dark setting and transfer state.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
enum Theme {
    Rusty,
    #[default]
    Clean,
    /// A vector CRT: one green phosphor on black, wireframe outlines, monospace type. Light
    /// mode is the same machine's green-bar line-printer paper.
    Phosphor,
}

#[cfg(target_arch = "wasm32")]
const BROWSER_THEME_KEY: &str = "oxfer.theme.v1";

// Keep header and footer controls clear of the shared content column's edges.
const CONTROL_EDGE_INSET: f32 = 16.0;

impl Theme {
    /// Menu order.
    const ALL: [Self; 3] = [Self::Rusty, Self::Clean, Self::Phosphor];

    const fn label(self) -> &'static str {
        match self {
            Self::Rusty => "Rusty",
            Self::Clean => "Clean",
            Self::Phosphor => "Phosphor",
        }
    }

    /// Primary buttons and egui's own widgets.
    const fn control_radius(self) -> u8 {
        match self {
            Self::Rusty => 10,
            Self::Clean => 18,
            Self::Phosphor => 2,
        }
    }

    /// Cards.
    const fn card_radius(self) -> u8 {
        match self {
            Self::Rusty => 16,
            Self::Clean => 20,
            Self::Phosphor => 4,
        }
    }

    /// Ceiling for every other rounded frame, pill, and icon tile; see [`Tc::corner`].
    const fn corner_cap(self) -> Option<u8> {
        match self {
            Self::Phosphor => Some(4),
            Self::Rusty | Self::Clean => None,
        }
    }

    /// Set all UI text in the monospace family, not only meta, links, and logs.
    const fn monospace(self) -> bool {
        matches!(self, Self::Phosphor)
    }

    #[cfg(any(test, target_arch = "wasm32"))]
    const fn storage_value(self) -> &'static str {
        match self {
            Self::Rusty => "rusty",
            Self::Clean => "clean",
            Self::Phosphor => "phosphor",
        }
    }

    #[cfg(any(test, target_arch = "wasm32"))]
    fn from_storage_value(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|theme| theme.storage_value() == value)
    }
}

/// What a theme paints on the page fill. Cards are opaque, so it shows only in the gutters
/// and the gaps between them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Backdrop {
    Plain,
    /// A wireframe floor receding to a horizon, like the board on a vector display.
    VectorFloor,
    /// Alternating bands of line-printer paper.
    GreenBar,
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
    /// Halo behind cards, and behind Phosphor's popups. `Shadow::NONE` for themes that do
    /// not glow.
    glow: egui::Shadow,
    /// Multiplied into the black or white wordmark raster.
    logo_tint: Color32,
    backdrop: Backdrop,
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
            glow: egui::Shadow::NONE,
            logo_tint: Color32::WHITE,
            backdrop: Backdrop::Plain,
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
            glow: egui::Shadow::NONE,
            logo_tint: Color32::WHITE,
            backdrop: Backdrop::Plain,
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
            glow: egui::Shadow::NONE,
            logo_tint: Color32::WHITE,
            backdrop: Backdrop::Plain,
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
            glow: egui::Shadow::NONE,
            logo_tint: Color32::WHITE,
            backdrop: Backdrop::Plain,
        }
    }

    const fn phosphor_dark() -> Self {
        let primary = Color32::from_rgb(57, 255, 20); // #39ff14
        Self {
            theme: Theme::Phosphor,
            bg: Color32::from_rgb(3, 10, 4),             // #030a04
            surface_lowest: Color32::from_rgb(0, 0, 0),  // #000000
            surface_low: Color32::from_rgb(6, 18, 8),    // #061208
            surface: Color32::from_rgb(10, 26, 12),      // #0a1a0c
            surface_high: Color32::from_rgb(19, 58, 20), // #133a14
            primary,
            on_primary: Color32::from_rgb(2, 26, 2), // #021a02
            secondary: Color32::from_rgb(198, 255, 61), // #c6ff3d
            on_surface: Color32::from_rgb(184, 255, 160), // #b8ffa0
            on_surface_var: Color32::from_rgb(116, 214, 92), // #74d65c
            outline: Color32::from_rgb(79, 209, 58), // #4fd13a
            outline_var: Color32::from_rgb(30, 92, 27), // #1e5c1b
            error: Color32::from_rgb(255, 106, 61),  // #ff6a3d
            glow: egui::Shadow {
                offset: [0, 0],
                blur: 18,
                spread: 0,
                color: Color32::from_rgba_unmultiplied_const(
                    primary.r(),
                    primary.g(),
                    primary.b(),
                    34,
                ),
            },
            logo_tint: primary,
            backdrop: Backdrop::VectorFloor,
        }
    }

    const fn phosphor_light() -> Self {
        Self {
            theme: Theme::Phosphor,
            bg: Color32::from_rgb(241, 247, 236), // #f1f7ec
            surface_lowest: Color32::from_rgb(251, 253, 249), // #fbfdf9
            surface_low: Color32::from_rgb(250, 253, 246), // #fafdf6
            surface: Color32::from_rgb(220, 236, 210), // #dcecd2
            surface_high: Color32::from_rgb(199, 226, 184), // #c7e2b8
            primary: Color32::from_rgb(23, 112, 26), // #17701a
            on_primary: Color32::from_rgb(244, 255, 238), // #f4ffee
            secondary: Color32::from_rgb(63, 106, 0), // #3f6a00
            on_surface: Color32::from_rgb(15, 42, 16), // #0f2a10
            on_surface_var: Color32::from_rgb(61, 90, 58), // #3d5a3a
            outline: Color32::from_rgb(68, 112, 60), // #44703c
            outline_var: Color32::from_rgb(181, 209, 166), // #b5d1a6
            error: Color32::from_rgb(178, 58, 20), // #b23a14
            glow: egui::Shadow::NONE,
            logo_tint: Color32::WHITE,
            backdrop: Backdrop::GreenBar,
        }
    }

    fn of(theme: Theme, dark: bool) -> Self {
        match (theme, dark) {
            (Theme::Rusty, true) => Self::rusty_dark(),
            (Theme::Rusty, false) => Self::rusty_light(),
            (Theme::Clean, true) => Self::clean_dark(),
            (Theme::Clean, false) => Self::clean_light(),
            (Theme::Phosphor, true) => Self::phosphor_dark(),
            (Theme::Phosphor, false) => Self::phosphor_light(),
        }
    }

    /// A frame's own rounding, capped so Phosphor draws every outline nearly square.
    fn corner(&self, radius: u8) -> CornerRadius {
        CornerRadius::same(
            self.theme
                .corner_cap()
                .map_or(radius, |cap| radius.min(cap)),
        )
    }
}

// ── View state ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Copied {
    Link,
    AgentLink,
}

/// Per-room view state; reset whenever the core reports a different room.
#[derive(Default)]
struct ChatView {
    room: u64,
    input: String,
    /// Which link was copied last, for the button label.
    copied: Option<Copied>,
    /// The agent prompt card shows the full prompt.
    prompt_open: bool,
    focus_input: bool,
    /// Keyboard inset when the input was last scrolled into view; the keyboard sliding
    /// up must bring the input back into view.
    input_seen_inset: f32,
}

impl ChatView {
    fn sync(&mut self, chat: &ChatSnapshot) {
        if self.room != chat.id {
            *self = Self {
                room: chat.id,
                focus_input: true,
                ..Self::default()
            };
        }
    }

    /// Copy the agent brief for this room's agent link.
    fn copy_for_agent(&mut self, ctx: &egui::Context, chat: &ChatSnapshot) {
        if let Some(links) = &chat.links {
            ctx.copy_text(agent_prompt(links.agent.as_str()));
            self.copied = Some(Copied::AgentLink);
        }
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

    /// Native app setting and browser backup. The browser's immediate theme key wins on reload.
    theme: Theme,
    /// Every session, handle and task. Never serialized.
    #[serde(skip)]
    core: Oxfer,
    /// The core's view model, refreshed after every tick and every dispatch.
    #[serde(skip)]
    snap: Snapshot,
    #[serde(skip)]
    show_terminal_view: bool,
    /// This frame's [`keyboard_inset`], read once in `ui()`.
    #[serde(skip)]
    keyboard_inset: f32,
    /// The share whose link was copied; "Copied" stays until that share stops.
    #[serde(skip)]
    link_copied: Option<u64>,
    #[serde(skip)]
    chat_view: ChatView,
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
    /// The local copy whose Delete is waiting for confirmation.
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    confirm_discard: Option<String>,
}

impl Default for P2PTransfer {
    fn default() -> Self {
        let core = Oxfer::default();
        let snap = core.snapshot();
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            save_directory: None,
            theme: Theme::default(),
            core,
            snap,
            keyboard_inset: 0.0,
            show_terminal_view: false,
            link_copied: None,
            chat_view: ChatView::default(),
            header_logos: None,
            last_dark_mode: None,
            last_theme: None,
            #[cfg(target_arch = "wasm32")]
            file_input_closure: None,
            #[cfg(target_arch = "wasm32")]
            hashchange_closure: None,
            #[cfg(target_arch = "wasm32")]
            confirm_discard: None,
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

// ── Adapter ──────────────────────────────────────────────────────────────────

impl P2PTransfer {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        #[cfg(target_arch = "wasm32")]
        cc.egui_ctx
            .options_mut(|options| options.sync_window_theme = false);
        let app: Self = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, eframe::APP_KEY))
            .unwrap_or_default();
        // Background tasks repaint on demand instead of the UI polling idle.
        let repaint = cc.egui_ctx.clone();
        app.core
            .notifier()
            .set_wake(Arc::new(move || repaint.request_repaint()));
        #[cfg(target_arch = "wasm32")]
        let mut app = app;
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast as _;

            // eframe saves app settings periodically. A theme tap must survive even if
            // mobile Safari kills this page before that next checkpoint.
            if let Some(theme) = eframe::web::storage::local_storage_get(BROWSER_THEME_KEY)
                .as_deref()
                .and_then(Theme::from_storage_value)
            {
                app.theme = theme;
            }
            // Migrate any earlier eframe-saved choice into the immediate key.
            eframe::web::storage::local_storage_set(BROWSER_THEME_KEY, app.theme.storage_value());

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
            app.core.dispatch(Intent::Copies(CopiesIntent::Refresh));
        }
        app
    }

    /// Hand an intent to the core and refresh the snapshot, so the rest of this frame
    /// draws the new state.
    fn dispatch(&mut self, intent: Intent) {
        self.core.dispatch(intent);
        self.snap = self.core.snapshot();
    }

    fn link_copied(&self) -> bool {
        self.link_copied == Some(self.snap.send.share_id)
    }

    // ── Pick ─────────────────────────────────────────────────────────────

    #[cfg(not(target_arch = "wasm32"))]
    fn pick_file(&mut self) {
        let Some(path) = rfd::FileDialog::new().pick_file() else {
            return;
        };
        self.core.picks().push_path(path);
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
                self.core
                    .report("this browser would not open a file picker".to_string());
                return;
            }
        };
        input.set_type("file");

        let picks = self.core.picks();
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
            crate::sessions::Platform::set_wake_lock(&crate::sessions::WebPlatform, true);
            picks.push(PickedFile::from_web_file(file));
        })
            as Box<dyn FnMut(web_sys::Event)>);

        input.set_onchange(Some(closure.as_ref().unchecked_ref()));
        input.click();
        // Keep the closure alive for as long as the input can fire it.
        self.file_input_closure = Some(closure);
    }

    /// Queue files dropped on the window, as if each had been picked.
    fn accept_dropped_files(&mut self, ctx: &egui::Context) {
        if !self.core.can_pick_files() {
            return;
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let picks = self.core.picks();
        #[cfg(not(target_arch = "wasm32"))]
        for file in dropped {
            picks.push_path(file.path().to_path_buf());
        }
        #[cfg(target_arch = "wasm32")]
        {
            // eframe hands over the `File` itself and reads no bytes unless asked to.
            let files: Vec<web_sys::File> = dropped
                .iter()
                .filter_map(|file| file.web_file().cloned())
                .collect();
            if !files.is_empty() {
                picks.push_dropped(files);
            }
        }
    }

    /// Native Save: reuse the remembered folder or ask for one, then hand it to the core.
    #[cfg(not(target_arch = "wasm32"))]
    fn save_click(&mut self) {
        let dir = match self.save_directory.clone() {
            Some(dir) => Some(dir),
            None => rfd::FileDialog::new().pick_folder(),
        };
        let Some(dir) = dir else {
            return; // the user dismissed the folder picker: stay in AwaitingSave
        };
        self.save_directory = Some(dir.clone());
        self.dispatch(Intent::Receive(ReceiveIntent::Save(SaveTarget::Directory(
            dir,
        ))));
    }

    /// Browser Save: dispatched inside the click, so the picker keeps the gesture.
    #[cfg(target_arch = "wasm32")]
    fn save_click(&mut self) {
        self.dispatch(Intent::Receive(ReceiveIntent::Save(SaveTarget::Browser)));
    }

    #[cfg(target_arch = "wasm32")]
    fn show_transfer_attention(
        ui: &mut Ui,
        tc: &Tc,
        sender: bool,
        awaiting_save: bool,
        wake_lock: WakeLock,
    ) {
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(
                tc.primary.r(),
                tc.primary.g(),
                tc.primary.b(),
                24,
            ))
            .stroke(Stroke::new(1.0, tc.primary))
            .corner_radius(tc.corner(14))
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
                    match wake_lock {
                        WakeLock::Active => (
                            "Your browser is helping keep the screen on (uses more battery).",
                            tc.secondary,
                        ),
                        WakeLock::Requesting => {
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

    fn apply_theme(ctx: &egui::Context, ui: &mut Ui, theme: Theme, dark: bool) {
        let tc = Tc::of(theme, dark);
        #[cfg(target_arch = "wasm32")]
        set_browser_theme(theme.storage_value(), dark);
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
        let radius = CornerRadius::same(theme.control_radius());
        v.widgets.inactive.corner_radius = radius;
        v.widgets.hovered.corner_radius = radius;
        v.widgets.active.corner_radius = radius;
        // An open menu button draws with `open`; without this it snaps to egui's 2.
        v.widgets.open.corner_radius = radius;
        v.window_corner_radius = tc.corner(6);
        v.menu_corner_radius = tc.corner(6);
        if theme == Theme::Phosphor {
            // One phosphor has no room for egui's stock grays and blues, so the widget
            // states the other themes leave at their defaults come from the palette too.
            // Idle icons and the floating scroll bar stay mid-green; the beam brightens
            // only under the pointer.
            v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, tc.outline);
            for state in [
                &mut v.widgets.hovered,
                &mut v.widgets.active,
                &mut v.widgets.open,
            ] {
                state.fg_stroke = Stroke::new(1.0_f32, tc.on_surface);
                state.bg_fill = tc.surface_high;
                state.weak_bg_fill = tc.surface_high;
            }
            v.widgets.open.bg_stroke = Stroke::new(1.0_f32, tc.primary);
            v.selection.stroke = Stroke::new(1.0_f32, tc.primary);
            v.hyperlink_color = tc.secondary;
            v.warn_fg_color = tc.error;
            v.error_fg_color = tc.error;
            v.text_cursor.stroke = Stroke::new(2.0_f32, tc.primary);
            v.window_stroke = Stroke::new(1.0_f32, tc.outline);
            v.window_shadow = tc.glow;
            v.popup_shadow = tc.glow;
        }
        let family = if theme.monospace() {
            egui::FontFamily::Monospace
        } else {
            egui::FontFamily::Proportional
        };
        let tweak = |style: &mut egui::Style| {
            style.spacing.button_padding = egui::vec2(18.0, 11.0);
            style.spacing.item_spacing = egui::vec2(10.0, 10.0);
            style.spacing.interact_size.y = 44.0;
            set_text_family(style, &family);
        };
        // Both, and in this order: the context so later frames start correct, and the live
        // `Ui` so *this* frame is already themed. Setting only the context would leave the
        // root `Ui` — built before `logic()` ran — one frame behind on every toggle.
        // Spacing and type do not depend on light or dark, so they go into both of egui's
        // styles and survive it swapping to the other one when the system theme changes.
        ctx.set_visuals(v.clone());
        ctx.all_styles_mut(tweak);
        *ui.visuals_mut() = v;
        tweak(ui.style_mut());
    }
}

// ── Rendering ────────────────────────────────────────────────────────────────

/// Switch every text style except `Monospace` itself, so labels that only set a size follow
/// the theme while `.monospace()` text stays monospace in all of them.
fn set_text_family(style: &mut egui::Style, family: &egui::FontFamily) {
    for (text_style, font) in &mut style.text_styles {
        if *text_style != egui::TextStyle::Monospace {
            font.family = family.clone();
        }
    }
}

/// Paints the theme's [`Backdrop`] across the whole panel, beneath anything added after it.
fn paint_backdrop(ui: &Ui, tc: &Tc) {
    let rect = ui.clip_rect();
    let painter = ui.painter();
    match tc.backdrop {
        Backdrop::Plain => {}
        Backdrop::VectorFloor => {
            // Floor rows sit at depths z = 1, 1 + STEP, … and project to
            // y = horizon + depth / z, so they crowd toward the horizon as they recede.
            const ROWS: usize = 10;
            const STEP: f32 = 0.6;
            let horizon = rect.top() + rect.height() * 0.45;
            let depth = rect.bottom() - horizon;
            // Column spacing follows depth, so a sliver of floor would need thousands of
            // lines and show nothing.
            if depth < 48.0 {
                return;
            }
            let far = 1.0 + (ROWS - 1) as f32 * STEP;
            let line = tc.outline_var;
            for row in 0..ROWS {
                let z = 1.0 + row as f32 * STEP;
                // Dimmer with distance, the way a beam's trace fades.
                let stroke = Stroke::new(1.0, line.gamma_multiply(z.recip().sqrt()));
                painter.hline(rect.x_range(), horizon + depth / z, stroke);
            }
            // Columns are evenly spaced on the floor, so on screen they converge on the
            // vanishing point. Keep adding them until the far ends leave the panel too.
            let spacing = depth * STEP * 0.8;
            let center = rect.center().x;
            let far_y = horizon + depth / far;
            let reach = (rect.width() * 0.5 * far / spacing).ceil() as i32;
            let stroke = Stroke::new(1.0, line.gamma_multiply(0.7));
            for column in -reach..=reach {
                let offset = column as f32 * spacing;
                painter.line_segment(
                    [
                        egui::pos2(center + offset / far, far_y),
                        egui::pos2(center + offset, rect.bottom()),
                    ],
                    stroke,
                );
            }
        }
        Backdrop::GreenBar => {
            // Three 18-point printer lines per band, alternating with plain paper.
            const BAND: f32 = 54.0;
            let mut top = rect.top();
            while top < rect.bottom() {
                let band = egui::Rect::from_x_y_ranges(
                    rect.x_range(),
                    top..=(top + BAND).min(rect.bottom()),
                );
                painter.rect_filled(band, 0.0, tc.surface);
                top += 2.0 * BAND;
            }
        }
    }
}

/// A card: the one container every panel below is built from.
fn card(tc: &Tc) -> egui::Frame {
    egui::Frame::new()
        .fill(tc.surface_low)
        .corner_radius(CornerRadius::same(tc.theme.card_radius()))
        .stroke(Stroke::new(1.0_f32, tc.outline_var))
        .shadow(tc.glow)
        .inner_margin(egui::Margin::same(22))
}

fn primary_text(label: &str) -> RichText {
    RichText::new(label).strong().size(15.0)
}

fn primary_button(tc: &Tc, label: &str) -> Button<'static> {
    Button::new(primary_text(label).color(tc.on_primary))
        .fill(tc.primary)
        .stroke(Stroke::new(1.0, tc.primary))
        .corner_radius(CornerRadius::same(tc.theme.control_radius()))
        .min_size(egui::vec2(0.0, 46.0))
}

const fn copy_label(copied: bool) -> &'static str {
    if copied {
        "✔ Copied to clipboard"
    } else {
        "Copy link"
    }
}

/// A primary button that turns the secondary colour once its text was copied.
fn copy_button(tc: &Tc, label: &str, copied: bool) -> Button<'static> {
    let color = if copied { tc.secondary } else { tc.primary };
    primary_button(tc, label)
        .fill(color)
        .stroke(Stroke::new(1.0, color))
}

/// The light/dark toggle names the mode it switches to.
const fn mode_toggle_label(dark: bool) -> &'static str {
    if dark {
        "Light mode"
    } else {
        "Dark mode"
    }
}

fn outline_text(label: &str) -> RichText {
    RichText::new(label).strong().size(14.0)
}

fn outline_button(tc: &Tc, label: &str, color: Color32) -> Button<'static> {
    Button::new(outline_text(label).color(color))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0_f32, color))
        .corner_radius(tc.corner(10))
        .min_size(egui::vec2(0.0, 46.0))
}

/// The width of a fixed-width button: its design width `min`, or wider when any of `texts`
/// needs more room on one line in the current font, so monospace labels still fit.
fn fit_width<const N: usize>(ui: &Ui, texts: [RichText; N], min: f32) -> f32 {
    texts.into_iter().fold(min, |width, text| {
        let text = egui::WidgetText::from(text).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Button,
        );
        width.max(text.size().x + 2.0 * ui.spacing().button_padding.x)
    })
}

/// egui rounds a progress bar to half its height; keep that, under the theme's corner cap.
fn progress_bar(tc: &Tc, progress: f32, height: f32) -> egui::ProgressBar {
    egui::ProgressBar::new(progress)
        .desired_height(height)
        .corner_radius(tc.corner((height / 2.0) as u8))
}

fn compact(ui: &Ui) -> bool {
    ui.available_width() < 620.0
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
        .corner_radius(tc.corner(20))
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
        .corner_radius(tc.corner(16))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                egui::Frame::new()
                    .fill(tc.primary)
                    .corner_radius(tc.corner(18))
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
    painter.rect_filled(rect, tc.corner(12), tc.surface_high);
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
    card(tc)
        .corner_radius(tc.corner(16))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                home_story_icon(ui, tc, icon);
                ui.vertical(|ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(title)
                                .color(tc.on_surface)
                                .size(16.0)
                                .strong(),
                        )
                        .wrap(),
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
        .corner_radius(tc.corner(14))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_min_height(142.0);
            ui.horizontal(|ui| {
                pill(ui, tc, number, number == "03");
                // Wrap long titles: an unwrapped row widens the card past a phone screen.
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .color(tc.on_surface)
                            .size(17.0)
                            .strong(),
                    )
                    .wrap(),
                );
            });
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(RichText::new(body).color(tc.on_surface_var).size(13.0)).wrap(),
            );
        });
}

fn show_how_it_works(ui: &mut Ui, tc: &Tc) {
    const STEPS: [(&str, &str, &str); 4] = [
        (
            "01",
            "Hash on your device",
            "Oxfer reads the selected file in bounded slices and computes its BLAKE3 digest locally. \
             The file is not uploaded to Cloudflare, an Oxfer server, or cloud storage.",
        ),
        (
            "02",
            "Authorize with one private link",
            "The URL fragment contains the endpoint ticket and a fresh bearer capability. Fragments \
             are not sent to the web host, but anyone holding the complete link can receive while \
             the sender keeps sharing.",
        ),
        (
            "03",
            "Open an encrypted peer path",
            "An authenticated iroh connection carries signaling, then browsers prefer a direct \
             WebRTC DataChannel protected by DTLS. If direct ICE fails, encrypted QUIC traffic \
             falls back through the iroh relay without exposing plaintext.",
        ),
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
    card(tc)
        .fill(tc.surface_lowest)
        .corner_radius(tc.corner(16))
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
                    for (number, title, body) in STEPS {
                        process_step(ui, tc, number, title, body);
                        ui.add_space(10.0);
                    }
                } else {
                    for pair in STEPS.chunks(2) {
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
                    .corner_radius(tc.corner(14))
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
                                RichText::new(
                                    "Cloudflare serves only the app shell. STUN and relay \
                                     infrastructure can observe network addresses, timing, and \
                                     approximate traffic volume, but not file contents or the \
                                     capability fragment. Direct peers may learn each other's \
                                     network address through ICE.",
                                )
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
        self.snap.preparing
    }

    /// Home and diagnostics do not revoke a share. Preserve its status and return path.
    fn show_active_share_overview(&mut self, ui: &mut Ui, tc: &Tc) {
        if !self.snap.send.sharing {
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
            Self::show_transfer_attention(ui, tc, true, false, self.snap.wake_lock);
            return_to_share = ui.add(primary_button(tc, "Return to sharing")).clicked();
        });
        if return_to_share {
            self.dispatch(Intent::ReturnToSharing);
        }
        self.show_peers(ui);
        ui.add_space(12.0);
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
        card(&tc)
            .corner_radius(tc.corner(20))
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
                ui.add_space(8.0);
                if ui
                    .add_sized(
                        [width, 44.0],
                        outline_button(&tc, "Start a chat room", tc.outline),
                    )
                    .clicked()
                {
                    self.dispatch(Intent::HostChat { for_agents: false });
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
        let Some(diag) = self.snap.diagnostics.clone() else {
            return;
        };
        let mut intent = None;
        card(&tc).show(ui, |ui| {
            ui.heading("Network diagnostics");
            ui.label("Use the peer test on both devices, then copy each report. Compare their session IDs and timestamps.");
            ui.add_space(8.0);
            if let Some(session) = &diag.session {
                ui.label(format!("Session ID: {session}"));
            }
            if diag.has_target {
                if ui.add_enabled(!diag.busy, Button::new("Test peer connection")).clicked() {
                    intent = Some(DiagnosticsIntent::StartPeerTest);
                }
            } else if diag.peer_link.is_none()
                && diag.completed.is_none()
                && ui.add_enabled(!diag.busy, Button::new("Start peer test")).clicked()
            {
                intent = Some(DiagnosticsIntent::StartPeerTest);
            }
            if let Some(peer_link) = &diag.peer_link {
                ui.label("Keep this page open and send this test-only link to the other device. It cannot download files.");
                if ui.button("Copy peer test link").clicked() {
                    ui.ctx().copy_text(peer_link.clone());
                }
                ui.add(egui::Label::new(RichText::new(peer_link).monospace().size(12.0)).wrap());
                if ui.button("Stop peer test").clicked() {
                    intent = Some(DiagnosticsIntent::StopPeerTest);
                }
            }
            if diag.busy {
                ui.spinner();
                ui.label("Starting or dialling the diagnostic peer…");
            }
            if let Some(outcome) = &diag.outcome {
                ui.label(outcome);
            }
            if let Some(completed) = diag.completed {
                ui.label(format!("Successful incoming peer pings: {completed}"));
            }
            ui.add_space(10.0);
            ui.label("For local checks only, share this link. It contains no peer address or file access code.");
            if ui.button("Copy local checks link").clicked() && !diag.local_link.is_empty() {
                ui.ctx().copy_text(diag.local_link.clone());
            }
            ui.add(egui::Label::new(RichText::new(&diag.local_link).monospace().size(12.0)).wrap());
            ui.add_space(12.0);
            if diag.running {
                ui.spinner();
                ui.label("Testing relay WebSockets and iroh registration (up to 60 seconds if the default fails)…");
            } else if ui.button("Run checks again").clicked() {
                intent = Some(DiagnosticsIntent::RunChecks);
            }
            if let Some(full) = diag.full_report() {
                ui.add_space(12.0);
                ui.label("Review before sharing: the report contains your browser version, time, relay checks and peer test result. It does not include tickets or files.");
                if ui.button("Copy report").clicked() {
                    ui.ctx().copy_text(full.clone());
                }
                for line in full.lines() {
                    ui.add(egui::Label::new(RichText::new(line).monospace().size(12.0)).wrap());
                }
            }
        });
        if let Some(intent) = intent {
            self.dispatch(Intent::Diagnostics(intent));
        }
    }

    fn show_send(&mut self, ui: &mut Ui) {
        let tc = Tc::of(self.theme, ui.visuals().dark_mode);
        let compact = compact(ui);
        let send = self.snap.send.clone();
        let link_copied = self.link_copied();
        let mut copied = false;
        let mut stop = false;
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
            if send.sharing
                || matches!(send.link, LinkState::Ready(_))
                || !send.preparing.is_empty()
            {
                Self::show_transfer_attention(ui, &tc, true, false, self.snap.wake_lock);
                ui.add_space(10.0);
            }

            {
                for f in &send.files {
                    if tc.theme == Theme::Rusty {
                        let file_row = |ui: &mut Ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&f.name)
                                        .color(tc.on_surface)
                                        .size(15.0)
                                        .strong(),
                                )
                                .wrap(),
                            );
                            ui.label(
                                RichText::new(format_size(f.size))
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
                        file_attachment(ui, &tc, &f.name, &format_size(f.size), true);
                        ui.add_space(6.0);
                    }
                }
            }

            // The prepare phase (plan amendment 14): no link while any hash is provisional.
            for preparing in &send.preparing {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "Preparing {}… {:.0} %",
                        preparing.name,
                        preparing.progress * 100.0
                    ))
                    .color(tc.on_surface_var)
                    .size(14.0),
                );
                ui.add(progress_bar(&tc, preparing.progress, 10.0));
            }

            ui.add_space(14.0);
            match &send.link {
                LinkState::Ready(link) => {
                    let link = link.as_str();
                    ui.label(RichText::new("Send this link").color(tc.outline).size(12.0));
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .fill(tc.surface_lowest)
                        .corner_radius(tc.corner(8))
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
                    ui.add_space(8.0);
                    if compact {
                        let width = ui.available_width();
                        if ui
                            .add_sized(
                                [width, 48.0],
                                copy_button(&tc, copy_label(link_copied), link_copied),
                            )
                            .clicked()
                        {
                            ui.ctx().copy_text(link.to_string());
                            copied = true;
                        }
                        if ui
                            .add_sized(
                                [width, 48.0],
                                outline_button(&tc, "Stop sharing", tc.outline),
                            )
                            .clicked()
                        {
                            stop = true;
                        }
                    } else {
                        ui.horizontal(|ui| {
                            // Wide enough for either copy label, so the row stays centered
                            // and does not shift when the link is copied.
                            let copy_width = fit_width(
                                ui,
                                [false, true].map(|copied| primary_text(copy_label(copied))),
                                212.0,
                            );
                            let stop_width = fit_width(ui, [outline_text("Stop sharing")], 140.0);
                            let row_width = copy_width + stop_width + ui.spacing().item_spacing.x;
                            ui.add_space(((ui.available_width() - row_width) / 2.0).max(0.0));
                            if ui
                                .add_sized(
                                    [copy_width, 48.0],
                                    copy_button(&tc, copy_label(link_copied), link_copied),
                                )
                                .clicked()
                            {
                                ui.ctx().copy_text(link.to_string());
                                copied = true;
                            }
                            if ui
                                .add_sized(
                                    [stop_width, 48.0],
                                    outline_button(&tc, "Stop sharing", tc.outline),
                                )
                                .clicked()
                            {
                                stop = true;
                            }
                        });
                    }
                    if link_copied || copied {
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
                        .corner_radius(tc.corner(10))
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
                LinkState::Preparing => {
                    ui.label(
                        RichText::new("Getting your link ready…")
                            .color(tc.outline)
                            .size(12.0),
                    );
                }
                LinkState::Binding => {
                    ui.label(
                        RichText::new("Getting your link ready…")
                            .color(tc.outline)
                            .size(12.0),
                    );
                    ui.add(egui::Spinner::new().size(20.0).color(tc.secondary));
                }
                LinkState::Failed => {
                    retry_share = if compact {
                        let width = ui.available_width();
                        ui.add_sized(
                            [width, 48.0],
                            outline_button(&tc, "Try again", tc.secondary),
                        )
                        .clicked()
                    } else {
                        ui.add(outline_button(&tc, "Try again", tc.secondary))
                            .clicked()
                    };
                }
                LinkState::Idle => {}
            }

            if let Some(err) = &send.error {
                ui.add_space(8.0);
                ui.label(RichText::new(err).color(tc.error).size(12.0));
            }
        });

        if copied {
            // Copying is a gesture: WebKit grants a wake lock only inside one.
            self.core.platform().retry_wake_lock();
            self.link_copied = Some(send.share_id);
        }
        if stop {
            self.dispatch(Intent::Send(SendIntent::StopSharing));
        }
        if retry_share {
            self.dispatch(Intent::Send(SendIntent::RetrySharing));
        }
        self.show_peers(ui);
    }

    /// Per-peer progress for whatever this node is currently serving.
    fn show_peers(&mut self, ui: &mut Ui) {
        let tc = Tc::of(self.theme, ui.visuals().dark_mode);
        let peers = self.snap.send.peers.clone();
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
            if self.snap.send.serving() {
                ui.add_space(8.0);
                Self::show_transfer_attention(ui, &tc, true, false, self.snap.wake_lock);
            }
            for peer in peers {
                ui.add_space(8.0);
                let short = text::short_id(&peer.id).to_string();
                let p = peer.progress;
                let peer_row = |ui: &mut Ui| {
                    ui.label(
                        RichText::new(&short)
                            .color(tc.on_surface_var)
                            .monospace()
                            .size(12.0),
                    );
                    ui.label(
                        RichText::new(text::phase_text(&p.phase))
                            .color(tc.outline)
                            .size(12.0),
                    );
                    ui.label(
                        RichText::new(text::path_badge(&p.phase, p.path))
                            .color(tc.secondary)
                            .size(12.0),
                    );
                    if let Some(speed) = text::transfer_speed(&p) {
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
                    ui.add(progress_bar(&tc, frac, 10.0));
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
                                format_size(p.bytes_done),
                                format_size(p.bytes_total)
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
        let Some(receive) = self.snap.receive.clone() else {
            return;
        };
        let progress = receive.progress.clone();
        let error = receive.error.clone();
        let flags = receive.flags.clone();
        let save_pending = receive.save_pending;
        let opening = receive.opening;
        #[cfg(target_arch = "wasm32")]
        let mut keep_local_copy = receive.keep_local_copy;
        let mut input = receive.input.as_str().to_string();

        let mut submit = false;
        let mut save = false;
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
                Self::show_transfer_attention(ui, &tc, false, awaiting_save, self.snap.wake_lock);
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
                    ui.add_sized([width, 48.0], outline_button(&tc, "Cancel", tc.outline))
                        .clicked()
                } else {
                    ui.add(outline_button(&tc, "Cancel", tc.outline))
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
                inset(&tc, egui::Margin::same(12)).show(ui, |ui| {
                    ui.add_sized(
                        [ui.available_width(), 48.0],
                        egui::TextEdit::singleline(&mut input)
                            .hint_text("Paste a link here")
                            .frame(egui::Frame::new())
                            .font(egui::FontId::monospace(14.0))
                            .text_color(tc.on_surface)
                            .desired_width(f32::INFINITY),
                    );
                });
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
                        RichText::new(text::phase_text(&p.phase))
                            .color(tc.on_surface)
                            .size(15.0)
                            .strong(),
                    );
                    pill(ui, &tc, text::path_badge(&p.phase, p.path), true);
                    if let Some(speed) = text::transfer_speed(p) {
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
                                    RichText::new(format_size(meta.size))
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
                                &format_size(meta.size),
                                false,
                            );
                            ui.add_space(6.0);
                        }
                    }
                    ui.add_space(12.0);
                    ui.label("Oxfer checks the file after it's saved.");
                    #[cfg(target_arch = "wasm32")]
                    ui.add_enabled_ui(!save_pending, |ui| {
                        ui.checkbox(
                            &mut keep_local_copy,
                            "Keep a copy here so I can continue later",
                        );
                        if keep_local_copy {
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
                    let save_label = if keep_local_copy {
                        "Save a copy in this browser"
                    } else {
                        "Choose where to save"
                    };
                    #[cfg(not(target_arch = "wasm32"))]
                    let save_label = "Choose where to save";
                    let save_clicked = if compact {
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
                    if save_clicked {
                        save = true;
                    }
                    if save_pending {
                        #[cfg(target_arch = "wasm32")]
                        let preparing = if keep_local_copy {
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
                    ui.add(progress_bar(&tc, frac, 12.0));
                    ui.label(
                        RichText::new(format!(
                            "{} of {}",
                            format_size(p.bytes_done),
                            format_size(p.bytes_total)
                        ))
                        .color(tc.outline)
                        .size(12.0),
                    );
                }

                if !p.phase.is_terminal() {
                    ui.add_space(10.0);
                    cancel = if compact {
                        let width = ui.available_width();
                        ui.add_sized([width, 48.0], outline_button(&tc, "Cancel", tc.outline))
                            .clicked()
                    } else {
                        ui.add(outline_button(&tc, "Cancel", tc.outline)).clicked()
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

        if input != receive.input.as_str() {
            self.dispatch(Intent::Receive(ReceiveIntent::SetInput(input)));
        }
        #[cfg(target_arch = "wasm32")]
        if keep_local_copy != receive.keep_local_copy {
            self.dispatch(Intent::Receive(ReceiveIntent::SetKeepLocalCopy(
                keep_local_copy,
            )));
        }
        if submit {
            self.dispatch(Intent::Receive(ReceiveIntent::OpenInput));
        }
        if save {
            self.save_click();
        }
        if cancel {
            self.dispatch(Intent::Receive(ReceiveIntent::Cancel));
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn show_local_copies(&mut self, ui: &mut Ui) {
        let copies = self.snap.copies.clone();
        let (entries, error, busy) = (copies.entries, copies.error, copies.busy);
        if entries.is_empty() && error.is_none() {
            return;
        }
        let tc = Tc::of(self.theme, ui.visuals().dark_mode);
        let receiving = self.snap.receive.as_ref().is_some_and(|r| r.active());
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
                    .add_enabled(!busy, outline_button(&tc, "Refresh", tc.outline))
                    .clicked()
                {
                    action = Some(CopiesIntent::Refresh);
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
                            format_size(file.written),
                            format_size(file.size),
                        ));
                        if file.verified {
                            pill(ui, &tc, "READY TO SAVE", true);
                            if ui
                                .add_enabled(
                                    !busy && !receiving,
                                    outline_button(&tc, "Download file", tc.secondary),
                                )
                                .clicked()
                            {
                                action = Some(CopiesIntent::Export(entry.id.clone(), index));
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
                                .add(outline_button(&tc, "Continue download", tc.secondary))
                                .clicked()
                        {
                            resume = Some(entry.id.clone());
                        }
                        if self.confirm_discard.as_ref() == Some(&entry.id) {
                            ui.label("Delete this copy and its saved progress?");
                            if ui
                                .add(outline_button(&tc, "Delete permanently", tc.error))
                                .clicked()
                            {
                                action = Some(CopiesIntent::Discard(entry.id.clone()));
                                self.confirm_discard = None;
                            }
                            if ui.add(outline_button(&tc, "Keep", tc.outline)).clicked() {
                                self.confirm_discard = None;
                            }
                        } else if ui
                            .add(outline_button(&tc, "Delete saved copy", tc.outline))
                            .clicked()
                        {
                            self.confirm_discard = Some(entry.id.clone());
                        }
                    });
                });
            }
            if let Some(error) = &error {
                ui.label(RichText::new(error).color(tc.error).size(13.0));
            }
        });
        if let Some(id) = resume {
            self.dispatch(Intent::ResumeLocalCopy(id));
        }
        if let Some(action) = action {
            self.dispatch(Intent::Copies(action));
        }
    }

    fn show_received_files(&mut self, ui: &mut Ui) {
        let tc = Tc::of(self.theme, ui.visuals().dark_mode);
        let compact = compact(ui);
        let files = &self.snap.received;
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
                            RichText::new(format_size(f.size))
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
                        &format!("{} · {}", format_size(f.size), f.when),
                        false,
                    );
                }
                ui.label(
                    RichText::new(&f.location)
                        .color(tc.on_surface_var)
                        .size(11.0),
                );
                if let Some(rate) = f.average_bytes_per_sec {
                    let speed = text::format_rate(rate);
                    ui.label(
                        RichText::new(format!("Avg download (this session): {speed}"))
                            .color(tc.on_surface_var)
                            .size(12.0),
                    );
                }
                pill(ui, &tc, text::transfer_path_text(f.path), true);
            }
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
        // Monospace labels run wider, so the full Send header needs more room before its
        // controls stop crowding the mode pill.
        let compact = ui.available_width() < if tc.theme.monospace() { 760.0 } else { 680.0 };
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
                        .tint(tc.logo_tint)
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
                        .corner_radius(tc.corner(16))
                        .min_size(egui::vec2(32.0, 44.0)),
                    )
                    .on_hover_text("Show this build's version in Terminal Output")
                    .clicked()
            {
                self.show_terminal_view = true;
            }
            let activity = self.snap.activity;
            let at_home = activity == Activity::Home;
            let label = match activity {
                Activity::Home if self.snap.send.sharing => "SHARING",
                Activity::Home => "READY",
                #[cfg(target_arch = "wasm32")]
                Activity::Diagnostics => "DIAG",
                Activity::Send => "TX",
                Activity::Receive => "RX",
                Activity::Chat => "CHAT",
            };
            if !compact {
                ui.add_space(8.0);
                pill(ui, tc, label, activity == Activity::Receive);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(CONTROL_EDGE_INSET);
                let dark = ui.visuals().dark_mode;
                // Wide enough for either label, so the toggle does not jump when flipped.
                if !compact
                    && ui
                        .add_sized(
                            [
                                fit_width(
                                    ui,
                                    [false, true].map(|dark| outline_text(mode_toggle_label(dark))),
                                    108.0,
                                ),
                                44.0,
                            ],
                            outline_button(tc, mode_toggle_label(dark), tc.outline),
                        )
                        .clicked()
                {
                    let next_dark = !dark;
                    Self::apply_theme(ctx, ui, self.theme, next_dark);
                    self.last_dark_mode = Some(next_dark);
                }
                // On narrow screens the menu also holds the controls that did not fit.
                ui.menu_button(if compact { "More" } else { "Theme" }, |ui| {
                    ui.label(RichText::new("Appearance").strong());
                    let mut picked = false;
                    for theme in Theme::ALL {
                        picked |= ui
                            .selectable_value(&mut self.theme, theme, theme.label())
                            .clicked();
                    }
                    if picked {
                        #[cfg(target_arch = "wasm32")]
                        eframe::web::storage::local_storage_set(
                            BROWSER_THEME_KEY,
                            self.theme.storage_value(),
                        );
                        self.last_theme = None;
                        ctx.request_repaint();
                        ui.close();
                    }
                    if compact && ui.button(mode_toggle_label(dark)).clicked() {
                        let next_dark = !dark;
                        Self::apply_theme(ctx, ui, self.theme, next_dark);
                        self.last_dark_mode = Some(next_dark);
                        ui.close();
                    }
                    if compact && ui.button("Show build version").clicked() {
                        self.show_terminal_view = true;
                        ui.close();
                    }
                    if compact {
                        self.agents_room_button(ui);
                    }
                });
                if !compact {
                    self.show_agents_menu(ui);
                }
                if !compact
                    && !at_home
                    && self.snap.can_pick_files
                    && ui.add(primary_button(tc, "Choose File")).clicked()
                {
                    self.pick_file();
                }
                let preparing = self.is_preparing_share();
                if !at_home {
                    // The core refuses Home while a file is hashing; say why the button is off.
                    // silently abort file preparation while the endpoint remains live.
                    let home = ui
                        .add_enabled_ui(!preparing, |ui| {
                            ui.add_sized(
                                [if compact { 52.0 } else { 84.0 }, 44.0],
                                outline_button(tc, "Home", tc.outline),
                            )
                        })
                        .inner
                        .on_disabled_hover_text("Wait for files to finish preparing");
                    if home.clicked() {
                        self.dispatch(Intent::GoHome);
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
                            self.dispatch(Intent::OpenReceive);
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
            #[cfg(target_arch = "wasm32")]
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(CONTROL_EDGE_INSET);
                let diagnostics = ui
                    .add_enabled_ui(!self.is_preparing_share(), |ui| {
                        let width = fit_width(ui, [outline_text("Diags")], 72.0);
                        ui.add_sized([width, 44.0], outline_button(tc, "Diags", tc.outline))
                    })
                    .inner
                    .on_disabled_hover_text("Wait for files to finish preparing");
                if diagnostics.clicked() {
                    self.dispatch(Intent::OpenDiagnostics);
                }
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
            #[cfg(not(target_arch = "wasm32"))]
            if !compact && !self.show_terminal_view {
                if let Ok(logs) = logging::terminal_buffer().lock() {
                    let msg = logs
                        .back()
                        .cloned()
                        .unwrap_or_else(|| "No logs yet…".into());
                    ui.label(RichText::new(msg).color(tc.outline).monospace().size(12.0));
                }
            }
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

// ── Chat rooms ───────────────────────────────────────────────────────────────

impl P2PTransfer {
    fn show_agents_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("For agents", |ui| {
            ui.set_max_width(340.0);
            ui.label(RichText::new("Let AI agents talk and send files").strong());
            ui.label(
                "Start a room and copy the agent link. Give it to your agent, and send the \
                 same link to the other person for their agent. The link tells an agent how \
                 to join this room, talk, and send files through it.",
            );
            ui.add_space(6.0);
            match &self.snap.chat {
                Some(chat) if chat.links.is_some() => {
                    if ui.button("Copy for agent").clicked() {
                        self.chat_view.copy_for_agent(ui.ctx(), chat);
                        ui.close();
                    }
                }
                _ => self.agents_room_button(ui),
            }
        });
    }

    /// Menu entry that opens a room for agents and closes the menu.
    fn agents_room_button(&mut self, ui: &mut Ui) {
        if ui
            .add_enabled(
                // Like "Choose File", never offered over a running receive: opening a room
                // replaces the mode and would drop the download.
                !self.is_preparing_share()
                    && !self
                        .snap
                        .receive
                        .as_ref()
                        .is_some_and(|r| r.progress.is_some()),
                Button::new("Start a room for agents"),
            )
            .clicked()
        {
            self.dispatch(Intent::HostChat { for_agents: true });
            ui.close();
        }
    }

    fn show_chat(&mut self, ui: &mut Ui) {
        let tc = Tc::of(self.theme, ui.visuals().dark_mode);
        let compact = compact(ui);
        let Some(chat) = self.snap.chat.clone() else {
            return;
        };
        self.chat_view.sync(&chat);
        let keyboard_inset = self.keyboard_inset;
        let state = &mut self.chat_view;
        let mut leave = false;
        let mut submit: Option<String> = None;
        let host = chat.role == ChatRole::Host;
        card(&tc).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading(RichText::new("Chat room").color(tc.on_surface));
                pill(ui, &tc, "ENCRYPTED", true);
            });
            ui.add_space(6.0);
            match &chat.status {
                ChatStatus::Starting => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(if host {
                            "Opening the room…"
                        } else {
                            "Connecting to the room…"
                        });
                    });
                }
                ChatStatus::Open => {
                    ui.label(
                        RichText::new(if host {
                            "Keep this page open: the room closes when you leave it."
                        } else {
                            "Keep this page open while you talk."
                        })
                        .color(tc.on_surface_var)
                        .size(13.0),
                    );
                }
                ChatStatus::Failed(message) => {
                    ui.label(RichText::new(message.as_str()).color(tc.error));
                }
                ChatStatus::Closed => {
                    ui.label(RichText::new("The room is closed.").color(tc.on_surface_var));
                }
            }

            if let Some(links) = &chat.links {
                let link = links.link.as_str();
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Send this link to the person you want to talk to.")
                        .color(tc.on_surface)
                        .strong(),
                );
                link_box(ui, &tc, link);
                ui.add_space(6.0);
                let copy_width = fit_width(
                    ui,
                    [false, true].map(|copied| primary_text(copy_label(copied))),
                    200.0,
                );
                if ui
                    .add_sized([copy_width, 44.0], {
                        let copied = state.copied == Some(Copied::Link);
                        copy_button(&tc, copy_label(copied), copied)
                    })
                    .clicked()
                {
                    ui.ctx().copy_text(link.to_string());
                    state.copied = Some(Copied::Link);
                }
                ui.label(
                    RichText::new(
                        "Anyone with this link can join while this page is open. Send it only \
                         to people you trust.",
                    )
                    .color(tc.error)
                    .size(12.0),
                );
                ui.add_space(12.0);
                show_agent_prompt(ui, &tc, &chat, state);
            }

            ui.add_space(12.0);
            if !chat.members.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    ui.label(
                        RichText::new("In the room")
                            .color(tc.on_surface_var)
                            .size(13.0),
                    );
                    for member in &chat.members {
                        member_chip(ui, &tc, member, chat.you.as_deref() == Some(member));
                    }
                });
            }
            ui.add_space(6.0);
            inset(&tc, egui::Margin::same(10)).show(ui, |ui| {
                ui.set_min_height(320.0);
                egui::ScrollArea::vertical()
                    .max_height(if compact { 460.0 } else { 680.0 })
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        if chat.lines.is_empty() {
                            ui.label(RichText::new("No messages yet.").color(tc.on_surface_var));
                        }
                        let you = chat.you.as_deref();
                        let style = ui.style().clone();
                        for line in &chat.lines {
                            match &line.from {
                                None => {
                                    ui.label(
                                        RichText::new(&line.text)
                                            .italics()
                                            .color(tc.on_surface_var)
                                            .size(13.0),
                                    );
                                }
                                Some(from) => {
                                    // One wrapped galley for name and text: two labels in a
                                    // wrapped row misplace a long unbroken link, drawing part of
                                    // it outside the box.
                                    let mut job = egui::text::LayoutJob::default();
                                    RichText::new(format!("{from}  "))
                                        .strong()
                                        .size(15.0)
                                        .color(member_color(
                                            &tc,
                                            ui.visuals().dark_mode,
                                            from,
                                            Some(from.as_str()) == you,
                                        ))
                                        .append_to(
                                            &mut job,
                                            &style,
                                            egui::FontSelection::Default,
                                            egui::Align::LEFT,
                                        );
                                    RichText::new(&line.text).color(tc.on_surface).append_to(
                                        &mut job,
                                        &style,
                                        egui::FontSelection::Default,
                                        egui::Align::LEFT,
                                    );
                                    // The transcript is in a centered column; messages read
                                    // from the left edge.
                                    ui.with_layout(
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| ui.add(egui::Label::new(job).wrap()),
                                    );
                                }
                            }
                        }
                    });
            });
            ui.add_space(8.0);
            let open = chat.open();
            ui.horizontal(|ui| {
                let send_width = 84.0;
                let input_width =
                    (ui.available_width() - send_width - ui.spacing().item_spacing.x).max(80.0);
                let response = inset(&tc, egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.add_enabled(
                            open,
                            egui::TextEdit::singleline(&mut state.input)
                                .id(egui::Id::new("chat_input"))
                                .hint_text("Type a message and press Enter")
                                .frame(egui::Frame::new())
                                .text_color(tc.on_surface)
                                .desired_width(input_width - 24.0)
                                // Keep focus on Enter: losing it closes a phone keyboard,
                                // and refocusing for the next message reopens it.
                                .return_key(None),
                        )
                    })
                    .inner;
                if open && state.focus_input {
                    response.request_focus();
                    state.focus_input = false;
                }
                let inset = keyboard_inset;
                if response.has_focus()
                    && (response.gained_focus() || inset != state.input_seen_inset)
                {
                    response.scroll_to_me(Some(egui::Align::Center));
                    state.input_seen_inset = inset;
                }
                let enter = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let clicked = ui
                    .add_enabled_ui(open, |ui| {
                        ui.add_sized([send_width, 40.0], primary_button(&tc, "Send"))
                    })
                    .inner
                    .clicked();
                if open && (enter || clicked) && !state.input.trim().is_empty() {
                    submit = Some(std::mem::take(&mut state.input));
                    // The Send button took focus; Enter left it on the input.
                    state.focus_input = clicked;
                }
            });
            ui.add_space(10.0);
            let leave_label = chat.leave_label();
            let leave_width = fit_width(ui, [outline_text(leave_label)], 140.0);
            if ui
                .add_sized(
                    [leave_width, 44.0],
                    outline_button(&tc, leave_label, tc.outline),
                )
                .clicked()
            {
                leave = true;
            }
        });
        if let Some(text) = submit {
            self.dispatch(Intent::Chat(ChatIntent::Say(text)));
        }
        if leave {
            self.dispatch(Intent::GoHome);
        }
    }
}

/// Names the transcript and roster draw in, so a speaker is easy to pick out. You are the
/// theme's primary colour; everyone else gets a hue of their own from their name. Phosphor
/// is one phosphor colour by design, so its members differ in brightness instead.
fn member_color(tc: &Tc, dark: bool, name: &str, you: bool) -> Color32 {
    if you {
        return tc.primary;
    }
    const DARK: [Color32; 7] = [
        Color32::from_rgb(255, 180, 84),  // amber
        Color32::from_rgb(177, 140, 255), // violet
        Color32::from_rgb(79, 214, 190),  // teal
        Color32::from_rgb(255, 143, 177), // pink
        Color32::from_rgb(155, 225, 93),  // lime
        Color32::from_rgb(255, 209, 102), // yellow
        Color32::from_rgb(122, 162, 255), // periwinkle
    ];
    const LIGHT: [Color32; 7] = [
        Color32::from_rgb(166, 86, 0),
        Color32::from_rgb(107, 63, 214),
        Color32::from_rgb(0, 122, 104),
        Color32::from_rgb(194, 24, 91),
        Color32::from_rgb(63, 125, 18),
        Color32::from_rgb(138, 109, 0),
        Color32::from_rgb(47, 85, 201),
    ];
    // FNV-1a: stable across sessions and peers, so a name keeps its colour.
    let hash = name.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    if tc.theme == Theme::Phosphor {
        let shades = [tc.secondary, tc.on_surface, tc.on_surface_var];
        return shades[hash as usize % shades.len()];
    }
    let palette = if dark { DARK } else { LIGHT };
    palette[hash as usize % palette.len()]
}

/// One member in the "In the room" roster: their name in their colour, in a small pill.
fn member_chip(ui: &mut Ui, tc: &Tc, name: &str, you: bool) {
    let color = member_color(tc, ui.visuals().dark_mode, name, you);
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.6)))
        .corner_radius(tc.corner(12))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            let label = if you {
                format!("{name} (you)")
            } else {
                name.to_string()
            };
            ui.label(RichText::new(label).strong().size(13.0).color(color));
        });
}

/// The agent brief as a prompt card, the way agent docs present one: what it is, one copy
/// button, and the full text folded away until someone wants to read it.
fn show_agent_prompt(ui: &mut Ui, tc: &Tc, chat: &ChatSnapshot, state: &mut ChatView) {
    let Some(links) = &chat.links else {
        return;
    };
    let copied = state.copied == Some(Copied::AgentLink);
    let mut copy = false;
    let mut toggle = false;
    inset(tc, egui::Margin::same(12)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("💻").size(20.0).color(tc.secondary));
            ui.label(
                RichText::new("Prompt for your AI agent")
                    .strong()
                    .size(15.0)
                    .color(tc.on_surface),
            );
        });
        ui.label(
            RichText::new(
                "Paste it into Claude Code, Codex, Warp or any agent that can run commands. It \
                 tells the agent how to join this room, talk, and send files. Each agent needs \
                 its own copy.",
            )
            .color(tc.on_surface_var)
            .size(13.0),
        );
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            const LABELS: [&str; 2] = ["📋 Copy prompt", "✔ Copied"];
            let label = LABELS[usize::from(copied)];
            let width = fit_width(ui, LABELS.map(primary_text), 150.0);
            let button = if chat.for_agents {
                copy_button(tc, label, copied)
            } else {
                outline_button(tc, label, if copied { tc.secondary } else { tc.outline })
            };
            copy = ui.add_sized([width, 40.0], button).clicked();
            let show = if state.prompt_open {
                "⏷ Hide prompt"
            } else {
                "⏵ Show prompt"
            };
            toggle = ui
                .add_sized(
                    [0.0, 40.0],
                    Button::new(RichText::new(show).size(14.0).color(tc.on_surface_var))
                        .frame(false),
                )
                .clicked();
        });
        if state.prompt_open {
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(tc.bg)
                .stroke(Stroke::new(1.0, tc.outline_var))
                .corner_radius(tc.corner(8))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("agent_prompt")
                        .max_height(240.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(agent_prompt(links.agent.as_str()))
                                        .monospace()
                                        .size(12.0)
                                        .color(tc.on_surface),
                                )
                                .wrap()
                                .selectable(true),
                            );
                        });
                });
        }
    });
    if copy {
        state.copy_for_agent(ui.ctx(), chat);
    }
    if toggle {
        state.prompt_open = !state.prompt_open;
    }
}

/// How much of the canvas, in points, an on-screen keyboard covers right now.
#[cfg(target_arch = "wasm32")]
fn keyboard_inset(ctx: &egui::Context) -> f32 {
    // The browser reports no event egui sees while the keyboard slides, so look again soon
    // whenever text input is active on a touch screen; elsewhere there is no such keyboard.
    if coarse_pointer() && ctx.egui_wants_keyboard_input() {
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
    }
    let Some(window) = web_sys::window() else {
        return 0.0;
    };
    let Some(viewport) = window.visual_viewport() else {
        return 0.0;
    };
    // Pinch zoom shrinks the visual viewport too; that is not a keyboard.
    if viewport.scale() > 1.01 {
        return 0.0;
    }
    let layout = window
        .inner_height()
        .ok()
        .and_then(|h| h.as_f64())
        .unwrap_or_default();
    let covered = (layout - viewport.height() - viewport.offset_top()).max(0.0) as f32;
    // A collapsing URL bar moves the viewport by a few dozen pixels; a keyboard by hundreds.
    if covered < 100.0 {
        0.0
    } else {
        covered / ctx.zoom_factor()
    }
}

/// Whether the main pointer is a finger (a phone or tablet), checked once per page load.
#[cfg(target_arch = "wasm32")]
fn coarse_pointer() -> bool {
    thread_local! {
        static COARSE: bool = web_sys::window()
            .and_then(|window| window.match_media("(pointer: coarse)").ok().flatten())
            .is_some_and(|query| query.matches());
    }
    COARSE.with(|coarse| *coarse)
}

#[cfg(not(target_arch = "wasm32"))]
fn keyboard_inset(_ctx: &egui::Context) -> f32 {
    0.0
}

/// Parks the browser's hidden text input at the top of the canvas on touch screens.
///
/// eframe puts that input at the text cursor. A focused input low on the page sits under a
/// phone keyboard, so the browser pans the page to reveal it; the keyboard inset then
/// changes, the layout moves the cursor, and the page pans back, which flickers. At the top
/// the input is always visible and the page never pans. Desktop keeps the real position so
/// an IME candidate window still opens next to the cursor.
#[cfg(target_arch = "wasm32")]
fn keep_text_agent_on_screen(ctx: &egui::Context) {
    if !coarse_pointer() {
        return;
    }
    ctx.output_mut(|output| {
        if let Some(ime) = &mut output.ime {
            let x = ime.cursor_rect.center().x;
            ime.cursor_rect = egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(1.0, 1.0));
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn keep_text_agent_on_screen(_ctx: &egui::Context) {}

/// The quiet, recessed frame used for links, message lists and inputs.
fn inset(tc: &Tc, margin: egui::Margin) -> egui::Frame {
    egui::Frame::new()
        .fill(tc.surface_lowest)
        .corner_radius(tc.corner(10))
        .stroke(Stroke::new(1.0, tc.outline_var))
        .inner_margin(margin)
}

/// A wrapped, selectable monospace link inside an inset frame.
fn link_box(ui: &mut Ui, tc: &Tc, link: &str) {
    inset(tc, egui::Margin::same(10)).show(ui, |ui| {
        ui.add(
            egui::Label::new(
                RichText::new(link)
                    .monospace()
                    .size(12.0)
                    .color(tc.on_surface),
            )
            .wrap()
            .selectable(true),
        );
    });
}

impl eframe::App for P2PTransfer {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Non-drawing per-frame work: feed drops in, tick the core, take its snapshot.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.accept_dropped_files(ctx);
        self.core.tick();
        self.snap = self.core.snapshot();
        if self.snap.live {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;

        // Apply the theme here, not in `logic()`: the root `Ui` is built before `logic()`
        // runs, so a context-only update would leave this frame with the previous palette.
        let style = ctx.global_style();
        let dark = style.visuals.dark_mode;
        // egui swaps to its other dark/light style when the system theme changes. If that
        // style was never themed, its panel fill is not ours even when `dark` is unchanged.
        if self.last_dark_mode != Some(dark)
            || self.last_theme != Some(self.theme)
            || style.visuals.panel_fill != Tc::of(self.theme, dark).bg
        {
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
        // A phone keyboard covers the bottom of the canvas without resizing it. Resizing the
        // page with it re-lays out the whole app on every keyboard step and makes it flicker,
        // so reserve the covered strip instead and keep everything above it.
        // The panel is always added, even at zero height: adding it only while the keyboard
        // is up would shift the ids of every widget after it, and the focused input would lose
        // focus and close the keyboard again.
        self.keyboard_inset = keyboard_inset(ctx);
        egui::Panel::bottom("keyboard_inset")
            .exact_size(self.keyboard_inset)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(tc.bg))
            .show(ui, |_| {});
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
                // Fixed to the panel, so the content scrolls across it like a screen overlay.
                paint_backdrop(ui, &tc);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let content_width = ui.available_width().min(1040.0);
                    ui.vertical_centered(|ui| {
                        ui.set_width(content_width);
                        let activity = self.snap.activity;
                        match activity {
                            Activity::Home => self.show_home(ui),
                            #[cfg(target_arch = "wasm32")]
                            Activity::Diagnostics => self.show_diagnostics(ui),
                            Activity::Send => self.show_send(ui),
                            Activity::Receive => self.show_receive(ui),
                            Activity::Chat => self.show_chat(ui),
                        }
                        #[cfg(target_arch = "wasm32")]
                        if !matches!(activity, Activity::Diagnostics | Activity::Chat) {
                            self.show_received_files(ui);
                            self.show_local_copies(ui);
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        if activity != Activity::Chat {
                            self.show_received_files(ui);
                        }
                    });
                });
            });
        keep_text_agent_on_screen(ctx);
        paint_press_feedback(ctx, &tc);
        if self.snap.can_pick_files {
            paint_drop_target(ctx, &tc);
        }
    }
}

/// While files are dragged over the window, cover it with a target that says a drop sends them.
fn paint_drop_target(ctx: &egui::Context, tc: &Tc) {
    if ctx.input(|i| i.raw.hovered_files.is_empty()) {
        return;
    }
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("file_drop_target"),
    ));
    painter.rect_filled(screen, 0.0, tc.bg.gamma_multiply(0.85));
    let target = screen.shrink(16.0);
    let border = egui::Shape::dashed_line(
        &[
            target.left_top(),
            target.right_top(),
            target.right_bottom(),
            target.left_bottom(),
            target.left_top(),
        ],
        Stroke::new(2.0, tc.primary),
        10.0,
        6.0,
    );
    painter.extend(border);
    // On a card of its own, so the faded page behind cannot run through the words.
    let label = painter.layout_no_wrap(
        "Drop files to send".to_string(),
        egui::FontId::proportional(24.0),
        tc.on_surface,
    );
    let card = egui::Rect::from_center_size(target.center(), label.size() + egui::vec2(48.0, 28.0));
    painter.rect(
        card,
        tc.corner(tc.theme.card_radius()),
        tc.surface,
        Stroke::new(1.0, tc.primary),
        egui::StrokeKind::Inside,
    );
    painter.galley(card.center() - label.size() / 2.0, label, tc.on_surface);
}

/// How long a clicked button keeps a fading tint after the pointer lets go.
const PRESS_FLASH_SECS: f64 = 0.3;

/// Shows that a click landed: a tint on any clickable widget while it is held, fading out
/// after the click. Most buttons here set their own fill, which egui keeps in every state,
/// and a quick click is held for a frame or two, so without this nothing visibly changes.
fn paint_press_feedback(ctx: &egui::Context, tc: &Tc) {
    let flash_key = egui::Id::new("press_flash");
    let now = ctx.input(|i| i.time);
    let held = ctx.input(|i| i.pointer.primary_down());
    let (clicked, hovered) = ctx.interaction_snapshot(|s| (s.clicked, s.hovered.clone()));
    if let Some(id) = clicked {
        ctx.data_mut(|d| d.insert_temp(flash_key, (id, now)));
    }
    let flash = ctx
        .data(|d| d.get_temp::<(egui::Id, f64)>(flash_key))
        .filter(|(_, at)| now - at < PRESS_FLASH_SECS);
    let mut tinted: Vec<(egui::Id, f32)> = Vec::new();
    if held {
        tinted.extend(hovered.into_iter().map(|id| (id, 1.0)));
    }
    if let Some((id, at)) = flash {
        let left = (1.0 - (now - at) / PRESS_FLASH_SECS) as f32;
        tinted.push((id, left));
        ctx.request_repaint();
    }
    for (id, strength) in tinted {
        let Some(response) = ctx.read_response(id) else {
            continue;
        };
        // Buttons, links and toggles; not text fields, which sense drags too.
        if response.sense != egui::Sense::click() || !response.enabled() {
            continue;
        }
        let alpha = (56.0 * strength) as u8;
        let tint = Color32::from_rgba_unmultiplied(
            tc.on_surface.r(),
            tc.on_surface.g(),
            tc.on_surface.b(),
            alpha,
        );
        ctx.layer_painter(response.layer_id)
            .with_clip_rect(response.interact_rect)
            .rect_filled(
                response.rect,
                CornerRadius::same(tc.theme.control_radius()),
                tint,
            );
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
        assert_eq!(Theme::from_storage_value("phosphor"), Some(Theme::Phosphor));
        assert_eq!(Theme::from_storage_value("unknown"), None);
        for theme in Theme::ALL {
            let encoded = postcard::to_stdvec(&theme).unwrap();
            let restored: Theme = postcard::from_bytes(&encoded).unwrap();
            assert_eq!(restored, theme);
        }
        for dark in [false, true] {
            for (index, a) in Theme::ALL.into_iter().enumerate() {
                for b in Theme::ALL.into_iter().skip(index + 1) {
                    let (a, b) = (Tc::of(a, dark), Tc::of(b, dark));
                    assert_ne!(a.primary, b.primary);
                    assert_ne!(a.bg, b.bg);
                }
            }
        }
    }

    #[test]
    fn local_test_phosphor_shape_and_type_do_not_leak_into_other_themes() {
        for dark in [false, true] {
            let phosphor = Tc::of(Theme::Phosphor, dark);
            assert_eq!(phosphor.corner(20), CornerRadius::same(4));
            assert_eq!(phosphor.corner(2), CornerRadius::same(2));
            for theme in [Theme::Rusty, Theme::Clean] {
                let tc = Tc::of(theme, dark);
                assert_eq!(tc.corner(20), CornerRadius::same(20));
                assert_eq!(tc.glow, egui::Shadow::NONE);
                assert_eq!(tc.backdrop, Backdrop::Plain);
                assert!(!theme.monospace());
            }
        }

        // Switching away from Phosphor must restore proportional text in the same context.
        let ctx = egui::Context::default();
        for (theme, family) in [
            (Theme::Phosphor, egui::FontFamily::Monospace),
            (Theme::Clean, egui::FontFamily::Proportional),
        ] {
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                P2PTransfer::apply_theme(&ctx, ui, theme, true);
                for style in [ctx.global_style(), ui.style().clone()] {
                    for text_style in [
                        egui::TextStyle::Body,
                        egui::TextStyle::Button,
                        egui::TextStyle::Heading,
                    ] {
                        assert_eq!(text_style.resolve(&style).family, family, "{theme:?}");
                    }
                    assert_eq!(
                        egui::TextStyle::Monospace.resolve(&style).family,
                        egui::FontFamily::Monospace
                    );
                }
            });
            output.drop_without_applying_deltas();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn local_test_theme_storage_restores_old_settings_and_new_choice() {
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

    /// A dropped file queues exactly like a picked one; a dropped folder is refused by name.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn local_test_dropped_files_queue_like_picks() {
        #[derive(Debug)]
        struct TestDrop(std::path::PathBuf);

        impl egui::DroppedFile for TestDrop {
            fn path(&self) -> &std::path::Path {
                &self.0
            }

            fn bytes(&self) -> Result<Vec<u8>, String> {
                std::fs::read(&self.0).map_err(|e| e.to_string())
            }
        }

        let dir = crate::tests::TestDir::new("drop");
        let folder = dir.0.join("photos");
        std::fs::create_dir(&folder).unwrap();
        let file = dir.0.join("notes.txt");
        std::fs::write(&file, b"hello").unwrap();
        let drops = || egui::RawInput {
            dropped_files: vec![
                Arc::new(TestDrop(file.clone())) as egui::DroppedFileHandle,
                Arc::new(TestDrop(folder.clone())),
            ],
            ..Default::default()
        };

        let ctx = egui::Context::default();
        let mut app = P2PTransfer::default();
        ctx.run_ui(drops(), |ui| app.accept_dropped_files(ui.ctx()))
            .drop_without_applying_deltas();
        // The file waits for the next tick; the folder is refused by name.
        assert_eq!(app.core.picks().len(), 1);
        let error = app.core.snapshot().send.error.unwrap();
        assert!(
            error.contains("photos") && error.contains(crate::file_io::FOLDER_REFUSED),
            "{error}"
        );

        // Receive has no picker, so it takes no drops either.
        let mut app = P2PTransfer::default();
        app.dispatch(Intent::OpenReceive);
        ctx.run_ui(drops(), |ui| app.accept_dropped_files(ui.ctx()))
            .drop_without_applying_deltas();
        assert!(app.core.picks().is_empty());
        assert!(app.core.snapshot().send.error.is_none());
    }

    #[test]
    fn local_test_narrow_header_fits_send_and_receive_modes() {
        let width = 320.0;
        for theme in Theme::ALL {
            for (intent, activity) in [
                (Intent::ReturnToSharing, Activity::Send),
                (Intent::OpenReceive, Activity::Receive),
            ] {
                let ctx = egui::Context::default();
                let mut app = P2PTransfer {
                    theme,
                    ..P2PTransfer::default()
                };
                app.dispatch(intent);
                assert_eq!(app.snap.activity, activity);
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
        for theme in Theme::ALL {
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
}
