//! Writing fetched metadata onto a title, and remembering what has been tried.
//!
//! The write is deliberately narrow. A playlist refresh already knows not to touch the
//! enrichment-owned columns (`repo::library`); this is the other half of that contract —
//! enrichment never touches the provider-owned ones, so the two can run in any order
//! without either clobbering the other.

use aurora_core::tmdb::Metadata;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// What kind of title a row refers to. Polymorphic like `watch_progress`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Movie,
    Series,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Movie => "movie",
            ItemKind::Series => "series",
        }
    }

    /// The table this kind lives in. Only ever called with the two variants above, so
    /// the value never reaches SQL from user input.
    fn table(self) -> &'static str {
        match self {
            ItemKind::Movie => "movies",
            ItemKind::Series => "series",
        }
    }
}

/// Outcome of one enrichment attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Matched,
    /// Searched, and nothing was confident enough. Not an error — some titles are not
    /// in the database, and asking again tomorrow will not change that.
    NoMatch,
    /// The attempt itself failed (network, rate limit). Worth retrying.
    Failed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Matched => "matched",
            State::NoMatch => "nomatch",
            State::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "matched" => State::Matched,
            "failed" => State::Failed,
            _ => State::NoMatch,
        }
    }
}

/// A title waiting to be enriched.
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub id: i64,
    pub title: String,
    pub year: Option<i32>,
}

/// Artwork URLs, already resolved to absolute by the client.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Artwork {
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub logo: Option<String>,
}

/// Titles that have never been attempted, or whose attempt failed and is worth retrying.
///
/// `retry_failed_before` is a timestamp: failures older than it come back, so a refresh
/// during an outage does not permanently mark a library unmatchable.
pub fn pending(
    conn: &Connection,
    kind: ItemKind,
    limit: u32,
    retry_failed_before: i64,
) -> Result<Vec<Pending>> {
    let sql = format!(
        "SELECT t.id, t.title, t.year FROM {} t
         LEFT JOIN enrichment e ON e.item_kind = ?1 AND e.item_id = t.id
         WHERE e.item_id IS NULL
            OR (e.state = 'failed' AND e.attempted_at < ?2)
         ORDER BY t.added_at DESC, t.id
         LIMIT ?3",
        kind.table()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![kind.as_str(), retry_failed_before, limit], |r| {
        Ok(Pending {
            id: r.get(0)?,
            title: r.get(1)?,
            year: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Record that a title was searched and nothing matched, or that the attempt failed.
pub fn mark(conn: &Connection, kind: ItemKind, item_id: i64, state: State, now: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO enrichment (item_kind, item_id, state, attempted_at)
         VALUES (?1,?2,?3,?4)
         ON CONFLICT (item_kind, item_id) DO UPDATE SET
           state = excluded.state,
           attempted_at = excluded.attempted_at,
           tmdb_id = NULL,
           confidence = NULL",
        params![kind.as_str(), item_id, state.as_str(), now],
    )?;
    Ok(())
}

/// Write metadata onto a title, replace its credits, and record the match.
///
/// A field TMDB does not have never overwrites one the provider supplied: `COALESCE`
/// on the incoming value means a missing overview leaves the existing one alone rather
/// than blanking it.
pub fn save(
    conn: &mut Connection,
    kind: ItemKind,
    item_id: i64,
    meta: &Metadata,
    artwork: &Artwork,
    confidence: f32,
    now: i64,
) -> Result<()> {
    let tx = conn.transaction()?;

    let genres = if meta.genres.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&meta.genres)?)
    };

    // `runtime_mins` only exists on movies, so the two statements differ by that column.
    match kind {
        ItemKind::Movie => {
            tx.execute(
                "UPDATE movies SET
                   overview      = COALESCE(?2, overview),
                   poster        = COALESCE(?3, poster),
                   backdrop      = COALESCE(?4, backdrop),
                   logo_art      = COALESCE(?5, logo_art),
                   rating        = COALESCE(?6, rating),
                   certification = COALESCE(?7, certification),
                   genres        = COALESCE(?8, genres),
                   tmdb_id       = ?9,
                   runtime_mins  = COALESCE(?10, runtime_mins)
                 WHERE id = ?1",
                params![
                    item_id,
                    meta.overview,
                    artwork.poster,
                    artwork.backdrop,
                    artwork.logo,
                    meta.rating,
                    meta.certification,
                    genres,
                    meta.tmdb_id,
                    meta.runtime_mins,
                ],
            )?;
        }
        ItemKind::Series => {
            tx.execute(
                "UPDATE series SET
                   overview      = COALESCE(?2, overview),
                   poster        = COALESCE(?3, poster),
                   backdrop      = COALESCE(?4, backdrop),
                   logo_art      = COALESCE(?5, logo_art),
                   rating        = COALESCE(?6, rating),
                   certification = COALESCE(?7, certification),
                   genres        = COALESCE(?8, genres),
                   tmdb_id       = ?9
                 WHERE id = ?1",
                params![
                    item_id,
                    meta.overview,
                    artwork.poster,
                    artwork.backdrop,
                    artwork.logo,
                    meta.rating,
                    meta.certification,
                    genres,
                    meta.tmdb_id,
                ],
            )?;
        }
    }

    // Replace rather than merge: a re-match to a different title must not leave the
    // previous cast attached to it.
    tx.execute(
        "DELETE FROM credits WHERE item_kind = ?1 AND item_id = ?2",
        params![kind.as_str(), item_id],
    )?;
    for credit in &meta.credits {
        tx.execute(
            "INSERT INTO people (tmdb_id, name, profile_path) VALUES (?1,?2,?3)
             ON CONFLICT (tmdb_id) DO UPDATE SET
               name = excluded.name,
               profile_path = COALESCE(excluded.profile_path, profile_path)",
            params![
                credit.person.tmdb_id,
                credit.person.name,
                credit.person.profile_path
            ],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO credits (item_kind, item_id, person_id, role, is_cast, ord)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                kind.as_str(),
                item_id,
                credit.person.tmdb_id,
                credit.role.clone().unwrap_or_default(),
                credit.is_cast as i64,
                credit.order,
            ],
        )?;
    }

    tx.execute(
        "INSERT INTO enrichment (item_kind, item_id, state, tmdb_id, confidence, attempted_at)
         VALUES (?1,?2,'matched',?3,?4,?5)
         ON CONFLICT (item_kind, item_id) DO UPDATE SET
           state = 'matched',
           tmdb_id = excluded.tmdb_id,
           confidence = excluded.confidence,
           attempted_at = excluded.attempted_at",
        params![kind.as_str(), item_id, meta.tmdb_id, confidence, now],
    )?;

    tx.commit()?;
    Ok(())
}

/// One person as a detail page lists them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditRow {
    pub person_id: i64,
    pub name: String,
    pub profile_path: Option<String>,
    pub role: Option<String>,
    pub is_cast: bool,
}

/// Billed cast first, then crew, each in the order TMDB gave.
pub fn credits_for(conn: &Connection, kind: ItemKind, item_id: i64) -> Result<Vec<CreditRow>> {
    let mut stmt = conn.prepare(
        "SELECT c.person_id, p.name, p.profile_path, c.role, c.is_cast
         FROM credits c JOIN people p ON p.tmdb_id = c.person_id
         WHERE c.item_kind = ?1 AND c.item_id = ?2
         ORDER BY c.is_cast DESC, c.ord, p.name",
    )?;
    let rows = stmt.query_map(params![kind.as_str(), item_id], |r| {
        let role: String = r.get(3)?;
        Ok(CreditRow {
            person_id: r.get(0)?,
            name: r.get(1)?,
            profile_path: r.get(2)?,
            role: Some(role).filter(|s| !s.is_empty()),
            is_cast: r.get::<_, i64>(4)? != 0,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Everything a person appears in, for the "more with this actor" row.
pub fn titles_for_person(conn: &Connection, person_id: i64) -> Result<Vec<(ItemKind, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT item_kind, item_id FROM credits WHERE person_id = ?1 ORDER BY item_kind, ord",
    )?;
    let rows = stmt.query_map([person_id], |r| {
        let kind: String = r.get(0)?;
        Ok((
            if kind == "series" {
                ItemKind::Series
            } else {
                ItemKind::Movie
            },
            r.get::<_, i64>(1)?,
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Forget everything about a title, for when it leaves the library.
///
/// `credits` and `enrichment` are polymorphic and so carry no foreign key onto `movies`
/// or `series`; without this they would outlive the title they describe.
pub fn forget(conn: &Connection, kind: ItemKind, item_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM credits WHERE item_kind = ?1 AND item_id = ?2",
        params![kind.as_str(), item_id],
    )?;
    conn.execute(
        "DELETE FROM enrichment WHERE item_kind = ?1 AND item_id = ?2",
        params![kind.as_str(), item_id],
    )?;
    Ok(())
}

/// People no longer credited on anything. Run after `forget` to keep the table bounded.
pub fn prune_people(conn: &Connection) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM people WHERE tmdb_id NOT IN (SELECT person_id FROM credits)",
        [],
    )?)
}

/// What enrichment recorded for a title, for the settings screen and for re-matching.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: State,
    pub tmdb_id: Option<i64>,
    pub confidence: Option<f32>,
    pub attempted_at: i64,
}

pub fn status(conn: &Connection, kind: ItemKind, item_id: i64) -> Result<Option<Status>> {
    Ok(conn
        .query_row(
            "SELECT state, tmdb_id, confidence, attempted_at FROM enrichment
             WHERE item_kind = ?1 AND item_id = ?2",
            params![kind.as_str(), item_id],
            |r| {
                let state: String = r.get(0)?;
                Ok(Status {
                    state: State::parse(&state),
                    tmdb_id: r.get(1)?,
                    confidence: r.get(2)?,
                    attempted_at: r.get(3)?,
                })
            },
        )
        .optional()?)
}

/// How far enrichment has got, for the progress line in settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub total: i64,
    pub matched: i64,
    pub no_match: i64,
    pub failed: i64,
}

impl Coverage {
    pub fn pending(&self) -> i64 {
        (self.total - self.matched - self.no_match - self.failed).max(0)
    }
}

pub fn coverage(conn: &Connection, kind: ItemKind) -> Result<Coverage> {
    let total: i64 =
        conn.query_row(&format!("SELECT COUNT(*) FROM {}", kind.table()), [], |r| {
            r.get(0)
        })?;
    let mut stmt =
        conn.prepare("SELECT state, COUNT(*) FROM enrichment WHERE item_kind = ?1 GROUP BY state")?;
    let mut out = Coverage {
        total,
        ..Default::default()
    };
    let rows = stmt.query_map([kind.as_str()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (state, count) = row?;
        match State::parse(&state) {
            State::Matched => out.matched = count,
            State::NoMatch => out.no_match = count,
            State::Failed => out.failed = count,
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::tmdb::{Credit, Person};
    fn db() -> Connection {
        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn
    }

    /// A movie row. Inserted directly rather than through `upsert_movies`, because what
    /// is under test here is what enrichment writes, not how a playlist import writes.
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

    fn person(id: i64, name: &str) -> Person {
        Person {
            tmdb_id: id,
            name: name.into(),
            profile_path: Some(format!("/p{id}.jpg")),
        }
    }

    fn sample_metadata() -> Metadata {
        Metadata {
            tmdb_id: 603,
            overview: Some("A hacker learns the truth.".into()),
            runtime_mins: Some(136),
            rating: Some(8.2),
            certification: Some("R".into()),
            genres: vec!["Action".into(), "Science Fiction".into()],
            credits: vec![
                Credit {
                    person: person(6384, "Keanu Reeves"),
                    role: Some("Neo".into()),
                    is_cast: true,
                    order: 0,
                },
                Credit {
                    person: person(2975, "Laurence Fishburne"),
                    role: Some("Morpheus".into()),
                    is_cast: true,
                    order: 1,
                },
                Credit {
                    person: person(9339, "Lana Wachowski"),
                    role: Some("Director".into()),
                    is_cast: false,
                    order: 0,
                },
            ],
            ..Default::default()
        }
    }

    fn artwork() -> Artwork {
        Artwork {
            poster: Some("https://img/p.jpg".into()),
            backdrop: Some("https://img/b.jpg".into()),
            logo: Some("https://img/l.png".into()),
        }
    }

    #[test]
    fn saving_metadata_fills_the_title_and_its_credits() {
        let mut conn = db();
        let id = movie(&conn, "m1", "The Matrix", Some(1999));
        save(
            &mut conn,
            ItemKind::Movie,
            id,
            &sample_metadata(),
            &artwork(),
            0.98,
            100,
        )
        .unwrap();

        let (overview, poster, runtime, rating, cert, genres, tmdb): (
            String,
            String,
            i64,
            f64,
            String,
            String,
            i64,
        ) = conn
            .query_row(
                "SELECT overview, poster, runtime_mins, rating, certification, genres, tmdb_id
                 FROM movies WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .unwrap();
        assert!(overview.starts_with("A hacker"));
        assert_eq!(poster, "https://img/p.jpg");
        assert_eq!(runtime, 136);
        assert!((rating - 8.2).abs() < 0.001);
        assert_eq!(cert, "R");
        assert_eq!(genres, r#"["Action","Science Fiction"]"#);
        assert_eq!(tmdb, 603);

        let credits = credits_for(&conn, ItemKind::Movie, id).unwrap();
        // Cast before crew, each in billing order.
        assert_eq!(
            credits.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["Keanu Reeves", "Laurence Fishburne", "Lana Wachowski"]
        );
        assert_eq!(credits[0].role.as_deref(), Some("Neo"));
        assert!(credits[0].is_cast);
        assert!(!credits[2].is_cast);
    }

    #[test]
    fn a_field_tmdb_lacks_does_not_blank_what_the_provider_supplied() {
        let mut conn = db();
        let id = movie(&conn, "m1", "Obscure Film", Some(1977));
        conn.execute(
            "UPDATE movies SET overview = 'From the playlist', certification = '15' WHERE id = ?1",
            [id],
        )
        .unwrap();

        // TMDB matched, but has no overview and no certification for this one.
        let meta = Metadata {
            tmdb_id: 42,
            poster_path: Some("/p.jpg".into()),
            ..Default::default()
        };
        save(&mut conn, ItemKind::Movie, id, &meta, &artwork(), 0.8, 100).unwrap();

        let (overview, cert): (String, String) = conn
            .query_row(
                "SELECT overview, certification FROM movies WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            overview, "From the playlist",
            "a missing overview must not blank one"
        );
        assert_eq!(cert, "15");
    }

    #[test]
    fn re_matching_replaces_the_cast_rather_than_appending_to_it() {
        let mut conn = db();
        let id = movie(&conn, "m1", "Wrong Match", None);
        save(
            &mut conn,
            ItemKind::Movie,
            id,
            &sample_metadata(),
            &artwork(),
            0.8,
            100,
        )
        .unwrap();
        assert_eq!(credits_for(&conn, ItemKind::Movie, id).unwrap().len(), 3);

        let corrected = Metadata {
            tmdb_id: 999,
            credits: vec![Credit {
                person: person(1, "Someone Else"),
                role: Some("Lead".into()),
                is_cast: true,
                order: 0,
            }],
            ..Default::default()
        };
        save(
            &mut conn,
            ItemKind::Movie,
            id,
            &corrected,
            &artwork(),
            0.9,
            200,
        )
        .unwrap();

        let credits = credits_for(&conn, ItemKind::Movie, id).unwrap();
        assert_eq!(
            credits.len(),
            1,
            "the previous cast must not survive a re-match"
        );
        assert_eq!(credits[0].name, "Someone Else");
    }

    #[test]
    fn saving_the_same_metadata_twice_is_idempotent() {
        let mut conn = db();
        let id = movie(&conn, "m1", "The Matrix", Some(1999));
        for _ in 0..3 {
            save(
                &mut conn,
                ItemKind::Movie,
                id,
                &sample_metadata(),
                &artwork(),
                0.98,
                100,
            )
            .unwrap();
        }
        assert_eq!(credits_for(&conn, ItemKind::Movie, id).unwrap().len(), 3);
        let people: i64 = conn
            .query_row("SELECT COUNT(*) FROM people", [], |r| r.get(0))
            .unwrap();
        assert_eq!(people, 3);
    }

    #[test]
    fn a_person_in_two_films_is_stored_once() {
        let mut conn = db();
        let a = movie(&conn, "m1", "First", None);
        let b = movie(&conn, "m2", "Second", None);
        save(
            &mut conn,
            ItemKind::Movie,
            a,
            &sample_metadata(),
            &artwork(),
            0.9,
            100,
        )
        .unwrap();
        save(
            &mut conn,
            ItemKind::Movie,
            b,
            &sample_metadata(),
            &artwork(),
            0.9,
            100,
        )
        .unwrap();

        let people: i64 = conn
            .query_row("SELECT COUNT(*) FROM people", [], |r| r.get(0))
            .unwrap();
        assert_eq!(people, 3);
        assert_eq!(titles_for_person(&conn, 6384).unwrap().len(), 2);
    }

    #[test]
    fn pending_skips_what_has_already_been_answered() {
        let mut conn = db();
        let matched = movie(&conn, "m1", "Matched", None);
        let no_match = movie(&conn, "m2", "Not In The Database", None);
        let untried = movie(&conn, "m3", "Untried", None);

        save(
            &mut conn,
            ItemKind::Movie,
            matched,
            &sample_metadata(),
            &artwork(),
            0.9,
            100,
        )
        .unwrap();
        mark(&conn, ItemKind::Movie, no_match, State::NoMatch, 100).unwrap();

        let ids: Vec<i64> = pending(&conn, ItemKind::Movie, 100, 0)
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(
            ids,
            vec![untried],
            "a settled title must not be asked about again"
        );
    }

    #[test]
    fn a_failed_attempt_comes_back_but_only_after_its_cooldown() {
        let conn = db();
        let id = movie(&conn, "m1", "Timed Out", None);
        mark(&conn, ItemKind::Movie, id, State::Failed, 1_000).unwrap();

        // Still inside the cooldown: leave it alone.
        assert!(pending(&conn, ItemKind::Movie, 100, 500)
            .unwrap()
            .is_empty());
        // Past it: worth another try, because the failure was the network's, not the
        // title's.
        assert_eq!(
            pending(&conn, ItemKind::Movie, 100, 2_000).unwrap().len(),
            1
        );
    }

    #[test]
    fn pending_is_bounded_and_carries_what_a_search_needs() {
        let conn = db();
        for i in 0..5 {
            movie(
                &conn,
                &format!("m{i}"),
                &format!("Film {i}"),
                Some(2000 + i),
            );
        }
        let batch = pending(&conn, ItemKind::Movie, 2, 0).unwrap();
        assert_eq!(batch.len(), 2);
        assert!(batch[0].year.is_some());
        assert!(!batch[0].title.is_empty());
    }

    #[test]
    fn marking_a_previously_matched_title_clears_its_match() {
        let mut conn = db();
        let id = movie(&conn, "m1", "The Matrix", Some(1999));
        save(
            &mut conn,
            ItemKind::Movie,
            id,
            &sample_metadata(),
            &artwork(),
            0.98,
            100,
        )
        .unwrap();
        assert_eq!(
            status(&conn, ItemKind::Movie, id).unwrap().unwrap().tmdb_id,
            Some(603)
        );

        mark(&conn, ItemKind::Movie, id, State::Failed, 200).unwrap();
        let s = status(&conn, ItemKind::Movie, id).unwrap().unwrap();
        assert_eq!(s.state, State::Failed);
        assert_eq!(s.tmdb_id, None, "a stale id must not outlive its match");
        assert_eq!(s.confidence, None);
    }

    #[test]
    fn status_of_an_untried_title_is_none() {
        let conn = db();
        let id = movie(&conn, "m1", "Untried", None);
        assert_eq!(status(&conn, ItemKind::Movie, id).unwrap(), None);
    }

    #[test]
    fn coverage_counts_every_outcome_and_what_is_left() {
        let mut conn = db();
        let a = movie(&conn, "m1", "A", None);
        let b = movie(&conn, "m2", "B", None);
        let c = movie(&conn, "m3", "C", None);
        movie(&conn, "m4", "D", None);

        save(
            &mut conn,
            ItemKind::Movie,
            a,
            &sample_metadata(),
            &artwork(),
            0.9,
            100,
        )
        .unwrap();
        mark(&conn, ItemKind::Movie, b, State::NoMatch, 100).unwrap();
        mark(&conn, ItemKind::Movie, c, State::Failed, 100).unwrap();

        let cov = coverage(&conn, ItemKind::Movie).unwrap();
        assert_eq!(cov.total, 4);
        assert_eq!(cov.matched, 1);
        assert_eq!(cov.no_match, 1);
        assert_eq!(cov.failed, 1);
        assert_eq!(cov.pending(), 1);
    }

    #[test]
    fn coverage_of_an_empty_library_is_all_zeroes_not_an_error() {
        let conn = db();
        let cov = coverage(&conn, ItemKind::Series).unwrap();
        assert_eq!(cov, Coverage::default());
        assert_eq!(cov.pending(), 0);
    }

    #[test]
    fn forgetting_a_title_leaves_no_orphans_behind() {
        let mut conn = db();
        let id = movie(&conn, "m1", "Gone", None);
        save(
            &mut conn,
            ItemKind::Movie,
            id,
            &sample_metadata(),
            &artwork(),
            0.9,
            100,
        )
        .unwrap();

        forget(&conn, ItemKind::Movie, id).unwrap();
        assert!(credits_for(&conn, ItemKind::Movie, id).unwrap().is_empty());
        assert_eq!(status(&conn, ItemKind::Movie, id).unwrap(), None);

        // The people are still there until they are pruned — they may be in other films.
        assert_eq!(prune_people(&conn).unwrap(), 3);
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM people", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn pruning_keeps_people_who_are_still_credited() {
        let mut conn = db();
        let a = movie(&conn, "m1", "First", None);
        let b = movie(&conn, "m2", "Second", None);
        save(
            &mut conn,
            ItemKind::Movie,
            a,
            &sample_metadata(),
            &artwork(),
            0.9,
            100,
        )
        .unwrap();
        save(
            &mut conn,
            ItemKind::Movie,
            b,
            &sample_metadata(),
            &artwork(),
            0.9,
            100,
        )
        .unwrap();

        forget(&conn, ItemKind::Movie, a).unwrap();
        assert_eq!(
            prune_people(&conn).unwrap(),
            0,
            "they are still in the other film"
        );
        assert_eq!(credits_for(&conn, ItemKind::Movie, b).unwrap().len(), 3);
    }

    #[test]
    fn movies_and_series_with_the_same_id_do_not_collide() {
        let mut conn = db();
        let id = movie(&conn, "m1", "Shared Id", None);
        conn.execute(
            "INSERT INTO series (id,provider_id,provider_key,title,match_key,last_seen_at)
             VALUES (?1,1,'s1','Shared Id','shared id',0)",
            [id],
        )
        .unwrap();

        save(
            &mut conn,
            ItemKind::Movie,
            id,
            &sample_metadata(),
            &artwork(),
            0.9,
            100,
        )
        .unwrap();
        assert_eq!(credits_for(&conn, ItemKind::Movie, id).unwrap().len(), 3);
        assert!(
            credits_for(&conn, ItemKind::Series, id).unwrap().is_empty(),
            "the series with the same row id must be untouched"
        );
        assert_eq!(status(&conn, ItemKind::Series, id).unwrap(), None);
    }
}
