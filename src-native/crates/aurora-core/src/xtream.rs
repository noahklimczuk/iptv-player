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

fn lenient_string<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<String>, D::Error> {
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
        serde_json::Value::Array(_) => {
            Ok(serde_json::from_value(v).unwrap_or_default())
        }
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
            "provider returned HTML instead of JSON (bad credentials, or the panel is down)"
                .into(),
        ));
    }
    if trimmed.is_empty() {
        return Err(CoreError::Provider("provider returned an empty body".into()));
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
            stream_url("https://example.com", "u", "p", MediaKind::Movie, 3, Some("mkv")),
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
}
