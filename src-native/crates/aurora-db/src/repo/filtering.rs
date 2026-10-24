//! Library-wide filters: one language, one copy of everything (README §7.3, §13).
//!
//! Both filters are answered at query time rather than by deleting rows. A provider
//! refresh brings the hidden things back anyway, and a viewer who turns "English only"
//! off expects their library to return intact — so nothing here destroys anything. The
//! cost is one predicate per list query, which is why `lang_code` and `quality_rank`
//! are columns rather than something computed per paint.

use aurora_core::{lang, title};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::repo::settings;

const KEY_ENGLISH_ONLY: &str = "filter.english_only";
const KEY_HIDE_DUPLICATES: &str = "filter.hide_duplicates";
const KEY_HIDE_UNTAGGED: &str = "filter.hide_untagged";
/// Bumped when classification changes, so an existing library is re-read rather than
/// silently keeping whatever the previous build decided.
///
/// 3: `QFR`, `EXYU`, `TWN` and `AFG`, and a bare prefix may be up to four letters.
/// 2: `aurora_core::lang` learned the codes a real subscription used (`SW`, `BE`, `MT`,
/// `CH`, `HT`, `SU`, `SG`, `ID`) and how to read a bare two-letter prefix.
pub const CLASSIFIER_VERSION: i64 = 4;
const KEY_CLASSIFIER_VERSION: &str = "filter.classifier_version";

/// Which list a query is about. Not a string, so no caller can put one into SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Live,
    Movies,
    Series,
}

impl Kind {
    pub fn table(self) -> &'static str {
        match self {
            Kind::Live => "channels",
            Kind::Movies => "movies",
            Kind::Series => "series",
        }
    }

    /// Movies and series are the same title only if they are also the same year;
    /// channels have no year to disagree about.
    fn has_year(self) -> bool {
        !matches!(self, Kind::Live)
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "live" | "channels" => Some(Kind::Live),
            "movies" | "movie" => Some(Kind::Movies),
            "series" => Some(Kind::Series),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFilter {
    /// Hide what is positively identified as another language. Untagged content stays:
    /// see `aurora_core::lang`.
    pub english_only: bool,
    /// Show one entry per title, the best copy of it.
    pub hide_duplicates: bool,
    /// Hide what could not be identified at all, as well as what was identified as
    /// something else. Off by default, and deliberately separate from `english_only`.
    ///
    /// The two are different questions. "Hide other languages" is safe everywhere: a
    /// playlist that tags nothing loses nothing. "Only English" is the stricter thing a
    /// viewer usually means, and on a playlist with no tags at all it empties the
    /// library — which is why it cannot simply be what `english_only` does.
    ///
    /// Whether it is safe is a property of the library, not of the code, and the
    /// settings screen already knows the number: `FilterCounts::untagged` is exactly
    /// what this hides. On the subscription this was built against that is 1,774 of
    /// 22,121 channels, and the strict reading leaves 7,417 — which is what the viewer
    /// asked for. On a playlist that tags nothing it would be all of them, and the
    /// screen says so before the switch is touched.
    pub hide_untagged: bool,
}

impl LibraryFilter {
    pub fn load(conn: &Connection) -> Result<Self> {
        Ok(Self {
            english_only: settings::get_or(conn, KEY_ENGLISH_ONLY, false)?,
            hide_duplicates: settings::get_or(conn, KEY_HIDE_DUPLICATES, false)?,
            hide_untagged: settings::get_or(conn, KEY_HIDE_UNTAGGED, false)?,
        })
    }

    pub fn save(&self, conn: &Connection) -> Result<()> {
        settings::set(conn, KEY_ENGLISH_ONLY, &self.english_only)?;
        settings::set(conn, KEY_HIDE_DUPLICATES, &self.hide_duplicates)?;
        settings::set(conn, KEY_HIDE_UNTAGGED, &self.hide_untagged)?;
        Ok(())
    }

    /// Conditions to `AND` onto any query over `kind`'s table, referring to it by its
    /// own name. Empty when nothing is filtered, so the query is unchanged.
    ///
    /// Duplicate collapsing is phrased as "no better copy of this exists" rather than
    /// as a grouping, so it composes with whatever else the caller is already asking —
    /// a genre, a sort, a limit — instead of forcing every query to be rewritten around
    /// a subquery.
    pub fn where_sql(&self, kind: Kind) -> String {
        let t = kind.table();
        let mut out = String::new();
        if self.english_only {
            out.push_str(&self.language_clause(t));
        }
        if self.hide_duplicates {
            out.push_str(&format!(
                " AND NOT EXISTS (SELECT 1 FROM {t} dup
                     WHERE {}
                       AND (dup.quality_rank > {t}.quality_rank
                            OR (dup.quality_rank = {t}.quality_rank AND dup.id < {t}.id)))",
                self.same_title_sql(kind, "dup", t),
            ));
        }
        out
    }

    /// What makes two rows of `kind` the same thing, between two aliases of its table.
    ///
    /// One definition, used by everything that cares about a duplicate group: the
    /// subquery in `where_sql` that picks the survivor, and `group_of`, which says what
    /// that survivor is standing for. Written once because the two have to agree exactly
    /// — a group the collapser and the merger disagree about either shows a row twice or
    /// loses what the hidden row was carrying.
    fn same_title_sql(&self, kind: Kind, a: &str, b: &str) -> String {
        let mut out = format!("{a}.match_key = {b}.match_key AND {a}.hidden = 0");
        // Movies and series are the same title only if they are also the same year.
        if kind.has_year() {
            out.push_str(&format!(" AND COALESCE({a}.year,0) = COALESCE({b}.year,0)"));
        }
        // A channel's country, because the prefix that carried it is not in the key.
        //
        // `match_key` is built from the name after `split_country_prefix` has taken the
        // prefix off -- which is what makes `UK | BBC One` and `BBC One` one channel, and
        // is wanted. The consequence not written down anywhere is that `US *  TNT` and
        // `CA *  TNT` both key to `tnt`, so one of them disappeared when "hide
        // duplicates" was on: two different regional channels, collapsed because the only
        // thing distinguishing them had already been removed.
        //
        // The country survived that strip -- `channels.country` is written by the same
        // split -- so it is what tells the two cases apart. Two feeds of one brand from
        // one country still collapse, which is the case this feature is for. `IS` rather
        // than `=` so that two untagged channels compare equal instead of both being kept
        // (R-08).
        if matches!(kind, Kind::Live) {
            out.push_str(&format!(" AND {a}.country IS {b}.country"));
        }
        // The same language rule as the outer query: a Spanish copy must not suppress the
        // English one when English-only is on.
        if self.english_only {
            out.push_str(&self.language_clause(a));
        }
        out
    }

    /// The `seasons` column for a series query: every season any copy of the show has.
    ///
    /// The SQL half of `group_of`, for the one place that cannot call it — a column in a
    /// list query, evaluated per row. Same predicate, so the number on the card and the
    /// episodes behind it are the same answer.
    ///
    /// With collapsing off this is the plain question, because then every copy is its own
    /// card and each should count only what it holds.
    pub fn merged_seasons_sql(&self) -> String {
        if !self.hide_duplicates {
            return "(SELECT group_concat(DISTINCT e.season)
                       FROM episodes e WHERE e.series_id = series.id)"
                .to_string();
        }
        format!(
            "(SELECT group_concat(DISTINCT e.season)
                FROM episodes e JOIN series sib ON sib.id = e.series_id
               WHERE {})",
            self.same_title_sql(Kind::Series, "sib", "series"),
        )
    }

    /// Every row the one visible row stands for, best copy first.
    ///
    /// `where_sql` answers "does a better copy of this exist" and so keeps exactly one
    /// row per group. This answers the other half of the same question — *which* rows
    /// that one is standing for — so a caller can merge what they hold instead of only
    /// showing the winner. The first id returned is the row `where_sql` keeps, because
    /// the ordering here is that predicate read forwards: better quality first, and the
    /// lower id among equals.
    ///
    /// Collapsing turned off means a group of one. Nothing is hidden then, so nothing is
    /// being stood for, and merging rows the viewer can already see separately would show
    /// each of them the others' contents.
    ///
    /// Not [`alternates`], which asks a neighbouring question for the source picker: it
    /// lists every copy of a title whatever its language or country and whatever the
    /// filters say, because a viewer who opened the picker wants the whole choice. This
    /// one is bounded by the filter in force, because it decides what a *collapsed* row
    /// stands for, and a row the viewer cannot see must not contribute to one they can.
    ///
    /// This exists because hiding was losing content rather than just losing a choice.
    /// A panel's 4K copy of a show and its ordinary copy are one title with one year, so
    /// the 4K row wins and the other disappears — but the two listings are fetched from
    /// the provider separately and need not agree. Measured against the subscription in
    /// docs/ROADMAP.md: 859 of 7,435 series groups have more than one copy, and in a
    /// sample of fourteen, one (`Landman`) had a second season that existed *only* in the
    /// copy being hidden. The season was unreachable in the app and present on the panel.
    pub fn group_of(&self, conn: &Connection, kind: Kind, id: i64) -> Result<Vec<i64>> {
        if !self.hide_duplicates {
            return Ok(vec![id]);
        }
        let t = kind.table();
        let sql = format!(
            "SELECT g.id FROM {t} g JOIN {t} me ON me.id = ?1
              WHERE {}
              ORDER BY g.quality_rank DESC, g.id",
            self.same_title_sql(kind, "g", "me"),
        );
        let mut stmt = conn.prepare(&sql)?;
        let ids = stmt
            .query_map(params![id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<i64>, _>>()?;
        // A row the filter itself excludes -- a Spanish copy under English-only -- is not
        // in its own result: the language rule is applied to the candidates, so the only
        // rows that come back are ones the viewer could have been shown, and the asked-for
        // row is not one of them. It is still the row the caller asked about, reached by a
        // search or a deep link, and it is standing for nothing. Answering with the group
        // it is *not* part of would show it somebody else's episodes; answering with
        // nothing would show it none of its own.
        Ok(if ids.contains(&id) { ids } else { vec![id] })
    }

    /// The language condition for one table alias, in whichever reading is in force.
    ///
    /// One function because the duplicate subquery has to ask exactly the same question
    /// as the outer one: a copy the viewer cannot see must not be allowed to suppress
    /// the copy they can.
    fn language_clause(&self, alias: &str) -> String {
        if self.hide_untagged {
            format!(" AND {alias}.lang_code = 'en'")
        } else {
            format!(" AND ({alias}.lang_code IS NULL OR {alias}.lang_code = 'en')")
        }
    }
}

/// How many rows each filter is responsible for hiding, so the settings screen can say
/// what turning one on would cost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterCounts {
    pub total: i64,
    pub non_english: i64,
    pub untagged: i64,
    pub duplicates: i64,
}

pub fn counts(conn: &Connection, kind: Kind) -> Result<FilterCounts> {
    let t = kind.table();
    let total: i64 = conn.query_row(
        &format!("SELECT count(*) FROM {t} WHERE hidden = 0"),
        [],
        |r| r.get(0),
    )?;
    let non_english: i64 = conn.query_row(
        &format!("SELECT count(*) FROM {t} WHERE hidden = 0 AND lang_code IS NOT NULL AND lang_code != 'en'"),
        [],
        |r| r.get(0),
    )?;
    let untagged: i64 = conn.query_row(
        &format!("SELECT count(*) FROM {t} WHERE hidden = 0 AND lang_code IS NULL"),
        [],
        |r| r.get(0),
    )?;

    let only_dupes = LibraryFilter {
        english_only: false,
        hide_duplicates: true,
        hide_untagged: false,
    };
    let kept: i64 = conn.query_row(
        &format!(
            "SELECT count(*) FROM {t} WHERE hidden = 0{}",
            only_dupes.where_sql(kind)
        ),
        [],
        |r| r.get(0),
    )?;

    Ok(FilterCounts {
        total,
        non_english,
        untagged,
        duplicates: total - kept,
    })
}

/// The other copies of one title, best first — the source picker behind a collapsed
/// entry (README §13, "one card with a source picker").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alternate {
    pub id: i64,
    pub name: String,
    pub quality: Option<String>,
    pub provider: Option<String>,
}

pub fn alternates(conn: &Connection, kind: Kind, id: i64) -> Result<Vec<Alternate>> {
    let t = kind.table();
    let name_col = if matches!(kind, Kind::Live) {
        "COALESCE(a.custom_name, a.name)"
    } else {
        "COALESCE(a.custom_title, a.title)"
    };
    let year = if kind.has_year() {
        " AND COALESCE(a.year,0) = COALESCE(me.year,0)"
    } else {
        ""
    };
    let sql = format!(
        "SELECT a.id, {name_col}, a.quality, p.name
         FROM {t} a
         JOIN {t} me ON me.id = ?1
         LEFT JOIN providers p ON p.id = a.provider_id
         WHERE a.match_key = me.match_key{year} AND a.hidden = 0
         ORDER BY a.quality_rank DESC, a.id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![id], |r| {
            Ok(Alternate {
                id: r.get(0)?,
                name: r.get(1)?,
                quality: r.get(2)?,
                provider: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Recompute `lang_code` and `quality_rank` for everything.
///
/// **This is the only thing that sets them.** This comment used to say that ingestion
/// classifies as it writes and that this existed for the two cases ingestion could not
/// cover. It does not: neither `library::upsert_movies` nor `channels::upsert_batch`
/// mentions `lang_code` or `quality_rank` in its insert, so both columns are set here
/// and nowhere else -- by the call `sync.rs` makes at the end of every import. It reads
/// the same fields a fresh import would.
///
/// That is correct and necessary, and it means the import has an ordering dependency
/// worth knowing about rather than denying (R-55):
///
/// * An import interrupted between the upserts and this call leaves every new row with
///   `lang_code = NULL` and `quality_rank = 0`, which the library filter reads as
///   "untagged" and "worst copy" -- so "English only" hides them and "hide duplicates"
///   prefers anything else. The next completed refresh fixes it.
/// * `reclassify_if_stale`, the sibling that skips the work when the classifier version
///   has not changed, would therefore be the *wrong* call at the end of an import: a
///   fresh row has nothing to be stale against. Its only caller is
///   `aurora-ingest/examples/import.rs`, which is right for what that does.
pub fn reclassify(conn: &mut Connection) -> Result<usize> {
    let mut done = 0usize;
    done += reclassify_channels(conn)?;
    done += reclassify_vod(conn, Kind::Movies)?;
    done += reclassify_vod(conn, Kind::Series)?;
    settings::set(conn, KEY_CLASSIFIER_VERSION, &CLASSIFIER_VERSION)?;
    Ok(done)
}

/// Run `reclassify` only if this build classifies differently from whatever last did.
pub fn reclassify_if_stale(conn: &mut Connection) -> Result<usize> {
    let seen: i64 = settings::get_or(conn, KEY_CLASSIFIER_VERSION, 0)?;
    if seen == CLASSIFIER_VERSION {
        return Ok(0);
    }
    reclassify(conn)
}

/// What classification reads off a channel: id, name, group, reported language, quality.
type ChannelFacts = (i64, String, Option<String>, Option<String>, Option<String>);

fn reclassify_channels(conn: &mut Connection) -> Result<usize> {
    let rows: Vec<ChannelFacts> = {
        let mut stmt =
            conn.prepare("SELECT id, name, group_title, language, quality FROM channels")?;
        let mapped = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        mapped
    };

    let tx = conn.transaction()?;
    {
        let mut update = tx.prepare(
            "UPDATE channels SET lang_code = ?2, quality_rank = ?3, match_key = ?4
             WHERE id = ?1",
        )?;
        for (id, name, group, language, quality) in &rows {
            let code = lang::detect(name, group.as_deref(), language.as_deref());
            // A channel that never advertised a quality may still spell it in its name.
            let quality = quality
                .clone()
                .or_else(|| title::detect_quality(name))
                .or_else(|| group.as_deref().and_then(title::detect_quality));
            // See `reclassify_vod`: the key is recomputed because what derives it changed.
            update.execute(params![
                id,
                code,
                title::quality_rank(quality.as_deref()),
                title::match_key(name),
            ])?;
        }
    }
    tx.commit()?;
    Ok(rows.len())
}

/// The same, for a film or a show: id, title, group, quality.
type VodFacts = (i64, String, Option<String>, Option<String>);

fn reclassify_vod(conn: &mut Connection, kind: Kind) -> Result<usize> {
    let t = kind.table();
    let rows: Vec<VodFacts> = {
        let mut stmt = conn.prepare(&format!("SELECT id, title, group_title, quality FROM {t}"))?;
        let mapped = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        mapped
    };

    let tx = conn.transaction()?;
    {
        let mut update = tx.prepare(&format!(
            "UPDATE {t} SET lang_code = ?2, quality = ?3, quality_rank = ?4, match_key = ?5
             WHERE id = ?1"
        ))?;
        for (id, name, group, quality) in &rows {
            let code = lang::detect(name, group.as_deref(), None);
            // A category like "VOD | 4K" is the provider's last word on quality when
            // neither the field nor the title says.
            let quality = quality
                .clone()
                .or_else(|| title::detect_quality(name))
                .or_else(|| group.as_deref().and_then(title::detect_quality));
            // Recomputed, not left alone. The key is derived from the title, and what
            // the derivation reads has changed: a bare language tag now comes off it, so
            // a library written by an older build holds keys that no longer group the way
            // this build would group them. Leaving them would mean the duplicate fix
            // reached only what is imported from now on.
            update.execute(params![
                id,
                code,
                quality,
                title::quality_rank(quality.as_deref()),
                title::match_key(name),
            ])?;
        }
    }
    tx.commit()?;
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The duplicate that was showing twice, and the reason a re-key is needed.
    ///
    /// An older build derived the key without taking a bare language tag off the title,
    /// so the two ways this panel writes one film ended up under different keys and
    /// duplicate collapsing never saw them as the same thing. Fixing the derivation is
    /// only half of it: the keys are *stored*, so a library that already exists keeps the
    /// old ones until something recomputes them.
    #[test]
    fn reclassify_regroups_copies_an_older_build_keyed_differently() {
        let mut conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();

        // Written the way a build before this one wrote them: the starred form lost its
        // code, the spaced form kept it.
        for (key, title, quality) in [
            ("m1", "EN \u{2605}My Penguin Friend", "HD"),
            ("m2", "EN My Penguin Friend", "FHD"),
        ] {
            conn.execute(
                "INSERT INTO movies (provider_id, provider_key, title, match_key, year,
                                     quality, url, added_at, last_seen_at)
                 VALUES (1,?1,?2,?3,2024,?4,'https://example.com/m.mkv',0,0)",
                params![key, title, old_style_key(title), quality],
            )
            .unwrap();
        }

        let keys: Vec<String> = conn
            .prepare("SELECT match_key FROM movies ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_ne!(keys[0], keys[1], "the fixture is not reproducing the bug");

        reclassify(&mut conn).unwrap();

        let keys: Vec<String> = conn
            .prepare("SELECT match_key FROM movies ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(keys[0], keys[1], "reclassify left the old keys in place");

        // And the filter now shows one card rather than two.
        let filter = LibraryFilter {
            hide_duplicates: true,
            ..Default::default()
        };
        let shown: i64 = conn
            .query_row(
                &format!(
                    "SELECT count(*) FROM movies WHERE movies.hidden = 0{}",
                    filter.where_sql(Kind::Movies)
                ),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(shown, 1, "both copies are still on screen");
    }

    /// How the key was derived before a bare tag was read: the separated form only.
    fn old_style_key(name: &str) -> String {
        let (base, _) = aurora_core::title::split_country_prefix(name);
        base.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    }

    /// Enough films, sharing enough match keys, that which index the planner picks for
    /// the duplicate subquery actually matters.
    fn many_films(conn: &Connection, count: i64) {
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'Provider One','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        let mut stmt = conn
            .prepare(
                "INSERT INTO movies (provider_id, provider_key, title, match_key, year,
                                     quality, quality_rank, lang_code, url, last_seen_at,
                                     added_at)
                 VALUES (1, ?1, ?2, ?3, 2020, 'HD', ?4, 'en', 'http://example.com', 0, ?5)",
            )
            .unwrap();
        for i in 0..count {
            // Three copies of each title, so the duplicate clause has real work to do.
            let key = format!("film{}", i / 3);
            stmt.execute(params![format!("m{i}"), key.clone(), key, i % 3, i])
                .unwrap();
        }
    }

    /// Opening a library analyses it, so an installation that already exists is fixed
    /// by launching rather than by re-importing.
    ///
    /// This guards the *call*, which is the thing at risk; why it matters is on
    /// `aurora_db::analyze`, with the measurements. Deliberately not a query-plan
    /// assertion — the mis-plan needs a real library to reproduce, and a synthetic table
    /// picks the right index with or without statistics, so such a test would have
    /// passed before this fix as well as after it.
    #[test]
    fn opening_a_library_analyses_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");

        let conn = crate::open(&path).unwrap();
        many_films(&conn, 60);
        drop(conn);

        // Reopened, because the first open created the file and this is the path every
        // launch after it takes.
        let conn = crate::open(&path).unwrap();
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_stat1", [], |r| r.get(0))
            .expect("sqlite_stat1 should exist once ANALYZE has run");
        assert!(rows > 0, "the library was opened and never analysed");
    }

    /// A library with the shapes that matter: an untagged English channel, tagged
    /// foreign ones, and the same title at three qualities.
    fn seeded() -> Connection {
        let mut conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'Provider One','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        let channels = [
            // (name, group, quality)
            ("CNN", None, Some("HD")),
            ("CNN", None, Some("FHD")),
            ("CNN", None, Some("4K")),
            ("FR | TF1", Some("FRANCE"), Some("HD")),
            ("AR | MBC 1", Some("ARABIC"), None),
            ("UK | BBC One", Some("UK"), Some("FHD")),
            ("Discovery", None, None),
        ];
        for (i, (name, group, quality)) in channels.iter().enumerate() {
            conn.execute(
                "INSERT INTO channels (provider_id, provider_key, name, match_key,
                                       group_title, quality, last_seen_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, 0)",
                params![
                    format!("c{i}"),
                    name,
                    aurora_core::title::match_key(name),
                    group,
                    quality
                ],
            )
            .unwrap();
        }
        let movies = [
            ("The Matrix", Some(1999), Some("HD")),
            ("The Matrix", Some(1999), Some("4K")),
            ("The Matrix", Some(2021), Some("HD")),
            ("Amelie (FRENCH)", Some(2001), Some("FHD")),
        ];
        for (i, (title, year, quality)) in movies.iter().enumerate() {
            conn.execute(
                "INSERT INTO movies (provider_id, provider_key, title, match_key, year,
                                     quality, url, last_seen_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, 'http://x', 0)",
                params![
                    format!("m{i}"),
                    title,
                    aurora_core::title::match_key(title),
                    year,
                    quality
                ],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO series (provider_id, provider_key, title, match_key, last_seen_at)
             VALUES (1,'s0','Breaking Bad','breakingbad',0),
                    (1,'s1','[SPANISH] Breaking Bad','breakingbad',0)",
            [],
        )
        .unwrap();
        reclassify(&mut conn).unwrap();
        conn
    }

    fn ids(conn: &Connection, kind: Kind, f: &LibraryFilter) -> Vec<i64> {
        let sql = format!(
            "SELECT {t}.id FROM {t} WHERE {t}.hidden = 0{} ORDER BY {t}.id",
            f.where_sql(kind),
            t = kind.table()
        );
        let mut stmt = conn.prepare(&sql).unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<i64>, _>>()
            .unwrap()
    }

    fn names(conn: &Connection, kind: Kind, f: &LibraryFilter) -> Vec<String> {
        let col = if matches!(kind, Kind::Live) {
            "name"
        } else {
            "title"
        };
        let sql = format!(
            "SELECT {t}.{col} FROM {t} WHERE {t}.hidden = 0{} ORDER BY {t}.id",
            f.where_sql(kind),
            t = kind.table()
        );
        let mut stmt = conn.prepare(&sql).unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<String>, _>>()
            .unwrap()
    }

    const OFF: LibraryFilter = LibraryFilter {
        english_only: false,
        hide_duplicates: false,
        hide_untagged: false,
    };
    const ENGLISH: LibraryFilter = LibraryFilter {
        english_only: true,
        hide_duplicates: false,
        hide_untagged: false,
    };
    const DUPES: LibraryFilter = LibraryFilter {
        english_only: false,
        hide_duplicates: true,
        hide_untagged: false,
    };
    const BOTH: LibraryFilter = LibraryFilter {
        english_only: true,
        hide_duplicates: true,
        hide_untagged: false,
    };
    /// What a viewer means by "only English": nothing else, and nothing unidentified.
    const STRICT: LibraryFilter = LibraryFilter {
        english_only: true,
        hide_duplicates: false,
        hide_untagged: true,
    };

    #[test]
    fn with_no_filters_nothing_is_hidden() {
        let conn = seeded();
        assert_eq!(ids(&conn, Kind::Live, &OFF).len(), 7);
        assert_eq!(ids(&conn, Kind::Movies, &OFF).len(), 4);
        assert_eq!(ids(&conn, Kind::Series, &OFF).len(), 2);
        assert!(OFF.where_sql(Kind::Live).is_empty());
    }

    /// Two regional feeds of one brand survive "hide duplicates".
    ///
    /// `match_key` is built after `split_country_prefix` has taken the prefix off, which
    /// is what makes `UK | BBC One` and `BBC One` one channel. The consequence was that
    /// `US TNT` and `CA TNT` both key to `tnt`, so one of them vanished -- two different
    /// channels collapsed because the only thing telling them apart had been stripped
    /// before the key was built (R-08).
    #[test]
    fn two_countries_feeds_are_not_duplicates_of_each_other() {
        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','http://example.invalid',0)",
            [],
        )
        .unwrap();
        // Two regional feeds, and two copies of one of them at different qualities.
        for (id, name, country, rank) in [
            (1, "TNT", Some("US"), 2),
            (2, "TNT", Some("CA"), 2),
            (3, "TNT", Some("US"), 1),
            // And one with no country at all, twice, which must still collapse.
            (4, "Discovery", None, 2),
            (5, "Discovery", None, 1),
        ] {
            conn.execute(
                "INSERT INTO channels (id, provider_id, provider_key, name, match_key,
                                       country, quality_rank, lang_code, hidden,
                                       last_seen_at)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, 'en', 0, 0)",
                params![
                    id,
                    format!("c{id}"),
                    name,
                    name.to_lowercase(),
                    country,
                    rank
                ],
            )
            .unwrap();
        }

        let dedup = LibraryFilter {
            hide_duplicates: true,
            ..Default::default()
        };
        let kept = ids(&conn, Kind::Live, &dedup);

        assert!(kept.contains(&1), "the US feed was hidden: {kept:?}");
        assert!(kept.contains(&2), "the CA feed was hidden: {kept:?}");
        // The lesser copy of the US feed is still a duplicate, which is the feature.
        assert!(
            !kept.contains(&3),
            "a worse copy of the US feed survived: {kept:?}"
        );
        // And two untagged copies of one channel still collapse to the better one.
        assert!(kept.contains(&4), "{kept:?}");
        assert!(!kept.contains(&5), "{kept:?}");
    }

    #[test]
    fn english_only_drops_what_is_tagged_and_keeps_what_is_not() {
        let conn = seeded();
        let kept = names(&conn, Kind::Live, &ENGLISH);
        // Untagged names stay: guessing here would empty most real libraries.
        assert!(kept.contains(&"CNN".to_string()));
        assert!(kept.contains(&"Discovery".to_string()));
        assert!(kept.contains(&"UK | BBC One".to_string()));
        assert!(!kept.contains(&"FR | TF1".to_string()));
        assert!(!kept.contains(&"AR | MBC 1".to_string()));
    }

    #[test]
    fn the_strict_reading_hides_what_could_not_be_identified() {
        let conn = seeded();
        let kept = names(&conn, Kind::Live, &STRICT);
        // What `ENGLISH` keeps out of caution, this drops on purpose.
        assert!(!kept.contains(&"CNN".to_string()), "{kept:?}");
        assert!(!kept.contains(&"Discovery".to_string()), "{kept:?}");
        // And what is positively English still stays.
        assert!(kept.contains(&"UK | BBC One".to_string()), "{kept:?}");
        assert!(!kept.contains(&"FR | TF1".to_string()));
    }

    #[test]
    fn strict_and_lenient_are_the_same_question_in_the_duplicate_subquery() {
        // A copy the viewer cannot see must not suppress the copy they can — which only
        // holds if the inner search asks whatever the outer one asked.
        let strict = STRICT.where_sql(Kind::Movies);
        assert!(!strict.contains("lang_code IS NULL"), "{strict}");
        let lenient = ENGLISH.where_sql(Kind::Movies);
        assert!(lenient.contains("lang_code IS NULL"), "{lenient}");

        let both_strict = LibraryFilter {
            english_only: true,
            hide_duplicates: true,
            hide_untagged: true,
        }
        .where_sql(Kind::Movies);
        assert_eq!(
            both_strict.matches("lang_code = 'en'").count(),
            2,
            "the outer query and the duplicate subquery must agree: {both_strict}"
        );
    }

    #[test]
    fn english_only_applies_to_movies_and_series_too() {
        let conn = seeded();
        let movies = names(&conn, Kind::Movies, &ENGLISH);
        assert!(!movies.iter().any(|t| t.contains("Amelie")), "{movies:?}");
        assert_eq!(movies.len(), 3);

        let series = names(&conn, Kind::Series, &ENGLISH);
        assert_eq!(series, vec!["Breaking Bad".to_string()]);
    }

    /// The group is the collapser's own answer, read forwards.
    ///
    /// This is the property the merge depends on: whatever `where_sql` leaves standing
    /// has to be the first id `group_of` returns. If the two ever disagree the viewer
    /// sees one copy's title with another copy's episodes.
    #[test]
    fn the_surviving_row_is_the_first_of_its_group() {
        let conn = seeded();
        for kind in [Kind::Live, Kind::Movies, Kind::Series] {
            for id in ids(&conn, kind, &DUPES) {
                let group = DUPES.group_of(&conn, kind, id).unwrap();
                assert_eq!(
                    group.first(),
                    Some(&id),
                    "{kind:?} {id} survives the filter but is not its own group's best copy:                      {group:?}"
                );
            }
        }
    }

    /// And every hidden row names the survivor as its own best copy, so a merge started
    /// from either row reaches the same group.
    #[test]
    fn a_hidden_row_belongs_to_the_group_of_the_row_that_replaced_it() {
        let conn = seeded();
        let visible = ids(&conn, Kind::Live, &DUPES);
        let all = ids(&conn, Kind::Live, &LibraryFilter::default());
        let hidden: Vec<i64> = all
            .iter()
            .copied()
            .filter(|i| !visible.contains(i))
            .collect();
        assert!(
            !hidden.is_empty(),
            "the fixture has no collapsed duplicates"
        );
        for id in hidden {
            let group = DUPES.group_of(&conn, Kind::Live, id).unwrap();
            assert!(
                group.contains(&id),
                "{id} is missing from its own group: {group:?}"
            );
            assert!(
                visible.contains(&group[0]),
                "hidden channel {id} points at {} , which is not what the grid shows: {group:?}",
                group[0]
            );
        }
    }

    /// With collapsing off, a group is one row — because nothing is standing for anything.
    #[test]
    fn nothing_is_merged_when_nothing_is_hidden() {
        let conn = seeded();
        for id in ids(&conn, Kind::Series, &LibraryFilter::default()) {
            assert_eq!(
                LibraryFilter::default()
                    .group_of(&conn, Kind::Series, id)
                    .unwrap(),
                vec![id],
            );
        }
    }

    /// The case this was built for, in the shape the panel actually has it.
    ///
    /// `EN ★ Landman [4K]` and `EN ★ Landman` are one title with one year, so the 4K row
    /// wins and the other is hidden — and the hidden one was the only copy carrying
    /// season 2. The group has to name both rows, best first, so the caller can union
    /// their episodes.
    #[test]
    fn a_shows_duplicate_copies_are_both_in_its_group() {
        let mut conn = seeded();
        conn.execute(
            "INSERT INTO series (id, provider_id, provider_key, title, match_key, year,
                                 quality, last_seen_at)
             VALUES (90,'1','s90','Landman','landman',2024,NULL,0),
                    (91,'1','s91','Landman [4K]','landman',2024,'4K',0)",
            [],
        )
        .unwrap();
        reclassify(&mut conn).unwrap();

        // The grid shows one of them, and it is the 4K copy.
        let shown = ids(&conn, Kind::Series, &DUPES);
        assert!(
            shown.contains(&91),
            "the 4K copy should be the one shown: {shown:?}"
        );
        assert!(
            !shown.contains(&90),
            "both copies are in the grid: {shown:?}"
        );

        // And the copy it hid is reachable from it, which is what makes the merge
        // possible: season 2 lives only in row 90.
        assert_eq!(
            DUPES.group_of(&conn, Kind::Series, 91).unwrap(),
            vec![91, 90]
        );
    }

    /// A copy the filter excludes still answers for itself.
    ///
    /// Asked about a Spanish row under English-only, the group cannot be empty: the
    /// caller is about to list that row's episodes, and an empty group would turn
    /// "merge the siblings in" into "show nothing".
    #[test]
    fn a_row_the_filter_excludes_is_still_its_own_group() {
        let conn = seeded();
        let spanish: i64 = conn
            .query_row(
                "SELECT id FROM series WHERE title = '[SPANISH] Breaking Bad'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let group = BOTH.group_of(&conn, Kind::Series, spanish).unwrap();
        assert!(
            group.contains(&spanish),
            "an excluded row answered with somebody else's group: {group:?}"
        );
    }

    #[test]
    fn collapsing_duplicates_keeps_the_best_copy() {
        let conn = seeded();
        let kept = ids(&conn, Kind::Live, &DUPES);
        // Three CNNs collapse to one, and it is the 4K one.
        let quality: String = conn
            .query_row(
                "SELECT quality FROM channels WHERE name = 'CNN' AND id IN
                   (SELECT id FROM channels WHERE id = ?1)",
                params![kept[0]],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(quality, "4K");
        assert_eq!(kept.len(), 5, "7 channels, 3 of them the same one");
    }

    #[test]
    fn a_year_apart_is_not_a_duplicate() {
        let conn = seeded();
        // Two 1999 Matrixes collapse; the 2021 one is a different film.
        let kept = ids(&conn, Kind::Movies, &DUPES);
        let years: Vec<i64> = kept
            .iter()
            .map(|id| {
                conn.query_row("SELECT year FROM movies WHERE id = ?1", params![id], |r| {
                    r.get(0)
                })
                .unwrap()
            })
            .collect();
        assert_eq!(years, vec![1999, 2021, 2001]);
    }

    #[test]
    fn a_foreign_copy_never_suppresses_the_english_one() {
        let mut conn = seeded();
        // A 4K Spanish copy of a film we also have in HD English. With both filters on,
        // the English HD copy must survive: the better copy is not an eligible one.
        conn.execute(
            "INSERT INTO movies (provider_id, provider_key, title, match_key, year,
                                 quality, url, last_seen_at)
             VALUES (1,'m9','[SPANISH] The Matrix','thematrix',1999,'4K','http://x',0)",
            [],
        )
        .unwrap();
        reclassify(&mut conn).unwrap();

        let kept = names(&conn, Kind::Movies, &BOTH);
        assert!(
            kept.iter().any(|t| t == "The Matrix"),
            "the English copy was suppressed by a Spanish one: {kept:?}"
        );
        assert!(!kept.iter().any(|t| t.contains("SPANISH")), "{kept:?}");
    }

    #[test]
    fn a_hidden_copy_does_not_suppress_a_visible_one() {
        let conn = seeded();
        // Hide the 4K CNN by hand; the FHD one should become the surviving copy rather
        // than everything disappearing behind a row nobody can see.
        conn.execute(
            "UPDATE channels SET hidden = 1 WHERE name = 'CNN' AND quality = '4K'",
            [],
        )
        .unwrap();
        let kept = ids(&conn, Kind::Live, &DUPES);
        let qualities: Vec<String> = kept
            .iter()
            .filter_map(|id| {
                conn.query_row(
                    "SELECT quality FROM channels WHERE id = ?1 AND name = 'CNN'",
                    params![id],
                    |r| r.get(0),
                )
                .ok()
            })
            .collect();
        assert_eq!(qualities, vec!["FHD".to_string()]);
    }

    #[test]
    fn two_copies_of_equal_quality_collapse_to_a_stable_one() {
        let mut conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        for i in 0..3 {
            conn.execute(
                "INSERT INTO channels (provider_id, provider_key, name, match_key, last_seen_at)
                 VALUES (1, ?1, 'Same Channel', 'samechannel', 0)",
                params![format!("c{i}")],
            )
            .unwrap();
        }
        reclassify(&mut conn).unwrap();
        let kept = ids(&conn, Kind::Live, &DUPES);
        assert_eq!(kept.len(), 1);
        // The same one every time, not whichever the planner happened to visit first.
        assert_eq!(kept, ids(&conn, Kind::Live, &DUPES));
        assert_eq!(kept[0], 1);
    }

    #[test]
    fn alternates_list_every_copy_best_first() {
        let conn = seeded();
        let best = ids(&conn, Kind::Live, &DUPES)[0];
        let alts = alternates(&conn, Kind::Live, best).unwrap();
        let qualities: Vec<Option<String>> = alts.iter().map(|a| a.quality.clone()).collect();
        assert_eq!(
            qualities,
            vec![
                Some("4K".to_string()),
                Some("FHD".to_string()),
                Some("HD".to_string())
            ]
        );
        assert_eq!(alts[0].provider.as_deref(), Some("Provider One"));
    }

    #[test]
    fn counts_say_what_each_filter_would_cost() {
        let conn = seeded();
        let c = counts(&conn, Kind::Live).unwrap();
        assert_eq!(c.total, 7);
        assert_eq!(c.non_english, 2, "TF1 and MBC");
        assert_eq!(c.duplicates, 2, "two of the three CNNs");
        assert_eq!(
            c.untagged + c.non_english + 1,
            c.total,
            "BBC One is tagged en"
        );
    }

    #[test]
    fn reclassify_fills_in_what_ingestion_would_have() {
        let mut conn = seeded();
        conn.execute("UPDATE channels SET lang_code = NULL, quality_rank = 0", [])
            .unwrap();
        assert_eq!(counts(&conn, Kind::Live).unwrap().non_english, 0);

        reclassify(&mut conn).unwrap();
        assert_eq!(counts(&conn, Kind::Live).unwrap().non_english, 2);
        let rank: i64 = conn
            .query_row(
                "SELECT quality_rank FROM channels WHERE name='CNN' AND quality='4K'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rank, 4);
    }

    #[test]
    fn reclassify_reads_a_quality_out_of_the_name_when_the_field_is_empty() {
        let mut conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO movies (provider_id, provider_key, title, match_key, url, last_seen_at)
             VALUES (1,'m0','Dune 2021 2160p','dune',   'http://x',0)",
            [],
        )
        .unwrap();
        reclassify(&mut conn).unwrap();
        let (quality, rank): (Option<String>, i64) = conn
            .query_row("SELECT quality, quality_rank FROM movies", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(quality.as_deref(), Some("4K"));
        assert_eq!(rank, 4);
    }

    #[test]
    fn reclassify_runs_once_per_classifier_version() {
        let mut conn = seeded();
        assert_eq!(
            reclassify_if_stale(&mut conn).unwrap(),
            0,
            "already current"
        );
        settings::set(&conn, "filter.classifier_version", &0i64).unwrap();
        assert!(reclassify_if_stale(&mut conn).unwrap() > 0);
    }

    #[test]
    fn settings_round_trip_and_default_to_off() {
        let conn = crate::open_memory().unwrap();
        assert_eq!(
            LibraryFilter::load(&conn).unwrap(),
            LibraryFilter::default()
        );
        let f = LibraryFilter {
            english_only: true,
            hide_duplicates: true,
            hide_untagged: false,
        };
        f.save(&conn).unwrap();
        assert_eq!(LibraryFilter::load(&conn).unwrap(), f);
    }

    #[test]
    fn a_kind_comes_only_from_a_known_word() {
        assert_eq!(Kind::parse("live"), Some(Kind::Live));
        assert_eq!(Kind::parse("movies"), Some(Kind::Movies));
        assert_eq!(Kind::parse("series"), Some(Kind::Series));
        assert_eq!(Kind::parse("channels; DROP TABLE movies"), None);
        assert_eq!(Kind::parse(""), None);
    }
}
