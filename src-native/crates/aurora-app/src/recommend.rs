//! "Recommended for you", joined up.
//!
//! Three layers, deliberately: `aurora_core::recommend` is the algorithm and knows
//! nothing about storage, `aurora_db::repo::recommend` is the queries, and this is the
//! join — load the history, build the taste, rank the pool, turn the winners back into
//! library rows the UI already knows how to draw.

use std::collections::{HashMap, HashSet};

use aurora_core::recommend::{self, Kind};
use aurora_db::repo::{library, recommend as store};
use aurora_db::rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::Result;
use crate::library::CatalogItem;
use crate::services::Services;

/// How many shelves and genres of the viewer's own taste are fetched from.
///
/// Three of each is enough to describe somebody without turning one evening's
/// watching into a wall: past the third, the decayed weights are small enough that
/// the rows they contribute never win a slot anyway.
const TASTE_BUCKETS: usize = 3;

/// How many rows each of those buckets contributes.
const PER_BUCKET: u32 = 600;

/// And how many general rows, so there is somewhere to discover from.
const EXPLORE: u32 = 600;

/// Where a recommendation's reason is keyed from, matching `Rail::progress`:
/// `movie:12`, `series:7`.
fn key(kind: Kind, id: i64) -> String {
    format!("{}:{}", kind.as_str(), id)
}

/// The winners, and what to say about each.
#[derive(Debug, Default)]
pub struct Picks {
    pub items: Vec<CatalogItem>,
    /// Keyed as above, so the rail carries them the same way it carries progress.
    pub reasons: HashMap<String, String>,
    /// The genres this viewer watches most, for a heading that is about them.
    pub top_genres: Vec<String>,
    /// Whether any of this came from watch history, or it is all cold-start guessing.
    pub personalised: bool,
}

/// Work out what to recommend, and hydrate the winners into library rows.
pub fn pick(conn: &Connection, profile_id: i64, now: i64, limit: usize) -> Result<Picks> {
    let history = store::history(conn, profile_id)?;
    let taste = recommend::profile(&history, now);
    // Retrieval is directed by the taste, not independent of it — see
    // `store::candidates` for why an arbitrary slice of 146,000 rows does not work.
    let candidates = store::candidates(
        conn,
        &taste.top_categories(TASTE_BUCKETS),
        &taste.top_genres(TASTE_BUCKETS),
        PER_BUCKET,
        EXPLORE,
    )?;

    let seen: HashSet<(Kind, i64)> = store::already_seen(conn, profile_id)?.into_iter().collect();
    let ranked = recommend::rank(
        &taste,
        &candidates,
        &|kind, id| seen.contains(&(kind, id)),
        now,
        limit,
    );

    let mut picks = Picks {
        top_genres: taste.top_genres(3),
        personalised: taste.is_known(),
        ..Default::default()
    };
    for entry in ranked {
        // A row that vanished between the two queries is skipped rather than faked:
        // a refresh can delete one while this is running.
        let item = match entry.kind {
            Kind::Movie => library::movie(conn, entry.id)?.map(CatalogItem::Movie),
            Kind::Series => library::series(conn, entry.id)?.map(CatalogItem::Series),
        };
        let Some(item) = item else { continue };
        picks
            .reasons
            .insert(key(entry.kind, entry.id), entry.reason.label());
        picks.items.push(item);
    }
    Ok(picks)
}

/// The heading the rail gets.
///
/// Named after the viewer's own taste where there is any, because "More Sci-Fi and
/// Crime" says more than "Recommended" and is checkable by the person reading it.
pub fn heading(picks: &Picks) -> String {
    if !picks.personalised {
        return "Worth a look".to_string();
    }
    match picks.top_genres.len() {
        0 => "Recommended for you".to_string(),
        1 => format!("More {}", title_case(&picks.top_genres[0])),
        _ => format!(
            "More {} and {}",
            title_case(&picks.top_genres[0]),
            title_case(&picks.top_genres[1])
        ),
    }
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, word) in s.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendArgs {
    pub profile_id: i64,
    /// How many to return. Clamped, because this is reachable from the UI.
    #[serde(default)]
    pub limit: Option<u32>,
}

/// What the Browse screen asks for when it wants more than the rail shows.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommended {
    pub title: String,
    pub items: Vec<CatalogItem>,
    pub reasons: HashMap<String, String>,
    /// False when there is no watch history yet and this is rating-led rather than
    /// personal — so the screen can say so instead of implying it knows them.
    pub personalised: bool,
}

/// The most a single call will return, so a UI bug cannot ask for the library.
const MAX_LIMIT: u32 = 200;
const DEFAULT_LIMIT: u32 = 40;

#[tauri::command(async)]
pub fn library_recommended(
    services: State<'_, Services>,
    args: RecommendArgs,
) -> Result<Recommended> {
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let db = services.db.lock();
    let picks = pick(&db, args.profile_id, crate::now_unix(), limit)?;
    Ok(Recommended {
        title: heading(&picks),
        items: picks.items,
        reasons: picks.reasons,
        personalised: picks.personalised,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picks_with(genres: &[&str], personalised: bool) -> Picks {
        Picks {
            top_genres: genres.iter().map(|g| g.to_string()).collect(),
            personalised,
            ..Default::default()
        }
    }

    #[test]
    fn a_heading_names_the_viewers_own_taste() {
        assert_eq!(
            heading(&picks_with(&["sci-fi", "crime", "drama"], true)),
            "More Sci-fi and Crime"
        );
        assert_eq!(heading(&picks_with(&["horror"], true)), "More Horror");
    }

    /// A fresh install has no history, and a heading that implies otherwise is a
    /// small lie the viewer can immediately catch.
    #[test]
    fn a_cold_start_does_not_pretend_to_know_them() {
        assert_eq!(heading(&picks_with(&[], false)), "Worth a look");
        assert_eq!(
            heading(&picks_with(&["sci-fi"], false)),
            "Worth a look",
            "genres from nowhere must not become a claim about the viewer"
        );
    }

    #[test]
    fn reason_keys_match_how_the_rail_keys_progress() {
        assert_eq!(key(Kind::Movie, 12), "movie:12");
        assert_eq!(key(Kind::Series, 7), "series:7");
    }

    #[test]
    fn title_case_survives_the_shapes_genres_actually_arrive_in() {
        assert_eq!(title_case("science fiction"), "Science Fiction");
        assert_eq!(title_case("tv movie"), "Tv Movie");
        assert_eq!(title_case(""), "");
    }
}
