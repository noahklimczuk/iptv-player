//! Aurora TV host: owns the services and exposes them over typed IPC.
//!
//! README §3: the UI never touches the network or the database directly, and playback
//! state is owned here and mirrored to the UI.

pub mod commands;
pub mod dvr;
pub mod error;
pub mod library;
pub mod logging;
pub mod metadata;
pub mod playback;
pub mod playlist;
pub mod profiles;
pub mod providers;
pub mod recommend;
pub mod series;
pub mod services;
pub mod supervise;
pub mod timeshift;
pub mod updates;
pub mod window;

/// How long after launch the background passes start.
///
/// Long enough to be out of the way of the first thing anybody does. Startup already has
/// the migration check, the classifier and the first screen's queries to get through, and
/// the most likely next act is playing something — none of which wants to be sharing the
/// connection with a metadata sweep.
const STARTUP_GRACE: std::time::Duration = std::time::Duration::from_secs(20);

/// Pick up whatever the library is still missing, a little after launch.
///
/// Both of these passes existed and both were triggered **only by a refresh**. So a
/// library that was imported once and then simply used never finished its metadata and
/// never learned how many seasons anything had: the work was queued, nothing was wrong,
/// and nothing was going to run it. Knowing to press Refresh is not a reasonable thing to
/// require, and an app that only makes progress when prodded reads as an app that does
/// not work.
///
/// Safe to call unconditionally, which is why there is no "is there a backlog" check here
/// to get out of step with the ones that matter. Both passes already ask that question
/// properly and stop the moment the answer is no: enrichment does nothing at all without
/// a key, and each loop ends as soon as a batch finds nothing to do. On a library that is
/// up to date this costs one query each and two threads that exit.
pub fn catch_up_in_background(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(STARTUP_GRACE);
        // Different hosts — TMDB and the viewer's own panel — so these do not contend
        // with each other for anything but the database lock, which neither holds across
        // a request.
        crate::metadata::enrich_in_background(app.clone());
        crate::series::sweep_in_background(app);
    });
}

pub use error::AppError;

/// Where a portable copy keeps its data, or `None` for an installed one (README §13).
///
/// A `portable.txt` beside the exe is the marker. One definition, because two places
/// now need the answer: where the database goes, and whether an installer could
/// replace this copy at all (docs/DECISIONS.md D17).
pub fn portable_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .filter(|dir| dir.join("portable.txt").exists())
        .map(|dir| dir.join("data"))
}

/// Send an event to the UI, under a name Tauri will actually accept.
///
/// Two silent failures used to sit on this wire, and between them no host event ever
/// reached the interface.
///
/// The first is the name. Tauri 2 rejects any event name containing a `.` —
/// `is_event_name_valid` allows alphanumerics, `-`, `/`, `:` and `_`, and nothing
/// else — and every event this app defines was dotted: `player.state`,
/// `ingest.progress`, `update.download`, `dvr.tick`, `metadata.progress`. So `emit`
/// returned `Err(IllegalEventName)` every single time.
///
/// The second is what happened to that error. Every call site was written
/// `let _ = app.emit(…)`, on the reasonable-sounding grounds that a dropped event
/// must never fail the work that produced it. The result was that a *systematically*
/// dropped event looked exactly like a quiet one.
///
/// What a viewer saw: an OSD stuck on "Nothing playing" over a stream that was
/// running, so every transport button was drawn from empty state and the player
/// looked dead; an update whose progress bar never moved; a refresh whose spinner
/// never moved. The UI half of this is in `src-ui/src/ipc/subscribe.ts`, which was
/// listening to a global that does not exist — so even a legal name would have gone
/// nowhere.
///
/// Callers keep passing the dotted name they already use in TypeScript; the
/// translation happens here and in `subscribe.ts`, so the two ends cannot drift.
/// A failure is logged rather than discarded, because that is the whole lesson.
pub fn emit<S: serde::Serialize + Clone>(app: &tauri::AppHandle, event: &str, payload: S) {
    use tauri::Emitter;
    if let Err(e) = app.emit(&wire_event_name(event), payload) {
        tracing::error!(event, "the UI never received this event: {e}");
    }
}

/// The name an event travels under. `player.state` becomes `player:state`.
pub fn wire_event_name(event: &str) -> String {
    event.replace('.', ":")
}

/// Wall-clock seconds. One definition, because four copies of it drifting apart is a
/// class of bug nobody finds.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod event_name_tests {
    use super::wire_event_name;

    /// Tauri's own rule, copied from `tauri::event::event_name::is_event_name_valid`.
    /// Copied rather than called because it is private — and because the point of
    /// this test is to fail here rather than at runtime, where the failure was a
    /// discarded `Result` and an interface that quietly stopped updating.
    fn tauri_would_accept(event: &str) -> bool {
        event
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '/' || c == ':' || c == '_')
    }

    /// Every event this app emits, exactly as the call sites spell it. A new one added
    /// without a line here is a new one nobody has checked.
    const EVENTS: &[&str] = &[
        "player.state",
        "ingest.progress",
        "update.available",
        "update.download",
        "dvr.tick",
        "metadata.progress",
        "metadata.done",
        "artwork.progress",
    ];

    /// The bug: none of these could ever be sent.
    #[test]
    fn the_names_the_app_uses_are_not_ones_tauri_accepts() {
        for event in EVENTS {
            assert!(
                !tauri_would_accept(event),
                "{event} no longer needs translating; if the dots have gone, this \
                 module and `subscribe.ts` can go too"
            );
        }
    }

    #[test]
    fn every_event_survives_translation() {
        for event in EVENTS {
            let wire = wire_event_name(event);
            assert!(
                tauri_would_accept(&wire),
                "{event} still cannot be emitted as {wire}"
            );
        }
    }

    /// The UI translates the same way, in `src-ui/src/ipc/subscribe.ts`. If these two
    /// ever disagree the events go out under names nothing is listening for, which is
    /// silent in exactly the way this whole fix is about.
    #[test]
    fn translation_matches_what_the_ui_listens_for() {
        assert_eq!(wire_event_name("player.state"), "player:state");
        assert_eq!(wire_event_name("update.download"), "update:download");
        // Only dots change; nothing else is touched.
        assert_eq!(wire_event_name("plain"), "plain");
        assert_eq!(wire_event_name("a.b.c"), "a:b:c");
    }
}
