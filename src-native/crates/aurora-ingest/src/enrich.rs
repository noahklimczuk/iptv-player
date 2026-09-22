//! The enrichment pass: search, choose, write (README §4.5).
//!
//! Runs in bounded batches against a rate-limited service, over a library that may hold
//! forty thousand titles. So the shape of it is: take what has not been answered, ask
//! once, record the answer whatever it is, stop when the budget is spent. Nothing here
//! retries forever and nothing asks the same question twice.

use aurora_core::tmdb::{self, Query};
use aurora_db::repo::enrichment::{self, Artwork, ItemKind, State};
use aurora_db::rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::tmdb::{ImageSize, Kind, MetadataClient};

/// How long a failed attempt waits before it is worth trying again. A provider outage or
/// a rate-limit burst should not permanently mark half a library unmatchable.
pub const RETRY_AFTER_SECS: i64 = 6 * 3600;

/// Titles per pass. Bounded so a first run shows progress and can be interrupted, rather
/// than blocking for an hour and losing everything if it is cancelled.
pub const DEFAULT_BATCH: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub batch: u32,
    /// Enrich movies, series, or both.
    pub movies: bool,
    pub series: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            batch: DEFAULT_BATCH,
            movies: true,
            series: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub matched: usize,
    /// Searched, nothing confident enough. Not a failure.
    pub no_match: usize,
    /// The attempt itself failed. These come back after the cooldown.
    pub failed: usize,
}

impl Report {
    pub fn attempted(&self) -> usize {
        self.matched + self.no_match + self.failed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

fn kind_of(item: ItemKind) -> Kind {
    match item {
        ItemKind::Movie => Kind::Movie,
        ItemKind::Series => Kind::Series,
    }
}

/// One title to look up.
#[derive(Debug, Clone, PartialEq)]
pub struct Work {
    pub kind: ItemKind,
    pub item: enrichment::Pending,
}

/// A successful lookup: everything to be written onto the title.
#[derive(Debug, Clone, PartialEq)]
pub struct Matched {
    pub meta: aurora_core::tmdb::Metadata,
    pub artwork: Artwork,
    pub confidence: f32,
}

/// What a lookup produced, before anything is written.
///
/// The match is boxed because it carries a whole `Metadata` while the other two
/// variants carry nothing, and every `Outcome` in the pass would otherwise be sized for
/// the largest.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Matched(Box<Matched>),
    /// Searched, nothing confident enough.
    NoMatch,
    /// The attempt failed; worth retrying after the cooldown.
    Failed,
}

/// Which titles this pass would look up. Needs the database only for as long as this
/// call takes.
pub fn plan(db: &Connection, options: &Options, now: i64) -> aurora_db::Result<Vec<Work>> {
    let mut kinds = Vec::new();
    if options.movies {
        kinds.push(ItemKind::Movie);
    }
    if options.series {
        kinds.push(ItemKind::Series);
    }

    let retry_before = now - RETRY_AFTER_SECS;
    let mut work = Vec::new();
    for kind in kinds {
        for item in enrichment::pending(db, kind, options.batch, retry_before)? {
            work.push(Work { kind, item });
        }
    }
    Ok(work)
}

/// Look one title up. Touches the network and nothing else — deliberately, so the
/// caller can hold no database lock while this runs.
///
/// That separation is the point: enrichment does one or two network round trips per
/// title, and holding the single writer connection across them would stall every other
/// command, including the DVR scheduler deciding whether a recording is due.
pub fn fetch_one(client: &dyn MetadataClient, work: &Work) -> Outcome {
    let query = Query::new(&work.item.title, work.item.year);
    let candidates = match client.search(kind_of(work.kind), &query.title, query.year) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(
                "metadata search failed for {:?}: {}",
                work.item.title,
                e.message
            );
            return Outcome::Failed;
        }
    };

    let Some(best) = tmdb::pick_best(&query, &candidates) else {
        // Declining is a real answer, and recording it is what stops the same question
        // being asked on every refresh from now on.
        return Outcome::NoMatch;
    };
    let (tmdb_id, confidence) = (best.candidate.id, best.score);

    let meta = match client.details(kind_of(work.kind), tmdb_id) {
        Ok(m) => m,
        Err(e) => {
            // Matched, but the details call failed. Not a no-match: the title is in the
            // database, so this is worth retrying.
            tracing::debug!("metadata details failed for {tmdb_id}: {}", e.message);
            return Outcome::Failed;
        }
    };

    let artwork = Artwork {
        poster: client.image_url(meta.poster_path.as_deref(), ImageSize::Poster),
        backdrop: client.image_url(meta.backdrop_path.as_deref(), ImageSize::Backdrop),
        logo: client.image_url(meta.logo_path.as_deref(), ImageSize::Logo),
    };
    Outcome::Matched(Box::new(Matched {
        meta,
        artwork,
        confidence,
    }))
}

/// Write one outcome. Brief, so the lock is held per title rather than per pass.
pub fn apply(
    db: &mut Connection,
    work: &Work,
    outcome: &Outcome,
    now: i64,
) -> aurora_db::Result<()> {
    match outcome {
        Outcome::Matched(m) => enrichment::save(
            db,
            work.kind,
            work.item.id,
            &m.meta,
            &m.artwork,
            m.confidence,
            now,
        ),
        Outcome::NoMatch => enrichment::mark(db, work.kind, work.item.id, State::NoMatch, now),
        Outcome::Failed => enrichment::mark(db, work.kind, work.item.id, State::Failed, now),
    }
}

/// Count one outcome into a report.
pub fn tally(report: &mut Report, outcome: &Outcome) {
    match outcome {
        Outcome::Matched(_) => report.matched += 1,
        Outcome::NoMatch => report.no_match += 1,
        Outcome::Failed => report.failed += 1,
    }
}

/// Enrich one batch against a connection the caller owns outright.
///
/// Convenience for tests and for any caller that is not sharing its connection. A host
/// that holds one behind a lock should drive [`plan`], [`fetch_one`] and [`apply`]
/// itself so the lock is never held across the network.
pub fn run(
    db: &mut Connection,
    client: &dyn MetadataClient,
    options: &Options,
    now: i64,
    mut on_progress: impl FnMut(Progress),
) -> aurora_db::Result<Report> {
    let work = plan(db, options, now)?;
    let total = work.len();
    let mut report = Report::default();

    for (done, item) in work.iter().enumerate() {
        on_progress(Progress { done, total });
        let outcome = fetch_one(client, item);
        apply(db, item, &outcome, now)?;
        tally(&mut report, &outcome);
    }

    on_progress(Progress { done: total, total });
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::neterr::NetFailure;
    use aurora_core::tmdb::{Candidate, Credit, Metadata, Person};
    use aurora_db::rusqlite::params;
    use std::sync::Mutex;

    /// Answers from a script, and counts what it was asked.
    #[derive(Default)]
    struct FakeClient {
        results: Vec<Candidate>,
        details: Option<Metadata>,
        fail_search: bool,
        fail_details: bool,
        searches: Mutex<Vec<String>>,
        detail_calls: Mutex<Vec<i64>>,
    }

    impl MetadataClient for FakeClient {
        fn search(
            &self,
            _kind: Kind,
            title: &str,
            _year: Option<i32>,
        ) -> Result<Vec<Candidate>, NetFailure> {
            self.searches.lock().unwrap().push(title.to_string());
            if self.fail_search {
                return Err(NetFailure::classify("connection timed out"));
            }
            Ok(self.results.clone())
        }

        fn details(&self, _kind: Kind, id: i64) -> Result<Metadata, NetFailure> {
            self.detail_calls.lock().unwrap().push(id);
            if self.fail_details {
                return Err(NetFailure::classify("HTTP 429"));
            }
            Ok(self.details.clone().unwrap_or(Metadata {
                tmdb_id: id,
                ..Default::default()
            }))
        }

        fn image_url(&self, path: Option<&str>, size: ImageSize) -> Option<String> {
            Some(format!("https://img/{}{}", size.as_path(), path?))
        }
    }

    fn db() -> Connection {
        let conn = aurora_db::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn
    }

    fn movie(conn: &Connection, key: &str, title: &str, year: Option<i32>) -> i64 {
        conn.execute(
            "INSERT INTO movies (provider_id, provider_key, title, match_key, year, url,
                                 added_at, last_seen_at)
             VALUES (1,?1,?2,?3,?4,'https://example.com/m.mkv',0,0)",
            params![key, title, aurora_core::title::match_key(title), year],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn movies_only() -> Options {
        Options {
            series: false,
            ..Default::default()
        }
    }

    #[test]
    fn a_confident_match_is_written_onto_the_title() {
        let mut conn = db();
        let id = movie(&conn, "m1", "The Matrix", Some(1999));
        let client = FakeClient {
            results: vec![Candidate::new(603, "The Matrix", Some(1999))],
            details: Some(Metadata {
                tmdb_id: 603,
                overview: Some("A hacker learns the truth.".into()),
                poster_path: Some("/p.jpg".into()),
                runtime_mins: Some(136),
                credits: vec![Credit {
                    person: Person {
                        tmdb_id: 6384,
                        name: "Keanu Reeves".into(),
                        profile_path: None,
                    },
                    role: Some("Neo".into()),
                    is_cast: true,
                    order: 0,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let report = run(&mut conn, &client, &movies_only(), 1_000, |_| {}).unwrap();
        assert_eq!(report.matched, 1);
        assert_eq!(report.no_match, 0);

        let (overview, poster, tmdb): (String, String, i64) = conn
            .query_row(
                "SELECT overview, poster, tmdb_id FROM movies WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert!(overview.starts_with("A hacker"));
        // The artwork URL is absolute and at the size the UI renders.
        assert_eq!(poster, "https://img/w342/p.jpg");
        assert_eq!(tmdb, 603);
        assert_eq!(
            enrichment::credits_for(&conn, ItemKind::Movie, id).unwrap()[0].name,
            "Keanu Reeves"
        );
    }

    #[test]
    fn an_unmatched_title_is_recorded_so_it_is_not_asked_about_again() {
        let mut conn = db();
        let id = movie(&conn, "m1", "Some Home Video", None);
        let client = FakeClient::default(); // searches return nothing

        let report = run(&mut conn, &client, &movies_only(), 1_000, |_| {}).unwrap();
        assert_eq!(report.no_match, 1);
        assert_eq!(
            enrichment::status(&conn, ItemKind::Movie, id)
                .unwrap()
                .unwrap()
                .state,
            State::NoMatch
        );

        // Second pass: nothing left to ask.
        let again = run(&mut conn, &client, &movies_only(), 2_000, |_| {}).unwrap();
        assert_eq!(again.attempted(), 0);
        assert_eq!(
            client.searches.lock().unwrap().len(),
            1,
            "asked once, not twice"
        );
    }

    #[test]
    fn an_ambiguous_result_declines_rather_than_guessing() {
        let mut conn = db();
        let id = movie(&conn, "m1", "Alone", None);
        let client = FakeClient {
            results: vec![
                Candidate::new(1, "Alone", Some(2020)),
                Candidate::new(2, "Alone", Some(2015)),
            ],
            ..Default::default()
        };

        let report = run(&mut conn, &client, &movies_only(), 1_000, |_| {}).unwrap();
        assert_eq!(report.no_match, 1);
        assert!(
            client.detail_calls.lock().unwrap().is_empty(),
            "no details fetched for a guess"
        );
        let tmdb: Option<i64> = conn
            .query_row("SELECT tmdb_id FROM movies WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(tmdb, None);
    }

    #[test]
    fn a_failed_search_is_retried_after_the_cooldown_but_not_before() {
        let mut conn = db();
        movie(&conn, "m1", "The Matrix", Some(1999));
        let failing = FakeClient {
            fail_search: true,
            ..Default::default()
        };

        let report = run(&mut conn, &failing, &movies_only(), 1_000, |_| {}).unwrap();
        assert_eq!(report.failed, 1);

        // Too soon.
        let soon = run(&mut conn, &failing, &movies_only(), 1_100, |_| {}).unwrap();
        assert_eq!(soon.attempted(), 0);

        // Past the cooldown, and the service is back.
        let working = FakeClient {
            results: vec![Candidate::new(603, "The Matrix", Some(1999))],
            ..Default::default()
        };
        let later = run(
            &mut conn,
            &working,
            &movies_only(),
            1_000 + RETRY_AFTER_SECS + 1,
            |_| {},
        )
        .unwrap();
        assert_eq!(later.matched, 1, "a network failure must not be permanent");
    }

    #[test]
    fn a_details_failure_after_a_match_is_retryable_not_a_no_match() {
        let mut conn = db();
        let id = movie(&conn, "m1", "The Matrix", Some(1999));
        let client = FakeClient {
            results: vec![Candidate::new(603, "The Matrix", Some(1999))],
            fail_details: true,
            ..Default::default()
        };

        let report = run(&mut conn, &client, &movies_only(), 1_000, |_| {}).unwrap();
        assert_eq!(report.failed, 1);
        // The title IS in the database — recording no-match would give up on it for good.
        assert_eq!(
            enrichment::status(&conn, ItemKind::Movie, id)
                .unwrap()
                .unwrap()
                .state,
            State::Failed
        );
    }

    #[test]
    fn one_title_failing_does_not_stop_the_batch() {
        let mut conn = db();
        for i in 0..4 {
            movie(&conn, &format!("m{i}"), "The Matrix", Some(1999));
        }
        // Every search succeeds; details fail for all of them. The point is that the run
        // completes rather than aborting at the first error.
        let client = FakeClient {
            results: vec![Candidate::new(603, "The Matrix", Some(1999))],
            fail_details: true,
            ..Default::default()
        };
        let report = run(&mut conn, &client, &movies_only(), 1_000, |_| {}).unwrap();
        assert_eq!(report.failed, 4);
        assert_eq!(report.attempted(), 4);
    }

    #[test]
    fn the_batch_size_bounds_the_work() {
        let mut conn = db();
        for i in 0..10 {
            movie(&conn, &format!("m{i}"), &format!("Film {i}"), None);
        }
        let options = Options {
            batch: 3,
            series: false,
            ..Default::default()
        };
        let client = FakeClient::default();

        let report = run(&mut conn, &client, &options, 1_000, |_| {}).unwrap();
        assert_eq!(report.attempted(), 3);
        assert_eq!(client.searches.lock().unwrap().len(), 3);
    }

    #[test]
    fn progress_runs_from_zero_to_the_total() {
        let mut conn = db();
        for i in 0..3 {
            movie(&conn, &format!("m{i}"), &format!("Film {i}"), None);
        }
        let mut seen = Vec::new();
        run(
            &mut conn,
            &FakeClient::default(),
            &movies_only(),
            1_000,
            |p| seen.push((p.done, p.total)),
        )
        .unwrap();

        assert_eq!(seen.first(), Some(&(0, 3)));
        assert_eq!(
            seen.last(),
            Some(&(3, 3)),
            "the last tick must show completion"
        );
    }

    #[test]
    fn an_empty_library_reports_nothing_and_asks_nothing() {
        let mut conn = db();
        let client = FakeClient::default();
        let report = run(&mut conn, &client, &Options::default(), 1_000, |_| {}).unwrap();
        assert_eq!(report, Report::default());
        assert!(client.searches.lock().unwrap().is_empty());
    }

    #[test]
    fn turning_off_a_content_type_skips_it_entirely() {
        let mut conn = db();
        movie(&conn, "m1", "A Film", None);
        conn.execute(
            "INSERT INTO series (provider_id,provider_key,title,match_key,last_seen_at)
             VALUES (1,'s1','A Show','a show',0)",
            [],
        )
        .unwrap();

        let client = FakeClient::default();
        let options = Options {
            movies: false,
            series: true,
            ..Default::default()
        };
        run(&mut conn, &client, &options, 1_000, |_| {}).unwrap();

        assert_eq!(
            client.searches.lock().unwrap().as_slice(),
            &["A Show".to_string()]
        );
    }

    #[test]
    fn the_year_from_the_playlist_reaches_the_search() {
        let mut conn = db();
        movie(&conn, "m1", "Heat", Some(1995));
        let client = FakeClient {
            results: vec![
                Candidate::new(1, "Heat", Some(1995)),
                Candidate::new(2, "Heat", Some(1986)),
            ],
            ..Default::default()
        };

        let report = run(&mut conn, &client, &movies_only(), 1_000, |_| {}).unwrap();
        // Without the year these two would be inseparable and the run would decline.
        assert_eq!(report.matched, 1);
        assert_eq!(client.detail_calls.lock().unwrap().as_slice(), &[1]);
    }
}
