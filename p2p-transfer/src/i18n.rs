//! User-facing copy, looked up by id from Fluent (`locales/<lang>/oxfer.ftl`).
//!
//! Every UI (egui, Dioxus, a future GPUI) asks for the same [`Msg`], so a wording fix or a
//! translation lands once. Plain sentences: `Msg::SendThisLink.text()`. Sentences with
//! placeholders: `Msg::DiagSessionId.with(&[("session", id)])`.
//!
//! Only English ships today. To add a language, add `locales/<tag>/oxfer.ftl`, list it in
//! [`Locale`], and call [`set_locale`] at start-up; a message missing from a locale falls
//! back to English, and a missing id renders as the id itself (and fails the tests).

use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

/// A language the UI can be shown in.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum Locale {
    #[default]
    English,
}

impl Locale {
    /// Every shipped language, in menu order.
    pub const ALL: [Self; 1] = [Self::English];

    /// The BCP 47 tag.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        let primary = tag.split(['-', '_']).next().unwrap_or(tag);
        Self::ALL
            .into_iter()
            .find(|l| l.tag().eq_ignore_ascii_case(primary))
    }

    const fn source(self) -> &'static str {
        match self {
            Self::English => include_str!("../locales/en/oxfer.ftl"),
        }
    }
}

static CURRENT: RwLock<Locale> = RwLock::new(Locale::English);

/// Choose the language for every later lookup.
pub fn set_locale(locale: Locale) {
    if let Ok(mut current) = CURRENT.write() {
        *current = locale;
    }
}

pub fn locale() -> Locale {
    CURRENT.read().map(|l| *l).unwrap_or_default()
}

fn bundle(locale: Locale) -> &'static FluentBundle<FluentResource> {
    static ENGLISH: OnceLock<FluentBundle<FluentResource>> = OnceLock::new();
    match locale {
        Locale::English => ENGLISH.get_or_init(|| build(locale)),
    }
}

fn build(locale: Locale) -> FluentBundle<FluentResource> {
    let lang: LanguageIdentifier = locale.tag().parse().expect("static language tag");
    let mut bundle = FluentBundle::new_concurrent(vec![lang]);
    // Placeables are plain text in every UI; bidi isolation marks would show up in egui.
    bundle.set_use_isolating(false);
    let resource = FluentResource::try_new(locale.source().to_owned())
        .unwrap_or_else(|(resource, _)| resource);
    // A duplicate id keeps the first definition; the tests reject duplicates.
    let _ = bundle.add_resource(resource);
    bundle
}

fn lookup(id: &str, args: Option<&FluentArgs<'_>>) -> Option<String> {
    for locale in [locale(), Locale::English] {
        let bundle = bundle(locale);
        let Some(pattern) = bundle.get_message(id).and_then(|m| m.value()) else {
            continue;
        };
        let mut errors = Vec::new();
        return Some(
            bundle
                .format_pattern(pattern, args, &mut errors)
                .into_owned(),
        );
    }
    None
}

macro_rules! messages {
    ($($variant:ident => $id:literal,)+) => {
        /// Every message the UIs share. The Fluent id of each is its kebab-case name.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Msg {
            $($variant,)+
        }

        impl Msg {
            /// All messages, for tests and tooling.
            pub const ALL: &[Msg] = &[$(Msg::$variant,)+];

            /// The Fluent message id.
            pub const fn id(self) -> &'static str {
                match self {
                    $(Msg::$variant => $id,)+
                }
            }
        }
    };
}

messages! {
    WakeLockAsking => "wake-lock-asking",
    CheckingYourSavedCopy => "checking-your-saved-copy",
    HomeStoryChoose => "home-story-choose",
    ChooseWhereToSave => "choose-where-to-save",
    ChatConnectingRoom => "chat-connecting-room",
    ReceiveConnectingSender => "receive-connecting-sender",
    ContinueDownload => "continue-download",
    CopyLocalChecksLink => "copy-local-checks-link",
    CopyPeerTestLink => "copy-peer-test-link",
    DeletePermanently => "delete-permanently",
    DeleteSavedCopy => "delete-saved-copy",
    CopiesDeleteConfirm => "copies-delete-confirm",
    DownloadFile => "download-file",
    DiagLocalLinkNote => "diag-local-link-note",
    HomeStoryDirect => "home-story-direct",
    GettingYourLinkReady => "getting-your-link-ready",
    KeepLocalCopy => "keep-local-copy",
    DiagPeerTestInstructions => "diag-peer-test-instructions",
    ChatKeepOpenTalking => "chat-keep-open-talking",
    ChatKeepOpenRoomCloses => "chat-keep-open-room-closes",
    ReceiveKeepTabOpen => "receive-keep-tab-open",
    SendKeepTabOpen => "send-keep-tab-open",
    ReceiveLookingForSender => "receive-looking-for-sender",
    NoAccountNeeded => "no-account-needed",
    NetworkDiagnostics => "network-diagnostics",
    ChatNoMessages => "chat-no-messages",
    ChatOpeningRoom => "chat-opening-room",
    OpeningYourSaveLocation => "opening-your-save-location",
    ReceiveChecksAfterSave => "receive-checks-after-save",
    PasteALinkHere => "paste-a-link-here",
    ReceivePasteFullLink => "receive-paste-full-link",
    HomeStoryPickSend => "home-story-pick-send",
    ReadyToSave => "ready-to-save",
    ReceiveOnThisDevice => "receive-on-this-device",
    DiagReportReview => "diag-report-review",
    RunChecksAgain => "run-checks-again",
    SaveACopyInThisBrowser => "save-a-copy-in-this-browser",
    SavedFilesInThisBrowser => "saved-files-in-this-browser",
    SendFilesWithoutCloudStorage => "send-files-without-cloud-storage",
    SendLinkHintShare => "send-link-hint-share",
    SendLinkHintChosen => "send-link-hint-chosen",
    ChatLinkHintTalk => "chat-link-hint-talk",
    SendThisLink => "send-this-link",
    DiagSessionId => "diag-session-id",
    StartAChatRoom => "start-a-chat-room",
    StartPeerTest => "start-peer-test",
    DiagPeerStarting => "diag-peer-starting",
    StopPeerTest => "stop-peer-test",
    DiagIncomingPings => "diag-incoming-pings",
    TestPeerConnection => "test-peer-connection",
    DiagRelayTesting => "diag-relay-testing",
    ChatRoomClosed => "chat-room-closed",
    HomeStoryTheySave => "home-story-they-save",
    ReceiveRetrying => "receive-retrying",
    ChatInputHint => "chat-input-hint",
    DiagCompareReports => "diag-compare-reports",
    SendWaitPreparing => "send-wait-preparing",
    WakeLockWillAsk => "wake-lock-will-ask",
    WakeLockUnsupported => "wake-lock-unsupported",
    WakeLockActive => "wake-lock-active",
    SendStillAvailable => "send-still-available",
}

impl Msg {
    /// The message in the current language. Each (language, message) pair is rendered once
    /// and kept for the life of the process (a few dozen short strings), so UIs can pass it
    /// wherever a `&str` is expected.
    pub fn text(self) -> &'static str {
        static CACHE: RwLock<BTreeMap<(Locale, &'static str), &'static str>> =
            RwLock::new(BTreeMap::new());
        let key = (locale(), self.id());
        if let Some(text) = CACHE.read().ok().and_then(|c| c.get(&key).copied()) {
            return text;
        }
        let rendered = lookup(self.id(), None).unwrap_or_else(|| self.id().to_owned());
        let leaked: &'static str = Box::leak(rendered.into_boxed_str());
        if let Ok(mut cache) = CACHE.write() {
            return cache.entry(key).or_insert(leaked);
        }
        leaked
    }

    /// The message with its placeholders filled in, e.g. `[("session", "ab12")]`.
    pub fn with(self, args: &[(&str, &dyn std::fmt::Display)]) -> String {
        let mut fluent = FluentArgs::new();
        for (name, value) in args {
            fluent.set(*name, FluentValue::from(value.to_string()));
        }
        lookup(self.id(), Some(&fluent)).unwrap_or_else(|| self.id().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids_in(source: &str) -> Vec<&str> {
        source
            .lines()
            .filter(|l| !l.starts_with('#') && l.contains(" = "))
            .filter_map(|l| l.split(" = ").next())
            .collect()
    }

    #[test]
    fn local_test_every_message_exists_in_every_locale() {
        for locale in Locale::ALL {
            let b = bundle(locale);
            for msg in Msg::ALL {
                assert!(
                    b.has_message(msg.id()),
                    "{} is missing from {}",
                    msg.id(),
                    locale.tag()
                );
            }
        }
    }

    #[test]
    fn local_test_no_unused_or_duplicate_ids_in_a_locale() {
        for locale in Locale::ALL {
            let ids = ids_in(locale.source());
            for id in &ids {
                assert!(
                    Msg::ALL.iter().any(|m| m.id() == *id),
                    "{id} in {} has no Msg variant",
                    locale.tag()
                );
                assert_eq!(
                    ids.iter().filter(|i| *i == id).count(),
                    1,
                    "{id} duplicated"
                );
            }
        }
        let mut seen = std::collections::HashSet::new();
        assert!(Msg::ALL.iter().all(|m| seen.insert(m.id())));
    }

    #[test]
    fn local_test_messages_render_without_formatting_errors() {
        for locale in Locale::ALL {
            let b = bundle(locale);
            for msg in Msg::ALL {
                let pattern = b.get_message(msg.id()).and_then(|m| m.value()).unwrap();
                let mut errors = Vec::new();
                let text = b.format_pattern(pattern, None, &mut errors);
                // Placeholder messages report a missing argument here; the rest must be clean.
                if !text.contains('{') && !text.contains('$') {
                    assert!(errors.is_empty(), "{}: {errors:?}", msg.id());
                }
                assert!(!text.trim().is_empty());
            }
        }
    }

    #[test]
    fn local_test_placeholders_are_filled() {
        assert_eq!(
            Msg::DiagSessionId.with(&[("session", &"ab12")]),
            "Session ID: ab12"
        );
        assert_eq!(
            Msg::ReceiveRetrying.with(&[("attempt", &2), ("max_attempts", &5)]),
            "Trying again (2 of 5). Keep the sender's tab open. We'll try to continue where we stopped."
        );
        assert_eq!(
            Msg::DiagIncomingPings.with(&[("completed", &3)]),
            "Successful incoming peer pings: 3"
        );
    }

    #[test]
    fn local_test_plain_messages_match_the_old_literals() {
        assert_eq!(Msg::SendThisLink.text(), "Send this link");
        assert_eq!(Msg::NoAccountNeeded.text(), "NO ACCOUNT NEEDED");
        assert_eq!(Locale::from_tag("en-GB"), Some(Locale::English));
        assert_eq!(Locale::from_tag("xx"), None);
    }
}
