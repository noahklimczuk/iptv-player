//! Which settings follow the viewer between devices, and which stay on the machine.
//!
//! Sync carries settings from an **allowlist**, not a blocklist (`docs/APPLE_PORT_AND_SYNC.md`
//! §3.4). A setting nobody has classified is device-local, so a Windows-only knob -- the
//! timeshift folder, the DVR quota, the credential namespace -- cannot arrive on an
//! Apple TV where it means nothing or, worse, points at a path that does not exist.
//!
//! Every key the code reads or writes must appear in [`SETTINGS`]. That is enforced
//! mechanically: `aurora-contract/tests/sync_settings.rs` finds every setting key in the
//! sources and fails on any this table does not name. Adding a setting therefore forces
//! the question "should this follow the viewer?" at the moment it is cheapest to answer.

/// Whether a setting syncs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingSync {
    /// About the person: their taste, their filters, their choices. Follows them to
    /// every device signed into the household account.
    Personal,
    /// About this machine: paths, hardware, caches, bookkeeping. Never leaves it.
    Device,
}

use SettingSync::{Device, Personal};

/// Every setting key, and whether it syncs.
pub const SETTINGS: &[(&str, SettingSync)] = &[
    // The viewer's own choices.
    ("filter.english_only", Personal),
    ("filter.hide_duplicates", Personal),
    ("filter.hide_untagged", Personal),
    ("guide.free_sources", Personal),
    ("gemini.enabled", Personal),
    // The household's parental choice, like the locks it applies to.
    ("parental.hide_locked", Personal),
    // Per-device by nature.
    //
    // The assistant's history is per profile but can run to many kilobytes, and a
    // conversation is about what was on this screen at the time.
    ("assistant.history", Device),
    ("credentials.namespace", Device),
    ("dvr.folder", Device),
    ("dvr.quotaBytes", Device),
    ("enrichment.query_version", Device),
    ("filter.classifier_version", Device),
    ("gemini.cache", Device),
    // What a device imports is a storage decision: an Apple TV may keep live TV only
    // (§5 risk 3) while the PC keeps the whole catalogue.
    ("import.live", Device),
    ("import.series", Device),
    ("import.vod", Device),
    ("metadata.concurrency", Device),
    ("timeshift.bytes", Device),
    ("timeshift.enabled", Device),
    ("timeshift.folder", Device),
    ("timeshift.secs", Device),
    ("updates.automatic", Device),
    ("updates.lastCheckedAt", Device),
    ("updates.lastResult", Device),
];

/// How `key` syncs, or `None` for a key nobody has classified.
///
/// Callers treat `None` as [`SettingSync::Device`]; it is a separate answer so the
/// contract test can tell "classified as device" from "forgotten".
pub fn classify(key: &str) -> Option<SettingSync> {
    SETTINGS.iter().find(|(k, _)| *k == key).map(|(_, s)| *s)
}

/// Whether a change to `key` should be sent to other devices.
pub fn syncs(key: &str) -> bool {
    classify(key) == Some(Personal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unclassified_key_stays_on_the_device() {
        assert_eq!(classify("something.new"), None);
        assert!(!syncs("something.new"));
    }

    #[test]
    fn personal_keys_sync_and_device_keys_do_not() {
        assert!(syncs("filter.english_only"));
        assert!(!syncs("timeshift.folder"));
        assert!(!syncs("credentials.namespace"));
    }

    #[test]
    fn no_key_is_listed_twice() {
        let mut keys: Vec<_> = SETTINGS.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "a key appears twice in SETTINGS");
    }
}
