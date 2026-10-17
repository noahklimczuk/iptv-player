//! What an episode sweep costs, against the panel it will actually run against.
//!
//! The sweep's queue was "shows with no episodes stored", which is the same answer for a
//! show nobody has asked about and for one that has been asked and will not list. A show
//! that produced nothing stayed in the queue, the order is deterministic, and the loop
//! ran until a batch asked about nothing — so the unanswerable remainder of a sweep was
//! asked for again on every one of its 200 passes. With N such shows that is up to
//! 200 × N requests to the viewer's own panel: the same host their video comes from, on a
//! line that answers HTTP 555 when it has one connection too many (R-83).
//!
//! The unit tests in `aurora-db` prove the queue no longer does that. What they cannot
//! say is whether the outcomes that trigger it happen on a real line, so this asks the
//! real panel for a handful of listings, feeds the answers it gives into the real queue,
//! and counts the requests a whole sweep would make.
//!
//! `#[ignore]` because CI has no subscription, and the credentials come from the
//! environment rather than from this file:
//!
//! ```text
//! set -a; . /c/aurora-dev/panel.env; set +a
//! cargo test -p aurora-ingest --test real_series_listings -- --ignored --nocapture
//! ```
//!
//! Deliberately cheap: one `get_series` and `SAMPLE` listings, sequentially, because the
//! line allows one connection.

use aurora_db::repo::filtering::LibraryFilter;
use aurora_db::repo::library::{self, ListingAttempt};
use aurora_db::rusqlite::{params, Connection};
use aurora_ingest::http::{HttpClient, HttpConfig};
use aurora_ingest::xtream::XtreamClient;

/// How many shows to ask about. Enough to meet both answers, few enough to be polite.
const SAMPLE: usize = 12;

/// What `sweep_in_background` gives up after, and the multiplier in "200 × N".
const SWEEP_MAX_BATCHES: usize = 200;

/// Six hours, as `aurora_app::series::LISTING_RETRY_AFTER_SECS`. Not imported: this
/// crate does not depend on `aurora-app`, and a copy that drifts would only make the
/// test kinder.
const LISTING_RETRY_AFTER_SECS: i64 = 6 * 3600;

const NOW: i64 = 1_700_000_000;

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

/// A library holding nothing but the shows this test is about.
///
/// In memory, because the one thing this must not do is touch a real library — see the
/// note in `real_letters.rs`. The provider row exists only to satisfy the foreign key.
fn library(ids: &[(i64, u32)]) -> Connection {
    let conn = aurora_db::open_memory().expect("a fresh library opens");
    conn.execute(
        "INSERT INTO providers (id,name,kind,base_url,created_at)
         VALUES (1,'P','xtream','https://example.invalid',0)",
        [],
    )
    .unwrap();
    for (row_id, stream_id) in ids {
        conn.execute(
            "INSERT INTO series (id, provider_id, provider_key, title, match_key,
                                 hidden, added_at, last_seen_at)
             VALUES (?1, 1, ?2, ?3, ?3, 0, 0, 0)",
            params![
                row_id,
                format!("series:{stream_id}"),
                format!("Show {stream_id}")
            ],
        )
        .unwrap();
    }
    conn
}

/// `fetch_episodes`' own filter: an id that parses as a `u32` and an episode number.
/// A listing of episodes that fail it writes no rows, which is the `Ok(0)` path.
fn usable(info: &aurora_core::xtream::SeriesInfo) -> usize {
    info.episodes
        .iter()
        .filter(|ep| {
            ep.episode_num.is_some()
                && ep
                    .id
                    .as_deref()
                    .is_some_and(|id| id.trim().parse::<u32>().is_ok())
        })
        .count()
}

#[test]
#[ignore = "real panel: run with --ignored and panel.env sourced"]
fn a_sweep_asks_about_each_show_once_however_the_panel_answers() {
    let (base, user, pass) = panel();
    let http = HttpClient::new(HttpConfig::default()).expect("an HTTP client");
    let client = XtreamClient::new(&http, &base, &user, &pass);

    let listed = client.series().expect("the panel lists its shows");
    assert!(!listed.is_empty(), "the panel returned no shows at all");
    println!("shows on this line: {}", listed.len());

    // The first few, plus one id the panel cannot have, so both outcomes are present
    // whatever this subscription happens to answer for its real shows. A sweep hits the
    // failure path through any transient refusal, which is the trigger the audit could
    // not provoke on demand.
    let mut stream_ids: Vec<u32> = listed
        .iter()
        // A listing with no id is one a sweep could not ask about either.
        .filter_map(|s| s.series_id)
        .take(SAMPLE - 1)
        .collect();
    stream_ids.push(u32::MAX);

    let rows: Vec<(i64, u32)> = stream_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (i as i64 + 1, *id))
        .collect();
    let conn = library(&rows);
    let filter = LibraryFilter::default();

    // One pass of the real queue, answered by the real panel.
    let mut requests = 0usize;
    let mut empty = Vec::new();
    let mut failed = Vec::new();
    let mut wrote = Vec::new();

    for batch in 0..SWEEP_MAX_BATCHES {
        let queue =
            library::series_needing_episodes(&conn, &filter, 200, NOW - LISTING_RETRY_AFTER_SECS)
                .expect("the queue reads");
        if queue.is_empty() {
            println!("sweep ended after {} batch(es)", batch);
            break;
        }
        for row_id in queue {
            let stream_id = rows
                .iter()
                .find(|(r, _)| *r == row_id)
                .map(|(_, s)| *s)
                .expect("every queued row is one this test inserted");
            requests += 1;

            match client.series_info(stream_id) {
                Ok(info) if usable(&info) > 0 => {
                    // What `fetch_episodes` would store. One row is enough to take the
                    // show out of the queue, which is the condition that was always
                    // right.
                    conn.execute(
                        "INSERT INTO episodes (series_id, season, episode, url, added_at)
                         VALUES (?1, 1, 1, '', 0)",
                        params![row_id],
                    )
                    .unwrap();
                    library::clear_listing_attempt(&conn, row_id).unwrap();
                    wrote.push(stream_id);
                }
                Ok(_) => {
                    library::record_listing_attempt(&conn, row_id, ListingAttempt::Empty, NOW)
                        .unwrap();
                    empty.push(stream_id);
                }
                Err(e) => {
                    library::record_listing_attempt(&conn, row_id, ListingAttempt::Failed, NOW)
                        .unwrap();
                    failed.push((stream_id, e.message));
                }
            }
        }
    }

    println!(
        "asked {requests} time(s) for {} show(s): {} listed, {} empty, {} refused",
        rows.len(),
        wrote.len(),
        empty.len(),
        failed.len()
    );
    for (id, why) in &failed {
        println!("  {id} would not list: {why}");
    }

    // The claim. Before this fix every show in `empty` and `failed` stayed in the queue,
    // so the sweep's last batch became its next one and the count was
    // `shows + 200 * unanswerable`.
    assert_eq!(
        requests,
        rows.len(),
        "a sweep asked for {} listings to cover {} shows",
        requests,
        rows.len()
    );
    assert!(
        !empty.is_empty() || !failed.is_empty(),
        "every show answered, so this run proves nothing about the path that did not. \
         The id that cannot exist should have refused."
    );

    // And the queue is empty rather than holding the same shows back.
    let left = library::series_needing_episodes(&conn, &filter, 200, NOW - LISTING_RETRY_AFTER_SECS)
        .unwrap();
    assert!(left.is_empty(), "{left:?} would be asked about again");

    // Once the cooldown passes they are worth one more request each -- which is the
    // other half of the trade, and is bounded the same way.
    let later = NOW + LISTING_RETRY_AFTER_SECS + 1;
    let again =
        library::series_needing_episodes(&conn, &filter, 200, later - LISTING_RETRY_AFTER_SECS)
            .unwrap();
    assert_eq!(
        again.len(),
        empty.len() + failed.len(),
        "the shows that produced nothing should come back exactly once each: {again:?}"
    );
}
