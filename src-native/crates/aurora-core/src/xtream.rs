//! Xtream Codes API response models and normalization.
//!
//! README §4.1: "Handle the many non-standard Xtream forks gracefully: missing fields,
//! string-vs-number type inconsistency, `null` where an array is expected, HTML error pages
//! returned with HTTP 200." Every numeric field here therefore goes through a lenient
//! deserializer, and the top-level entry point rejects non-JSON bodies with a useful message.

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{CoreError, Result};
use crate::model::MediaKind;

/// Accepts `5`, `"5"`, `""`, `null`, and `5.0` — all of which appear in the wild.
fn lenient_u32<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<u32>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Number(n) => n.as_f64().map(|f| f as u32),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok().map(|f| f as u32),
        _ => None,
    })
}

fn lenient_f32<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<f32>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Number(n) => n.as_f64().map(|f| f as f32),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    })
}

fn lenient_string<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<String>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(s) if !s.trim().is_empty() => Some(s),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

/// `null` instead of `[]` is extremely common.
fn lenient_vec<'de, D, T>(d: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Array(_) => Ok(serde_json::from_value(v).unwrap_or_default()),
        _ => Ok(Vec::new()),
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct UserInfo {
    #[serde(default, deserialize_with = "lenient_string")]
    pub username: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub status: Option<String>,
    /// Unix seconds. `null` means a non-expiring line.
    #[serde(default, deserialize_with = "lenient_u32")]
    pub exp_date: Option<u32>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub max_connections: Option<u32>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub active_cons: Option<u32>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub is_trial: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ServerInfo {
    #[serde(default, deserialize_with = "lenient_string")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub port: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub https_port: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub server_protocol: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AuthResponse {
    #[serde(default)]
    pub user_info: UserInfo,
    #[serde(default)]
    pub server_info: ServerInfo,
}

impl AuthResponse {
    pub fn is_active(&self) -> bool {
        self.user_info
            .status
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("Active"))
            .unwrap_or(false)
    }

    /// Days until the line expires, if it expires at all.
    pub fn days_until_expiry(&self, now_unix: i64) -> Option<i64> {
        let exp = self.user_info.exp_date? as i64;
        Some((exp - now_unix).div_euclid(86_400))
    }

    /// README §4.1: refuse to open more streams than the line allows.
    pub fn connection_slots_free(&self) -> Option<u32> {
        let max = self.user_info.max_connections?;
        let active = self.user_info.active_cons.unwrap_or(0);
        Some(max.saturating_sub(active))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Category {
    #[serde(default, deserialize_with = "lenient_string")]
    pub category_id: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub category_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveStream {
    #[serde(default, deserialize_with = "lenient_u32")]
    pub stream_id: Option<u32>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub stream_icon: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub epg_channel_id: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub category_id: Option<String>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub num: Option<u32>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub tv_archive: Option<u32>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub tv_archive_duration: Option<u32>,
}

impl LiveStream {
    pub fn has_catchup(&self) -> bool {
        self.tv_archive.unwrap_or(0) > 0
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VodStream {
    #[serde(default, deserialize_with = "lenient_u32")]
    pub stream_id: Option<u32>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub stream_icon: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub category_id: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub container_extension: Option<String>,
    #[serde(default, deserialize_with = "lenient_f32")]
    pub rating: Option<f32>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub added: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SeriesListing {
    #[serde(default, deserialize_with = "lenient_u32")]
    pub series_id: Option<u32>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub cover: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub plot: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub cast: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub category_id: Option<String>,
    #[serde(
        default,
        rename = "releaseDate",
        alias = "release_date",
        deserialize_with = "lenient_string"
    )]
    pub release_date: Option<String>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub backdrop_path: Vec<String>,
}

/// `get_series_info` for one show: the seasons and their episodes.
///
/// Panels key `episodes` by season number *as a string* — `{"1": [...], "2": [...]}` —
/// and a few write a JSON array instead when the seasons happen to be contiguous. Both
/// are accepted, because the alternative is a show that looks empty on one panel and
/// full on another for a reason no viewer could ever guess at.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SeriesInfo {
    #[serde(default, deserialize_with = "lenient_episode_map")]
    pub episodes: Vec<EpisodeListing>,
}

/// One episode as a panel lists it.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EpisodeListing {
    /// The stream id, which is what the playable URL is built from. A string on most
    /// panels, a number on some.
    #[serde(default, deserialize_with = "lenient_string")]
    pub id: Option<String>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub episode_num: Option<u32>,
    /// Absent on panels that only key the season in the map; filled in from that key.
    #[serde(default, deserialize_with = "lenient_u32")]
    pub season: Option<u32>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub container_extension: Option<String>,
    #[serde(default)]
    pub info: EpisodeInfoDetail,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EpisodeInfoDetail {
    #[serde(default, deserialize_with = "lenient_string")]
    pub plot: Option<String>,
    /// The still. Panels disagree about the name, and a show whose episodes have no
    /// artwork looks broken next to one whose do.
    #[serde(
        default,
        alias = "movie_image",
        alias = "cover_big",
        deserialize_with = "lenient_string"
    )]
    pub movie_image: Option<String>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub duration_secs: Option<u32>,
}

/// Flatten `{"1": [...], "2": [...]}` — or `[[...], [...]]` — into one list, carrying
/// the season down from the key when the episode itself does not say.
fn lenient_episode_map<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<EpisodeListing>, D::Error> {
    use serde::de::Error as _;
    let raw = serde_json::Value::deserialize(d)?;
    let mut out = Vec::new();

    let mut take = |season_key: Option<&str>, value: &serde_json::Value| {
        let Some(list) = value.as_array() else { return };
        for item in list {
            let Ok(mut ep) = serde_json::from_value::<EpisodeListing>(item.clone()) else {
                // One malformed episode is not a reason to lose the other twenty-two.
                continue;
            };
            if ep.season.is_none() {
                ep.season = season_key.and_then(|k| k.trim().parse::<u32>().ok());
            }
            out.push(ep);
        }
    };

    match raw {
        serde_json::Value::Object(map) => {
            for (key, value) in &map {
                take(Some(key), value);
            }
        }
        // An array of seasons: the index is the season, and panels that do this start
        // at zero for "specials" exactly as the keyed form does.
        serde_json::Value::Array(seasons) => {
            for (i, value) in seasons.iter().enumerate() {
                take(Some(&i.to_string()), value);
            }
        }
        // `"episodes": []` on a show with none, and `null` on a panel that writes the
        // key regardless. Neither is an error.
        serde_json::Value::Null => {}
        other => {
            return Err(D::Error::custom(format!(
                "episodes was neither an object nor an array: {other}"
            )))
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct VodInfoDetail {
    #[serde(default, deserialize_with = "lenient_string")]
    pub plot: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub cast: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub director: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub genre: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub releasedate: Option<String>,
    #[serde(default, deserialize_with = "lenient_f32")]
    pub rating: Option<f32>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub duration: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub tmdb_id: Option<String>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub backdrop_path: Vec<String>,
}

/// Parse a `player_api.php` body, turning the classic "HTML error page with HTTP 200" into a
/// readable error instead of a serde panic-adjacent mess.
pub fn parse_json<T: serde::de::DeserializeOwned>(body: &str) -> Result<T> {
    let trimmed = body.trim_start();
    if trimmed.starts_with('<') {
        return Err(CoreError::Provider(
            "provider returned HTML instead of JSON (bad credentials, or the panel is down)".into(),
        ));
    }
    if trimmed.is_empty() {
        return Err(CoreError::Provider(
            "provider returned an empty body".into(),
        ));
    }
    serde_json::from_str(trimmed).map_err(|e| CoreError::Provider(format!("invalid JSON: {e}")))
}

/// Build the playable URL for an Xtream stream.
pub fn stream_url(
    base: &str,
    username: &str,
    password: &str,
    kind: MediaKind,
    stream_id: u32,
    extension: Option<&str>,
) -> String {
    let base = base.trim_end_matches('/');
    match kind {
        MediaKind::Live => format!("{base}/live/{username}/{password}/{stream_id}.ts"),
        MediaKind::Movie => format!(
            "{base}/movie/{username}/{password}/{stream_id}.{}",
            extension.unwrap_or("mp4")
        ),
        MediaKind::Episode => format!(
            "{base}/series/{username}/{password}/{stream_id}.{}",
            extension.unwrap_or("mp4")
        ),
    }
}

/// Build a catch-up (timeshift) URL. `start` is `YYYY-MM-DD:HH-MM`, duration in minutes.
pub fn timeshift_url(
    base: &str,
    username: &str,
    password: &str,
    stream_id: u32,
    start: &str,
    duration_min: u32,
) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/streaming/timeshift.php?username={username}&password={password}&stream={stream_id}&start={start}&duration={duration_min}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_as_strings_are_accepted() {
        let j = r#"{"user_info":{"status":"Active","exp_date":"1735689600","max_connections":"2","active_cons":1}}"#;
        let a: AuthResponse = parse_json(j).unwrap();
        assert!(a.is_active());
        assert_eq!(a.user_info.max_connections, Some(2));
        assert_eq!(a.user_info.active_cons, Some(1));
        assert_eq!(a.connection_slots_free(), Some(1));
    }

    #[test]
    fn missing_and_null_fields_do_not_fail() {
        let a: AuthResponse = parse_json(r#"{"user_info":{"status":null}}"#).unwrap();
        assert!(!a.is_active());
        assert_eq!(a.connection_slots_free(), None);
        assert_eq!(a.days_until_expiry(0), None);
    }

    #[test]
    fn html_error_pages_produce_a_useful_error() {
        let err = parse_json::<AuthResponse>("<html><body>403</body></html>").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("HTML"), "unhelpful message: {msg}");
    }

    #[test]
    fn empty_bodies_produce_a_useful_error() {
        assert!(parse_json::<AuthResponse>("   ").is_err());
    }

    #[test]
    fn expiry_is_reported_in_days() {
        let j = r#"{"user_info":{"status":"Active","exp_date":864000}}"#;
        let a: AuthResponse = parse_json(j).unwrap();
        assert_eq!(a.days_until_expiry(0), Some(10));
        // Already expired reads negative rather than underflowing.
        assert_eq!(a.days_until_expiry(864_000 + 86_400), Some(-1));
    }

    #[test]
    fn active_connections_above_the_cap_clamp_to_zero() {
        let j = r#"{"user_info":{"max_connections":1,"active_cons":5}}"#;
        let a: AuthResponse = parse_json(j).unwrap();
        assert_eq!(a.connection_slots_free(), Some(0));
    }

    #[test]
    fn null_arrays_become_empty_vecs() {
        let s: SeriesListing =
            parse_json(r#"{"series_id":1,"name":"Show","backdrop_path":null}"#).unwrap();
        assert!(s.backdrop_path.is_empty());
    }

    #[test]
    fn live_streams_report_catchup() {
        let s: LiveStream = parse_json(r#"{"stream_id":7,"tv_archive":1}"#).unwrap();
        assert!(s.has_catchup());
        let s2: LiveStream = parse_json(r#"{"stream_id":7,"tv_archive":0}"#).unwrap();
        assert!(!s2.has_catchup());
    }

    #[test]
    fn builds_stream_urls_per_kind() {
        assert_eq!(
            stream_url("https://example.com/", "u", "p", MediaKind::Live, 12, None),
            "https://example.com/live/u/p/12.ts"
        );
        assert_eq!(
            stream_url(
                "https://example.com",
                "u",
                "p",
                MediaKind::Movie,
                3,
                Some("mkv")
            ),
            "https://example.com/movie/u/p/3.mkv"
        );
        assert_eq!(
            stream_url("https://example.com", "u", "p", MediaKind::Episode, 9, None),
            "https://example.com/series/u/p/9.mp4"
        );
    }

    #[test]
    fn builds_a_timeshift_url() {
        let u = timeshift_url("https://example.com", "u", "p", 5, "2024-01-15:20-00", 60);
        assert!(u.contains("stream=5"));
        assert!(u.contains("duration=60"));
    }

    #[test]
    fn rating_accepts_both_string_and_number() {
        let a: VodStream = parse_json(r#"{"stream_id":1,"rating":"7.4"}"#).unwrap();
        let b: VodStream = parse_json(r#"{"stream_id":1,"rating":7.4}"#).unwrap();
        assert_eq!(a.rating, b.rating);
    }

    /// The shape every panel probed actually sends: seasons as string keys, ids as
    /// strings, and the season number present only in the key.
    #[test]
    fn series_info_flattens_a_season_keyed_map() {
        let info: SeriesInfo = parse_json(
            r#"{"seasons":[],"info":{},"episodes":{
                 "1":[{"id":"501","episode_num":1,"title":"Pilot",
                       "container_extension":"mkv",
                       "info":{"movie_image":"http://e.com/1.jpg","duration_secs":2700}},
                      {"id":"502","episode_num":2,"title":"Second",
                       "container_extension":"mkv","info":{}}],
                 "2":[{"id":"601","episode_num":1,"title":"Return",
                       "container_extension":"mp4","info":{}}]}}"#,
        )
        .unwrap();

        assert_eq!(info.episodes.len(), 3);
        let mut seen: Vec<(u32, u32)> = info
            .episodes
            .iter()
            .map(|e| (e.season.unwrap(), e.episode_num.unwrap()))
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![(1, 1), (1, 2), (2, 1)]);

        let pilot = info
            .episodes
            .iter()
            .find(|e| e.title.as_deref() == Some("Pilot"))
            .unwrap();
        assert_eq!(pilot.id.as_deref(), Some("501"));
        assert_eq!(pilot.container_extension.as_deref(), Some("mkv"));
        assert_eq!(
            pilot.info.movie_image.as_deref(),
            Some("http://e.com/1.jpg")
        );
        assert_eq!(pilot.info.duration_secs, Some(2700));
    }

    /// Some panels send an array of seasons instead of a map. The index is the season.
    #[test]
    fn series_info_accepts_an_array_of_seasons_too() {
        let info: SeriesInfo = parse_json(
            r#"{"episodes":[[],[{"id":7,"episode_num":"3","title":"Third","info":{}}]]}"#,
        )
        .unwrap();
        assert_eq!(info.episodes.len(), 1);
        let ep = &info.episodes[0];
        // Index 1 of the array is season 1, and both a numeric id and a string
        // episode number survive the trip.
        assert_eq!(ep.season, Some(1));
        assert_eq!(ep.episode_num, Some(3));
        assert_eq!(ep.id.as_deref(), Some("7"));
    }

    /// A show with no episodes is a show with no episodes, not a failed import — and
    /// one malformed entry must not take the rest of the season with it.
    #[test]
    fn series_info_survives_nothing_and_nonsense() {
        let empty: SeriesInfo = parse_json(r#"{"episodes":{}}"#).unwrap();
        assert!(empty.episodes.is_empty());

        let null: SeriesInfo = parse_json(r#"{"episodes":null}"#).unwrap();
        assert!(null.episodes.is_empty());

        let missing: SeriesInfo = parse_json(r#"{"info":{}}"#).unwrap();
        assert!(missing.episodes.is_empty());

        // A bare string and a number where an object belongs: neither can be read as
        // an episode, and neither is allowed to cost the one that can.
        let partly: SeriesInfo =
            parse_json(r#"{"episodes":{"1":[{"id":"1","episode_num":1,"info":{}},"nope",42]}}"#)
                .unwrap();
        assert_eq!(partly.episodes.len(), 1, "the good episode survived");
    }

    /// An episode that carries its own season disagrees with nothing.
    #[test]
    fn an_episode_that_names_its_own_season_keeps_it() {
        let info: SeriesInfo =
            parse_json(r#"{"episodes":{"1":[{"id":"9","episode_num":4,"season":3,"info":{}}]}}"#)
                .unwrap();
        assert_eq!(info.episodes[0].season, Some(3));
    }
}
