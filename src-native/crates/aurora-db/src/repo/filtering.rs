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
/// Bumped when classification changes, so an existing library is re-read rather than
/// silently keeping whatever the previous build decided.
pub const CLASSIFIER_VERSION: i64 = 1;
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
}

impl LibraryFilter {
    pub fn load(conn: &Connection) -> Result<Self> {
        Ok(Self {
            english_only: settings::get_or(conn, KEY_ENGLISH_ONLY, false)?,
            hide_duplicates: settings::get_or(conn, KEY_HIDE_DUPLICATES, false)?,
        })
    }

    pub fn save(&self, conn: &Connection) -> Result<()> {
        settings::set(conn, KEY_ENGLISH_ONLY, &self.english_only)?;
        settings::set(conn, KEY_HIDE_DUPLICATES, &self.hide_duplicates)?;
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
            out.push_str(&format!(
                " AND ({t}.lang_code IS NULL OR {t}.lang_code = 'en')"
            ));
        }
        if self.hide_duplicates {
            let year = if kind.has_year() {
                format!(" AND COALESCE(dup.year,0) = COALESCE({t}.year,0)")
            } else {
                String::new()
            };
            // The inner search carries the same language rule: a Spanish copy must not
            // suppress the English one when English-only is on.
            let lang = if self.english_only {
                " AND (dup.lang_code IS NULL OR dup.lang_code = 'en')"
            } else {
                ""
            };
            out.push_str(&format!(
                " AND NOT EXISTS (SELECT 1 FROM {t} dup
                     WHERE dup.match_key = {t}.match_key{year}{lang}
                       AND dup.hidden = 0
                       AND (dup.quality_rank > {t}.quality_rank
                            OR (dup.quality_rank = {t}.quality_rank AND dup.id < {t}.id)))"
            ));
        }
        out
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
/// Ingestion classifies as it writes, so this exists for the two cases ingestion cannot
/// cover: a library that predates the columns, and a build whose classifier has changed
/// its mind. It reads the same fields a fresh import would.
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
        let mut update =
            tx.prepare("UPDATE channels SET lang_code = ?2, quality_rank = ?3 WHERE id = ?1")?;
        for (id, name, group, language, quality) in &rows {
            let code = lang::detect(name, group.as_deref(), language.as_deref());
            // A channel that never advertised a quality may still spell it in its name.
            let quality = quality
                .clone()
                .or_else(|| title::detect_quality(name))
                .or_else(|| group.as_deref().and_then(title::detect_quality));
            update.execute(params![id, code, title::quality_rank(quality.as_deref())])?;
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
            "UPDATE {t} SET lang_code = ?2, quality = ?3, quality_rank = ?4 WHERE id = ?1"
        ))?;
        for (id, name, group, quality) in &rows {
            let code = lang::detect(name, group.as_deref(), None);
            // A category like "VOD | 4K" is the provider's last word on quality when
            // neither the field nor the title says.
            let quality = quality
                .clone()
                .or_else(|| title::detect_quality(name))
                .or_else(|| group.as_deref().and_then(title::detect_quality));
            update.execute(params![
                id,
                code,
                quality,
                title::quality_rank(quality.as_deref())
            ])?;
        }
    }
    tx.commit()?;
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    };
    const ENGLISH: LibraryFilter = LibraryFilter {
        english_only: true,
        hide_duplicates: false,
    };
    const DUPES: LibraryFilter = LibraryFilter {
        english_only: false,
        hide_duplicates: true,
    };
    const BOTH: LibraryFilter = LibraryFilter {
        english_only: true,
        hide_duplicates: true,
    };

    #[test]
    fn with_no_filters_nothing_is_hidden() {
        let conn = seeded();
        assert_eq!(ids(&conn, Kind::Live, &OFF).len(), 7);
        assert_eq!(ids(&conn, Kind::Movies, &OFF).len(), 4);
        assert_eq!(ids(&conn, Kind::Series, &OFF).len(), 2);
        assert!(OFF.where_sql(Kind::Live).is_empty());
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
    fn english_only_applies_to_movies_and_series_too() {
        let conn = seeded();
        let movies = names(&conn, Kind::Movies, &ENGLISH);
        assert!(!movies.iter().any(|t| t.contains("Amelie")), "{movies:?}");
        assert_eq!(movies.len(), 3);

        let series = names(&conn, Kind::Series, &ENGLISH);
        assert_eq!(series, vec!["Breaking Bad".to_string()]);
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
