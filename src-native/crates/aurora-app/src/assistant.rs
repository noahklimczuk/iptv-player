//! The assistant: a conversation that can read the library and hand back things to play.
//!
//! `gemini.rs` asks one question — "given what they watched, what else?" — and resolves
//! the answer into a rail. This is the interactive half. The viewer types, the model may
//! call functions to look through their library, and the answer can carry titles with a
//! play button on them.
//!
//! ## The loop
//!
//! One `send` is: append the viewer's words, then up to [`MAX_ROUNDS`] rounds of asking
//! the model. Each round it either answers (done) or asks for one or more lookups, which
//! are executed here, against SQLite, and fed back. Bounded in three ways, because every
//! round is a request somebody is paying for and waiting on: rounds, calls per message,
//! and rows per call.
//!
//! ## Why the model cannot invent something playable
//!
//! Every card the viewer sees came from [`Tool::ShowTitles`], and the ids it takes are
//! ids this module handed out in a previous tool result. A title the model made up has no
//! id, so it cannot become a card — it can only appear in the prose, where it reads as a
//! suggestion rather than as something with a stream behind it. That is the same property
//! the one-shot recommender gets by resolving names against the library, kept here by
//! construction instead.
//!
//! ## What is remembered
//!
//! Only the readable conversation — the viewer's turns and the assistant's — is kept, in
//! the settings table, per profile. Tool calls and their results are deliberately *not*
//! persisted: they are the model's working out, they are far larger than the conversation,
//! and Gemini requires a call and its response to appear as a matched pair, so trimming
//! them by age is how a history becomes invalid. If a later turn needs the same facts it
//! looks them up again, which is cheap and always current.

use aurora_db::repo::{filtering, library, recommend as store, search, settings};
use aurora_db::rusqlite::Connection;
use aurora_ingest::chat::{ChatClient, ChatReply, Chatter, ToolCall, ToolDecl, Turn};
use aurora_ingest::http::{HttpClient, HttpConfig};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{AppError, Result};
use crate::services::Services;

/// How many times the model may come back wanting another lookup before it has to answer.
///
/// Six is enough for "orient, search, narrow, check, show" with room to spare, and small
/// enough that a model stuck in a loop costs seconds rather than a bill.
pub const MAX_ROUNDS: usize = 6;

/// How many lookups one message may run in total, across all rounds.
pub const MAX_CALLS: usize = 14;

/// The most rows any one lookup will return.
///
/// A model given two hundred titles reasons about the list; given twenty it reasons about
/// the request. The cap is also what keeps a turn's prompt from growing without bound.
pub const MAX_ROWS: u32 = 25;

/// How many exchanges are kept. Older ones fall off the front.
const KEEP_TURNS: usize = 40;

const HISTORY_KEY: &str = "assistant.history";

/// One thing the viewer can press play on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatItem {
    /// `movie`, `series` or `live`.
    pub kind: String,
    pub id: i64,
    pub title: String,
    pub year: Option<i32>,
    pub poster: Option<String>,
    /// The model's one line about why this one, for this person.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// `user` or `assistant`.
    pub role: String,
    pub text: String,
    #[serde(default)]
    pub items: Vec<ChatItem>,
    pub at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    messages: Vec<ChatMessage>,
}

/// What a `send` produced: the assistant's message, plus what it did to get there.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub message: ChatMessage,
    /// The lookups it ran, in order, so the UI can show its working.
    ///
    /// Worth surfacing rather than hiding: "searched your library for *heist*, 11 hits"
    /// is the difference between a pause that looks broken and one that looks like
    /// thinking, and it is also how somebody notices the assistant looked at the wrong
    /// thing.
    pub steps: Vec<String>,
}

/* ── Tools ─────────────────────────────────────────────────────────────────── */

/// The functions the model may call. One enum so the declaration and the dispatch
/// cannot drift apart — adding a variant forces both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    LibraryDigest,
    SearchLibrary,
    BrowseLibrary,
    WatchHistory,
    TitleDetails,
    WhatsOnNow,
    ShowTitles,
}

impl Tool {
    pub const ALL: [Tool; 7] = [
        Tool::LibraryDigest,
        Tool::SearchLibrary,
        Tool::BrowseLibrary,
        Tool::WatchHistory,
        Tool::TitleDetails,
        Tool::WhatsOnNow,
        Tool::ShowTitles,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::LibraryDigest => "library_digest",
            Tool::SearchLibrary => "search_library",
            Tool::BrowseLibrary => "browse_library",
            Tool::WatchHistory => "watch_history",
            Tool::TitleDetails => "title_details",
            Tool::WhatsOnNow => "whats_on_now",
            Tool::ShowTitles => "show_titles",
        }
    }

    pub fn parse(name: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.name() == name)
    }

    fn declaration(self) -> ToolDecl {
        match self {
            Tool::LibraryDigest => ToolDecl {
                name: self.name(),
                description: "Counts, the provider's shelf names, and the genres that \
                              actually exist in this library. Call this first if you need \
                              to know what is available at all.",
                parameters: json!({ "type": "OBJECT", "properties": {} }),
            },
            Tool::SearchLibrary => ToolDecl {
                name: self.name(),
                description: "Search the viewer's own library by title or keyword. \
                              Returns ids you can pass to show_titles or title_details. \
                              Use this to check something is actually available before \
                              recommending it.",
                parameters: json!({
                    "type": "OBJECT",
                    "properties": {
                        "query": { "type": "STRING", "description": "Title or keyword." },
                        "kind": {
                            "type": "STRING",
                            "enum": ["movie", "series", "live", "any"],
                            "description": "Narrow to one kind. Defaults to any.",
                        },
                        "limit": { "type": "INTEGER" },
                    },
                    "required": ["query"],
                }),
            },
            Tool::BrowseLibrary => ToolDecl {
                name: self.name(),
                description: "List films or series by genre or shelf, sorted. Use this \
                              for open requests like \"something funny\" where there is \
                              no title to search for.",
                parameters: json!({
                    "type": "OBJECT",
                    "properties": {
                        "kind": { "type": "STRING", "enum": ["movie", "series"] },
                        "genre": { "type": "STRING", "description": "Exactly as library_digest spells it." },
                        "shelf": { "type": "STRING", "description": "The provider's category name." },
                        "sort": {
                            "type": "STRING",
                            "enum": ["rating", "recentlyAdded", "year", "title"],
                        },
                        "limit": { "type": "INTEGER" },
                    },
                    "required": ["kind"],
                }),
            },
            Tool::WatchHistory => ToolDecl {
                name: self.name(),
                description: "What this viewer has watched, most recent first, with how \
                              much of each and whether they favourited it. May be short \
                              or empty — a new library has no history, and guessing from \
                              nothing is worse than asking them.",
                parameters: json!({
                    "type": "OBJECT",
                    "properties": { "limit": { "type": "INTEGER" } },
                }),
            },
            Tool::TitleDetails => ToolDecl {
                name: self.name(),
                description: "Everything known about one film or series: overview, \
                              genres, rating, year, and for a series how many seasons and \
                              episodes are in the library.",
                parameters: json!({
                    "type": "OBJECT",
                    "properties": {
                        "kind": { "type": "STRING", "enum": ["movie", "series"] },
                        "id": { "type": "INTEGER" },
                    },
                    "required": ["kind", "id"],
                }),
            },
            Tool::WhatsOnNow => ToolDecl {
                name: self.name(),
                description: "Live channels and what is on them right now, from the \
                              guide. Use this for \"what's on\" rather than searching.",
                parameters: json!({
                    "type": "OBJECT",
                    "properties": {
                        "query": { "type": "STRING", "description": "Narrow by channel name or group." },
                        "limit": { "type": "INTEGER" },
                    },
                }),
            },
            Tool::ShowTitles => ToolDecl {
                name: self.name(),
                description: "Put titles in front of the viewer as cards they can play. \
                              Only ids returned by a previous lookup work — this is the \
                              only way anything becomes playable, so call it with your \
                              picks before you finish, and give each one a short note \
                              saying why it suits THIS viewer.",
                parameters: json!({
                    "type": "OBJECT",
                    "properties": {
                        "items": {
                            "type": "ARRAY",
                            "items": {
                                "type": "OBJECT",
                                "properties": {
                                    "kind": { "type": "STRING", "enum": ["movie", "series", "live"] },
                                    "id": { "type": "INTEGER" },
                                    "note": { "type": "STRING" },
                                },
                                "required": ["kind", "id"],
                            },
                        },
                    },
                    "required": ["items"],
                }),
            },
        }
    }
}

pub fn declarations() -> Vec<ToolDecl> {
    Tool::ALL.into_iter().map(Tool::declaration).collect()
}

fn clamp_limit(args: &serde_json::Value, default: u32) -> u32 {
    args.get("limit")
        .and_then(|l| l.as_u64())
        .map(|l| l as u32)
        .unwrap_or(default)
        .clamp(1, MAX_ROWS)
}

fn str_arg(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn kind_of(name: &str) -> Option<filtering::Kind> {
    match name {
        "movie" | "movies" => Some(filtering::Kind::Movies),
        "series" => Some(filtering::Kind::Series),
        "live" | "channel" => Some(filtering::Kind::Live),
        _ => None,
    }
}

/// One tool's answer, plus the line the UI shows for having run it.
struct Ran {
    result: serde_json::Value,
    step: String,
    /// Cards to attach to the reply, from `show_titles`.
    items: Vec<ChatItem>,
}

/// Run one lookup against the library.
///
/// Everything here is read-only and shaped by hand: a tool returns the fields it was
/// written to return and nothing else, which is what keeps stream URLs and provider
/// details out of a request to Google however the model phrases its question.
fn run_tool(
    db: &Connection,
    profile_id: i64,
    call: &ToolCall,
    now: i64,
) -> std::result::Result<Ran, String> {
    let Some(tool) = Tool::parse(&call.name) else {
        return Err(format!("There is no tool called {}.", call.name));
    };
    let filter = filtering::LibraryFilter::load(db).unwrap_or_default();

    match tool {
        Tool::LibraryDigest => {
            let stats = library::stats(db).map_err(|e| e.to_string())?;
            let movie_genres =
                library::genres(db, filtering::Kind::Movies, &filter).map_err(|e| e.to_string())?;
            let series_genres =
                library::genres(db, filtering::Kind::Series, &filter).map_err(|e| e.to_string())?;
            let shelves = library::categories(db, filtering::Kind::Movies, &filter)
                .map_err(|e| e.to_string())?;

            // Names with counts, trimmed: the top of each list is what a viewer's
            // library is actually made of, and the long tail is noise in a prompt.
            let top = |rows: Vec<library::Category>, n: usize| -> Vec<serde_json::Value> {
                rows.into_iter()
                    .take(n)
                    .map(|c| json!({ "name": c.name, "count": c.count }))
                    .collect()
            };

            Ok(Ran {
                result: json!({
                    "channels": stats.channels,
                    "movies": stats.movies,
                    "series": stats.series,
                    "episodes": stats.episodes,
                    "movie_genres": top(movie_genres, 30),
                    "series_genres": top(series_genres, 30),
                    "movie_shelves": top(shelves, 30),
                }),
                step: format!(
                    "looked over your library — {} films, {} series, {} channels",
                    stats.movies, stats.series, stats.channels
                ),
                items: Vec::new(),
            })
        }

        Tool::SearchLibrary => {
            let query = str_arg(&call.args, "query")
                .ok_or_else(|| "search_library needs a query.".to_string())?;
            let limit = clamp_limit(&call.args, 12);
            let wanted = str_arg(&call.args, "kind").unwrap_or_else(|| "any".into());

            let hits = search::query(db, &query, MAX_ROWS).map_err(|e| e.to_string())?;
            let rows: Vec<serde_json::Value> = hits
                .into_iter()
                .filter(|h| wanted == "any" || h.kind == wanted)
                .take(limit as usize)
                .map(|h| {
                    json!({
                        "kind": h.kind,
                        // `ref_id` is the library row, which is what show_titles and
                        // title_details take. The search index has its own rowid and
                        // handing that over would produce cards pointing at nothing.
                        "id": h.ref_id,
                        "title": h.title,
                        "subtitle": h.subtitle,
                    })
                })
                .collect();

            Ok(Ran {
                step: format!(
                    "searched your library for \u{201c}{query}\u{201d} — {} found",
                    rows.len()
                ),
                result: json!({ "items": rows }),
                items: Vec::new(),
            })
        }

        Tool::BrowseLibrary => {
            let kind = str_arg(&call.args, "kind")
                .and_then(|k| kind_of(&k))
                .ok_or_else(|| "browse_library needs kind to be movie or series.".to_string())?;
            let limit = clamp_limit(&call.args, 15);
            let genre = str_arg(&call.args, "genre");
            let shelf = str_arg(&call.args, "shelf");
            let sort = match str_arg(&call.args, "sort").as_deref() {
                Some("rating") => library::MovieSort::Rating,
                Some("year") => library::MovieSort::Year,
                Some("title") => library::MovieSort::Title,
                _ => library::MovieSort::RecentlyAdded,
            };

            let q = library::BrowseQuery {
                sort,
                genre: genre.clone(),
                category: shelf.clone(),
                query: None,
                letter: None,
                limit,
                offset: 0,
                library: filter,
            };

            let (rows, described): (Vec<serde_json::Value>, String) =
                if kind == filtering::Kind::Series {
                    let list = library::list_series(db, &q).map_err(|e| e.to_string())?;
                    (
                        list.iter()
                            .map(|s| {
                                json!({
                                    "kind": "series",
                                    "id": s.id,
                                    "title": s.title,
                                    "year": s.year,
                                    "rating": s.rating,
                                    "genres": s.genres,
                                })
                            })
                            .collect(),
                        "series".into(),
                    )
                } else {
                    let list = library::list_movies(db, &q).map_err(|e| e.to_string())?;
                    (
                        list.iter()
                            .map(|m| {
                                json!({
                                    "kind": "movie",
                                    "id": m.id,
                                    "title": m.title,
                                    "year": m.year,
                                    "rating": m.rating,
                                    "genres": m.genres,
                                })
                            })
                            .collect(),
                        "films".into(),
                    )
                };

            let narrowed = genre
                .or(shelf)
                .map(|g| format!(" in {g}"))
                .unwrap_or_default();
            Ok(Ran {
                step: format!("browsed {described}{narrowed} — {} found", rows.len()),
                result: json!({ "items": rows }),
                items: Vec::new(),
            })
        }

        Tool::WatchHistory => {
            let limit = clamp_limit(&call.args, 20) as usize;
            let history = store::history(db, profile_id).unwrap_or_default();
            let total = history.len();
            let rows: Vec<serde_json::Value> = history
                .into_iter()
                .take(limit)
                .map(|w| {
                    json!({
                        "title": w.title,
                        "year": w.year,
                        "genres": w.genres,
                        "watched_fraction": w.fraction,
                        "favourite": w.favourite,
                    })
                })
                .collect();
            Ok(Ran {
                step: if total == 0 {
                    "checked your history — nothing watched yet".into()
                } else {
                    format!("checked your history — {total} titles")
                },
                result: json!({ "items": rows, "total": total }),
                items: Vec::new(),
            })
        }

        Tool::TitleDetails => {
            let kind = str_arg(&call.args, "kind")
                .and_then(|k| kind_of(&k))
                .ok_or_else(|| "title_details needs kind to be movie or series.".to_string())?;
            let id = call
                .args
                .get("id")
                .and_then(|i| i.as_i64())
                .ok_or_else(|| "title_details needs an id.".to_string())?;

            if kind == filtering::Kind::Series {
                let Some(s) = library::series(db, id).map_err(|e| e.to_string())? else {
                    return Err(format!("There is no series {id} in the library."));
                };
                let episodes = library::episodes_for(db, id, None).map_err(|e| e.to_string())?;
                let seasons: std::collections::BTreeSet<u16> =
                    episodes.iter().map(|e| e.season).collect();
                Ok(Ran {
                    step: format!("read up on {}", s.title),
                    result: json!({
                        "kind": "series",
                        "id": s.id,
                        "title": s.title,
                        "year": s.year,
                        "rating": s.rating,
                        "genres": s.genres,
                        "overview": s.overview,
                        "seasons": seasons.len(),
                        "episodes_in_library": episodes.len(),
                    }),
                    items: Vec::new(),
                })
            } else {
                let Some(m) = library::movie(db, id).map_err(|e| e.to_string())? else {
                    return Err(format!("There is no film {id} in the library."));
                };
                Ok(Ran {
                    step: format!("read up on {}", m.title),
                    result: json!({
                        "kind": "movie",
                        "id": m.id,
                        "title": m.title,
                        "year": m.year,
                        "rating": m.rating,
                        "genres": m.genres,
                        "overview": m.overview,
                        "runtime_mins": m.runtime_mins,
                    }),
                    items: Vec::new(),
                })
            }
        }

        Tool::WhatsOnNow => {
            let limit = clamp_limit(&call.args, 12) as usize;
            let query = str_arg(&call.args, "query").map(|q| q.to_lowercase());
            let channels = aurora_db::repo::channels::list(db, &Default::default())
                .map_err(|e| e.to_string())?;

            let mut rows = Vec::new();
            for ch in channels {
                if let Some(q) = &query {
                    let hay = format!(
                        "{} {}",
                        ch.name.to_lowercase(),
                        ch.group.clone().unwrap_or_default().to_lowercase()
                    );
                    if !hay.contains(q) {
                        continue;
                    }
                }
                // The guide is keyed by the channel's EPG id, not its row id, and a
                // channel the provider never matched has none — which is ordinary, and
                // means "no listing" rather than an error.
                let now_on = ch
                    .epg_channel_id
                    .as_deref()
                    .filter(|id| !id.is_empty())
                    .and_then(|id| aurora_db::repo::epg::now_next(db, id, now).ok())
                    .and_then(|(current, _next)| current);
                rows.push(json!({
                    "kind": "live",
                    "id": ch.id,
                    "channel": ch.name,
                    "group": ch.group,
                    "now": now_on.map(|p| json!({ "title": p.title, "ends_at": p.stop })),
                }));
                if rows.len() >= limit {
                    break;
                }
            }

            Ok(Ran {
                step: format!("checked the guide — {} channels", rows.len()),
                result: json!({ "items": rows }),
                items: Vec::new(),
            })
        }

        Tool::ShowTitles => {
            let wanted = call
                .args
                .get("items")
                .and_then(|i| i.as_array())
                .ok_or_else(|| "show_titles needs an items array.".to_string())?;

            let mut items = Vec::new();
            let mut refused = Vec::new();
            for entry in wanted.iter().take(MAX_ROWS as usize) {
                let kind = entry
                    .get("kind")
                    .and_then(|k| k.as_str())
                    .unwrap_or("movie");
                let Some(id) = entry.get("id").and_then(|i| i.as_i64()) else {
                    continue;
                };
                let note = entry
                    .get("note")
                    .and_then(|n| n.as_str())
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(str::to_string);

                // Resolved here, not trusted: an id the model invented, or one from a
                // row that has since been removed by a refresh, must not reach the UI as
                // a card with a play button that cannot work.
                match resolve_item(db, kind, id, note) {
                    Some(item) => items.push(item),
                    None => refused.push(json!({ "kind": kind, "id": id })),
                }
            }

            let shown = items.len();
            Ok(Ran {
                step: if shown == 1 {
                    "put one title in front of you".into()
                } else {
                    format!("put {shown} titles in front of you")
                },
                result: json!({
                    "shown": shown,
                    // Told rather than hidden, so the model can say "one of those has
                    // gone" instead of silently showing fewer than it meant to.
                    "not_in_library": refused,
                }),
                items,
            })
        }
    }
}

/// Turn a kind and an id into a card, or nothing if the row is not there.
fn resolve_item(db: &Connection, kind: &str, id: i64, note: Option<String>) -> Option<ChatItem> {
    match kind {
        "series" => library::series(db, id).ok().flatten().map(|s| ChatItem {
            kind: "series".into(),
            id: s.id,
            title: s.title,
            year: s.year,
            poster: s.poster,
            note,
        }),
        "live" => aurora_db::repo::channels::get(db, id)
            .ok()
            .flatten()
            .map(|c| ChatItem {
                kind: "live".into(),
                id: c.id,
                title: c.name,
                year: None,
                poster: c.logo,
                note,
            }),
        _ => library::movie(db, id).ok().flatten().map(|m| ChatItem {
            kind: "movie".into(),
            id: m.id,
            title: m.title,
            year: m.year,
            poster: m.poster,
            note,
        }),
    }
}

/* ── The system prompt ─────────────────────────────────────────────────────── */

/// Who the assistant is and what it must not do.
///
/// The three rules that matter, each because of what happens without it: it must check
/// the library before recommending (otherwise it suggests films this subscription does
/// not carry, which is the one thing a local recommender never gets wrong); it must call
/// `show_titles` (otherwise every answer is prose with nothing to press); and it must ask
/// rather than guess when the request is vague *and* the history is thin, which on a
/// fresh library is most of the time.
fn system_prompt(digest: &str) -> String {
    format!(
        "You are the assistant inside Aurora TV, a television app. You help one person \
decide what to watch from the library their IPTV subscription gives them.\n\n\
{digest}\n\n\
How to work:\n\
- Recommend only what they actually have. Check with search_library or browse_library \
before naming something. If you want to mention something they do not have, say plainly \
that it is not in their library.\n\
- Anything you want them to be able to play must go through show_titles, using ids from a \
previous lookup. Prose alone gives them nothing to press.\n\
- Give each card a short note saying why it suits THIS person — what they watched, or what \
they just asked for. Not a plot summary.\n\
- If what they asked for is vague and you have little history to go on, ask one short \
question instead of guessing. One question, not three.\n\
- Keep replies short. Two or three sentences around the cards, not an essay.\n\
- You can answer about their library itself — how many films they have, what is on now, \
whether they have a particular title. Those are useful questions.\n\
- Never claim to have started playback, changed a setting, or recorded anything. You can \
offer titles; the person presses play."
    )
}

/// A few facts about the library, in the system prompt, so an ordinary request does not
/// cost a round-trip just to find out what kind of library this is.
fn digest_line(db: &Connection) -> String {
    let Ok(stats) = library::stats(db) else {
        return "Their library could not be read just now.".into();
    };
    let filter = filtering::LibraryFilter::load(db).unwrap_or_default();
    let genres = library::genres(db, filtering::Kind::Movies, &filter).unwrap_or_default();
    let names: Vec<String> = genres.into_iter().take(14).map(|g| g.name).collect();

    let mut out = format!(
        "Their library right now: {} films, {} series, {} live channels.",
        stats.movies, stats.series, stats.channels
    );
    if !names.is_empty() {
        out.push_str(&format!(
            " The commonest film genres are: {}.",
            names.join(", ")
        ));
    }
    out.push_str(" Call library_digest for the full picture.");
    out
}

/* ── The loop ──────────────────────────────────────────────────────────────── */

fn load(db: &Connection, profile_id: i64) -> Stored {
    settings::get_in(db, &settings::profile_scope(profile_id), HISTORY_KEY)
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn save(db: &Connection, profile_id: i64, stored: &Stored) {
    let _ = settings::set_in(
        db,
        &settings::profile_scope(profile_id),
        HISTORY_KEY,
        stored,
    );
}

pub fn history(db: &Connection, profile_id: i64) -> Vec<ChatMessage> {
    load(db, profile_id).messages
}

pub fn clear(db: &Connection, profile_id: i64) {
    save(db, profile_id, &Stored::default());
}

/// The readable conversation, as turns the model is given.
fn turns_of(messages: &[ChatMessage]) -> Vec<Turn> {
    messages
        .iter()
        .map(|m| {
            if m.role == "user" {
                Turn::User {
                    text: m.text.clone(),
                }
            } else {
                // The cards go back as text, because what matters next turn is that it
                // already offered these — "not those, something lighter" has to resolve
                // against something.
                let mut text = m.text.clone();
                if !m.items.is_empty() {
                    let listed: Vec<String> = m
                        .items
                        .iter()
                        .map(|i| match i.year {
                            Some(y) => format!("{} ({y}) [{} {}]", i.title, i.kind, i.id),
                            None => format!("{} [{} {}]", i.title, i.kind, i.id),
                        })
                        .collect();
                    text.push_str(&format!("\n(Offered: {})", listed.join("; ")));
                }
                Turn::Model { text }
            }
        })
        .collect()
}

/// Ask the assistant something.
///
/// `chatter` is injected so the whole loop — the rounds, the bounds, the tool dispatch,
/// the card resolution — can be driven in a test by a scripted model, with no key and no
/// network.
/// How many times a turn is attempted before the message gives up.
///
/// Three, and only for failures the taxonomy calls retryable -- a quota, a 5xx, an empty
/// candidate list. A refused key or a rejected request is returned on the first attempt,
/// because asking again with the same key and the same payload cannot do anything except
/// cost another call.
const TURN_ATTEMPTS: usize = 3;

/// Between attempts. Short: somebody is watching a "thinking" indicator.
const TURN_BACKOFF: [u64; 2] = [1, 3];

fn turn_with_retries(
    chatter: &dyn Chatter,
    system: &str,
    working: &[Turn],
    tools: &[ToolDecl],
) -> std::result::Result<ChatReply, aurora_core::neterr::NetFailure> {
    let mut last = None;
    for attempt in 0..TURN_ATTEMPTS {
        if attempt > 0 {
            let wait = TURN_BACKOFF
                .get(attempt - 1)
                .copied()
                .unwrap_or(*TURN_BACKOFF.last().unwrap_or(&3));
            std::thread::sleep(std::time::Duration::from_secs(wait));
        }
        match chatter.turn(system, working, tools) {
            Ok(reply) => return Ok(reply),
            Err(e) => {
                if !e.retryable {
                    return Err(e);
                }
                tracing::warn!(
                    "assistant turn {} of {TURN_ATTEMPTS} failed: {} {}",
                    attempt + 1,
                    e.message,
                    e.cause
                );
                last = Some(e);
            }
        }
    }
    Err(last.expect("a failure to report after every attempt failed"))
}

pub fn send(
    db: &Connection,
    chatter: &dyn Chatter,
    profile_id: i64,
    text: &str,
    now: i64,
    mut on_step: impl FnMut(&str),
) -> Result<SendResult> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AppError::Other("Ask me something first.".into()));
    }

    let mut stored = load(db, profile_id);
    stored.messages.push(ChatMessage {
        role: "user".into(),
        text: text.to_string(),
        items: Vec::new(),
        at: now,
    });

    let system = system_prompt(&digest_line(db));
    let tools = declarations();
    let mut working = turns_of(&stored.messages);

    let mut steps: Vec<String> = Vec::new();
    let mut items: Vec<ChatItem> = Vec::new();
    let mut said: Vec<String> = Vec::new();
    let mut calls_made = 0usize;

    for round in 0..MAX_ROUNDS {
        // The model's own words for a failure, which `NetFailure` has already turned
        // into something a person can act on — a bad key, a quota, a safety stop.
        //
        // Retried here, and not only in the HTTP client, because the failures that most
        // often spoil a conversation are invisible from down there. A 200 whose body is
        // an `error` envelope, and a response with no candidate at all, both arrive as a
        // *successful* request: `send_with_retry` has already returned by the time
        // anything notices. Those are the ones this loop is for.
        let reply = match turn_with_retries(chatter, &system, &working, &tools) {
            Ok(reply) => reply,
            Err(e) => {
                let why = format!("{} {}", e.message, e.cause);
                // Work already done is not thrown away.
                //
                // A question that got as far as looking two things up and then lost the
                // connection can still be answered with what was found, and that is a
                // far better outcome than an error that discards it. With nothing found
                // and nothing said there is no answer to give, so the failure is the
                // answer — but that is the only case that still fails.
                if items.is_empty() && said.is_empty() {
                    return Err(AppError::Other(why));
                }
                tracing::warn!("answering with what was found after: {why}");
                steps.push("could not finish thinking about this one".into());
                break;
            }
        };

        if let Some(spoken) = reply.text.clone() {
            // Narration before a lookup is kept, but only as working text: the final
            // answer is what the viewer reads, so earlier lines are joined behind it.
            said.push(spoken);
        }

        if reply.is_final() {
            break;
        }

        for call in &reply.calls {
            if calls_made >= MAX_CALLS {
                // Told, not silently dropped: a model that asked for a fifteenth lookup
                // needs to know why it did not get one, or it asks again.
                working.push(Turn::ToolResult {
                    name: call.name.clone(),
                    result: json!({
                        "error": "No more lookups for this message. Answer with what you have."
                    }),
                });
                continue;
            }
            calls_made += 1;

            working.push(Turn::ToolCall {
                name: call.name.clone(),
                args: call.args.clone(),
            });

            match run_tool(db, profile_id, call, now) {
                Ok(ran) => {
                    on_step(&ran.step);
                    steps.push(ran.step);
                    items.extend(ran.items);
                    working.push(Turn::ToolResult {
                        name: call.name.clone(),
                        result: ran.result,
                    });
                }
                Err(why) => {
                    // A bad call is the model's mistake to recover from, not a failed
                    // message: it gets the complaint and another round.
                    working.push(Turn::ToolResult {
                        name: call.name.clone(),
                        result: json!({ "error": why }),
                    });
                }
            }
        }

        if round + 1 == MAX_ROUNDS {
            steps.push("ran out of lookups for this question".into());
        }
    }

    // De-duplicate the cards, keeping the first note for each: a model that calls
    // show_titles twice for the same film means it twice, not two cards.
    let mut seen = std::collections::HashSet::new();
    items.retain(|i| seen.insert((i.kind.clone(), i.id)));

    let spoken = said.join("\n\n").trim().to_string();
    let message = ChatMessage {
        role: "assistant".into(),
        text: if spoken.is_empty() {
            if items.is_empty() {
                "I could not think of anything for that — try asking another way.".to_string()
            } else {
                "Here is what I would watch.".to_string()
            }
        } else {
            spoken
        },
        items,
        at: now,
    };

    stored.messages.push(message.clone());
    if stored.messages.len() > KEEP_TURNS {
        let drop = stored.messages.len() - KEEP_TURNS;
        stored.messages.drain(0..drop);
    }
    save(db, profile_id, &stored);

    Ok(SendResult { message, steps })
}

/* ── Commands ──────────────────────────────────────────────────────────────── */

use tauri::State;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendArgs {
    pub profile_id: i64,
    pub text: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileArgs {
    pub profile_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantStatus {
    pub has_key: bool,
    pub model: String,
    /// How many exchanges are remembered, so the UI can offer "clear" only when there
    /// is something to clear.
    pub messages: usize,
}

#[tauri::command(async)]
pub fn assistant_status(
    services: State<'_, Services>,
    args: ProfileArgs,
) -> Result<AssistantStatus> {
    let db = services.db.lock();
    Ok(AssistantStatus {
        has_key: crate::gemini::key_available(&services),
        model: aurora_ingest::gemini::MODEL.to_string(),
        messages: history(&db, args.profile_id).len(),
    })
}

#[tauri::command(async)]
pub fn assistant_history(
    services: State<'_, Services>,
    args: ProfileArgs,
) -> Result<Vec<ChatMessage>> {
    let db = services.db.lock();
    Ok(history(&db, args.profile_id))
}

#[tauri::command(async)]
pub fn assistant_clear(services: State<'_, Services>, args: ProfileArgs) -> Result<()> {
    let db = services.db.lock();
    clear(&db, args.profile_id);
    Ok(())
}

/// Ask the assistant something.
///
/// The database lock is held for the whole call, which is the one thing here worth
/// questioning: a turn can take seconds. It is held because every tool reads through it
/// and a model is allowed to ask for six rounds of lookups — taking and dropping the lock
/// fourteen times would let a refresh land in the middle and answer half the questions
/// from a different library. A conversation is also something one person is waiting on, so
/// the contention is with themselves.
#[tauri::command(async)]
pub fn assistant_send(
    app: tauri::AppHandle,
    services: State<'_, Services>,
    args: SendArgs,
) -> Result<SendResult> {
    let key = crate::gemini::resolve_key(&services).ok_or_else(|| {
        AppError::Other("The assistant needs a Gemini key, which is set in Settings.".into())
    })?;

    let http = HttpClient::new(HttpConfig {
        max_attempts: 1,
        connect_timeout: std::time::Duration::from_secs(10),
        read_timeout: std::time::Duration::from_secs(90),
        ..Default::default()
    })
    .map_err(|e| AppError::Other(e.message))?;
    let chatter = ChatClient::new(&http, &key);

    let db = services.db.lock();
    send(
        &db,
        &chatter,
        args.profile_id,
        &args.text,
        crate::now_unix(),
        |step| {
            // Each lookup as it happens, so a long turn shows its working rather than a
            // spinner.
            crate::emit(&app, "assistant.step", &json!({ "step": step }));
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_ingest::chat::{ChatReply, ToolDecl};
    use std::cell::RefCell;

    /// A scripted model: each entry is one round's reply.
    struct Script {
        replies: RefCell<Vec<ChatReply>>,
        seen: RefCell<Vec<Vec<Turn>>>,
    }

    impl Script {
        fn new(replies: Vec<ChatReply>) -> Self {
            Self {
                replies: RefCell::new(replies),
                seen: RefCell::new(Vec::new()),
            }
        }
        fn text(s: &str) -> ChatReply {
            ChatReply {
                text: Some(s.into()),
                calls: Vec::new(),
            }
        }
        fn call(name: &str, args: serde_json::Value) -> ChatReply {
            ChatReply {
                text: None,
                calls: vec![ToolCall {
                    name: name.into(),
                    args,
                }],
            }
        }
    }

    impl Chatter for Script {
        fn turn(
            &self,
            _system: &str,
            history: &[Turn],
            _tools: &[ToolDecl],
        ) -> std::result::Result<ChatReply, aurora_core::neterr::NetFailure> {
            self.seen.borrow_mut().push(history.to_vec());
            let mut replies = self.replies.borrow_mut();
            if replies.is_empty() {
                return Ok(Script::text("(nothing more)"));
            }
            Ok(replies.remove(0))
        }
    }

    const NOW: i64 = 1_760_000_000;

    /// A model that fails a given number of times and then behaves.
    struct Flaky {
        left: RefCell<usize>,
        retryable: bool,
        then: RefCell<Vec<ChatReply>>,
    }

    fn failure(retryable: bool) -> aurora_core::neterr::NetFailure {
        aurora_core::neterr::NetFailure {
            code: aurora_core::neterr::ErrorCode::Unknown,
            message: "The assistant is busy".into(),
            cause: "try again".into(),
            actions: Vec::new(),
            retryable,
        }
    }

    impl Chatter for Flaky {
        fn turn(
            &self,
            _system: &str,
            _history: &[Turn],
            _tools: &[ToolDecl],
        ) -> std::result::Result<ChatReply, aurora_core::neterr::NetFailure> {
            let mut left = self.left.borrow_mut();
            if *left > 0 {
                *left -= 1;
                return Err(failure(self.retryable));
            }
            let mut then = self.then.borrow_mut();
            if then.is_empty() {
                return Err(failure(self.retryable));
            }
            Ok(then.remove(0))
        }
    }

    /// A retryable failure is a hiccup, not an answer. The HTTP client cannot see these
    /// -- a 200 carrying an `error` envelope, or a reply with no candidate, is a
    /// *successful* request by the time it gets there -- so the retry has to be here.
    #[test]
    fn a_retryable_failure_is_tried_again_rather_than_reported() {
        let db = db();
        let chatter = Flaky {
            left: RefCell::new(1),
            retryable: true,
            then: RefCell::new(vec![Script::text("Got there in the end.")]),
        };
        let out = send(&db, &chatter, 1, "anything", NOW, |_| {}).unwrap();
        assert_eq!(out.message.text, "Got there in the end.");
    }

    /// And a refused key is not: asking again with the same key costs a call and cannot
    /// succeed.
    #[test]
    fn a_failure_that_cannot_improve_is_not_retried() {
        let db = db();
        let chatter = Flaky {
            left: RefCell::new(1),
            retryable: false,
            then: RefCell::new(vec![Script::text("never reached")]),
        };
        assert!(send(&db, &chatter, 1, "anything", NOW, |_| {}).is_err());
    }

    /// The commonest shape of "the assistant gives me errors": it found things, then the
    /// next round failed, and the whole message was discarded along with the work.
    #[test]
    fn work_already_done_is_answered_with_rather_than_thrown_away() {
        let mut db = db();
        let id = seed_movie(&mut db, "Arrival", 2016, "Drama");
        let chatter = Flaky {
            left: RefCell::new(0),
            // Not retryable, so the second round fails once and immediately.
            retryable: false,
            then: RefCell::new(vec![Script::call(
                "show_titles",
                json!({ "items": [{ "kind": "movie", "id": id, "note": "Tense." }] }),
            )]),
        };

        let out = send(&db, &chatter, 1, "something tense", NOW, |_| {}).unwrap();
        assert_eq!(out.message.items.len(), 1, "the card it had already found");
        assert_eq!(out.message.items[0].title, "Arrival");
        assert!(
            out.steps.iter().any(|s| s.contains("could not finish")),
            "the viewer is told it was cut short: {:?}",
            out.steps
        );
    }

    /// With nothing found and nothing said there is no answer to give, so that one case
    /// still fails -- and says why.
    #[test]
    fn a_failure_with_nothing_to_show_is_still_a_failure() {
        let db = db();
        let chatter = Flaky {
            left: RefCell::new(1),
            retryable: false,
            then: RefCell::new(Vec::new()),
        };
        let err = send(&db, &chatter, 1, "anything", NOW, |_| {})
            .unwrap_err()
            .to_string();
        assert!(err.contains("busy") || err.contains("try again"), "{err}");
    }

    fn db() -> Connection {
        let db = aurora_db::open_memory().unwrap();
        db.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','xtream','https://example.com',0)",
            [],
        )
        .unwrap();
        db
    }

    fn seed_movie(db: &mut Connection, title: &str, year: i32, genres: &str) -> i64 {
        aurora_db::repo::library::upsert_movies(
            db,
            1,
            &[library::NewMovie {
                provider_key: title.into(),
                title: title.into(),
                match_key: aurora_core::title::match_key(title),
                url: format!("http://example.com/{title}.mkv"),
                year: Some(year),
                ..Default::default()
            }],
            0,
        )
        .unwrap();
        let id: i64 = db
            .query_row("SELECT id FROM movies WHERE title = ?1", [title], |r| {
                r.get(0)
            })
            .unwrap();
        db.execute(
            "UPDATE movies SET genres = ?1 WHERE id = ?2",
            aurora_db::rusqlite::params![genres, id],
        )
        .unwrap();
        aurora_db::repo::search::index(db, &[("movie".into(), id, title.into(), None)]).ok();
        id
    }

    #[test]
    fn a_plain_answer_is_stored_as_the_conversation() {
        let db = db();
        let script = Script::new(vec![Script::text("Try something short tonight.")]);

        let out = send(&db, &script, 1, "what should I watch?", NOW, |_| {}).unwrap();
        assert_eq!(out.message.role, "assistant");
        assert_eq!(out.message.text, "Try something short tonight.");
        assert!(out.message.items.is_empty());

        // Both halves are remembered, in order.
        let stored = history(&db, 1);
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].role, "user");
        assert_eq!(stored[0].text, "what should I watch?");
        assert_eq!(stored[1].role, "assistant");
    }

    #[test]
    fn an_empty_question_is_refused_without_asking_the_model() {
        let db = db();
        let script = Script::new(vec![Script::text("should not be reached")]);
        assert!(send(&db, &script, 1, "   ", NOW, |_| {}).is_err());
        assert!(history(&db, 1).is_empty());
        assert!(
            script.seen.borrow().is_empty(),
            "the model was asked anyway"
        );
    }

    /// The whole point: a lookup, then cards the viewer can play.
    #[test]
    fn a_search_then_show_titles_produces_playable_cards() {
        let mut db = db();
        let id = seed_movie(&mut db, "Arrival", 2016, r#"["Science Fiction"]"#);

        let script = Script::new(vec![
            Script::call("search_library", json!({ "query": "Arrival" })),
            Script::call(
                "show_titles",
                json!({ "items": [{ "kind": "movie", "id": id, "note": "Slow and tense." }] }),
            ),
            Script::text("This one fits."),
        ]);

        let mut steps = Vec::new();
        let out = send(&db, &script, 1, "something tense", NOW, |s| {
            steps.push(s.to_string())
        })
        .unwrap();

        assert_eq!(out.message.items.len(), 1);
        let card = &out.message.items[0];
        assert_eq!(card.id, id);
        assert_eq!(card.title, "Arrival");
        assert_eq!(card.year, Some(2016));
        assert_eq!(card.note.as_deref(), Some("Slow and tense."));
        assert_eq!(out.message.text, "This one fits.");

        // The working is reported, and reported as it happens.
        assert_eq!(steps, out.steps);
        assert!(steps[0].contains("searched your library"), "{steps:?}");
        assert!(steps[1].contains("put one title"), "{steps:?}");
    }

    /// A card is resolved against the library, so an id the model invented cannot become
    /// a play button that does nothing.
    #[test]
    fn an_invented_id_does_not_become_a_card() {
        let db = db();
        let script = Script::new(vec![
            Script::call(
                "show_titles",
                json!({ "items": [{ "kind": "movie", "id": 9999, "note": "Trust me." }] }),
            ),
            Script::text("Here you go."),
        ]);

        let out = send(&db, &script, 1, "anything", NOW, |_| {}).unwrap();
        assert!(out.message.items.is_empty());

        // And the model is told, so it can correct itself rather than claiming it showed
        // something.
        let last = script.seen.borrow().last().cloned().unwrap();
        let result = last.iter().rev().find_map(|t| match t {
            Turn::ToolResult { name, result } if name == "show_titles" => Some(result.clone()),
            _ => None,
        });
        let result = result.expect("the show_titles result");
        assert_eq!(result["shown"], 0);
        assert_eq!(result["not_in_library"][0]["id"], 9999);
    }

    #[test]
    fn the_same_title_twice_is_one_card() {
        let mut db = db();
        let id = seed_movie(&mut db, "Heat", 1995, r#"["Crime"]"#);
        let script = Script::new(vec![
            Script::call(
                "show_titles",
                json!({ "items": [{ "kind": "movie", "id": id }] }),
            ),
            Script::call(
                "show_titles",
                json!({ "items": [{ "kind": "movie", "id": id, "note": "again" }] }),
            ),
            Script::text("Done."),
        ]);
        let out = send(&db, &script, 1, "crime", NOW, |_| {}).unwrap();
        assert_eq!(out.message.items.len(), 1);
    }

    /// A model that will not stop asking has to be stopped, and told why.
    #[test]
    fn the_rounds_are_bounded() {
        let db = db();
        let script = Script::new(
            (0..MAX_ROUNDS + 4)
                .map(|_| Script::call("library_digest", json!({})))
                .collect(),
        );

        let out = send(&db, &script, 1, "hello", NOW, |_| {}).unwrap();
        // One round per ask, no more.
        assert_eq!(script.seen.borrow().len(), MAX_ROUNDS);
        assert!(
            out.steps.iter().any(|s| s.contains("ran out of lookups")),
            "{:?}",
            out.steps
        );
    }

    #[test]
    fn the_calls_per_message_are_bounded_and_the_model_is_told() {
        let db = db();
        // One round asking for far more calls than the budget allows.
        let many: Vec<ToolCall> = (0..MAX_CALLS + 3)
            .map(|_| ToolCall {
                name: "library_digest".into(),
                args: json!({}),
            })
            .collect();
        let script = Script::new(vec![
            ChatReply {
                text: None,
                calls: many,
            },
            Script::text("Fine."),
        ]);

        let out = send(&db, &script, 1, "hello", NOW, |_| {}).unwrap();
        assert_eq!(out.steps.len(), MAX_CALLS, "{:?}", out.steps);

        let last = script.seen.borrow().last().cloned().unwrap();
        let refusals = last
            .iter()
            .filter(|t| match t {
                Turn::ToolResult { result, .. } => result
                    .get("error")
                    .and_then(|e| e.as_str())
                    .map(|e| e.contains("No more lookups"))
                    .unwrap_or(false),
                _ => false,
            })
            .count();
        assert!(refusals >= 3, "the model was not told it had run out");
    }

    /// A tool called wrongly is the model's problem to recover from, not a failed reply.
    #[test]
    fn a_bad_tool_call_is_handed_back_rather_than_failing_the_message() {
        let db = db();
        let script = Script::new(vec![
            Script::call("search_library", json!({})), // no query
            Script::call("no_such_tool", json!({})),
            Script::text("Sorry, let me ask differently."),
        ]);

        let out = send(&db, &script, 1, "hello", NOW, |_| {}).unwrap();
        assert_eq!(out.message.text, "Sorry, let me ask differently.");

        let last = script.seen.borrow().last().cloned().unwrap();
        let errors: Vec<String> = last
            .iter()
            .filter_map(|t| match t {
                Turn::ToolResult { result, .. } => result
                    .get("error")
                    .and_then(|e| e.as_str())
                    .map(str::to_string),
                _ => None,
            })
            .collect();
        assert!(
            errors.iter().any(|e| e.contains("needs a query")),
            "{errors:?}"
        );
        assert!(
            errors.iter().any(|e| e.contains("no tool called")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_row_limit_is_clamped_however_the_model_asks() {
        let mut db = db();
        for i in 0..40 {
            seed_movie(&mut db, &format!("Film {i}"), 2000 + i, r#"["Drama"]"#);
        }
        let script = Script::new(vec![
            Script::call("browse_library", json!({ "kind": "movie", "limit": 500 })),
            Script::text("Lots."),
        ]);
        send(&db, &script, 1, "films", NOW, |_| {}).unwrap();

        let last = script.seen.borrow().last().cloned().unwrap();
        let rows = last
            .iter()
            .find_map(|t| match t {
                Turn::ToolResult { name, result } if name == "browse_library" => {
                    Some(result["items"].as_array().unwrap().len())
                }
                _ => None,
            })
            .unwrap();
        assert!(rows as u32 <= MAX_ROWS, "{rows} rows came back");
    }

    /// Nothing a tool returns may carry a stream URL: the request goes to Google.
    #[test]
    fn no_tool_result_contains_a_stream_url() {
        let mut db = db();
        let id = seed_movie(&mut db, "Arrival", 2016, r#"["Science Fiction"]"#);
        let script = Script::new(vec![
            Script::call("search_library", json!({ "query": "Arrival" })),
            Script::call("browse_library", json!({ "kind": "movie" })),
            Script::call("title_details", json!({ "kind": "movie", "id": id })),
            Script::call("library_digest", json!({})),
            Script::call("watch_history", json!({})),
            Script::text("Done."),
        ]);
        send(&db, &script, 1, "anything", NOW, |_| {}).unwrap();

        for history in script.seen.borrow().iter() {
            for turn in history {
                if let Turn::ToolResult { name, result } = turn {
                    let text = result.to_string();
                    assert!(
                        !text.contains("example.com") && !text.contains("http"),
                        "{name} leaked a URL: {text}"
                    );
                }
            }
        }
    }

    /// What it offered last time has to come back as context, or "not those" means
    /// nothing.
    #[test]
    fn the_next_turn_knows_what_was_offered() {
        let mut db = db();
        let id = seed_movie(&mut db, "Heat", 1995, r#"["Crime"]"#);

        let first = Script::new(vec![
            Script::call(
                "show_titles",
                json!({ "items": [{ "kind": "movie", "id": id }] }),
            ),
            Script::text("Try this."),
        ]);
        send(&db, &first, 1, "crime", NOW, |_| {}).unwrap();

        let second = Script::new(vec![Script::text("All right, something else.")]);
        send(&db, &second, 1, "not that one", NOW + 60, |_| {}).unwrap();

        let given = second.seen.borrow()[0].clone();
        let model_turn = given
            .iter()
            .find_map(|t| match t {
                Turn::Model { text } => Some(text.clone()),
                _ => None,
            })
            .expect("the previous answer");
        assert!(model_turn.contains("Heat"), "{model_turn}");
        assert!(model_turn.contains("movie"), "{model_turn}");
    }

    #[test]
    fn history_is_per_profile_and_clearable() {
        let db = db();
        send(
            &db,
            &Script::new(vec![Script::text("a")]),
            1,
            "one",
            NOW,
            |_| {},
        )
        .unwrap();
        send(
            &db,
            &Script::new(vec![Script::text("b")]),
            2,
            "two",
            NOW,
            |_| {},
        )
        .unwrap();

        assert_eq!(history(&db, 1).len(), 2);
        assert_eq!(history(&db, 2).len(), 2);
        assert_eq!(history(&db, 1)[0].text, "one");

        clear(&db, 1);
        assert!(history(&db, 1).is_empty());
        assert_eq!(
            history(&db, 2).len(),
            2,
            "the other profile was cleared too"
        );
    }

    #[test]
    fn the_conversation_is_trimmed_rather_than_growing_for_ever() {
        let db = db();
        for i in 0..KEEP_TURNS {
            send(
                &db,
                &Script::new(vec![Script::text("ok")]),
                1,
                &format!("question {i}"),
                NOW + i as i64,
                |_| {},
            )
            .unwrap();
        }
        let stored = history(&db, 1);
        assert_eq!(stored.len(), KEEP_TURNS);
        // The oldest went, the newest stayed.
        assert!(stored.first().unwrap().text != "question 0");
        assert_eq!(
            stored[stored.len() - 2].text,
            format!("question {}", KEEP_TURNS - 1)
        );
    }

    /// A model that says nothing at all still has to produce a message, or the UI shows
    /// an empty bubble.
    #[test]
    fn a_silent_model_still_produces_something_readable() {
        let db = db();
        let script = Script::new(vec![ChatReply {
            text: None,
            calls: Vec::new(),
        }]);
        let out = send(&db, &script, 1, "hello", NOW, |_| {}).unwrap();
        assert!(!out.message.text.is_empty());
        assert!(
            out.message.text.contains("could not think"),
            "{}",
            out.message.text
        );
    }

    #[test]
    fn the_system_prompt_describes_this_library() {
        let mut db = db();
        seed_movie(&mut db, "Arrival", 2016, r#"["Science Fiction"]"#);
        let digest = digest_line(&db);
        assert!(digest.contains("1 films"), "{digest}");
        assert!(digest.contains("Science Fiction"), "{digest}");

        let prompt = system_prompt(&digest);
        assert!(
            prompt.contains("show_titles"),
            "the rule about cards is missing"
        );
        assert!(prompt.contains("only what they actually have"));
        assert!(prompt.contains("Never claim to have started playback"));
    }

    #[test]
    fn every_tool_is_declared_and_every_declaration_dispatches() {
        let declared: Vec<&str> = declarations().iter().map(|d| d.name).collect();
        assert_eq!(declared.len(), Tool::ALL.len());
        for name in declared {
            assert!(
                Tool::parse(name).is_some(),
                "{name} is declared and unknown"
            );
        }
    }
}
