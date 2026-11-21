//! Theme and light/dark for the DOM.
//!
//! The colours themselves live in `p2p_transfer::theme`, the same table egui paints with;
//! [`css_variables`](p2p_transfer::theme::css_variables) turns it into custom properties
//! keyed on `data-theme` / `data-mode` on `<html>`. This module only chooses those two
//! attributes, remembers the choice, and tints the browser chrome (`theme-color`).

use dioxus::prelude::*;
use p2p_transfer::theme::{Palette, Theme, BROWSER_THEME_KEY};

/// `localStorage` key for a manual light/dark override. Absent means "follow the system".
pub const BROWSER_MODE_KEY: &str = "oxfer.mode.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    System,
    Light,
    Dark,
}

impl Mode {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "Match system",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    const fn storage_value(self) -> Option<&'static str> {
        match self {
            Self::System => None,
            Self::Light => Some("light"),
            Self::Dark => Some("dark"),
        }
    }

    fn from_storage_value(value: Option<&str>) -> Self {
        match value {
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            _ => Self::System,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Appearance {
    pub theme: Signal<Theme>,
    pub mode: Signal<Mode>,
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

fn system_dark() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        .is_some_and(|query| query.matches())
}

/// Write the attributes the generated CSS keys on, and the browser chrome colour.
fn apply(theme: Theme, mode: Mode) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if let Some(root) = document.document_element() {
        let _ = root.set_attribute("data-theme", theme.storage_value());
        let _ = match mode.storage_value() {
            Some(value) => root.set_attribute("data-mode", value),
            None => root.remove_attribute("data-mode"),
        };
    }
    let dark = match mode {
        Mode::System => system_dark(),
        Mode::Light => false,
        Mode::Dark => true,
    };
    let chrome = Palette::of(theme, dark).surface.hex();
    if let Ok(metas) = document.query_selector_all("meta[name=\"theme-color\"]") {
        for index in 0..metas.length() {
            if let Some(meta) = metas
                .item(index)
                .and_then(|node| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(node).ok())
            {
                let _ = meta.set_attribute("content", &chrome);
            }
        }
    }
}

pub fn use_appearance_provider() -> Appearance {
    let appearance = use_context_provider(|| {
        let stored = storage();
        let theme = stored
            .as_ref()
            .and_then(|s| s.get_item(BROWSER_THEME_KEY).ok().flatten())
            .as_deref()
            .and_then(Theme::from_storage_value)
            .unwrap_or_default();
        let mode = Mode::from_storage_value(
            stored
                .as_ref()
                .and_then(|s| s.get_item(BROWSER_MODE_KEY).ok().flatten())
                .as_deref(),
        );
        Appearance {
            theme: Signal::new(theme),
            mode: Signal::new(mode),
        }
    });
    use_effect(move || {
        let (theme, mode) = ((appearance.theme)(), (appearance.mode)());
        apply(theme, mode);
        if let Some(storage) = storage() {
            // Written at once: mobile Safari can kill a background tab at any moment.
            let _ = storage.set_item(BROWSER_THEME_KEY, theme.storage_value());
            let _ = match mode.storage_value() {
                Some(value) => storage.set_item(BROWSER_MODE_KEY, value),
                None => storage.remove_item(BROWSER_MODE_KEY),
            };
        }
    });
    appearance
}

pub fn use_appearance() -> Appearance {
    use_context::<Appearance>()
}
