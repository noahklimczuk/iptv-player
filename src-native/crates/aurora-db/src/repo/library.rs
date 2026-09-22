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
    pub genres: Vec<String>,
    pub url: String,
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
                url, poster, added_at, last_seen_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
               ON CONFLICT (provider_id, provider_key) DO UPDATE SET
                 title        = excluded.title,
                 match_key    = excluded.match_key,
                 year         = excluded.year,
                 quality      = excluded.quality,
                 group_title  = excluded.group_title,
                 url          = excluded.url,
                 poster       = COALESCE(excluded.poster, movies.poster),
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
                now
            ])?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

const MOVIE_SELECT: &str = "SELECT id, title, year, quality, poster, backdrop, overview,
                                   runtime_mins, rating, genres, url FROM movies";

fn map_movie(r: &rusqlite::Row<'_>) -> rusqlite::Result<MovieRow> {
    let genres: Option<String> = r.get(9)?;
    Ok(MovieRow {
        id: r.get(0)?,
        title: r.get(1)?,
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
        url: r.get(10)?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovieSort {
    RecentlyAdded,
    Title,
    Year,
    Rating,
}

pub fn list_movies(
    conn: &Connection,
    sort: MovieSort,
    limit: u32,
    offset: u32,
) -> Result<Vec<MovieRow>> {
    let order = match sort {
        MovieSort::RecentlyAdded => "added_at DESC, id DESC",
        MovieSort::Title => "title COLLATE NOCASE",
        MovieSort::Year => "year DESC NULLS LAST, title",
        MovieSort::Rating => "rating DESC NULLS LAST, title",
    };
    let sql = format!("{MOVIE_SELECT} ORDER BY {order} LIMIT ?1 OFFSET ?2");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![limit, offset], map_movie)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
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
    } = *series;
    conn.execute(
        r#"INSERT INTO series (provider_id, provider_key, title, match_key, year, poster,
                               added_at, last_seen_at)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?7)
           ON CONFLICT (provider_id, provider_key) DO UPDATE SET
             title = excluded.title, match_key = excluded.match_key,
             year = COALESCE(excluded.year, series.year),
             poster = COALESCE(excluded.poster, series.poster),
             last_seen_at = excluded.last_seen_at"#,
        params![
            provider_id,
            provider_key,
            title,
            match_key,
            year,
            poster,
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

        let by_title = list_movies(&conn, MovieSort::Title, 10, 0).unwrap();
        assert_eq!(by_title[0].title, "Alien");

        let by_year = list_movies(&conn, MovieSort::Year, 10, 0).unwrap();
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

        let m = &list_movies(&conn, MovieSort::Title, 10, 0).unwrap()[0];
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
            },
            1,
        )
        .unwrap();
        assert_eq!(a, b, "same provider_key must map to the same row");
    }
}
