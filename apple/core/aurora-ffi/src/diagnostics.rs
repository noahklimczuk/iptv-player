//! Settings → Diagnostics and About: Windows' `app_diagnostics` and `app_about`
//! (`aurora-app/src/logging.rs`), less what Apple answers for itself.
//!
//! Left to Swift: the app's version (its bundle says), the video engine (the player
//! asks mpv), and whether credentials persist (the Keychain always does). What is left
//! here is what only the core knows, and the one rule an exported log has to follow.

use crate::AuroraCore;

/// Aurora's own licence, as Windows' About states it.
pub const LICENSE: &str = "GPL-3.0-or-later";

/// Where releases and source are published (`aurora-app` `updates::RELEASES_URL`).
pub const RELEASES_URL: &str = "https://github.com/noahklimczuk/iptv-player/releases/latest";

/// What the Diagnostics panel shows that only the core knows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Diagnostics {
    pub data_dir: String,
    /// Where the unreadable library was moved, when this launch had to start a new one
    /// (`aurora_db::Opened::Replaced`). The one thing a viewer has to be told: their
    /// favourites and history are gone, and this is why.
    pub library_was_replaced: Option<String>,
}

/// What the About panel shows that only the core knows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AboutCore {
    pub core_version: String,
    pub license: String,
    pub releases_url: String,
}

#[uniffi::export]
pub fn about_core() -> AboutCore {
    AboutCore {
        core_version: env!("CARGO_PKG_VERSION").to_string(),
        license: LICENSE.to_string(),
        releases_url: RELEASES_URL.to_string(),
    }
}

/// A log made safe to hand to somebody: every URL in it put through the same
/// `redact` Windows' log lines go through, so a provider login never leaves in a bug
/// report (`docs/PRIVACY.md`). mpv's lines name the address they failed to open, and
/// the Apple log is exported from the system's, which no one redacted on the way in.
#[uniffi::export]
pub fn redact_log(text: String) -> String {
    aurora_core::redact::redact_in_text(&text)
}

#[uniffi::export]
impl AuroraCore {
    pub fn diagnostics(&self) -> Diagnostics {
        Diagnostics {
            data_dir: self.data_dir.display().to_string(),
            library_was_replaced: self.replaced.as_ref().map(|p| p.display().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exported_log_carries_no_login() {
        let line = "mpv [ffmpeg] http: HTTP error 404 opening \
                    http://panel.example:8080/someuser/s3cret/12345, giving up";
        let safe = redact_log(line.to_string());
        assert!(!safe.contains("someuser"), "{safe}");
        assert!(!safe.contains("s3cret"), "{safe}");
        assert!(safe.ends_with(", giving up"), "{safe}");
    }

    #[test]
    fn a_library_that_opened_is_not_reported_as_replaced() {
        let (core, dir) = crate::testing::core();
        let d = core.diagnostics();
        assert_eq!(d.library_was_replaced, None);
        assert_eq!(d.data_dir, dir.path().display().to_string());
    }

    #[test]
    fn an_unreadable_library_is_reported_with_where_it_went() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("library.db"), b"this is not a database").unwrap();
        let core = AuroraCore::open(
            dir.path().display().to_string(),
            std::sync::Arc::new(crate::secrets::tests::MemorySecrets::default()),
        )
        .unwrap();
        let moved = core.diagnostics().library_was_replaced.expect("reported");
        assert!(std::path::Path::new(&moved).exists(), "{moved}");
    }
}
