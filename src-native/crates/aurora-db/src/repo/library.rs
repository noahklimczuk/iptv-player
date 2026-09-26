//! Movies, series, and episodes.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovieRow {
    pub id: i64,
    pub title: String,
    pub year: Option<i32>,
    pub quality: Option<String>,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub overview: Option<String>,
    pub runtime_mins: Option<u32>,
    pub rating: Option<f32>,
    pub certification: Option<String>,
    pub genres: Vec<String>,
    pub cast: Vec<String>,
    pub added_at: Option<i64>,
    pub logo_art: Option<String>,
    /// ISO 639-1, or nothing when the title never said (README §7.3).
    pub lang: Option<String>,
    /// The trailer's YouTube key, when enrichment found one (migration 10).
    pub trailer_key: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NewMovie {
    pub provider_key: String,
    pub title: String,
    pub match_key: String,
    pub year: Option<i32>,
    pub quality: Option<String>,
    pub group: Option<String>,
    pub url: String,
    pub poster: Option<String>,
    pub added_at: Option<i64>,
    /// The provider's own score out of ten. Enrichment overwrites it when TMDB has
    /// an opinion; until then it is the only rating the library has.
    pub rating: Option<f32>,
}

pub fn upsert_movies(
    conn: &mut Connection,
    provider_id: i64,
    movies: &[NewMovie],
    now: i64,
) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut n = 0;
    {
        // Enrichment-owned columns (overview, backdrop, logo_art, tmdb_id, …) are not in the
        // UPDATE list: a playlist refresh must not wipe metadata fetched from TMDB.
        let mut stmt = tx.prepare(
            r#"INSERT INTO movies
               (provider_id, provider_key, title, match_key, year, quality, group_title,
                url, poster, added_at, rating, last_seen_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
               ON CONFLICT (provider_id, provider_key) DO UPDATE SET
                 title        = excluded.title,
                 match_key    = excluded.match_key,
                 year         = excluded.year,
                 quality      = excluded.quality,
                 group_title  = excluded.group_title,
                 url          = excluded.url,
                 poster       = COALESCE(excluded.poster, movies.poster),
                 -- The existing one wins: by the second refresh it may be TMDB's, and
                 -- a playlist must not undo enrichment. The panel only fills a gap.
                 rating       = COALESCE(movies.rating, excluded.rating),
                 last_seen_at = excluded.last_seen_at"#,
        )?;
        for m in movies {
            stmt.execute(params![
                provider_id,
                m.provider_key,
                m.title,
                m.match_key,
                m.year,
                m.quality,
                m.group,
                m.url,
                m.poster,
                m.added_at.unwrap_or(now),
                m.rating,
                now
            ])?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

/// The stream URL is deliberately absent. It carries the provider's username and
/// password in the path, and the renderer has no use for it: playback is resolved on
/// the host (README C10).
const MOVIE_SELECT: &str = "SELECT movies.id,
       -- What the viewer called it, then what it is actually called, then what the
       -- provider filed it under. See migration 9.
       COALESCE(custom_title, tmdb_title, title), year, quality,
                                   poster, backdrop, overview, runtime_mins, rating,
                                   genres, certification, added_at, logo_art, lang_code,
                                   (SELECT group_concat(p.name, char(31))
                                      FROM credits c JOIN people p ON p.tmdb_id = c.person_id
                                     WHERE c.item_kind = 'movie' AND c.item_id = movies.id
                                       AND c.is_cast = 1
                                     ORDER BY c.ord),
                                   trailer_key
                            FROM movies";

/// `group_concat` cannot be ordered portably, and a comma would split names that
/// contain one, so the separator is a unit separator no name has.
fn split_names(raw: Option<String>) -> Vec<String> {
    raw.map(|s| {
        s.split('\u{1f}')
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .collect()
    })
    .unwrap_or_default()
}

/// The display name, with the provider's language tag off the front.
///
/// Applied here, at the boundary where a row becomes something the UI will paint, rather
/// than in the SQL — the stored `title` has to keep the tag, because `reclassify` reads
/// it back to decide the language. See `aurora_core::lang::strip_language_prefix`.
fn shown(raw: String) -> String {
    aurora_core::lang::strip_language_prefix(&raw).unwrap_or(raw)
}

fn map_movie(r: &rusqlite::Row<'_>) -> rusqlite::Result<MovieRow> {
    let genres: Option<String> = r.get(9)?;
    Ok(MovieRow {
        id: r.get(0)?,
        title: shown(r.get(1)?),
        year: r.get::<_, Option<i64>>(2)?.map(|v| v as i32),
        quality: r.get(3)?,
        poster: r.get(4)?,
        backdrop: r.get(5)?,
        overview: r.get(6)?,
        runtime_mins: r.get::<_, Option<i64>>(7)?.map(|v| v as u32),
        rating: r.get::<_, Option<f64>>(8)?.map(|v| v as f32),
        genres: genres
            .and_then(|g| serde_json::from_str(&g).ok())
            .unwrap_or_default(),
        certification: r.get(10)?,
        added_at: r.get(11)?,
        logo_art: r.get(12)?,
        lang: r.get(13)?,
        cast: split_names(r.get(14)?),
        trailer_key: r.get(15)?,
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MovieSort {
    #[default]
    RecentlyAdded,
    Title,
    Year,
    Rating,
}

/// What a browse page asks for: a sort, a page, some optional filters, and whatever
/// the library-wide filters are set to (README §7.3, §8.5).
#[derive(Debug, Clone, Default)]
pub struct BrowseQuery {
    pub sort: MovieSort,
    pub genre: Option<String>,
    /// The provider's own shelf — `group_title`. On a library without a TMDB key this
    /// is the only structure there is: 202 categories on the subscription this was
    /// built against, and not one genre.
    pub category: Option<String>,
    /// Narrow by title. Substring rather than the FTS index on purpose: this filters
    /// a list somebody is already looking at, where the ranked cross-library search
    /// behind Ctrl-K is a different question with a different answer.
    pub query: Option<String>,
    /// Show only titles starting with this letter, or — for `#` — starting with anything
    /// that is not one.
    ///
    /// A filter rather than a scroll position, because the list is paged: 20,000 films
    /// sorted A–Z arrive 120 at a time, so "jump to W" cannot be answered on the client
    /// at all, and answering it by fetching every page up to W would read the whole
    /// library to display one screen of it.
    /// One character, or `#`. A `String` rather than a `char` because it is bound as a
    /// SQL parameter, and because the command layer takes it from JSON.
    pub letter: Option<String>,
    pub limit: u32,
    pub offset: u32,
    pub library: crate::repo::filtering::LibraryFilter,
}

/// The `WHERE` a browse query adds beyond `hidden = 0`, and the parameters it needs.
///
/// Built together because they have to agree: a clause with no parameter bound, or a
/// parameter with no clause, is a runtime error rather than a compile-time one.
fn browse_filters<'a>(
    table: &str,
    q: &'a BrowseQuery,
) -> (String, Vec<(&'static str, &'a dyn rusqlite::ToSql)>) {
    let mut sql = String::new();
    let mut params: Vec<(&'static str, &'a dyn rusqlite::ToSql)> = Vec::new();

    // Genres are stored as a JSON array, so matching one means matching its text. The
    // quotes make `"Action"` fail to match `"Action Comedy"`, which a bare LIKE would
    // not.
    if let Some(genre) = &q.genre {
        sql.push_str(&format!(
            " AND {table}.genres LIKE '%\"' || :genre || '\"%'"
        ));
        params.push((":genre", genre));
    }
    if let Some(category) = &q.category {
        sql.push_str(&format!(" AND {table}.group_title = :category"));
        params.push((":category", category));
    }
    if let Some(query) = &q.query {
        sql.push_str(&format!(
            " AND COALESCE({table}.custom_title, {table}.title) LIKE '%' || :query || '%'"
        ));
        params.push((":query", query));
    }
    // The letter is matched against the title as shown, which is the same expression the
    // A–Z sort orders by — otherwise the bar would filter on one name and the list would
    // be sorted by another, and "W" could land among the Vs.
    //
    // `#` means everything else: a provider's library is full of titles starting with a
    // digit, a bracket or a language tag, and they have to be reachable by something.
    // Compared with `upper()` rather than a `LIKE` prefix because `LIKE` on a column with
    // no index would be no faster and the intent is clearer.
    match q.letter.as_deref() {
        Some("#") => {
            let shown = format!("COALESCE({table}.custom_title, {table}.title)");
            sql.push_str(&format!(
                " AND (upper(substr({shown}, 1, 1)) < 'A' OR upper(substr({shown}, 1, 1)) > 'Z')"
            ));
        }
        Some(_) => {
            sql.push_str(&format!(
                " AND upper(substr(COALESCE({table}.custom_title, {table}.title), 1, 1)) \
                   = upper(:letter)"
            ));
            params.push((":letter", &q.letter));
        }
        None => {}
    }
    (sql, params)
}

/// Find a title by name, the way a recommendation arrives: as words, not an id.
///
/// `match_key` is what makes this work at all. A model answers "Am I Not Your Girl?" and
/// the library holds "FR ✪ AM I NOT YOUR GIRL 1992 FHD"; the key folds the country tag,
/// the quality suffix, the punctuation and the diacritics, so the two meet. It is the same
/// key duplicate collapsing already uses, which means a title with four copies resolves
/// once rather than four times.
///
/// The year narrows it where the caller has one, because remakes are exactly the case a
/// recommender trips over: suggesting the 1978 *Invasion of the Body Snatchers* and
/// playing the 2007 one is worse than not suggesting it. A year that matches nothing falls
/// back to the title alone rather than giving up — the model's year is often a year out,
/// and a near miss is better than a gap in the rail.
///
/// Respects the library filter and `hidden`, so a recommendation can never be something
/// the viewer has told this app not to show them.
pub fn find_by_title(
    conn: &Connection,
    kind: crate::repo::filtering::Kind,
    title: &str,
    year: Option<i32>,
    filter: &crate::repo::filtering::LibraryFilter,
) -> Result<Option<i64>> {
    let key = aurora_core::title::match_key(title);
    if key.is_empty() {
        return Ok(None);
    }
    let table = match kind {
        crate::repo::filtering::Kind::Movies => "movies",
        crate::repo::filtering::Kind::Series => "series",
        crate::repo::filtering::Kind::Live => return Ok(None),
    };
    let visible = filter.where_sql(kind);

    if let Some(year) = year {
        let sql = format!(
            "SELECT id FROM {table}
              WHERE match_key = ?1 AND hidden = 0 AND year IS NOT NULL
                AND abs(year - ?2) <= 1{visible}
              ORDER BY abs(year - ?2), id LIMIT 1"
        );
        if let Some(id) = conn
            .query_row(&sql, params![key, year], |r| r.get(0))
            .optional()?
        {
            return Ok(Some(id));
        }
    }

    let sql = format!(
        "SELECT id FROM {table} WHERE match_key = ?1 AND hidden = 0{visible} \
         ORDER BY id LIMIT 1"
    );
    Ok(conn
        .query_row(&sql, params![key], |r| r.get(0))
        .optional()?)
}

pub fn list_movies(conn: &Connection, q: &BrowseQuery) -> Result<Vec<MovieRow>> {
    let order = match q.sort {
        MovieSort::RecentlyAdded => "added_at DESC, movies.id DESC",
        MovieSort::Title => "title COLLATE NOCASE",
        MovieSort::Year => "year DESC NULLS LAST, title",
        MovieSort::Rating => "rating DESC NULLS LAST, title",
    };
    let (filters, mut params) = browse_filters("movies", q);
    let sql = format!(
        "{MOVIE_SELECT} WHERE movies.hidden = 0{}{filters} ORDER BY {order} \
         LIMIT :limit OFFSET :offset",
        q.library.where_sql(crate::repo::filtering::Kind::Movies),
    );
    params.push((":limit", &q.limit));
    params.push((":offset", &q.offset));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params.as_slice(), map_movie)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// How many rows a browse query matches, ignoring its page.
///
/// So the heading can say "117,508" rather than "120+". The count used to be whatever
/// had been fetched so far, which on the first page was the page size wearing a
/// library's clothes — and after that was a number that climbed while you scrolled.
pub fn count_movies(conn: &Connection, q: &BrowseQuery) -> Result<u32> {
    let (filters, params) = browse_filters("movies", q);
    let sql = format!(
        "SELECT count(*) FROM movies WHERE movies.hidden = 0{}{filters}",
        q.library.where_sql(crate::repo::filtering::Kind::Movies),
    );
    Ok(conn.query_row(&sql, params.as_slice(), |r| r.get(0))?)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesRow {
    pub id: i64,
    pub title: String,
    pub year: Option<i32>,
    pub quality: Option<String>,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub logo_art: Option<String>,
    pub overview: Option<String>,
    pub rating: Option<f32>,
    pub certification: Option<String>,
    pub genres: Vec<String>,
    pub cast: Vec<String>,
    pub seasons: Vec<u16>,
    pub added_at: Option<i64>,
    pub lang: Option<String>,
    /// The trailer's YouTube key, when enrichment found one (migration 10).
    pub trailer_key: Option<String>,
}

const SERIES_SELECT: &str = "SELECT series.id,
       COALESCE(custom_title, tmdb_title, title), year, quality,
                                    poster, backdrop, logo_art, overview, rating,
                                    certification, genres, added_at, lang_code,
                                    (SELECT group_concat(p.name, char(31))
                                       FROM credits c JOIN people p ON p.tmdb_id = c.person_id
                                      WHERE c.item_kind = 'series' AND c.item_id = series.id
                                        AND c.is_cast = 1
                                      ORDER BY c.ord),
                                    (SELECT group_concat(DISTINCT e.season)
                                       FROM episodes e WHERE e.series_id = series.id),
                                    trailer_key
                             FROM series";

fn map_series(r: &rusqlite::Row<'_>) -> rusqlite::Result<SeriesRow> {
    let genres: Option<String> = r.get(10)?;
    let seasons: Option<String> = r.get(14)?;
    let mut seasons: Vec<u16> = seasons
        .map(|s| s.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_default();
    seasons.sort_unstable();
    Ok(SeriesRow {
        id: r.get(0)?,
        title: shown(r.get(1)?),
        year: r.get::<_, Option<i64>>(2)?.map(|v| v as i32),
        quality: r.get(3)?,
        poster: r.get(4)?,
        backdrop: r.get(5)?,
        logo_art: r.get(6)?,
        overview: r.get(7)?,
        rating: r.get::<_, Option<f64>>(8)?.map(|v| v as f32),
        certification: r.get(9)?,
        genres: genres
            .and_then(|g| serde_json::from_str(&g).ok())
            .unwrap_or_default(),
        added_at: r.get(11)?,
        lang: r.get(12)?,
        cast: split_names(r.get(13)?),
        seasons,
        trailer_key: r.get(15)?,
    })
}

pub fn list_series(conn: &Connection, q: &BrowseQuery) -> Result<Vec<SeriesRow>> {
    let order = match q.sort {
        MovieSort::RecentlyAdded => "added_at DESC, series.id DESC",
        MovieSort::Title => "title COLLATE NOCASE",
        MovieSort::Year => "year DESC NULLS LAST, title",
        MovieSort::Rating => "rating DESC NULLS LAST, title",
    };
    let (filters, mut params) = browse_filters("series", q);
    let sql = format!(
        "{SERIES_SELECT} WHERE series.hidden = 0{}{filters} ORDER BY {order} \
         LIMIT :limit OFFSET :offset",
        q.library.where_sql(crate::repo::filtering::Kind::Series),
    );
    params.push((":limit", &q.limit));
    params.push((":offset", &q.offset));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params.as_slice(), map_series)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// How many series a browse query matches, ignoring its page.
pub fn count_series(conn: &Connection, q: &BrowseQuery) -> Result<u32> {
    let (filters, params) = browse_filters("series", q);
    let sql = format!(
        "SELECT count(*) FROM series WHERE series.hidden = 0{}{filters}",
        q.library.where_sql(crate::repo::filtering::Kind::Series),
    );
    Ok(conn.query_row(&sql, params.as_slice(), |r| r.get(0))?)
}

/// One shelf the provider files titles under, and how many are on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub name: String,
    pub count: u32,
}

/// The provider's own categories, biggest first.
///
/// The structure a real library actually has. Genres come from TMDB enrichment, which
/// needs an API key a viewer may never set — so on most libraries the genre filter is
/// an empty dropdown, while the panel has been filing everything under 202 named
/// shelves the whole time.
/// The shelves, with how many titles are on each — after the library filters.
///
/// The filter has to be here and not only on the list it leads to. These counts are the
/// sidebar, so "UK | FILMS 482" that opens onto 120 films is not a slow count, it is two
/// screens disagreeing about what the library contains — and the one with the number on
/// it is the one that is wrong.
pub fn categories(
    conn: &Connection,
    kind: crate::repo::filtering::Kind,
    filter: &crate::repo::filtering::LibraryFilter,
) -> Result<Vec<Category>> {
    let table = match kind {
        crate::repo::filtering::Kind::Movies => "movies",
        crate::repo::filtering::Kind::Series => "series",
        _ => return Ok(Vec::new()),
    };
    let visible = filter.where_sql(kind);
    let mut stmt = conn.prepare(&format!(
        "SELECT group_title, count(*) FROM {table}
         WHERE hidden = 0 AND group_title IS NOT NULL AND group_title != ''{visible}
         GROUP BY group_title
         ORDER BY count(*) DESC, group_title"
    ))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Category {
                name: r.get(0)?,
                count: r.get(1)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// One title by id, for a list of ids somebody curated rather than a browse page.
///
/// Deliberately not filtered by `hidden`: My List holds what a person put there, and
/// a title vanishing from it because a bulk edit hid it would look like data loss.
pub fn movie(conn: &Connection, id: i64) -> Result<Option<MovieRow>> {
    let sql = format!("{MOVIE_SELECT} WHERE movies.id = ?1");
    Ok(conn.query_row(&sql, params![id], map_movie).optional()?)
}

/// Shows with no episodes stored, worth asking the provider about, best first.
///
/// An import writes the show and not its episodes, so on a fresh library this is nearly
/// every show: 28,553 of them. The order is what makes a sweep worth running at all —
/// the same reasoning as `enrichment::pending`, for the same reason.
///
/// The library filter comes first, because a show the viewer has hidden, or that "English
/// only" keeps off their screen, is a show worth no requests. Then anything they have
/// already reached for, then the most recently added.
pub fn series_needing_episodes(
    conn: &Connection,
    filter: &crate::repo::filtering::LibraryFilter,
    limit: u32,
) -> Result<Vec<i64>> {
    let visible = filter.where_sql(crate::repo::filtering::Kind::Series);
    let sql = format!(
        "SELECT series.id FROM series
          WHERE series.hidden = 0{visible}
            AND NOT EXISTS (SELECT 1 FROM episodes e WHERE e.series_id = series.id)
          ORDER BY
            CASE WHEN EXISTS (SELECT 1 FROM favorites f
                               WHERE f.item_kind = 'series' AND f.item_id = series.id)
                   OR EXISTS (SELECT 1 FROM watch_progress w
                               JOIN episodes we ON we.id = w.item_id
                              WHERE w.item_kind = 'episode' AND we.series_id = series.id)
                 THEN 0 ELSE 1 END,
            series.added_at DESC,
            series.id
          LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn series(conn: &Connection, id: i64) -> Result<Option<SeriesRow>> {
    let sql = format!("{SERIES_SELECT} WHERE series.id = ?1");
    Ok(conn.query_row(&sql, params![id], map_series).optional()?)
}

/// What the library holds, for the Settings screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub channels: i64,
    pub movies: i64,
    pub series: i64,
    pub episodes: i64,
    pub programmes: i64,
    pub epg_coverage: EpgCoverage,
}

/// How much of the guide actually reaches the channels it is for.
///
/// The number that matters is not how many programmes were imported but how many
/// channels ended up with any — an EPG that parsed perfectly and matched nothing looks
/// identical to a missing one from the sofa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpgCoverage {
    pub total: i64,
    pub matched: i64,
    /// A few names, to make "what is missing" answerable without a query tool. Capped:
    /// a playlist with nine thousand unmatched channels should not ship all nine
    /// thousand to the renderer to be printed in one line.
    pub unmatched: Vec<String>,
}

const UNMATCHED_SAMPLE: usize = 12;

pub fn stats(conn: &Connection) -> Result<LibraryStats> {
    let count = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };

    let total = count("SELECT COUNT(*) FROM channels WHERE hidden = 0")?;
    let matched = count(
        "SELECT COUNT(*) FROM channels
         WHERE hidden = 0 AND epg_channel_id IS NOT NULL AND epg_channel_id <> ''",
    )?;

    let mut stmt = conn.prepare(
        "SELECT COALESCE(custom_name, name) FROM channels
         WHERE hidden = 0 AND (epg_channel_id IS NULL OR epg_channel_id = '')
         ORDER BY COALESCE(custom_number, number), id
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([UNMATCHED_SAMPLE as i64], |r| r.get(0))?;
    let unmatched = rows.collect::<std::result::Result<Vec<String>, _>>()?;

    Ok(LibraryStats {
        channels: total,
        movies: count("SELECT COUNT(*) FROM movies WHERE hidden = 0")?,
        series: count("SELECT COUNT(*) FROM series WHERE hidden = 0")?,
        episodes: count("SELECT COUNT(*) FROM episodes")?,
        programmes: count("SELECT COUNT(*) FROM epg_programmes")?,
        epg_coverage: EpgCoverage {
            total,
            matched,
            unmatched,
        },
    })
}

/// Every genre the visible half of the library has, for that screen's browse filter.
///
/// Scoped by kind, like `categories`. It used to union both tables, which did not
/// matter while genres arrived only from TMDB and a library generally had none of
/// them — and became wrong the moment the provider's own genres were kept
/// (docs/DECISIONS.md D26): `get_vod_streams` sends no genre, so Movies would offer a
/// dropdown of 28,529 shows' genres and filtering by one would return nothing at all.
///
/// Filtered for the same reason the categories are: a genre whose every title the
/// filters hide is an option that selects nothing.
///
/// Commonest first, like `categories`, and for the same reason. Alphabetical order
/// put `. ﺟﺮﻳﻤﺔ دراما` and `.الرسوم المتحركة` — one show each — at the top of a list
/// of 326, so the first two entries a viewer saw were punctuation while `Drama`
/// (11,259 shows) was somewhere in the middle. 201 of those 326 are on two shows or
/// fewer and the top 25 cover 91% of all tagging, so the order is the difference
/// between a usable control and a wall.
///
/// Grouped in SQL before the JSON is parsed, because tens of thousands of rows share
/// a few hundred distinct genre strings and this runs again on every filter change.
pub fn genres(
    conn: &Connection,
    kind: crate::repo::filtering::Kind,
    filter: &crate::repo::filtering::LibraryFilter,
) -> Result<Vec<Category>> {
    let table = match kind {
        crate::repo::filtering::Kind::Movies => "movies",
        crate::repo::filtering::Kind::Series => "series",
        _ => return Ok(Vec::new()),
    };
    let visible = filter.where_sql(kind);
    let mut stmt = conn.prepare(&format!(
        "SELECT genres, count(*) FROM {table}
          WHERE hidden = 0 AND genres IS NOT NULL AND genres != '' AND genres != '[]'{visible}
          GROUP BY genres"
    ))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // Grouped the way the filter compares, which is the whole point of a count beside
    // a name: SQLite's `LIKE` folds ASCII and nothing else, so `Drama`, `DRAMA` and
    // `drama` are one thing to it. Listing them as three rows showed 11,259 beside a
    // genre that returned 11,267, and put two near-duplicates in the list. Turkish
    // `Aksiyon` and `AKSİYON` stay apart here because they stay apart there too.
    //
    // Keyed by the folded form, holding every spelling seen so the commonest can be
    // the one displayed.
    type Spellings = (std::collections::HashMap<String, u32>, u32);
    let mut counts: std::collections::HashMap<String, Spellings> = std::collections::HashMap::new();
    for (raw, n) in rows {
        // A row whose genres are malformed loses its genres, not the whole filter.
        let Ok(list) = serde_json::from_str::<Vec<String>>(&raw) else {
            continue;
        };
        // Once per genre per row, however many times the row lists it. `LIKE` asks
        // whether the column contains the genre, so a show tagged both `Drama` and
        // `drama` is one show to the filter and was two here — which is the last one
        // of these the count was out by.
        let mut seen = std::collections::HashSet::new();
        for g in list.into_iter().filter(|g| !g.trim().is_empty()) {
            let key = g.to_ascii_lowercase();
            if !seen.insert(key.clone()) {
                continue;
            }
            let entry = counts.entry(key).or_default();
            *entry.0.entry(g).or_default() += n;
            entry.1 += n;
        }
    }

    let mut out: Vec<Category> = counts
        .into_values()
        .map(|(spellings, count)| Category {
            // The spelling most titles use. The name breaks a tie so a library that
            // writes it both ways does not get a different answer on each call.
            name: spellings
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
                .map(|(name, _)| name)
                .unwrap_or_default(),
            count,
        })
        .collect();
    // Name breaks the tie so the order is stable between calls; a `HashMap` alone
    // would reshuffle equally-common genres on every keystroke in the filter.
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// README §13: the same film from three providers should be one card, not three.
pub fn duplicate_groups(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT match_key, count(*) c FROM movies GROUP BY match_key, COALESCE(year,0)
         HAVING c > 1 ORDER BY c DESC",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Clone, Default)]
pub struct NewEpisode {
    pub season: u16,
    pub episode: u16,
    pub title: Option<String>,
    pub url: String,
    pub still: Option<String>,
}

/// A show as the provider describes it, before enrichment.
#[derive(Debug, Clone, Default)]
pub struct NewSeries<'a> {
    pub provider_key: &'a str,
    pub title: &'a str,
    pub match_key: &'a str,
    pub year: Option<i32>,
    pub poster: Option<&'a str>,
    /// The provider's own category, which is also where a language tag often hides.
    pub group: Option<&'a str>,
    /// The best quality any of its episode streams advertised.
    pub quality: Option<&'a str>,
    /// The provider's own score out of ten.
    pub rating: Option<f32>,
    /// The provider's plot summary.
    pub overview: Option<&'a str>,
    /// The provider's genres, already split. Stored as JSON, which is the shape
    /// enrichment writes and `parse_genres` reads.
    pub genres: &'a [String],
    /// When the provider last touched this, in unix seconds — `get_series` sends no
    /// added date, and this is the closest thing it has.
    pub added_at: Option<i64>,
}

pub fn upsert_series(
    conn: &mut Connection,
    provider_id: i64,
    series: &NewSeries<'_>,
    now: i64,
) -> Result<i64> {
    let NewSeries {
        provider_key,
        title,
        match_key,
        year,
        poster,
        group,
        quality,
        rating,
        overview,
        genres,
        added_at,
    } = *series;
    // Empty stays NULL rather than becoming `[]`: the recommender's candidate query
    // treats both as "no genres", and NULL is what an un-enriched row already reads as.
    let genres_json = if genres.is_empty() {
        None
    } else {
        Some(serde_json::to_string(genres)?)
    };
    conn.execute(
        r#"INSERT INTO series (provider_id, provider_key, title, match_key, year, poster,
                               group_title, quality, rating, overview, genres,
                               added_at, last_seen_at)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
           ON CONFLICT (provider_id, provider_key) DO UPDATE SET
             title = excluded.title, match_key = excluded.match_key,
             year = COALESCE(excluded.year, series.year),
             poster = COALESCE(excluded.poster, series.poster),
             group_title = COALESCE(excluded.group_title, series.group_title),
             quality = COALESCE(excluded.quality, series.quality),
             -- Existing wins on all three: enrichment may already have overwritten
             -- them with TMDB's, and a refresh must not undo that.
             rating = COALESCE(series.rating, excluded.rating),
             overview = COALESCE(series.overview, excluded.overview),
             genres = COALESCE(series.genres, excluded.genres),
             last_seen_at = excluded.last_seen_at"#,
        params![
            provider_id,
            provider_key,
            title,
            match_key,
            year,
            poster,
            group,
            quality,
            rating,
            overview,
            genres_json,
            added_at.unwrap_or(now),
            now
        ],
    )?;
    Ok(conn.query_row(
        "SELECT id FROM series WHERE provider_id = ?1 AND provider_key = ?2",
        params![provider_id, provider_key],
        |r| r.get(0),
    )?)
}

pub fn upsert_episodes(
    conn: &mut Connection,
    series_id: i64,
    episodes: &[NewEpisode],
    now: i64,
) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut n = 0;
    {
        let mut stmt = tx.prepare(
            r#"INSERT INTO episodes (series_id, season, episode, title, url, still, added_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7)
               ON CONFLICT (series_id, season, episode) DO UPDATE SET
                 title = COALESCE(excluded.title, episodes.title),
                 url   = excluded.url,
                 still = COALESCE(excluded.still, episodes.still)"#,
        )?;
        for e in episodes {
            stmt.execute(params![
                series_id, e.season, e.episode, e.title, e.url, e.still, now
            ])?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeRow {
    pub id: i64,
    pub season: u16,
    pub episode: u16,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub still: Option<String>,
    pub runtime_mins: Option<u32>,
    pub url: String,
}

/// Which show an episode belongs to.
///
/// Continue Watching stores progress against the *episode*, but what belongs on the
/// home screen is the show — one card reading "you are partway through this", not one
/// card per episode of it.
pub fn series_of_episode(conn: &Connection, episode_id: i64) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT series_id FROM episodes WHERE id = ?1",
            params![episode_id],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn episodes_for(
    conn: &Connection,
    series_id: i64,
    season: Option<u16>,
) -> Result<Vec<EpisodeRow>> {
    let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<EpisodeRow> {
        Ok(EpisodeRow {
            id: r.get(0)?,
            season: r.get::<_, i64>(1)? as u16,
            episode: r.get::<_, i64>(2)? as u16,
            title: r.get(3)?,
            overview: r.get(4)?,
            still: r.get(5)?,
            runtime_mins: r.get::<_, Option<i64>>(6)?.map(|v| v as u32),
            url: r.get(7)?,
        })
    };
    let rows = match season {
        Some(s) => {
            let mut stmt = conn.prepare(
                "SELECT id, season, episode, title, overview, still, runtime_mins, url
                 FROM episodes WHERE series_id = ?1 AND season = ?2 ORDER BY episode",
            )?;
            let out = stmt
                .query_map(params![series_id, s], map)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            out
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id, season, episode, title, overview, still, runtime_mins, url
                 FROM episodes WHERE series_id = ?1 ORDER BY season, episode",
            )?;
            let out = stmt
                .query_map(params![series_id], map)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            out
        }
    };
    Ok(rows)
}

/// The episode after this one, crossing a season boundary when needed.
///
/// This is deliberately *not* `progress::next_episode`, which finds the next
/// **unwatched** episode for a Continue Watching rail. The Next Episode button has to
/// go to the literal next one even if the viewer has seen it.
pub fn following_episode(conn: &Connection, episode_id: i64) -> Result<Option<EpisodeRow>> {
    let Some((series_id, season, episode)) = conn
        .query_row(
            "SELECT series_id, season, episode FROM episodes WHERE id = ?1",
            params![episode_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };

    Ok(conn
        .query_row(
            "SELECT id, season, episode, title, overview, still, runtime_mins, url
             FROM episodes
             WHERE series_id = ?1
               AND (season > ?2 OR (season = ?2 AND episode > ?3))
             ORDER BY season, episode
             LIMIT 1",
            params![series_id, season, episode],
            |r| {
                Ok(EpisodeRow {
                    id: r.get(0)?,
                    season: r.get::<_, i64>(1)? as u16,
                    episode: r.get::<_, i64>(2)? as u16,
                    title: r.get(3)?,
                    overview: r.get(4)?,
                    still: r.get(5)?,
                    runtime_mins: r.get::<_, Option<i64>>(6)?.map(|v| v as u32),
                    url: r.get(7)?,
                })
            },
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    /// A library with titles across the alphabet, plus the ones a provider's naming
    /// actually produces: digits, brackets, and a language tag on the front.
    fn lettered_library() -> crate::rusqlite::Connection {
        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        for (id, title) in [
            (1, "Arrival"),
            (2, "Alien"),
            (3, "Blade Runner"),
            (4, "the quiet hour"),
            (5, "2001: A Space Odyssey"),
            (6, "[4K] Dune"),
            (7, "Zodiac"),
        ] {
            conn.execute(
                "INSERT INTO movies (id, provider_id, provider_key, title, match_key, url,
                                     last_seen_at)
                 VALUES (?1, 1, ?2, ?3, ?3, 'https://example.com/m.mkv', 0)",
                params![id, format!("m{id}"), title],
            )
            .unwrap();
        }
        conn
    }

    fn by_letter(conn: &crate::rusqlite::Connection, letter: Option<&str>) -> Vec<String> {
        let q = BrowseQuery {
            sort: MovieSort::Title,
            genre: None,
            category: None,
            query: None,
            letter: letter.map(str::to_string),
            limit: 100,
            offset: 0,
            library: Default::default(),
        };
        list_movies(conn, &q)
            .unwrap()
            .into_iter()
            .map(|m| m.title)
            .collect()
    }

    /// The A–Z bar is a filter rather than a scroll position, because the list is paged:
    /// 20,000 films sorted A–Z arrive 120 at a time, so "jump to W" cannot be answered on
    /// the client, and answering it by fetching every page up to W would read the whole
    /// library to show one screen of it.
    #[test]
    fn a_letter_narrows_the_list_to_titles_starting_with_it() {
        let conn = lettered_library();
        assert_eq!(by_letter(&conn, Some("A")), vec!["Alien", "Arrival"]);
        assert_eq!(by_letter(&conn, Some("Z")), vec!["Zodiac"]);
        // Case is not the viewer's problem: a provider that files a film in lower case
        // must not hide it from its own letter.
        assert_eq!(by_letter(&conn, Some("T")), vec!["the quiet hour"]);
        assert_eq!(by_letter(&conn, Some("t")), vec!["the quiet hour"]);
        // Nothing selected is everything.
        assert_eq!(by_letter(&conn, None).len(), 7);
    }

    /// Everything that does not begin with a letter has to be reachable by something, and
    /// on a real provider's library that is thousands of titles: `[4K] …`, `2001 …`,
    /// `|UK| …`.
    #[test]
    fn the_hash_bucket_holds_everything_that_is_not_a_letter() {
        let conn = lettered_library();
        let mut found = by_letter(&conn, Some("#"));
        found.sort();
        assert_eq!(found, vec!["2001: A Space Odyssey", "[4K] Dune"]);
    }

    /// The count beside the heading has to be the count of what is on screen. Every
    /// letter's rows plus the `#` bucket's must add up to the library.
    #[test]
    fn the_letters_and_the_hash_bucket_partition_the_library() {
        let conn = lettered_library();
        let mut total = by_letter(&conn, Some("#")).len();
        for letter in 'A'..='Z' {
            total += by_letter(&conn, Some(&letter.to_string())).len();
        }
        assert_eq!(total, 7, "a title belonged to no bucket, or to two");
    }

    /// A recommendation arrives as words, and has to find the row a provider filed under
    /// a country tag, a quality suffix and a different kind of apostrophe.
    #[test]
    fn a_suggested_title_finds_the_row_a_provider_filed_it_under() {
        use crate::repo::filtering::{Kind, LibraryFilter};

        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        for (id, title, year) in [
            (1, "FR \u{2605} AM I NOT YOUR GIRL FHD", Some(1992)),
            (2, "Invasion of the Body Snatchers", Some(1978)),
            (3, "Invasion of the Body Snatchers", Some(2007)),
            (4, "Caf\u{e9} de Flore", Some(2011)),
        ] {
            conn.execute(
                "INSERT INTO movies (id, provider_id, provider_key, title, match_key, url,
                                     year, last_seen_at)
                 VALUES (?1, 1, ?2, ?3, ?4, 'u', ?5, 0)",
                params![
                    id,
                    format!("m{id}"),
                    title,
                    aurora_core::title::match_key(title),
                    year
                ],
            )
            .unwrap();
        }

        let find = |title: &str, year: Option<i32>| {
            find_by_title(&conn, Kind::Movies, title, year, &LibraryFilter::default()).unwrap()
        };

        // The provider's filing string is not the name anyone would type.
        assert_eq!(find("Am I Not Your Girl?", Some(1992)), Some(1));
        // Diacritics fold, so a model's plain-ASCII answer still lands.
        assert_eq!(find("Cafe de Flore", None), Some(4));
        // The remake case: the year decides, which is the whole reason it is passed.
        assert_eq!(find("Invasion of the Body Snatchers", Some(2007)), Some(3));
        assert_eq!(find("Invasion of the Body Snatchers", Some(1978)), Some(2));
        // A year a little out still finds it rather than leaving a gap in the rail.
        assert_eq!(find("Invasion of the Body Snatchers", Some(1979)), Some(2));
        // Nothing of that name is nothing, not a wrong guess.
        assert_eq!(find("A Film Nobody Has Made", None), None);
    }

    /// A recommendation must never be something the viewer has hidden or filtered away:
    /// being recommended what you asked not to see is the most annoying possible version
    /// of this feature.
    #[test]
    fn a_suggestion_cannot_resolve_to_something_the_filters_hide() {
        use crate::repo::filtering::{Kind, LibraryFilter};

        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO movies (id, provider_id, provider_key, title, match_key, url,
                                 lang_code, hidden, last_seen_at)
             VALUES (1,1,'a','Le Samourai',?1,'u','fr',0,0),
                    (2,1,'b','Hidden Film',?2,'u','en',1,0)",
            params![
                aurora_core::title::match_key("Le Samourai"),
                aurora_core::title::match_key("Hidden Film")
            ],
        )
        .unwrap();

        let english_only = LibraryFilter {
            english_only: true,
            ..Default::default()
        };
        assert_eq!(
            find_by_title(&conn, Kind::Movies, "Le Samourai", None, &english_only).unwrap(),
            None,
            "English only hid it from browsing; it must stay hidden here"
        );
        assert_eq!(
            find_by_title(
                &conn,
                Kind::Movies,
                "Hidden Film",
                None,
                &LibraryFilter::default()
            )
            .unwrap(),
            None,
            "the viewer hid this one by hand"
        );
    }

    /// The sidebar's number and the grid it opens have to be the same library.
    ///
    /// `list_movies` was read through the library filter and `categories` was not, so a
    /// shelf advertised titles it would then refuse to show. On a subscription that
    /// carries every country with "English only" on, that is most of them.
    #[test]
    fn a_category_counts_what_the_filter_would_actually_show() {
        use crate::repo::filtering::{Kind, LibraryFilter};

        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        for (id, title, lang) in [
            (1, "Arrival", Some("en")),
            (2, "Le Samouraï", Some("fr")),
            (3, "Solaris", None),
        ] {
            conn.execute(
                "INSERT INTO movies (id, provider_id, provider_key, title, match_key, url,
                                     group_title, lang_code, last_seen_at)
                 VALUES (?1, 1, ?2, ?3, ?3, 'https://example.com/m.mkv', 'FILMS', ?4, 0)",
                params![id, format!("m{id}"), title, lang],
            )
            .unwrap();
        }

        let counted = |filter: &LibraryFilter| {
            categories(&conn, Kind::Movies, filter).unwrap()[0].count as usize
        };
        let listed = |filter: LibraryFilter| {
            list_movies(
                &conn,
                &BrowseQuery {
                    sort: MovieSort::Title,
                    genre: None,
                    category: Some("FILMS".into()),
                    query: None,
                    letter: None,
                    limit: 100,
                    offset: 0,
                    library: filter,
                },
            )
            .unwrap()
            .len()
        };

        for filter in [
            LibraryFilter::default(),
            LibraryFilter {
                english_only: true,
                ..Default::default()
            },
            // Untagged hidden as well: "Solaris" has no language, which the looser
            // reading lets through and the stricter one does not.
            LibraryFilter {
                english_only: true,
                hide_untagged: true,
                ..Default::default()
            },
        ] {
            assert_eq!(
                counted(&filter),
                listed(filter),
                "the shelf promised a different number from the grid, for {filter:?}"
            );
        }
    }

    /// A genre whose every title the filters hide is an option that selects nothing.
    #[test]
    fn genres_come_from_the_visible_library() {
        use crate::repo::filtering::{Kind, LibraryFilter};

        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO movies (id, provider_id, provider_key, title, match_key, url,
                                 genres, lang_code, last_seen_at)
             VALUES (1,1,'a','A','a','u','[\"Chanson\"]','fr',0),
                    (2,1,'b','B','b','u','[\"Western\"]','en',0)",
            [],
        )
        .unwrap();

        // Names only: the counts are what `genres_are_offered_commonest_first` covers,
        // and what this test is about is which genres the language filter leaves behind.
        let names = |f: &LibraryFilter| -> Vec<String> {
            genres(&conn, Kind::Movies, f)
                .unwrap()
                .into_iter()
                .map(|g| g.name)
                .collect()
        };

        assert_eq!(
            names(&LibraryFilter::default()),
            vec!["Chanson".to_string(), "Western".to_string()]
        );
        assert_eq!(
            names(&LibraryFilter {
                english_only: true,
                ..Default::default()
            }),
            vec!["Western".to_string()],
            "a genre only French films have was still offered under English only"
        );
    }

    /// Which shows a listing sweep should ask about, and which it must not.
    ///
    /// Asking the provider about every show it lists is 28,553 requests, so the order and
    /// the exclusions are the whole difference between a sweep worth running and one that
    /// spends the viewer's connection on rows they will never see.
    #[test]
    fn the_listing_sweep_skips_what_is_answered_or_hidden_and_puts_favourites_first() {
        use crate::repo::filtering::LibraryFilter;

        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','xtream','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO profiles (id,name,created_at) VALUES (1,'Me',0)",
            [],
        )
        .unwrap();

        // (id, title, language, hidden, added_at)
        for (id, title, lang, hidden, added) in [
            (1, "Needs a listing", Some("en"), 0, 10),
            (2, "Already listed", Some("en"), 0, 20),
            (3, "Hidden by hand", Some("en"), 1, 30),
            (4, "Another language", Some("fr"), 0, 40),
            (5, "A favourite", Some("en"), 0, 1),
        ] {
            conn.execute(
                "INSERT INTO series (id, provider_id, provider_key, title, match_key,
                                     lang_code, hidden, added_at, last_seen_at)
                 VALUES (?1, 1, ?2, ?3, ?3, ?4, ?5, ?6, 0)",
                params![id, format!("series:{id}"), title, lang, hidden, added],
            )
            .unwrap();
        }
        // Show 2 already has one, so there is nothing to ask about.
        conn.execute(
            "INSERT INTO episodes (series_id, season, episode, url, added_at)
             VALUES (2, 1, 1, 'https://example.com/e.mkv', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favorites (profile_id, item_kind, item_id, added_at)
             VALUES (1, 'series', 5, 0)",
            [],
        )
        .unwrap();

        let strict = LibraryFilter {
            english_only: true,
            hide_untagged: true,
            ..Default::default()
        };
        let ids = series_needing_episodes(&conn, &strict, 50).unwrap();

        assert!(
            !ids.contains(&2),
            "a show that already has episodes was asked about"
        );
        assert!(!ids.contains(&3), "a hidden show was asked about");
        assert!(!ids.contains(&4), "a show the filter hides was asked about");
        assert_eq!(
            ids.first(),
            Some(&5),
            "a show the viewer marked comes before one they have never touched: {ids:?}"
        );
        assert!(ids.contains(&1));

        // And with no filter in force, the other-language show is in scope again.
        let ids = series_needing_episodes(&conn, &LibraryFilter::default(), 50).unwrap();
        assert!(
            ids.contains(&4),
            "nothing is filtered, so nothing should be skipped"
        );

        // The limit is a limit.
        assert_eq!(series_needing_episodes(&conn, &strict, 1).unwrap().len(), 1);
    }

    /// Continue Watching stores progress against an episode and shows the *show*, so
    /// this lookup is what turns one into the other. A wrong answer here puts the
    /// wrong poster on the home screen.
    #[test]
    fn an_episode_names_the_show_it_belongs_to() {
        let mut conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id, name, kind, base_url, created_at)
             VALUES (1, 'P', 'm3u', 'http://e.com', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO series (id, provider_id, provider_key, title, match_key, last_seen_at)
             VALUES (7, 1, 'series:7', 'A Show', 'ashow', 0),
                    (8, 1, 'series:8', 'Another', 'another', 0)",
            [],
        )
        .unwrap();
        upsert_episodes(
            &mut conn,
            7,
            &[NewEpisode {
                season: 2,
                episode: 4,
                title: Some("Fourth".into()),
                url: "u".into(),
                still: None,
            }],
            0,
        )
        .unwrap();

        let episode = episodes_for(&conn, 7, None).unwrap()[0].id;
        assert_eq!(series_of_episode(&conn, episode).unwrap(), Some(7));

        // A progress row outlives the episode a refresh removed, so this has to answer
        // rather than fail — the caller drops the card.
        assert_eq!(series_of_episode(&conn, 9_999).unwrap(), None);
    }

    #[test]
    fn stats_on_an_empty_library_are_all_zero() {
        let conn = crate::open_memory().unwrap();
        let s = stats(&conn).unwrap();
        assert_eq!((s.channels, s.movies, s.series, s.episodes), (0, 0, 0, 0));
        assert_eq!(s.epg_coverage.matched, 0);
        assert!(s.epg_coverage.unmatched.is_empty());
    }

    #[test]
    fn coverage_counts_channels_with_a_guide_not_programmes() {
        // A guide that imported ten thousand programmes and matched nothing looks
        // exactly like a missing one from the sofa, so this counts the channels.
        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id, name, kind, base_url, created_at)
             VALUES (1, 'P', 'm3u', 'http://e.com', 0)",
            [],
        )
        .unwrap();
        for (key, epg) in [("a", Some("one")), ("b", None), ("c", None)] {
            conn.execute(
                "INSERT INTO channels (provider_id, provider_key, name, match_key,
                                       epg_channel_id, last_seen_at)
                 VALUES (1, ?1, ?1, ?1, ?2, 0)",
                rusqlite::params![key, epg],
            )
            .unwrap();
        }

        let s = stats(&conn).unwrap();
        assert_eq!(s.channels, 3);
        assert_eq!(s.epg_coverage.total, 3);
        assert_eq!(s.epg_coverage.matched, 1);
        assert_eq!(s.epg_coverage.unmatched, vec!["b", "c"]);
    }

    #[test]
    fn an_empty_string_is_not_a_match() {
        // Some providers write tvg-id="" rather than omitting it, and counting that as
        // matched would report full coverage for a guide that never arrived.
        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id, name, kind, base_url, created_at)
             VALUES (1, 'P', 'm3u', 'http://e.com', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channels (provider_id, provider_key, name, match_key,
                                   epg_channel_id, last_seen_at)
             VALUES (1, 'a', 'A', 'a', '', 0)",
            [],
        )
        .unwrap();

        assert_eq!(stats(&conn).unwrap().epg_coverage.matched, 0);
    }

    use super::*;

    fn provider(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO providers (name, kind, base_url, created_at)
             VALUES ('P','xtream','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// The panel in docs/ROADMAP.md scores 109,999 of its 122,499 films and dates all
    /// 122,499, and the import used to parse both and drop them. That left `rating`
    /// NULL on every row of a library with no TMDB key — which is most libraries —
    /// and `added_at` set to the moment of the import, so Browse's default
    /// "Recently added" order was sorting by a constant.
    #[test]
    fn a_films_own_score_and_date_are_kept() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let m = NewMovie {
            rating: Some(6.97),
            added_at: Some(1_784_596_062),
            ..movie("m1", "Some Film", Some(2020))
        };
        upsert_movies(&mut conn, p, &[m], 99).unwrap();

        let (rating, added): (Option<f64>, i64) = conn
            .query_row("SELECT rating, added_at FROM movies", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(rating, Some(6.97_f32 as f64));
        assert_eq!(
            added, 1_784_596_062,
            "dated by the import, not by the provider"
        );
    }

    /// A refresh must not undo enrichment. TMDB's score is the better one and it is
    /// written *after* the import that would otherwise overwrite it every time.
    #[test]
    fn a_refresh_leaves_an_enriched_rating_alone() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let from_panel = || NewMovie {
            rating: Some(6.9),
            ..movie("m1", "Some Film", None)
        };
        upsert_movies(&mut conn, p, &[from_panel()], 0).unwrap();
        conn.execute("UPDATE movies SET rating = 8.4", []).unwrap(); // enrichment ran
        upsert_movies(&mut conn, p, &[from_panel()], 1).unwrap();

        let rating: Option<f64> = conn
            .query_row("SELECT rating FROM movies", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            rating,
            Some(8.4),
            "a playlist refresh undid what TMDB found"
        );
    }

    /// And where enrichment never ran, the provider's fills the gap on the next
    /// refresh rather than leaving the column empty for good.
    #[test]
    fn a_refresh_fills_a_rating_nothing_else_supplied() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        upsert_movies(&mut conn, p, &[movie("m1", "Some Film", None)], 0).unwrap();
        upsert_movies(
            &mut conn,
            p,
            &[NewMovie {
                rating: Some(7.1),
                ..movie("m1", "Some Film", None)
            }],
            1,
        )
        .unwrap();

        let rating: Option<f64> = conn
            .query_row("SELECT rating FROM movies", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rating, Some(7.1_f32 as f64));
    }

    /// Genres are the recommender's heaviest signal and came only from TMDB, so a
    /// library without an API key had none at all. The panel sends them for 27,661 of
    /// its 28,716 shows — stored in the same JSON shape enrichment writes, so the
    /// candidate query matches either source without knowing which it got.
    #[test]
    fn a_shows_metadata_is_stored_the_way_the_recommender_looks_it_up() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let genres = vec!["Crime".to_string(), "Drama".to_string()];
        upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Some Show",
                match_key: "someshow",
                rating: Some(9.0),
                overview: Some("A plot."),
                genres: &genres,
                added_at: Some(1_741_614_903),
                ..Default::default()
            },
            0,
        )
        .unwrap();

        let (raw, overview, rating, added): (String, String, f64, i64) = conn
            .query_row(
                "SELECT genres, overview, rating, added_at FROM series",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(raw, r#"["Crime","Drama"]"#);
        assert_eq!(overview, "A plot.");
        assert_eq!(rating, 9.0);
        assert_eq!(added, 1_741_614_903);

        // Exactly the predicate `repo::recommend::candidates` builds.
        let found: i64 = conn
            .query_row(
                r#"SELECT count(*) FROM series
                    WHERE LOWER(genres) LIKE '%"' || ?1 || '"%'"#,
                ["crime"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            found, 1,
            "the recommender cannot find this show by its genre"
        );
    }

    /// No genres stays NULL rather than becoming `[]`. Both read as "unknown"
    /// downstream, and NULL is already what an un-enriched row holds — two spellings
    /// of the same thing is how a query ends up missing one of them.
    #[test]
    fn a_show_with_no_genres_stores_nothing_rather_than_an_empty_list() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Some Show",
                match_key: "someshow",
                ..Default::default()
            },
            0,
        )
        .unwrap();

        let genres: Option<String> = conn
            .query_row("SELECT genres FROM series", [], |r| r.get(0))
            .unwrap();
        assert_eq!(genres, None);
    }

    /// Films and shows do not share a genre list, and offering one the other's is a
    /// filter that matches nothing.
    ///
    /// Invisible while genres came only from TMDB and most libraries had none. The
    /// moment the provider's own were kept (D26) it became the Movies screen showing
    /// a dropdown of 28,529 shows' genres on a panel that sends no film genre at all.
    #[test]
    fn each_half_of_the_library_offers_only_its_own_genres() {
        use crate::repo::filtering::{Kind, LibraryFilter};
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);

        upsert_movies(&mut conn, p, &[movie("m1", "Some Film", None)], 0).unwrap();
        conn.execute(r#"UPDATE movies SET genres = '["Western"]'"#, [])
            .unwrap();
        let shows = vec!["Crime".to_string(), "Drama".to_string()];
        upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Some Show",
                match_key: "someshow",
                genres: &shows,
                ..Default::default()
            },
            0,
        )
        .unwrap();

        let names = |k| -> Vec<String> {
            genres(&conn, k, &LibraryFilter::default())
                .unwrap()
                .into_iter()
                .map(|g| g.name)
                .collect()
        };
        assert_eq!(names(Kind::Movies), vec!["Western"]);
        assert_eq!(names(Kind::Series), vec!["Crime", "Drama"]);
    }

    /// Commonest first, like the shelves beside them.
    ///
    /// Alphabetical order is what put a full stop at the top of a list of 326 on a
    /// real panel, 201 of which are on two shows or fewer. The name breaks a tie so
    /// the list does not reshuffle between keystrokes.
    #[test]
    fn genres_are_offered_commonest_first() {
        use crate::repo::filtering::{Kind, LibraryFilter};
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);

        let mut add = |key: &str, genres: &[&str]| {
            let g: Vec<String> = genres.iter().map(|s| s.to_string()).collect();
            upsert_series(
                &mut conn,
                p,
                &NewSeries {
                    provider_key: key,
                    title: key,
                    match_key: key,
                    genres: &g,
                    ..Default::default()
                },
                0,
            )
            .unwrap();
        };
        add("a", &["Drama", "Crime"]);
        add("b", &["Drama"]);
        add("c", &["Drama", "Comedy"]);
        // Same count as Crime, and sorts after it.
        add("d", &["Comedy"]);

        let got: Vec<(String, u32)> = genres(&conn, Kind::Series, &LibraryFilter::default())
            .unwrap()
            .into_iter()
            .map(|g| (g.name, g.count))
            .collect();
        assert_eq!(
            got,
            vec![
                ("Drama".to_string(), 3),
                ("Comedy".to_string(), 2),
                ("Crime".to_string(), 1),
            ]
        );
    }

    /// The count beside a genre has to be what picking it returns.
    ///
    /// It was not. `Drama`, `DRAMA` and `drama` were three rows in the picker, and
    /// SQLite's `LIKE` — which folds ASCII — treated them as one, so a filter labelled
    /// 11,259 came back with 11,267 and two near-duplicate rows sat in the list.
    /// Measured on a real panel: seven genres had more than one spelling.
    #[test]
    fn spellings_the_filter_cannot_tell_apart_are_one_row() {
        use crate::repo::filtering::{Kind, LibraryFilter};
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);

        let mut add = |key: &str, genres: &[&str]| {
            let g: Vec<String> = genres.iter().map(|s| s.to_string()).collect();
            upsert_series(
                &mut conn,
                p,
                &NewSeries {
                    provider_key: key,
                    title: key,
                    match_key: key,
                    genres: &g,
                    ..Default::default()
                },
                0,
            )
            .unwrap();
        };
        add("a", &["Drama"]);
        add("b", &["Drama"]);
        add("c", &["DRAMA"]);
        add("d", &["drama"]);

        let got = genres(&conn, Kind::Series, &LibraryFilter::default()).unwrap();
        assert_eq!(got.len(), 1, "three spellings became {} rows", got.len());
        assert_eq!(got[0].count, 4, "the count must be what the filter returns");
        assert_eq!(got[0].name, "Drama", "and the spelling most titles use");
    }

    /// A show that lists the same genre twice is still one show.
    ///
    /// `LIKE` asks whether the column *contains* the genre, so `["Drama","drama"]`
    /// matches once. Counting per element made it two, and left the picker one ahead
    /// of what filtering returned — the last of several ways these two numbers had
    /// of disagreeing.
    #[test]
    fn a_row_counts_once_per_genre_however_often_it_lists_it() {
        use crate::repo::filtering::{Kind, LibraryFilter};
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let g = vec![
            "Drama".to_string(),
            "drama".to_string(),
            "Crime".to_string(),
        ];
        upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "a",
                title: "a",
                match_key: "a",
                genres: &g,
                ..Default::default()
            },
            0,
        )
        .unwrap();

        let got = genres(&conn, Kind::Series, &LibraryFilter::default()).unwrap();
        let drama = got.iter().find(|c| c.name == "Drama").unwrap();
        assert_eq!(drama.count, 1, "one show, counted {} times", drama.count);
        assert_eq!(got.len(), 2, "Drama and Crime, not three rows");
    }

    /// Only ASCII, because only ASCII is what `LIKE` folds. Turkish dotted and
    /// dotless I are different letters to SQLite, so they are different rows here —
    /// merging them would put a count on a filter that does not return it.
    #[test]
    fn folding_stops_where_the_filters_folding_stops() {
        use crate::repo::filtering::{Kind, LibraryFilter};
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let a = vec!["Aksiyon".to_string()];
        let b = vec!["AKSİYON".to_string()];
        for (key, g) in [("a", &a), ("b", &b)] {
            upsert_series(
                &mut conn,
                p,
                &NewSeries {
                    provider_key: key,
                    title: key,
                    match_key: key,
                    genres: g,
                    ..Default::default()
                },
                0,
            )
            .unwrap();
        }
        assert_eq!(
            genres(&conn, Kind::Series, &LibraryFilter::default())
                .unwrap()
                .len(),
            2
        );
    }

    /// A hidden row is not on the screen, so its genres are not in the filter either —
    /// a filter offering a value that yields nothing is the complaint this whole
    /// facet exists to answer.
    #[test]
    fn a_hidden_rows_genres_are_not_offered() {
        use crate::repo::filtering::{Kind, LibraryFilter};
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        upsert_movies(&mut conn, p, &[movie("m1", "Some Film", None)], 0).unwrap();
        conn.execute(
            r#"UPDATE movies SET genres = '["Western"]', hidden = 1"#,
            [],
        )
        .unwrap();
        assert!(genres(&conn, Kind::Movies, &LibraryFilter::default())
            .unwrap()
            .is_empty());
    }

    fn movie(key: &str, title: &str, year: Option<i32>) -> NewMovie {
        NewMovie {
            provider_key: key.into(),
            title: title.into(),
            match_key: aurora_core::title::match_key(title),
            year,
            url: format!("https://example.com/{key}.mkv"),
            ..Default::default()
        }
    }

    #[test]
    fn upserts_and_sorts_movies() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        upsert_movies(
            &mut conn,
            p,
            &[
                movie("1", "Zulu", Some(1964)),
                movie("2", "Alien", Some(1979)),
            ],
            100,
        )
        .unwrap();

        let by_title = list_movies(
            &conn,
            &BrowseQuery {
                sort: MovieSort::Title,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_title[0].title, "Alien");

        let by_year = list_movies(
            &conn,
            &BrowseQuery {
                sort: MovieSort::Year,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_year[0].title, "Alien");
    }

    #[test]
    fn refresh_does_not_wipe_enriched_metadata() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        upsert_movies(&mut conn, p, &[movie("1", "Alien", Some(1979))], 100).unwrap();

        // Simulate TMDB enrichment writing fields the playlist never supplies.
        conn.execute(
            "UPDATE movies SET overview = 'In space...', backdrop = 'b.jpg', rating = 8.4",
            [],
        )
        .unwrap();

        upsert_movies(&mut conn, p, &[movie("1", "Alien (1979)", Some(1979))], 200).unwrap();

        let m = &list_movies(
            &conn,
            &BrowseQuery {
                sort: MovieSort::Title,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap()[0];
        assert_eq!(m.overview.as_deref(), Some("In space..."));
        assert_eq!(m.backdrop.as_deref(), Some("b.jpg"));
        assert_eq!(m.rating, Some(8.4));
        assert_eq!(
            m.title, "Alien (1979)",
            "provider title should still update"
        );
    }

    #[test]
    fn duplicates_are_detectable_across_providers() {
        let mut conn = crate::open_memory().unwrap();
        let p1 = provider(&conn);
        let p2 = provider(&conn);
        upsert_movies(&mut conn, p1, &[movie("a", "Alien", Some(1979))], 0).unwrap();
        upsert_movies(&mut conn, p2, &[movie("b", "ALIEN 1080p", Some(1979))], 0).unwrap();

        let dupes = duplicate_groups(&conn).unwrap();
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0].1, 2);
    }

    #[test]
    fn series_and_episodes_round_trip() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let sid = upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Breaking Bad",
                match_key: "breakingbad",
                year: Some(2008),
                poster: None,
                group: None,
                quality: None,
                ..Default::default()
            },
            0,
        )
        .unwrap();
        upsert_episodes(
            &mut conn,
            sid,
            &[
                NewEpisode {
                    season: 1,
                    episode: 2,
                    title: Some("Two".into()),
                    url: "u2".into(),
                    still: None,
                },
                NewEpisode {
                    season: 1,
                    episode: 1,
                    title: Some("One".into()),
                    url: "u1".into(),
                    still: None,
                },
                NewEpisode {
                    season: 2,
                    episode: 1,
                    title: None,
                    url: "u3".into(),
                    still: None,
                },
            ],
            0,
        )
        .unwrap();

        let all = episodes_for(&conn, sid, None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!((all[0].season, all[0].episode), (1, 1));
        assert_eq!((all[2].season, all[2].episode), (2, 1));

        let s1 = episodes_for(&conn, sid, Some(1)).unwrap();
        assert_eq!(s1.len(), 2);
    }

    #[test]
    fn reimporting_an_episode_updates_rather_than_duplicating() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let sid = upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Show",
                match_key: "show",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let ep = NewEpisode {
            season: 1,
            episode: 1,
            title: Some("T".into()),
            url: "old".into(),
            still: None,
        };
        upsert_episodes(&mut conn, sid, std::slice::from_ref(&ep), 0).unwrap();
        upsert_episodes(
            &mut conn,
            sid,
            &[NewEpisode {
                url: "new".into(),
                title: None,
                ..ep
            }],
            0,
        )
        .unwrap();

        let rows = episodes_for(&conn, sid, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "new");
        assert_eq!(
            rows[0].title.as_deref(),
            Some("T"),
            "title should not be nulled"
        );
    }

    #[test]
    fn following_episode_walks_the_season_then_crosses_into_the_next() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let sid = upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s",
                title: "Show",
                match_key: "show",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        upsert_episodes(
            &mut conn,
            sid,
            &[
                NewEpisode {
                    season: 1,
                    episode: 1,
                    url: "a".into(),
                    ..Default::default()
                },
                NewEpisode {
                    season: 1,
                    episode: 2,
                    url: "b".into(),
                    ..Default::default()
                },
                NewEpisode {
                    season: 2,
                    episode: 1,
                    url: "c".into(),
                    ..Default::default()
                },
            ],
            0,
        )
        .unwrap();

        let eps = episodes_for(&conn, sid, None).unwrap();
        let next = following_episode(&conn, eps[0].id).unwrap().unwrap();
        assert_eq!((next.season, next.episode), (1, 2));

        // Crosses the season boundary.
        let across = following_episode(&conn, eps[1].id).unwrap().unwrap();
        assert_eq!((across.season, across.episode), (2, 1));

        // The last episode has no successor.
        assert!(following_episode(&conn, eps[2].id).unwrap().is_none());
    }

    #[test]
    fn following_episode_handles_gaps_and_unknown_ids() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let sid = upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s",
                title: "Show",
                match_key: "show",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        // A provider that is missing episode 2 entirely.
        upsert_episodes(
            &mut conn,
            sid,
            &[
                NewEpisode {
                    season: 1,
                    episode: 1,
                    url: "a".into(),
                    ..Default::default()
                },
                NewEpisode {
                    season: 1,
                    episode: 5,
                    url: "b".into(),
                    ..Default::default()
                },
            ],
            0,
        )
        .unwrap();

        let eps = episodes_for(&conn, sid, None).unwrap();
        let next = following_episode(&conn, eps[0].id).unwrap().unwrap();
        assert_eq!(next.episode, 5, "should skip the gap, not stop at it");

        assert!(following_episode(&conn, 99_999).unwrap().is_none());
    }

    #[test]
    fn series_upsert_is_stable_across_refreshes() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let a = upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Show",
                match_key: "show",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let b = upsert_series(
            &mut conn,
            p,
            &NewSeries {
                provider_key: "s1",
                title: "Show HD",
                match_key: "show",
                year: Some(2020),
                poster: None,
                group: None,
                quality: None,
                ..Default::default()
            },
            1,
        )
        .unwrap();
        assert_eq!(a, b, "same provider_key must map to the same row");
    }
}
