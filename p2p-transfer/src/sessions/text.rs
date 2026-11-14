//! The words every UI shows for engine states.
//!
//! Plain strings, no markup: egui and Dioxus render the same sentences, so a wording fix
//! lands once. Keep protocol names (WebRTC, iroh, QUIC) out of user-facing labels.

use crate::node::{FragmentParams, SinkPref};
use crate::transfer::{Path, Phase, TransferProgress};

/// Shown next to a link, because the link *is* the credential (design §2.6).
pub const LINK_WARNING: &str =
    "Anyone with this link can download your files while you're sharing. Send it only to people you trust.";
/// What a receiver is told when the link carries no `cap` (design §2.6, §4.8.2).
pub const MISSING_CAP: &str =
    "this link is missing its access code; ask the sender for the full link";
/// Both sentences of design §2.6 for a capability the sender rejected.
pub const CAP_REJECTED: &str =
    "this link is not valid for these files. Ask the sender for a fresh link — the old one stops \
     working when they restart sharing.";

/// `1.50 MB`-style sizes with binary units.
pub fn format_size(size_bytes: u64) -> String {
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

/// An average rate, including sub-byte rates a slow resume can produce.
pub fn format_rate(bytes_per_sec: f64) -> String {
    if bytes_per_sec < 1.0 {
        format!("{bytes_per_sec:.2} B/s")
    } else {
        format!("{}/s", format_size(bytes_per_sec as u64))
    }
}

/// The live speed, only while bytes are actually moving.
pub fn transfer_speed(progress: &TransferProgress) -> Option<String> {
    (matches!(progress.phase, Phase::Transferring)
        && progress.bytes_per_sec.is_finite()
        && progress.bytes_per_sec > 0.0)
        .then(|| format!("{}/s", format_size(progress.bytes_per_sec as u64)))
}

/// The QA flags this receive is running with (design §7.1), or empty for a plain run.
pub fn flag_summary(params: &FragmentParams) -> String {
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

/// The connection badge: a reconnect or signalling phase never claims the old path.
pub fn path_badge(phase: &Phase, path: Path) -> &'static str {
    if matches!(phase, Phase::Reconnecting { .. }) {
        return "Connecting again";
    }
    if matches!(phase, Phase::Signaling) {
        return "Finding a connection";
    }
    transfer_path_text(path)
}

pub fn transfer_path_text(path: Path) -> &'static str {
    match path {
        Path::Direct => "Direct connection",
        Path::Relayed => "Encrypted via a helper server",
        Path::Unknown => "Connecting",
    }
}

pub fn phase_text(phase: &Phase) -> &'static str {
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

/// Turn an engine error string into something a user can act on.
pub fn humanize(error: &str) -> String {
    if error.contains("access code is missing") {
        MISSING_CAP.to_string()
    } else if error.contains("access code") {
        CAP_REJECTED.to_string()
    } else {
        error.to_string()
    }
}

/// The first characters of a peer id, enough to tell peers apart on screen.
pub fn short_id(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_test_transfer_speed_only_appears_while_transferring() {
        let mut progress = TransferProgress::connecting();
        progress.bytes_per_sec = 1_572_864.0;
        assert_eq!(transfer_speed(&progress), None);

        progress.phase = Phase::Transferring;
        assert_eq!(transfer_speed(&progress).as_deref(), Some("1.50 MB/s"));
        progress.bytes_per_sec = 0.0;
        assert_eq!(transfer_speed(&progress), None);
        progress.bytes_per_sec = f64::NAN;
        assert_eq!(transfer_speed(&progress), None);

        progress.bytes_per_sec = 1_572_864.0;
        progress.phase = Phase::Verifying;
        assert_eq!(transfer_speed(&progress), None);
        progress.phase = Phase::Complete { saved: Vec::new() };
        assert_eq!(transfer_speed(&progress), None);
    }

    #[test]
    fn local_test_reconnect_ui_does_not_claim_the_old_path_is_active() {
        let phase = Phase::Reconnecting {
            attempt: 2,
            max_attempts: 5,
        };
        assert_eq!(phase_text(&phase), "Connecting again…");
        assert_eq!(path_badge(&phase, Path::Direct), "Connecting again");
    }

    #[test]
    fn local_test_connection_labels_explain_paths_without_protocol_names() {
        assert_eq!(transfer_path_text(Path::Direct), "Direct connection");
        assert_eq!(
            transfer_path_text(Path::Relayed),
            "Encrypted via a helper server"
        );
        assert_eq!(phase_text(&Phase::Signaling), "Finding a connection…");
        assert_eq!(
            phase_text(&Phase::Complete { saved: Vec::new() }),
            "Transfer complete and checked"
        );
    }

    #[test]
    fn local_test_rates_and_ids_format_for_display() {
        assert_eq!(format_rate(0.5), "0.50 B/s");
        assert_eq!(format_rate(2048.0), "2.0 KB/s");
        assert_eq!(short_id("0123456789abcdef"), "0123456789ab");
        assert_eq!(short_id("short"), "short");
        assert_eq!(humanize("the access code is missing"), MISSING_CAP);
        assert_eq!(humanize("wrong access code"), CAP_REJECTED);
        assert_eq!(humanize("timed out"), "timed out");
    }
}
