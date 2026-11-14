//! Components. Each route has a screen module; this module has the page frame (header,
//! main, footer) and the widgets screens share.
//!
//! Rules for screens: read state only from `core.snap()`, act only through
//! `core.dispatch(...)`, and take user-facing sentences from `p2p_transfer::sessions::text`
//! where one exists, so egui and the web say the same thing.

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
            class: "page",
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
            main { id: "main", tabindex: "-1", class: "content", {screen} }
            Footer {}
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

#[component]
fn Header() -> Element {
    let router = use_router();
    let core = use_core();
    let preparing = core.snap().read().preparing;
    let current = router.current();
    rsx! {
        header { class: "header",
            NavLink { route: Route::Home, class: "brand", label: "Oxfer home",
                img { class: "wordmark wordmark-light", src: "/assets/oxfer-wordmark-light.svg", alt: "Oxfer", width: "128", height: "20" }
                img { class: "wordmark wordmark-dark", src: "/assets/oxfer-wordmark-dark.svg", alt: "", width: "128", height: "20" }
            }
            nav { aria_label: "Main",
                ul { class: "nav",
                    for (route, label) in [(Route::Home, "Home"), (Route::Send, "Send"), (Route::Receive, "Receive"), (Route::Chat, "Chat"), (Route::Copies, "Saved")] {
                        li { key: "{label}",
                            NavLink {
                                route,
                                class: if current == route { "nav-link active" } else { "nav-link" },
                                current: current == route,
                                disabled: preparing && route != current && route != Route::Copies,
                                {label}
                            }
                        }
                    }
                }
            }
            AppearanceMenu {}
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

#[component]
fn AppearanceMenu() -> Element {
    let appearance = use_appearance();
    let (mut theme, mut mode) = (appearance.theme, appearance.mode);
    rsx! {
        details { class: "appearance",
            summary { "Appearance" }
            div { class: "appearance-panel",
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
        footer { class: "footer",
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
