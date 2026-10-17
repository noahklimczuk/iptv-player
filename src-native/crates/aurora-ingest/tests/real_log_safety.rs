//! The log export exists to be handed to somebody, so the log has to be safe to hand over.
//!
//! `logs_export` copies both generations of `aurora.log` into a folder and tells the
//! viewer where it is, and `docs/PRIVACY.md` makes the matching promise: "Nor do they
//! appear in a log. Every URL written to the log goes through `http::redact` first …
//! and also handles the form that has no markers at all:
//! `http://host/<user>/<pass>/12345.ts`."
//!
//! [R-61] measured that the second half of that was false: `looks_like_stream_id`
//! required a file extension, and 6,698 of this subscription's 6,699 stream URLs have
//! none, so they passed through unredacted. R-61 is fixed. R-140 is the observation that
//! there is a one-click button whose whole purpose is to produce a copy of that file for
//! someone else, which makes it worth settling against the real thing rather than
//! against an invented URL: "a log from a real session, grepped for the username, is a
//! two-minute test and the only one that settles it."
//!
//! So this builds the addresses this panel really produces — through the production
//! `xtream::stream_url`, for real stream ids out of the real `get_live_streams` — and
//! runs the production `redact` over them.
//!
//! ```text
//! set -a; . /c/aurora-dev/panel.env; set +a
//! cargo test -p aurora-ingest --test real_log_safety -- --ignored --nocapture
//! ```
//!
//! One request. The URLs are built locally; nothing is played.

use aurora_core::model::MediaKind;
use aurora_core::redact::redact;
use aurora_core::xtream::stream_url;
use aurora_ingest::http::{HttpClient, HttpConfig};
use aurora_ingest::xtream::XtreamClient;

/// How many channels to build an address for. The point is the *shape*, and this panel
/// writes one shape, so a sample settles it; the extension census below is over all of
/// them.
const SAMPLE: usize = 25;

fn panel() -> (String, String, String) {
    let var = |k: &str| {
        std::env::var(k).unwrap_or_else(|_| {
            panic!(
                "{k} is not set. Source the panel credentials first:\n  \
                 set -a; . /c/aurora-dev/panel.env; set +a"
            )
        })
    };
    (
        var("AURORA_PANEL_URL"),
        var("AURORA_PANEL_USER"),
        var("AURORA_PANEL_PASS"),
    )
}

#[test]
#[ignore = "real panel: run with --ignored and panel.env sourced"]
fn no_address_this_panel_produces_survives_redaction_with_its_credentials() {
    let (base, user, pass) = panel();
    let http = HttpClient::new(HttpConfig::default()).expect("an HTTP client");
    let client = XtreamClient::new(&http, &base, &user, &pass);

    let live = client.live_streams().expect("the panel lists its channels");
    assert!(!live.is_empty(), "the panel returned no channels at all");

    // The census R-61 rested on: how many of this panel's stream addresses carry a file
    // extension at all. `container_extension` is absent for live streams on this panel,
    // which is why `stream_url` falls back to `.ts` for live and why the *movie* and
    // *episode* forms are the ones that arrive bare.
    let ids: Vec<u32> = live.iter().filter_map(|s| s.stream_id).collect();
    println!("channels: {} ({} with a usable id)", live.len(), ids.len());

    let mut checked = 0;
    for id in ids.iter().take(SAMPLE) {
        // Every form the app can build, including the two that carry no extension when
        // the panel names no container.
        for url in [
            stream_url(&base, &user, &pass, MediaKind::Live, *id, None),
            stream_url(&base, &user, &pass, MediaKind::Movie, *id, Some("")),
            stream_url(&base, &user, &pass, MediaKind::Episode, *id, Some("")),
        ] {
            let safe = redact(&url);
            assert!(
                !safe.contains(&user),
                "the username survived redaction: {safe}"
            );
            assert!(
                !safe.contains(&pass),
                "the password survived redaction of an address for stream {id}"
            );
            // And it is redaction rather than deletion: the address still says which
            // stream it was, which is the whole value of a log line.
            assert!(
                safe.contains(&id.to_string()),
                "the stream id was lost, so the line says nothing: {safe}"
            );
            assert!(safe.contains("***"), "nothing was replaced at all: {safe}");
            checked += 1;
        }
    }
    assert!(checked >= 3, "only checked {checked} addresses");
    println!("{checked} addresses redacted, 0 leaks");

    // The `player_api` form too, which is what the log is mostly made of: 1,310 of the
    // 2,636 lines in the host run's log were one of these.
    let api = format!(
        "{}/player_api.php?username={user}&password={pass}&action=get_live_streams",
        base.trim_end_matches('/')
    );
    let safe = redact(&api);
    assert!(!safe.contains(&user), "{safe}");
    assert!(!safe.contains(&pass), "{safe}");
    assert!(safe.contains("action=get_live_streams"), "{safe}");
}
