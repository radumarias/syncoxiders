//! Components. Each route has a screen module; this module has the page frame (header,
//! responsive navigation, main, footer) and the widgets screens share.
//!
//! Rules for screens: read state only from `core.snap()`, act only through
//! `core.dispatch(...)`, and take user-facing sentences from `p2p_transfer::sessions::text`
//! where one exists, so egui and the web say the same thing.
//!
//! The frame is one component tree that reflows (#68): under 600px a top bar plus a
//! bottom tab bar (Home, Send, Receive, Copies, More), 600–899px a side rail, and 900px
//! and up a top nav. Which navigation shows is decided by CSS alone (`frame-*` classes in
//! `assets/oxfer.css`), so there is no width-dependent state here. Screens that pin an
//! action to the bottom on phones offset it by `var(--frame-bottom)`, and can size
//! themselves with `@container frame-main (…)`.

use dioxus::prelude::*;
use p2p_transfer::sessions::{PickedFile, Platform as _, WakeLock, WebPlatform};
use p2p_transfer::theme::{css_variables, Theme};
use wasm_bindgen::JsCast as _;

use crate::appearance::{use_appearance, use_appearance_provider, Mode};
use crate::core::{use_core, use_core_provider};
use crate::route::{use_route_sync, use_router, Route};

mod chat;
mod copies;
mod diags;
mod home;
mod receive;
mod send;

const BASE_CSS: &str = include_str!("../../assets/oxfer.css");
const FILE_INPUT_ID: &str = "oxfer-file-input";
const MORE_MENU_ID: &str = "frame-more";

#[component]
pub fn App() -> Element {
    let core = use_core_provider();
    use_appearance_provider();
    let router = use_route_sync(core);
    let snap = core.snap();
    let can_pick = snap.read().can_pick_files;
    let mut dragging = use_signal(|| false);

    let screen = match router.current() {
        Route::Home => rsx! { home::Home {} },
        Route::Send => rsx! { send::Send {} },
        Route::Receive => rsx! { receive::Receive {} },
        Route::Chat => rsx! { chat::Chat {} },
        Route::Copies => rsx! { copies::Copies {} },
        Route::Diags => rsx! { diags::Diags {} },
        Route::NotFound => rsx! { NotFound {} },
    };

    rsx! {
        document::Style { {css_variables()} }
        document::Style { {BASE_CSS} }
        button { r#type: "button", class: "skip-link", onclick: skip_to_main, "Skip to content" }
        div {
            class: "page frame-page",
            class: if dragging() { "dragging" },
            ondragover: move |event| {
                if can_pick {
                    event.prevent_default();
                    dragging.set(true);
                }
            },
            ondragleave: move |_| dragging.set(false),
            ondrop: move |event| {
                dragging.set(false);
                if !can_pick {
                    return;
                }
                event.prevent_default();
                use dioxus::web::WebEventExt as _;
                let Some(files) = event
                    .data()
                    .try_as_web_event()
                    .and_then(|drag| drag.data_transfer())
                    .and_then(|transfer| transfer.files())
                else {
                    return;
                };
                let files: Vec<web_sys::File> =
                    (0..files.length()).filter_map(|index| files.get(index)).collect();
                if !files.is_empty() {
                    core.picks().push_dropped(files);
                    core.refresh();
                }
            },
            Header {}
            main { id: "main", tabindex: "-1", class: "content frame-main", {screen} }
            Footer {}
            TabBar {}
            if dragging() {
                // Decoration only: the drop itself is handled on `.page`, and the overlay
                // ignores the pointer so it never steals the drag.
                div { class: "frame-drop", aria_hidden: "true",
                    p { "Drop files anywhere to start sharing" }
                }
            }
        }
        FileInput {}
    }
}

/// A button, not `<a href="#main">`: the fragment belongs to share links, and the core
/// would read `#main` as a damaged one.
fn skip_to_main(_: Event<MouseData>) {
    if let Some(main) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("main"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = main.focus();
    }
}

// ── Frame ────────────────────────────────────────────────────────────────────

/// One navigation destination: where it goes, what it is called, and its icon.
#[derive(Clone, Copy, PartialEq)]
struct NavItem {
    route: Route,
    label: &'static str,
    icon: Icon,
}

const fn item(route: Route, label: &'static str, icon: Icon) -> NavItem {
    NavItem { route, label, icon }
}

/// The rail (tablet) and top nav (desktop) list every destination.
const NAV_ITEMS: [NavItem; 6] = [
    item(Route::Home, "Home", Icon::Home),
    item(Route::Send, "Send", Icon::Send),
    item(Route::Receive, "Receive", Icon::Receive),
    item(Route::Chat, "Chat", Icon::Chat),
    item(Route::Copies, "Copies", Icon::Copies),
    item(Route::Diags, "Diagnostics", Icon::Diags),
];

/// The phone tab bar keeps the four core flows in thumb reach; the rest sit under More.
const TAB_ITEMS: [NavItem; 4] = [
    item(Route::Home, "Home", Icon::Home),
    item(Route::Send, "Send", Icon::Send),
    item(Route::Receive, "Receive", Icon::Receive),
    item(Route::Copies, "Copies", Icon::Copies),
];
const MORE_ITEMS: [NavItem; 2] = [
    item(Route::Chat, "Chat", Icon::Chat),
    item(Route::Diags, "Diagnostics", Icon::Diags),
];

/// Leaving a screen while files are still hashing would drop them, so every destination
/// but the current one and Copies (which shows over any activity) waits until they are ready.
fn locked(preparing: bool, current: Route, route: Route) -> bool {
    preparing && route != current && route != Route::Copies
}

/// What the phone top bar calls the current screen. The screen's own `h1` stays the
/// heading; this is a visual label only.
const fn page_title(route: Route) -> &'static str {
    match route {
        Route::Home => "Oxfer",
        Route::Send => "Send",
        Route::Receive => "Receive",
        Route::Chat => "Chat",
        Route::Copies => "Copies",
        Route::Diags => "Diagnostics",
        Route::NotFound => "Not found",
    }
}

#[component]
fn Header() -> Element {
    let router = use_router();
    let core = use_core();
    let preparing = core.snap().read().preparing;
    let current = router.current();
    let home = current == Route::Home;
    let back_locked = locked(preparing, current, Route::Home);
    rsx! {
        header { class: if home { "frame-header" } else { "frame-header frame-sub" },
            if !home {
                // Phones only (CSS): the tab bar is the main way around, but a screen
                // reached from a link still gets an obvious way home.
                button {
                    r#type: "button",
                    class: "frame-icon-button frame-back",
                    aria_label: "Back to Home",
                    title: if back_locked { "Wait for files to finish preparing" } else { "Back to Home" },
                    disabled: back_locked,
                    onclick: move |_| router.go(Route::Home),
                    FrameIcon { icon: Icon::Back }
                }
                span { class: "frame-title", aria_hidden: "true", {page_title(current)} }
            }
            NavLink { route: Route::Home, class: "frame-brand", label: "Oxfer home",
                img { class: "frame-mark", src: "/assets/oxfer-mark.svg", alt: "", width: "32", height: "32" }
                span { class: "frame-wordmarks",
                    img { class: "frame-wordmark frame-wordmark-light", src: "/assets/oxfer-wordmark-light.svg", alt: "Oxfer", width: "128", height: "20" }
                    img { class: "frame-wordmark frame-wordmark-dark", src: "/assets/oxfer-wordmark-dark.svg", alt: "", width: "128", height: "20" }
                }
            }
            nav { class: "frame-nav", aria_label: "Main",
                ul { class: "frame-nav-list",
                    for entry in NAV_ITEMS {
                        li { key: "{entry.label}",
                            NavLink {
                                route: entry.route,
                                class: if current == entry.route { "frame-nav-link active" } else { "frame-nav-link" },
                                current: current == entry.route,
                                disabled: locked(preparing, current, entry.route),
                                FrameIcon { icon: entry.icon }
                                span { class: "frame-nav-label", {entry.label} }
                            }
                        }
                    }
                }
            }
            div { class: "frame-tools",
                ModeToggle {}
                AppearanceMenu {}
            }
        }
    }
}

/// Phone navigation, fixed to the bottom for thumb reach. Hidden from 600px up by CSS,
/// which also takes it out of the accessibility tree there.
#[component]
fn TabBar() -> Element {
    let router = use_router();
    let core = use_core();
    let preparing = core.snap().read().preparing;
    let current = router.current();
    let mut more_open = use_signal(|| false);
    let more_active = MORE_ITEMS.iter().any(|entry| entry.route == current);

    // Any navigation (a tab, Back/Forward, the core moving the route) closes the menu.
    use_effect(move || {
        let _ = router.current();
        more_open.set(false);
    });

    rsx! {
        nav {
            class: "frame-tabs",
            aria_label: "Main",
            onkeydown: move |event: KeyboardEvent| {
                if event.key() == Key::Escape {
                    more_open.set(false);
                }
            },
            if more_open() {
                div { class: "frame-more-backdrop", aria_hidden: "true", onclick: move |_| more_open.set(false) }
            }
            ul { class: "frame-tabs-list",
                for entry in TAB_ITEMS {
                    li { key: "{entry.label}",
                        NavLink {
                            route: entry.route,
                            class: if current == entry.route { "frame-tab active" } else { "frame-tab" },
                            current: current == entry.route,
                            disabled: locked(preparing, current, entry.route),
                            FrameIcon { icon: entry.icon }
                            span { class: "frame-tab-label", {entry.label} }
                        }
                    }
                }
                li {
                    button {
                        r#type: "button",
                        class: if more_active { "frame-tab active" } else { "frame-tab" },
                        aria_expanded: if more_open() { "true" } else { "false" },
                        aria_controls: MORE_MENU_ID,
                        onclick: move |_| more_open.set(!more_open()),
                        FrameIcon { icon: Icon::More }
                        span { class: "frame-tab-label", "More" }
                    }
                }
            }
            if more_open() {
                // Clicks bubble up from the links, so choosing one also closes the menu.
                ul { id: MORE_MENU_ID, class: "frame-more", onclick: move |_| more_open.set(false),
                    for entry in MORE_ITEMS {
                        li { key: "{entry.label}",
                            NavLink {
                                route: entry.route,
                                class: if current == entry.route { "frame-more-link active" } else { "frame-more-link" },
                                current: current == entry.route,
                                disabled: locked(preparing, current, entry.route),
                                FrameIcon { icon: entry.icon }
                                span { {entry.label} }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A real link (middle-click and screen readers work), navigated in-page.
#[component]
pub fn NavLink(
    route: Route,
    #[props(default)] class: String,
    #[props(default)] label: Option<String>,
    #[props(default)] current: bool,
    #[props(default)] disabled: bool,
    children: Element,
) -> Element {
    let router = use_router();
    rsx! {
        a {
            href: route.path(),
            class,
            aria_label: label,
            aria_current: if current { "page" },
            aria_disabled: if disabled { "true" },
            title: if disabled { "Wait for files to finish preparing" },
            onclick: move |event| {
                event.prevent_default();
                if !disabled {
                    router.go(route);
                }
            },
            {children}
        }
    }
}

/// Whether the page is dark right now: the manual choice, or the system's when it follows
/// the system.
fn effective_dark(mode: Mode) -> bool {
    match mode {
        Mode::Light => false,
        Mode::Dark => true,
        Mode::System => web_sys::window()
            .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
            .is_some_and(|query| query.matches()),
    }
}

/// One-tap light/dark switch. "Match system" stays available in the Appearance menu.
#[component]
fn ModeToggle() -> Element {
    let mut mode = use_appearance().mode;
    let dark = effective_dark(mode());
    let label = if dark {
        "Switch to light mode"
    } else {
        "Switch to dark mode"
    };
    rsx! {
        button {
            r#type: "button",
            class: "frame-icon-button",
            aria_label: label,
            title: label,
            onclick: move |_| mode.set(if dark { Mode::Light } else { Mode::Dark }),
            FrameIcon { icon: Icon::Contrast }
        }
    }
}

#[component]
fn AppearanceMenu() -> Element {
    let appearance = use_appearance();
    let (mut theme, mut mode) = (appearance.theme, appearance.mode);
    rsx! {
        details { class: "frame-appearance",
            summary { class: "frame-icon-button", title: "Appearance",
                FrameIcon { icon: Icon::Settings }
                span { class: "visually-hidden", "Appearance" }
            }
            div { class: "frame-appearance-panel",
                label { r#for: "theme-select", "Theme" }
                select {
                    id: "theme-select",
                    onchange: move |event| {
                        if let Some(picked) = Theme::from_storage_value(&event.value()) {
                            theme.set(picked);
                        }
                    },
                    for option in Theme::ALL {
                        option { key: "{option.storage_value()}", value: option.storage_value(), selected: theme() == option, {option.label()} }
                    }
                }
                label { r#for: "mode-select", "Light or dark" }
                select {
                    id: "mode-select",
                    onchange: move |event| {
                        if let Some(picked) = Mode::ALL.into_iter().find(|m| m.label() == event.value()) {
                            mode.set(picked);
                        }
                    },
                    for option in Mode::ALL {
                        option { key: "{option.label()}", value: option.label(), selected: mode() == option, {option.label()} }
                    }
                }
            }
        }
    }
}

#[component]
fn Footer() -> Element {
    rsx! {
        footer { class: "frame-footer",
            span { class: "meta", {p2p_transfer::BUILD_LABEL} }
            NavLink { route: Route::Diags, class: "meta-link", "Connection check" }
            a { class: "meta-link", href: "/llms.txt", "For AI agents" }
        }
    }
}

#[component]
fn NotFound() -> Element {
    rsx! {
        section { class: "card", aria_labelledby: "nf-title",
            h1 { id: "nf-title", "Page not found" }
            p { "There is nothing at this address. Share links open from the home page." }
            NavLink { route: Route::Home, class: "button primary", "Go home" }
        }
    }
}

/// Line icons for the frame, drawn in `currentColor` so they follow the theme.
#[derive(Clone, Copy, PartialEq)]
enum Icon {
    Home,
    Send,
    Receive,
    Chat,
    Copies,
    Diags,
    More,
    Back,
    Settings,
    Contrast,
}

/// Decorative: every use sits next to a text label or inside a labelled control.
#[component]
fn FrameIcon(icon: Icon) -> Element {
    let shape = match icon {
        Icon::Home => rsx! {
            path { d: "M3 10.5 12 3l9 7.5" }
            path { d: "M5.5 9v11.5h13V9" }
        },
        Icon::Send => rsx! {
            path { d: "M12 20V4" }
            path { d: "m5 11 7-7 7 7" }
        },
        Icon::Receive => rsx! {
            path { d: "M12 4v16" }
            path { d: "m5 13 7 7 7-7" }
        },
        Icon::Chat => rsx! {
            path { d: "M20.5 12a8.5 8.5 0 0 1-12.4 7.5L3.5 20.5l1-4.4A8.5 8.5 0 1 1 20.5 12Z" }
        },
        Icon::Copies => rsx! {
            rect { x: "4", y: "3.5", width: "16", height: "17", rx: "2" }
            path { d: "M8 8.5h8M8 12h8M8 15.5h5" }
        },
        Icon::Diags => rsx! {
            path { d: "M3 12h4l3-7 4 14 3-7h4" }
        },
        Icon::More => rsx! {
            circle { cx: "5", cy: "12", r: "1.4", fill: "currentColor" }
            circle { cx: "12", cy: "12", r: "1.4", fill: "currentColor" }
            circle { cx: "19", cy: "12", r: "1.4", fill: "currentColor" }
        },
        Icon::Back => rsx! {
            path { d: "m15 5-7 7 7 7" }
        },
        Icon::Settings => rsx! {
            path { d: "M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1" }
            circle { cx: "15", cy: "6", r: "2" }
            circle { cx: "9", cy: "12", r: "2" }
            circle { cx: "17", cy: "18", r: "2" }
        },
        Icon::Contrast => rsx! {
            circle { cx: "12", cy: "12", r: "8.5" }
            path { d: "M12 3.5a8.5 8.5 0 0 1 0 17Z", fill: "currentColor" }
        },
    };
    rsx! {
        span { class: "frame-icon", aria_hidden: "true",
            svg {
                view_box: "0 0 24 24",
                width: "24",
                height: "24",
                fill: "none",
                stroke: "currentColor",
                stroke_width: "2",
                stroke_linecap: "round",
                stroke_linejoin: "round",
                {shape}
            }
        }
    }
}

// ── Shared widgets ───────────────────────────────────────────────────────────

/// The one hidden `<input type=file>`; [`PickButton`]s click it inside their own click
/// handler, so the browser sees the user gesture.
#[component]
fn FileInput() -> Element {
    let core = use_core();
    rsx! {
        input {
            id: FILE_INPUT_ID,
            r#type: "file",
            multiple: true,
            class: "visually-hidden",
            tabindex: "-1",
            aria_hidden: "true",
            onchange: move |event| {
                use dioxus::web::WebEventExt as _;
                let Some(input) = event
                    .data()
                    .try_as_web_event()
                    .and_then(|e| e.target())
                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                else {
                    return;
                };
                let Some(files) = input.files() else {
                    return;
                };
                if files.length() > 0 {
                    // A file-picker change still counts as a user action on WebKit: ask
                    // for the wake lock now, before hashing and binding go async.
                    WebPlatform.set_wake_lock(true);
                }
                let picks = core.picks();
                for index in 0..files.length() {
                    if let Some(file) = files.get(index) {
                        picks.push(PickedFile::from_web_file(file));
                    }
                }
                // Let the same file be picked again later.
                input.set_value("");
                core.refresh();
            },
        }
    }
}

#[component]
pub fn PickButton(#[props(default)] class: String, children: Element) -> Element {
    let core = use_core();
    rsx! {
        button {
            r#type: "button",
            class: "button primary {class}",
            onclick: move |_| {
                let input = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.get_element_by_id(FILE_INPUT_ID))
                    .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok());
                match input {
                    Some(input) => input.click(),
                    None => core.report("this browser would not open a file picker".into()),
                }
            },
            {children}
        }
    }
}

/// Copies `text` and says so; the label returns after a moment.
#[component]
pub fn CopyButton(
    text: String,
    #[props(default = "Copy link".to_string())] label: String,
    #[props(default)] class: String,
) -> Element {
    let mut copied = use_signal(|| false);
    rsx! {
        button {
            r#type: "button",
            class: "button {class}",
            onclick: move |_| {
                let text = text.clone();
                spawn(async move {
                    if copy_text(&text).await {
                        copied.set(true);
                        gloo_timers::future::TimeoutFuture::new(2_000).await;
                        copied.set(false);
                    }
                });
            },
            span { aria_live: "polite", if copied() { "Copied" } else { {label.clone()} } }
        }
    }
}

async fn copy_text(text: &str) -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let promise = window.navigator().clipboard().write_text(text);
    wasm_bindgen_futures::JsFuture::from(promise).await.is_ok()
}

#[component]
pub fn Progress(fraction: f32, label: String) -> Element {
    let percent = (fraction.clamp(0.0, 1.0) * 100.0).round();
    rsx! {
        progress { class: "progress", max: "100", value: "{percent}", aria_label: label }
    }
}

#[component]
pub fn Pill(text: &'static str) -> Element {
    rsx! { span { class: "pill", {text} } }
}

#[component]
pub fn ErrorText(text: String) -> Element {
    rsx! { p { class: "error", role: "alert", {text} } }
}

/// "Keep this tab open": browsers suspend background tabs, which stalls a transfer.
#[component]
pub fn TransferAttention(sender: bool, awaiting_save: bool, wake_lock: WakeLock) -> Element {
    let (status, tone) = if awaiting_save {
        (
            "We'll ask your browser to keep the screen on when you start saving.",
            "muted",
        )
    } else {
        match wake_lock {
            WakeLock::Active => (
                "Your browser is helping keep the screen on (uses more battery).",
                "ok",
            ),
            WakeLock::Requesting => ("Asking your browser to keep the screen on…", "muted"),
            _ => (
                "Your browser can't keep the screen on automatically. Keep it on yourself.",
                "warn",
            ),
        }
    };
    rsx! {
        div { class: "attention", role: "status",
            p { class: "attention-title",
                if sender { "Keep this tab open while they save your file." } else { "Keep this tab open until your file is saved." }
            }
            p { "Keep this page visible and your screen on. Locking your device or switching apps may interrupt the transfer." }
            p { class: "attention-status {tone}", {status} }
        }
    }
}
