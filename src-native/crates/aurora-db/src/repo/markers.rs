//! Skip-marker persistence and resolution (README §9).
//!
//! Storage keeps every source separately; [`resolve`] is what decides which marker the
//! Skip button actually offers, by merging the episode's own rows with anything the
//! series has learned from the user's earlier skips.

use aurora_core::markers::{learn_from_series, merge, MarkerKind, MarkerSource, SkipMarker};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// How many of the viewer's own skips it takes before the rest of the show inherits
/// them.
///
/// Two, which is `aurora_core::markers::MIN_AGREEING_SAMPLES` and has to stay equal to
/// it: that is where the agreement rule lives and this is the only caller that sets the
/// bar.
///
/// It was one, with a comment that named the cost and then accepted it — "one mis-drag
/// colours the show". It did, and worse: the median of one observation is that
/// observation, so a single twenty-second nudge became a marker on *every* episode of the
/// show. Recorded as `User`, it also outranked everything else and the per-show "always
/// skip" preference was allowed to fire it without asking.
///
/// Two agreeing skips is not a high bar for a real intro — somebody who skips the titles
/// skips them every week — and it is a bar an accident does not clear, because an accident
/// does not repeat in the same place. Chapters, where they exist, still outrank this
/// entirely and still need only themselves.
pub const DEFAULT_MIN_SAMPLES: usize = aurora_core::markers::MIN_AGREEING_SAMPLES;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPrefs {
    pub always_skip_intro: bool,
    pub always_skip_recap: bool,
    pub autoplay_next: bool,
}

impl Default for SeriesPrefs {
    fn default() -> Self {
        // Autoplay on, auto-skip off: skipping without being asked the first time is
        // startling, so the user opts in by using the button.
        Self {
            always_skip_intro: false,
            always_skip_recap: false,
            autoplay_next: true,
        }
    }
}

/// A stored row, with the safe reading of anything this build does not recognise.
///
/// The source fell back to `Learned`, and `may_auto_skip()` is
/// `!matches!(self, Convention)` -- so `Learned` returns **true**. A `source` value this
/// build does not know was therefore read back as a marker the per-show "always skip"
/// preference is allowed to fire automatically, which is the one thing
/// `may_auto_skip`'s own comment exists to prevent: "auto-skipping one would mean a
/// clock silently cutting the last minute of an episode whose runtime was wrong, which
/// is how somebody loses the final scene of a finale."
///
/// The safe fallback for an unknown value is the *least* trusted source, not the second
/// most. Reachable as soon as a newer build writes a source this one does not know, and
/// the UI's guard is the same string -- `activeMarker.source !== 'convention'` -- so it
/// inherited the fault rather than providing a second line of defence (R-45).
/// How a stored `source` is read when this build does not recognise it.
///
/// `Convention`, which is the only source `may_auto_skip()` says no to. The fallback was
/// `Learned`, and `may_auto_skip()` is `!matches!(self, Convention)` -- so `Learned` is
/// **true**, and an unrecognised value was read back as a marker the per-show "always
/// skip" preference is allowed to fire automatically. That is the one thing
/// `may_auto_skip`'s own comment exists to prevent: "auto-skipping one would mean a
/// clock silently cutting the last minute of an episode whose runtime was wrong, which
/// is how somebody loses the final scene of a finale."
///
/// The column constrains what can be stored -- `CHECK (source IN
/// ('chapters','user','learned','convention'))`, which migration 14 widened to match the
/// enum (R-44) -- so an unrecognised value can only arrive from a build newer than this
/// one. That is exactly when the old fallback would have bitten.
/// `convention` is itself such a value: it is computed and never stored, and if it ever
/// is, the old fallback would have turned the one source that must not auto-skip into
/// one that does (R-45).
fn source_or_safe(raw: &str) -> MarkerSource {
    MarkerSource::parse(raw).unwrap_or(MarkerSource::Convention)
}

fn row_to_marker(r: &rusqlite::Row<'_>) -> rusqlite::Result<SkipMarker> {
    let kind: String = r.get(0)?;
    let source: String = r.get(1)?;
    Ok(SkipMarker {
        kind: MarkerKind::parse(&kind).unwrap_or(MarkerKind::Intro),
        source: source_or_safe(&source),
        start_secs: r.get(2)?,
        end_secs: r.get(3)?,
    })
}

/// Store a marker. Replaces the previous row from the *same* source only, so a user's
/// skip never destroys chapter metadata and vice versa.
pub fn record(conn: &Connection, episode_id: i64, marker: &SkipMarker, now: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO skip_markers (episode_id, kind, source, start_secs, end_secs, created_at)
         VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT (episode_id, kind, source) DO UPDATE SET
           start_secs = excluded.start_secs,
           end_secs   = excluded.end_secs,
           created_at = excluded.created_at",
        params![
            episode_id,
            marker.kind.as_str(),
            marker.source.as_str(),
            marker.start_secs,
            marker.end_secs,
            now
        ],
    )?;
    Ok(())
}

/// Replace all chapter-derived markers for an episode. Called when a file loads and the
/// player reports its chapter list.
pub fn set_chapter_markers(
    conn: &mut Connection,
    episode_id: i64,
    markers: &[SkipMarker],
    now: i64,
) -> Result<usize> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM skip_markers WHERE episode_id = ?1 AND source = 'chapters'",
        params![episode_id],
    )?;
    let mut n = 0;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO skip_markers
               (episode_id, kind, source, start_secs, end_secs, created_at)
             VALUES (?1,?2,'chapters',?3,?4,?5)",
        )?;
        for m in markers.iter().filter(|m| m.is_plausible()) {
            stmt.execute(params![
                episode_id,
                m.kind.as_str(),
                m.start_secs,
                m.end_secs,
                now
            ])?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

/// Every stored marker for one episode, across all sources.
pub fn for_episode(conn: &Connection, episode_id: i64) -> Result<Vec<SkipMarker>> {
    let mut stmt = conn.prepare(
        "SELECT kind, source, start_secs, end_secs FROM skip_markers WHERE episode_id = ?1",
    )?;
    let rows = stmt
        .query_map(params![episode_id], row_to_marker)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Evidence of one kind, from one source, on *other* episodes of the same series.
pub fn series_observations(
    conn: &Connection,
    series_id: i64,
    kind: MarkerKind,
    source: MarkerSource,
    excluding_episode: i64,
) -> Result<Vec<(f64, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT m.start_secs, m.end_secs
         FROM skip_markers m
         JOIN episodes e ON e.id = m.episode_id
         WHERE e.series_id = ?1 AND m.kind = ?2 AND m.source = ?3 AND m.episode_id != ?4",
    )?;
    let rows = stmt
        .query_map(
            params![series_id, kind.as_str(), source.as_str(), excluding_episode],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn series_id_of(conn: &Connection, episode_id: i64) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT series_id FROM episodes WHERE id = ?1",
            params![episode_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// The markers the Skip button should use for this episode.
///
/// Four sources, each tried only where the one before it has nothing to say:
///
/// 1. **Stored on this episode** — its own chapters, or a skip somebody made here.
/// 2. **Chapters on a sibling episode.** One is enough: a chapter is evidence, not an
///    opinion, and a show whose first file carries them should give every other episode
///    its buttons without anybody pressing anything. This is the tier that was missing —
///    `series_observations` only ever looked at `source = 'user'`, so a library with
///    chapters on half its files learned nothing from them.
/// 3. **Skips the viewer made on other episodes**, median, needing `min_samples`.
/// 4. **Convention**, which can only ever produce the credits, and only when
///    `duration_secs` is known. See `aurora_core::markers::conventional` for why an
///    intro is not guessed.
///
/// `duration_secs` is what the player reports, or the episode's own runtime where the
/// stream does not say — `None` where neither is known, which simply skips tier 4.
pub fn resolve(
    conn: &Connection,
    episode_id: i64,
    min_samples: usize,
    duration_secs: Option<f64>,
) -> Result<Vec<SkipMarker>> {
    let stored = for_episode(conn, episode_id)?;
    let conventional = duration_secs
        .map(aurora_core::markers::conventional)
        .unwrap_or_default();

    let Some(series_id) = series_id_of(conn, episode_id)? else {
        return Ok(merge(&[stored, conventional]));
    };

    let mut learned = Vec::new();
    for kind in [MarkerKind::Intro, MarkerKind::Recap, MarkerKind::Credits] {
        if stored.iter().any(|m| m.kind == kind) {
            continue;
        }
        // Chapters first, and with a sample size of one, because they are the thing
        // this show actually says about itself.
        let from_chapters =
            series_observations(conn, series_id, kind, MarkerSource::Chapters, episode_id)?;
        if let Some(m) = learn_from_series(&from_chapters, kind, 1) {
            learned.push(m);
            continue;
        }
        // Then the viewer's, with presses and drags pooled.
        //
        // Pooled rather than ranked, because what the agreement rule needs is *samples of
        // the same region*, and a press and a drag over the same intro are exactly that.
        // Keeping them in separate buckets would mean a viewer who pressed Skip Intro on
        // one episode and dragged past the titles on another had taught the show nothing,
        // having told it the same thing twice.
        //
        // The ranking still happens, where it belongs: a drag is stored as `Seek`, so on
        // the episode it was made it ranks below a learned median and never auto-skips.
        // What is built here is `Learned` either way -- a median several episodes agree on
        // -- and that is the claim being made, whatever the samples behind it were.
        let mut from_viewer =
            series_observations(conn, series_id, kind, MarkerSource::User, episode_id)?;
        from_viewer.extend(series_observations(
            conn,
            series_id,
            kind,
            MarkerSource::Seek,
            episode_id,
        )?);
        if let Some(m) = learn_from_series(&from_viewer, kind, min_samples) {
            learned.push(m);
        }
    }
    Ok(merge(&[stored, learned, conventional]))
}

pub fn prefs(conn: &Connection, profile_id: i64, series_id: i64) -> Result<SeriesPrefs> {
    Ok(conn
        .query_row(
            "SELECT always_skip_intro, always_skip_recap, autoplay_next
             FROM series_prefs WHERE profile_id = ?1 AND series_id = ?2",
            params![profile_id, series_id],
            |r| {
                Ok(SeriesPrefs {
                    always_skip_intro: r.get::<_, i64>(0)? != 0,
                    always_skip_recap: r.get::<_, i64>(1)? != 0,
                    autoplay_next: r.get::<_, i64>(2)? != 0,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

pub fn set_prefs(
    conn: &Connection,
    profile_id: i64,
    series_id: i64,
    prefs: &SeriesPrefs,
    now: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO series_prefs
           (profile_id, series_id, always_skip_intro, always_skip_recap, autoplay_next, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT (profile_id, series_id) DO UPDATE SET
           always_skip_intro = excluded.always_skip_intro,
           always_skip_recap = excluded.always_skip_recap,
           autoplay_next     = excluded.autoplay_next,
           updated_at        = excluded.updated_at",
        params![
            profile_id,
            series_id,
            prefs.always_skip_intro as i32,
            prefs.always_skip_recap as i32,
            prefs.autoplay_next as i32,
            now
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::library::{upsert_series, NewEpisode, NewSeries};

    /// A series with `n` episodes in season 1, returning their ids in order.
    fn seeded(n: u16) -> (Connection, i64, Vec<i64>) {
        let mut conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO profiles (id,name,created_at) VALUES (1,'Me',0)",
            [],
        )
        .unwrap();

        let sid = upsert_series(
            &mut conn,
            1,
            &NewSeries {
                provider_key: "s",
                title: "Show",
                match_key: "show",
                ..Default::default()
            },
            0,
        )
        .unwrap();

        let eps: Vec<NewEpisode> = (1..=n)
            .map(|e| NewEpisode {
                season: 1,
                episode: e,
                title: Some(format!("E{e}")),
                url: format!("https://example.com/{e}.mkv"),
                still: None,
                ..Default::default()
            })
            .collect();
        crate::repo::library::upsert_episodes(&mut conn, sid, &eps, 0).unwrap();

        let ids = crate::repo::library::episodes_for(&conn, sid, None)
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        (conn, sid, ids)
    }

    fn intro(start: f64, end: f64, source: MarkerSource) -> SkipMarker {
        SkipMarker::new(MarkerKind::Intro, start, end, source)
    }

    /// A source this build does not recognise may not fire by itself.
    ///
    /// The fallback was `MarkerSource::Learned`, and `may_auto_skip()` is
    /// `!matches!(self, Convention)` — so `Learned` is **true**. An unrecognised
    /// `source` was therefore read back as a marker the per-show "always skip"
    /// preference is allowed to fire automatically, which is precisely what
    /// `may_auto_skip`'s own comment exists to prevent.
    ///
    /// Tested at the function rather than through a row, because `schema.rs` constrains
    /// the column to the three values this build writes — so the case arrives with the
    /// first migration that widens it, and `convention` is itself one of the values that
    /// would arrive that way (R-45).
    #[test]
    fn an_unknown_marker_source_is_never_skipped_automatically() {
        for raw in [
            "something-a-later-build-invented",
            "convention",
            "",
            "CHAPTERS",
        ] {
            let source = source_or_safe(raw);
            assert!(
                !source.may_auto_skip(),
                "{raw:?} was read back as {source:?}, which auto-skips"
            );
        }
    }

    /// And the three it does recognise still mean what they say.
    #[test]
    fn a_known_marker_source_round_trips() {
        for source in [
            MarkerSource::Chapters,
            MarkerSource::User,
            MarkerSource::Learned,
        ] {
            assert_eq!(source_or_safe(source.as_str()), source);
            assert!(source.may_auto_skip());
        }
    }

    /// The tier that was missing. A provider that ships chapters on one file should give
    /// every other episode its buttons, with nobody pressing anything — and before this
    /// `series_observations` only ever looked at `source = 'user'`, so it did not.
    #[test]
    fn chapters_on_one_episode_give_the_whole_show_its_buttons() {
        let (conn, _sid, eps) = seeded(4);
        record(&conn, eps[0], &intro(30.0, 95.0, MarkerSource::Chapters), 0).unwrap();

        for other in &eps[1..] {
            let resolved = resolve(&conn, *other, DEFAULT_MIN_SAMPLES, None).unwrap();
            let found = resolved
                .iter()
                .find(|m| m.kind == MarkerKind::Intro)
                .unwrap_or_else(|| panic!("episode {other} got no intro"));
            assert_eq!(found.start_secs, 30.0);
            assert_eq!(found.end_secs, 95.0);
            assert_eq!(found.source, MarkerSource::Learned);
        }
    }

    /// Two skips, not one — and this assertion has now been reversed twice, so the
    /// reasoning matters more than the number.
    ///
    /// It was two originally, became one to get a button onto a chapterless library, and
    /// is two again because one was unsafe in a way the change did not account for: the
    /// median of a single observation *is* that observation, so one twenty-second nudge
    /// became a marker on every episode of the show, recorded as `User` and therefore
    /// eligible to be auto-skipped without being asked.
    ///
    /// What makes two affordable now is that the first skip is no longer wasted. A drag
    /// is recorded as `Seek` and earns a button on *its own* episode immediately
    /// (`a_lone_drag_is_offered_but_never_fires_by_itself`), so the viewer is not made to
    /// watch two intros to earn the third. They get the button at once; the show only
    /// generalises once something agrees.
    #[test]
    fn one_skip_does_not_carry_to_the_rest_of_the_show() {
        let (conn, _sid, eps) = seeded(3);
        record(&conn, eps[0], &intro(10.0, 80.0, MarkerSource::User), 0).unwrap();

        let resolved = resolve(&conn, eps[1], DEFAULT_MIN_SAMPLES, None).unwrap();
        assert!(
            !resolved.iter().any(|m| m.kind == MarkerKind::Intro),
            "one skip became every episode's intro: {resolved:?}"
        );

        // A second, agreeing skip is what generalises it.
        record(&conn, eps[1], &intro(12.0, 78.0, MarkerSource::User), 0).unwrap();
        let after = resolve(&conn, eps[2], DEFAULT_MIN_SAMPLES, None).unwrap();
        let found = after
            .iter()
            .find(|m| m.kind == MarkerKind::Intro)
            .expect("two agreeing skips teach the show");
        assert_eq!(found.source, MarkerSource::Learned);
        assert_eq!((found.start_secs, found.end_secs), (12.0, 78.0));
    }

    /// A chapter is evidence and a skip is an opinion, so the chapter wins even when the
    /// viewer has skipped more often.
    #[test]
    fn a_sibling_chapter_outranks_the_viewers_own_skips() {
        let (conn, _sid, eps) = seeded(4);
        record(&conn, eps[0], &intro(5.0, 40.0, MarkerSource::User), 0).unwrap();
        record(&conn, eps[1], &intro(6.0, 41.0, MarkerSource::User), 0).unwrap();
        record(&conn, eps[2], &intro(30.0, 95.0, MarkerSource::Chapters), 0).unwrap();

        let resolved = resolve(&conn, eps[3], DEFAULT_MIN_SAMPLES, None).unwrap();
        let found = resolved
            .iter()
            .find(|m| m.kind == MarkerKind::Intro)
            .unwrap();
        assert_eq!(found.start_secs, 30.0, "the chapter should have won");
    }

    /// An episode's own marker is never overridden by what the rest of the show says.
    #[test]
    fn this_episodes_own_marker_wins_over_the_series() {
        let (conn, _sid, eps) = seeded(3);
        record(&conn, eps[0], &intro(30.0, 95.0, MarkerSource::Chapters), 0).unwrap();
        record(&conn, eps[1], &intro(0.0, 20.0, MarkerSource::User), 0).unwrap();

        let resolved = resolve(&conn, eps[1], DEFAULT_MIN_SAMPLES, None).unwrap();
        let found = resolved
            .iter()
            .find(|m| m.kind == MarkerKind::Intro)
            .unwrap();
        assert_eq!(found.start_secs, 0.0);
        assert_eq!(found.source, MarkerSource::User);
    }

    /// The whole point: no chapters anywhere, nothing ever skipped, and a Skip Credits
    /// button regardless — which is what makes autoplay work on an ordinary library.
    #[test]
    fn a_duration_alone_gives_credits_and_nothing_else() {
        let (conn, _sid, eps) = seeded(2);
        let resolved = resolve(&conn, eps[0], DEFAULT_MIN_SAMPLES, Some(45.0 * 60.0)).unwrap();

        let credits = resolved
            .iter()
            .find(|m| m.kind == MarkerKind::Credits)
            .expect("credits from the clock");
        assert_eq!(credits.source, MarkerSource::Convention);
        assert_eq!(credits.start_secs, 45.0 * 60.0 - 60.0);

        // And still no intro, because nothing has said where one is.
        assert!(!resolved.iter().any(|m| m.kind == MarkerKind::Intro));
    }

    #[test]
    fn a_stream_with_no_duration_gets_no_conventional_marker() {
        let (conn, _sid, eps) = seeded(2);
        let resolved = resolve(&conn, eps[0], DEFAULT_MIN_SAMPLES, None).unwrap();
        assert!(resolved.is_empty(), "{resolved:?}");
    }

    /// Evidence beats the clock, end to end through the database.
    #[test]
    fn a_real_credits_chapter_beats_the_clock() {
        let (conn, _sid, eps) = seeded(2);
        record(
            &conn,
            eps[0],
            &SkipMarker::new(MarkerKind::Credits, 2000.0, 2100.0, MarkerSource::Chapters),
            0,
        )
        .unwrap();

        let resolved = resolve(&conn, eps[0], DEFAULT_MIN_SAMPLES, Some(45.0 * 60.0)).unwrap();
        let credits: Vec<&SkipMarker> = resolved
            .iter()
            .filter(|m| m.kind == MarkerKind::Credits)
            .collect();
        assert_eq!(credits.len(), 1, "{resolved:?}");
        assert_eq!(credits[0].start_secs, 2000.0);
        assert_eq!(credits[0].source, MarkerSource::Chapters);
    }

    #[test]
    fn records_and_reads_back_a_marker() {
        let (conn, _, eps) = seeded(1);
        record(&conn, eps[0], &intro(30.0, 120.0, MarkerSource::User), 0).unwrap();

        let got = for_episode(&conn, eps[0]).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].start_secs, 30.0);
        assert_eq!(got[0].source, MarkerSource::User);
    }

    #[test]
    fn sources_coexist_and_chapters_win_the_merge() {
        let (conn, _, eps) = seeded(1);
        record(&conn, eps[0], &intro(30.0, 120.0, MarkerSource::User), 0).unwrap();
        record(
            &conn,
            eps[0],
            &intro(45.0, 135.0, MarkerSource::Chapters),
            0,
        )
        .unwrap();

        // Both rows are kept — the user's still feeds series learning.
        assert_eq!(for_episode(&conn, eps[0]).unwrap().len(), 2);

        let resolved = resolve(&conn, eps[0], DEFAULT_MIN_SAMPLES, None).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].source, MarkerSource::Chapters);
        assert_eq!(resolved[0].start_secs, 45.0);
    }

    #[test]
    fn re_recording_the_same_source_updates_in_place() {
        let (conn, _, eps) = seeded(1);
        record(&conn, eps[0], &intro(30.0, 120.0, MarkerSource::User), 0).unwrap();
        record(&conn, eps[0], &intro(35.0, 125.0, MarkerSource::User), 1).unwrap();

        let got = for_episode(&conn, eps[0]).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].start_secs, 35.0);
    }

    #[test]
    fn a_later_episode_learns_from_earlier_skips() {
        let (conn, _, eps) = seeded(4);
        record(&conn, eps[0], &intro(30.0, 120.0, MarkerSource::User), 0).unwrap();
        record(&conn, eps[1], &intro(32.0, 118.0, MarkerSource::User), 0).unwrap();

        // Episode 3 has no marker of its own, so it inherits what the other two agree
        // on -- the region *both* of them cover, which is 32..118 and not the pair of
        // independent medians (31..119) that neither episode had.
        let resolved = resolve(&conn, eps[2], DEFAULT_MIN_SAMPLES, None).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].source, MarkerSource::Learned);
        assert_eq!(resolved[0].start_secs, 32.0);
        assert_eq!(resolved[0].end_secs, 118.0);
    }

    /// Two skips in different places teach nothing, rather than teaching their average.
    #[test]
    fn skips_that_disagree_do_not_become_a_marker_on_a_third_episode() {
        let (conn, _, eps) = seeded(4);
        // One viewer skipping the titles; one losing patience with a scene.
        record(&conn, eps[0], &intro(0.0, 90.0, MarkerSource::User), 0).unwrap();
        record(&conn, eps[1], &intro(150.0, 200.0, MarkerSource::Seek), 0).unwrap();

        let resolved = resolve(&conn, eps[2], DEFAULT_MIN_SAMPLES, None).unwrap();
        assert!(
            !resolved.iter().any(|m| m.kind == MarkerKind::Intro),
            "two unrelated skips were averaged into an intro: {resolved:?}"
        );
    }

    /// A press and a drag over the same intro are two samples of one region.
    ///
    /// Pooled on purpose: the agreement rule needs samples of the same region, and a
    /// viewer who pressed Skip Intro on one episode and dragged past the titles on
    /// another has said the same thing twice. Bucketed by source they would have taught
    /// nothing.
    #[test]
    fn a_press_and_a_drag_agree_with_each_other() {
        let (conn, _, eps) = seeded(3);
        record(&conn, eps[0], &intro(10.0, 95.0, MarkerSource::User), 0).unwrap();
        record(&conn, eps[1], &intro(14.0, 92.0, MarkerSource::Seek), 0).unwrap();

        let resolved = resolve(&conn, eps[2], DEFAULT_MIN_SAMPLES, None).unwrap();
        let found = resolved
            .iter()
            .find(|m| m.kind == MarkerKind::Intro)
            .expect("a press and a drag over the same region are two samples");
        assert_eq!((found.start_secs, found.end_secs), (14.0, 92.0));
        assert_eq!(found.source, MarkerSource::Learned);
    }

    /// And what the drag earns on its *own* episode is a button, not a silent jump.
    #[test]
    fn a_lone_drag_is_offered_but_never_fires_by_itself() {
        let (conn, _, eps) = seeded(3);
        record(&conn, eps[0], &intro(8.0, 98.0, MarkerSource::Seek), 0).unwrap();

        // On the episode it was made: a button.
        let own = resolve(&conn, eps[0], DEFAULT_MIN_SAMPLES, None).unwrap();
        let found = own
            .iter()
            .find(|m| m.kind == MarkerKind::Intro)
            .expect("the drag earns a button on its own episode");
        assert_eq!(found.source, MarkerSource::Seek);
        assert!(
            !found.source.may_auto_skip(),
            "a single unconfirmed drag must never fire without being asked"
        );

        // And on every other episode: nothing, until something agrees with it.
        let next = resolve(&conn, eps[1], DEFAULT_MIN_SAMPLES, None).unwrap();
        assert!(
            !next.iter().any(|m| m.kind == MarkerKind::Intro),
            "one drag reached another episode: {next:?}"
        );
    }

    /// The bar the whole tier rests on, stated in one place.
    ///
    /// `DEFAULT_MIN_SAMPLES` is the only thing that sets it and it has to stay equal to
    /// the core's `MIN_AGREEING_SAMPLES`, which is where the agreement rule lives. If the
    /// two drift, this module asks for a number of samples the core does not cluster on
    /// and the result is silently neither rule.
    #[test]
    fn the_sample_bar_is_the_cores_own() {
        assert_eq!(
            DEFAULT_MIN_SAMPLES,
            aurora_core::markers::MIN_AGREEING_SAMPLES
        );
        // A const block, because clippy is right that this is decided at compile time
        // -- which is the point. A build whose bar drops back to one should not compile.
        const { assert!(DEFAULT_MIN_SAMPLES >= 2, "one observation is not a pattern") };
    }

    /// Ten skips in the same place do not widen the region beyond what they all cover.
    #[test]
    fn more_agreement_never_widens_the_skip() {
        let (conn, _, eps) = seeded(6);
        // Four skips of the same intro, each a little different, as real ones are.
        record(&conn, eps[0], &intro(30.0, 120.0, MarkerSource::User), 0).unwrap();
        record(&conn, eps[1], &intro(34.0, 116.0, MarkerSource::User), 0).unwrap();
        record(&conn, eps[2], &intro(28.0, 124.0, MarkerSource::Seek), 0).unwrap();
        record(&conn, eps[3], &intro(31.0, 119.0, MarkerSource::Seek), 0).unwrap();

        let resolved = resolve(&conn, eps[4], DEFAULT_MIN_SAMPLES, None).unwrap();
        let found = resolved
            .iter()
            .find(|m| m.kind == MarkerKind::Intro)
            .unwrap();
        assert_eq!(
            (found.start_secs, found.end_secs),
            (34.0, 116.0),
            "the region every sample covers, so the skip can never cut content one of \
             them said was wanted"
        );
    }

    #[test]
    fn an_episode_does_not_learn_from_its_own_skip() {
        let (conn, _, eps) = seeded(2);
        record(&conn, eps[0], &intro(30.0, 120.0, MarkerSource::User), 0).unwrap();
        record(&conn, eps[1], &intro(31.0, 121.0, MarkerSource::User), 0).unwrap();

        // Episode 1 already has its own row; the merge must use that, not a median
        // that includes itself.
        let resolved = resolve(&conn, eps[0], DEFAULT_MIN_SAMPLES, None).unwrap();
        assert_eq!(resolved[0].source, MarkerSource::User);
        assert_eq!(resolved[0].start_secs, 30.0);
    }

    #[test]
    fn learning_does_not_cross_series() {
        let (mut conn, _, eps_a) = seeded(2);
        record(&conn, eps_a[0], &intro(30.0, 120.0, MarkerSource::User), 0).unwrap();
        record(&conn, eps_a[1], &intro(31.0, 121.0, MarkerSource::User), 0).unwrap();

        let other = upsert_series(
            &mut conn,
            1,
            &NewSeries {
                provider_key: "s2",
                title: "Other",
                match_key: "other",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        crate::repo::library::upsert_episodes(
            &mut conn,
            other,
            &[NewEpisode {
                season: 1,
                episode: 1,
                url: "u".into(),
                ..Default::default()
            }],
            0,
        )
        .unwrap();
        let other_ep = crate::repo::library::episodes_for(&conn, other, None).unwrap()[0].id;

        assert!(resolve(&conn, other_ep, DEFAULT_MIN_SAMPLES, None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn chapter_markers_replace_the_previous_chapter_set() {
        let (mut conn, _, eps) = seeded(1);
        set_chapter_markers(
            &mut conn,
            eps[0],
            &[intro(45.0, 135.0, MarkerSource::Chapters)],
            0,
        )
        .unwrap();
        set_chapter_markers(
            &mut conn,
            eps[0],
            &[SkipMarker::new(
                MarkerKind::Credits,
                2600.0,
                2700.0,
                MarkerSource::Chapters,
            )],
            1,
        )
        .unwrap();

        let got = for_episode(&conn, eps[0]).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].kind, MarkerKind::Credits);
    }

    #[test]
    fn chapter_markers_do_not_disturb_user_rows() {
        let (mut conn, _, eps) = seeded(1);
        record(&conn, eps[0], &intro(30.0, 120.0, MarkerSource::User), 0).unwrap();
        set_chapter_markers(&mut conn, eps[0], &[], 1).unwrap();
        assert_eq!(for_episode(&conn, eps[0]).unwrap().len(), 1);
    }

    #[test]
    fn implausible_chapter_markers_are_not_stored() {
        let (mut conn, _, eps) = seeded(1);
        let n = set_chapter_markers(
            &mut conn,
            eps[0],
            &[intro(0.0, 2.0, MarkerSource::Chapters)],
            0,
        )
        .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn prefs_default_to_autoplay_on_and_auto_skip_off() {
        let (conn, sid, _) = seeded(1);
        let p = prefs(&conn, 1, sid).unwrap();
        assert!(p.autoplay_next);
        assert!(!p.always_skip_intro);
    }

    #[test]
    fn prefs_round_trip_per_profile_and_series() {
        let (conn, sid, _) = seeded(1);
        conn.execute(
            "INSERT INTO profiles (id,name,created_at) VALUES (2,'Kid',0)",
            [],
        )
        .unwrap();

        set_prefs(
            &conn,
            1,
            sid,
            &SeriesPrefs {
                always_skip_intro: true,
                always_skip_recap: false,
                autoplay_next: false,
            },
            0,
        )
        .unwrap();

        let mine = prefs(&conn, 1, sid).unwrap();
        assert!(mine.always_skip_intro);
        assert!(!mine.autoplay_next);

        // Another profile is unaffected.
        assert!(!prefs(&conn, 2, sid).unwrap().always_skip_intro);
    }

    #[test]
    fn deleting_an_episode_takes_its_markers_with_it() {
        let (conn, _, eps) = seeded(1);
        record(&conn, eps[0], &intro(30.0, 120.0, MarkerSource::User), 0).unwrap();
        conn.execute("DELETE FROM episodes WHERE id = ?1", params![eps[0]])
            .unwrap();

        let n: i64 = conn
            .query_row("SELECT count(*) FROM skip_markers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "cascade should clean up orphaned markers");
    }
}
