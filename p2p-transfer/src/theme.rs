//! The design tokens every Oxfer UI paints with: one table, three themes, light and dark.
//!
//! egui builds its `Tc` palette from [`Palette::of`]; the Dioxus web UI loads
//! [`css_variables`] as CSS custom properties. The token names match `theme.html` (the theme
//! lab) and `docs/theme-brief.html`, so a colour changed here changes everywhere.

use serde::{Deserialize, Serialize};

/// A display preference, independent of the light/dark setting and transfer state.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum Theme {
    Rusty,
    #[default]
    Clean,
    /// A vector CRT: one green phosphor on black, wireframe outlines, monospace type. Light
    /// mode is the same machine's green-bar line-printer paper.
    Phosphor,
}

/// `localStorage` key for the browser's theme choice, shared by every web UI.
pub const BROWSER_THEME_KEY: &str = "oxfer.theme.v1";

impl Theme {
    /// Menu order.
    pub const ALL: [Self; 3] = [Self::Rusty, Self::Clean, Self::Phosphor];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Rusty => "Rusty",
            Self::Clean => "Clean",
            Self::Phosphor => "Phosphor",
        }
    }

    /// Primary buttons and other controls, in px.
    pub const fn control_radius(self) -> u8 {
        match self {
            Self::Rusty => 10,
            Self::Clean => 18,
            Self::Phosphor => 2,
        }
    }

    /// Cards, in px.
    pub const fn card_radius(self) -> u8 {
        match self {
            Self::Rusty => 16,
            Self::Clean => 20,
            Self::Phosphor => 4,
        }
    }

    /// Ceiling for every other rounded frame, pill, and icon tile.
    pub const fn corner_cap(self) -> Option<u8> {
        match self {
            Self::Phosphor => Some(4),
            Self::Rusty | Self::Clean => None,
        }
    }

    /// Set all UI text in the monospace family, not only meta, links, and logs.
    pub const fn monospace(self) -> bool {
        matches!(self, Self::Phosphor)
    }

    /// The value stored under [`BROWSER_THEME_KEY`] and used as `data-theme` in CSS.
    pub const fn storage_value(self) -> &'static str {
        match self {
            Self::Rusty => "rusty",
            Self::Clean => "clean",
            Self::Phosphor => "phosphor",
        }
    }

    pub fn from_storage_value(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|theme| theme.storage_value() == value)
    }
}

/// An opaque sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb`
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// The 13 colour tokens of one theme in one mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub bg: Rgb,
    pub surface_lowest: Rgb,
    pub surface_low: Rgb,
    pub surface: Rgb,
    pub surface_high: Rgb,
    pub primary: Rgb,
    pub on_primary: Rgb,
    pub secondary: Rgb,
    pub on_surface: Rgb,
    pub on_surface_var: Rgb,
    pub outline: Rgb,
    pub outline_var: Rgb,
    pub error: Rgb,
}

impl Palette {
    pub const fn of(theme: Theme, dark: bool) -> Self {
        match (theme, dark) {
            (Theme::Clean, true) => Self {
                bg: Rgb(21, 23, 36),
                surface_lowest: Rgb(14, 16, 24),
                surface_low: Rgb(28, 31, 46),
                surface: Rgb(35, 38, 58),
                surface_high: Rgb(46, 50, 80),
                primary: Rgb(26, 136, 254),
                on_primary: Rgb(6, 16, 28),
                secondary: Rgb(4, 242, 162),
                on_surface: Rgb(245, 246, 248),
                on_surface_var: Rgb(177, 178, 184),
                outline: Rgb(154, 160, 176),
                outline_var: Rgb(58, 62, 82),
                error: Rgb(255, 139, 154),
            },
            (Theme::Clean, false) => Self {
                bg: Rgb(228, 228, 228),
                surface_lowest: Rgb(254, 254, 254),
                surface_low: Rgb(255, 255, 255),
                surface: Rgb(244, 245, 247),
                surface_high: Rgb(220, 232, 252),
                primary: Rgb(22, 116, 216),
                on_primary: Rgb(255, 255, 255),
                secondary: Rgb(0, 121, 81),
                on_surface: Rgb(21, 23, 36),
                on_surface_var: Rgb(92, 96, 112),
                outline: Rgb(78, 82, 96),
                outline_var: Rgb(197, 198, 203),
                error: Rgb(196, 55, 74),
            },
            (Theme::Rusty, true) => Self {
                bg: Rgb(16, 13, 12),
                surface_lowest: Rgb(10, 8, 7),
                surface_low: Rgb(28, 22, 19),
                surface: Rgb(35, 27, 23),
                surface_high: Rgb(54, 39, 31),
                primary: Rgb(239, 112, 56),
                on_primary: Rgb(34, 15, 7),
                secondary: Rgb(68, 218, 181),
                on_surface: Rgb(247, 235, 226),
                on_surface_var: Rgb(213, 190, 176),
                outline: Rgb(168, 137, 120),
                outline_var: Rgb(81, 57, 46),
                error: Rgb(255, 181, 164),
            },
            (Theme::Rusty, false) => Self {
                bg: Rgb(250, 246, 242),
                surface_lowest: Rgb(255, 253, 251),
                surface_low: Rgb(244, 235, 228),
                surface: Rgb(237, 222, 212),
                surface_high: Rgb(226, 202, 187),
                primary: Rgb(176, 66, 22),
                on_primary: Rgb(255, 249, 245),
                secondary: Rgb(0, 113, 88),
                on_surface: Rgb(48, 28, 20),
                on_surface_var: Rgb(94, 67, 55),
                outline: Rgb(132, 99, 82),
                outline_var: Rgb(211, 187, 173),
                error: Rgb(177, 46, 30),
            },
            (Theme::Phosphor, true) => Self {
                bg: Rgb(3, 10, 4),
                surface_lowest: Rgb(0, 0, 0),
                surface_low: Rgb(6, 18, 8),
                surface: Rgb(10, 26, 12),
                surface_high: Rgb(19, 58, 20),
                primary: Rgb(57, 255, 20),
                on_primary: Rgb(2, 26, 2),
                secondary: Rgb(198, 255, 61),
                on_surface: Rgb(184, 255, 160),
                on_surface_var: Rgb(116, 214, 92),
                outline: Rgb(79, 209, 58),
                outline_var: Rgb(30, 92, 27),
                error: Rgb(255, 106, 61),
            },
            (Theme::Phosphor, false) => Self {
                bg: Rgb(241, 247, 236),
                surface_lowest: Rgb(251, 253, 249),
                surface_low: Rgb(250, 253, 246),
                surface: Rgb(220, 236, 210),
                surface_high: Rgb(199, 226, 184),
                primary: Rgb(23, 112, 26),
                on_primary: Rgb(244, 255, 238),
                secondary: Rgb(63, 106, 0),
                on_surface: Rgb(15, 42, 16),
                on_surface_var: Rgb(61, 90, 58),
                outline: Rgb(68, 112, 60),
                outline_var: Rgb(181, 209, 166),
                error: Rgb(178, 58, 20),
            },
        }
    }

    /// Every token with its CSS custom-property name (as in `theme.html`), in table order.
    pub const fn tokens(&self) -> [(&'static str, Rgb); 13] {
        [
            ("bg", self.bg),
            ("surface-lowest", self.surface_lowest),
            ("surface-low", self.surface_low),
            ("surface", self.surface),
            ("surface-high", self.surface_high),
            ("primary", self.primary),
            ("on-primary", self.on_primary),
            ("secondary", self.secondary),
            ("on-surface", self.on_surface),
            ("on-surface-var", self.on_surface_var),
            ("outline", self.outline),
            ("outline-var", self.outline_var),
            ("error", self.error),
        ]
    }
}

/// Colours that tell chat participants apart: seven hues per mode, or three shades of the
/// one phosphor in [`Theme::Phosphor`].
const MEMBER_DARK: [Rgb; 7] = [
    Rgb(255, 180, 84),  // amber
    Rgb(177, 140, 255), // violet
    Rgb(79, 214, 190),  // teal
    Rgb(255, 143, 177), // pink
    Rgb(155, 225, 93),  // lime
    Rgb(255, 209, 102), // yellow
    Rgb(122, 162, 255), // periwinkle
];
const MEMBER_LIGHT: [Rgb; 7] = [
    Rgb(166, 86, 0),
    Rgb(107, 63, 214),
    Rgb(0, 122, 104),
    Rgb(194, 24, 91),
    Rgb(63, 125, 18),
    Rgb(138, 109, 0),
    Rgb(47, 85, 201),
];

/// How many distinct member colours a theme has.
pub const fn member_slots(theme: Theme) -> usize {
    match theme {
        Theme::Phosphor => 3,
        Theme::Rusty | Theme::Clean => 7,
    }
}

/// Which member colour a name gets. FNV-1a, so it is stable across sessions, devices and
/// UIs: a name keeps its colour for everyone in the room.
pub fn member_slot(theme: Theme, name: &str) -> usize {
    let hash = name.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    hash as usize % member_slots(theme)
}

/// The colour for member `slot` (see [`member_slot`]), in this theme and mode.
pub const fn member_palette(theme: Theme, dark: bool) -> [Rgb; 7] {
    let p = Palette::of(theme, dark);
    match theme {
        Theme::Phosphor => [
            p.secondary,
            p.on_surface,
            p.on_surface_var,
            p.secondary,
            p.on_surface,
            p.on_surface_var,
            p.secondary,
        ],
        Theme::Rusty | Theme::Clean if dark => MEMBER_DARK,
        Theme::Rusty | Theme::Clean => MEMBER_LIGHT,
    }
}

/// A participant's colour; the local user is always the primary colour.
pub fn member_color(theme: Theme, dark: bool, name: &str, you: bool) -> Rgb {
    if you {
        return Palette::of(theme, dark).primary;
    }
    member_palette(theme, dark)[member_slot(theme, name)]
}

fn declarations(theme: Theme, dark: bool) -> String {
    let mut out = String::new();
    for (name, rgb) in Palette::of(theme, dark).tokens() {
        out.push_str(&format!("--{name}:{};", rgb.hex()));
    }
    for (slot, rgb) in member_palette(theme, dark).iter().enumerate() {
        out.push_str(&format!("--member-{slot}:{};", rgb.hex()));
    }
    out.push_str(&format!(
        "color-scheme:{};",
        if dark { "dark" } else { "light" }
    ));
    out
}

/// CSS custom properties for every theme and mode.
///
/// The root element carries `data-theme="clean|rusty|phosphor"` and, for a manual override,
/// `data-mode="light|dark"`; without `data-mode`, `prefers-color-scheme` decides. Shape
/// tokens (`--r-control`, `--r-card`, `--r-cap`) and `--font-ui` come with each theme.
pub fn css_variables() -> String {
    let mut css = String::new();
    for theme in Theme::ALL {
        let name = theme.storage_value();
        let cap = theme
            .corner_cap()
            .map_or_else(|| "999px".to_string(), |cap| format!("{cap}px"));
        let font = if theme.monospace() {
            "var(--font-mono)"
        } else {
            "var(--font-sans)"
        };
        css.push_str(&format!(
            ":root[data-theme=\"{name}\"]{{{}--r-control:{}px;--r-card:{}px;--r-cap:{cap};--font-ui:{font};}}\n",
            declarations(theme, false),
            theme.control_radius(),
            theme.card_radius(),
        ));
        css.push_str(&format!(
            "@media (prefers-color-scheme: dark){{:root[data-theme=\"{name}\"]:not([data-mode=\"light\"]){{{}}}}}\n",
            declarations(theme, true)
        ));
        css.push_str(&format!(
            ":root[data-theme=\"{name}\"][data-mode=\"dark\"]{{{}}}\n",
            declarations(theme, true)
        ));
    }
    css
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_test_member_colours_are_stable_and_per_theme() {
        use super::*;
        // Same name, same slot, every time; the local user is always the primary colour.
        assert_eq!(
            member_slot(Theme::Clean, "ana"),
            member_slot(Theme::Clean, "ana")
        );
        assert!(member_slot(Theme::Phosphor, "ana") < 3);
        for theme in Theme::ALL {
            assert!(member_slot(theme, "bob") < member_slots(theme));
            assert_eq!(
                member_color(theme, true, "x", true),
                Palette::of(theme, true).primary
            );
        }
        assert!(css_variables().contains("--member-6:"));
    }

    use super::*;

    #[test]
    fn local_test_themes_differ_and_round_trip_through_storage() {
        for theme in Theme::ALL {
            assert_eq!(
                Theme::from_storage_value(theme.storage_value()),
                Some(theme)
            );
            let encoded = postcard::to_stdvec(&theme).unwrap();
            let restored: Theme = postcard::from_bytes(&encoded).unwrap();
            assert_eq!(restored, theme);
        }
        assert_eq!(Theme::from_storage_value("unknown"), None);
        for dark in [false, true] {
            for (index, a) in Theme::ALL.into_iter().enumerate() {
                for b in Theme::ALL.into_iter().skip(index + 1) {
                    let (a, b) = (Palette::of(a, dark), Palette::of(b, dark));
                    assert_ne!(a.primary, b.primary);
                    assert_ne!(a.bg, b.bg);
                }
            }
        }
    }

    #[test]
    fn local_test_css_variables_cover_every_token_theme_and_mode() {
        let css = css_variables();
        for theme in Theme::ALL {
            for dark in [false, true] {
                for (name, rgb) in Palette::of(theme, dark).tokens() {
                    assert!(
                        css.contains(&format!("--{name}:{};", rgb.hex())),
                        "{theme:?} dark={dark} {name}"
                    );
                }
            }
            assert!(css.contains(&format!(
                ":root[data-theme=\"{}\"][data-mode=\"dark\"]",
                theme.storage_value()
            )));
        }
        assert!(css.contains("prefers-color-scheme: dark"));
        // The browser chrome colours in assets/theme.js come from the same table.
        assert_eq!(Palette::of(Theme::Clean, true).surface.hex(), "#23263a");
        assert_eq!(Palette::of(Theme::Rusty, false).bg.hex(), "#faf6f2");
    }
}
