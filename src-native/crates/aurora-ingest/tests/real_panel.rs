//! Checks that only a real subscription can answer.
//!
//! Everything else in this crate's tests answers its own questions — a `TestServer`
//! replies with whatever the test wants to see, which is how `max_connections` came to
//! be parsed, carried into `ProviderStatus`, shown once in the wizard and never stored
//! (R-108) without a single red test anywhere.
//!
//! These are `#[ignore]` by default because CI has no subscription, and they read the
//! credentials from the environment rather than carrying any:
//!
//! ```text
//! set -a; . /c/aurora-dev/panel.env; set +a
//! cargo test -p aurora-ingest --test real_panel -- --ignored --nocapture
//! ```
//!
//! They are deliberately cheap — one request each. A full `sync::fetch` against this
//! line moves the whole playlist and a 14.8 MB guide, and the line allows one
//! connection, so a test that did that could not run beside anything else.

use aurora_ingest::http::{HttpClient, HttpConfig};
use aurora_ingest::xtream::XtreamClient;

/// The panel to ask, or a clear failure. Not a silent skip: these tests only run when
/// somebody asked for them by name, and a run that quietly did nothing would be worse
/// than one that says the credentials are missing.
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

fn client() -> HttpClient {
    HttpClient::new(HttpConfig {
        max_attempts: 1,
        ..Default::default()
    })
    .expect("an HTTP client")
}

/// The number every connection budget in the app depends on, read from the line itself.
///
/// `providers.max_connections` was never written by any production path, so the DVR cap,
/// the warning strip and multi-view's refusal all ran on a NULL — on *this* subscription,
/// which allows exactly one stream and answers HTTP 555 when that is exceeded. The value
/// was never unavailable: it is on the authentication response, which is what this asks
/// for. See R-108.
#[test]
#[ignore = "real panel: run with --ignored and panel.env sourced"]
fn the_line_declares_how_many_connections_it_allows() {
    let (url, user, pass) = panel();
    let http = client();
    let x = XtreamClient::new(&http, &url, &user, &pass);

    let status = x
        .authenticate(0)
        .expect("the panel should authenticate with these credentials");

    println!(
        "   panel: active={} max_connections={:?} active_connections={:?} trial={}",
        status.active, status.max_connections, status.active_connections, status.is_trial
    );

    assert!(status.active, "the subscription is not active");
    let limit = status
        .max_connections
        .expect("this line declares a connection limit, and the app now stores it");
    assert!(limit >= 1, "a limit of {limit} is not a limit");

    // Recorded rather than asserted as 1: the plan can change, and a test that pinned it
    // would fail on an upgrade instead of on a bug. What matters to R-108 is that a
    // number arrives at all, because the column held NULL regardless of what this said.
    if limit == 1 {
        println!("   panel: one connection, so a multi-tile mosaic must be refused here");
    }
}

/// The DVR's own default is wrong for this line, which is the point.
///
/// `DEFAULT_MAX_CONCURRENT` is 2 and was what every install used. Asserting the gap here
/// keeps the reason the fix exists attached to the real number.
#[test]
#[ignore = "real panel: run with --ignored and panel.env sourced"]
fn the_default_the_dvr_used_to_fall_back_to_is_more_than_this_line_allows() {
    let (url, user, pass) = panel();
    let http = client();
    let limit = XtreamClient::new(&http, &url, &user, &pass)
        .authenticate(0)
        .expect("the panel should authenticate")
        .max_connections
        .expect("a declared limit");

    const DVR_DEFAULT: u32 = 2;
    assert!(
        limit < DVR_DEFAULT,
        "this line allows {limit}, so the DVR's default of {DVR_DEFAULT} would have been \
         safe after all — check whether the subscription changed before relaxing anything"
    );
}
