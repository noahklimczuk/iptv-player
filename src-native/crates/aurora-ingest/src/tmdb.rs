//! The TMDB client (README §4.5).
//!
//! Search, details and artwork. The *choosing* lives in `aurora_core::tmdb`; this only
//! fetches and translates, so the part that can be subtly wrong stays testable without
//! a network or a key.
//!
//! The API key is a secret: it goes to the credential store, never to the database, and
//! `http::redact` strips it from anything logged.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure};
use aurora_core::tmdb::{Candidate, Credit, Metadata, Person};
use serde::Deserialize;

use crate::http::HttpClient;

pub const DEFAULT_BASE_URL: &str = "https://api.themoviedb.org/3";
pub const DEFAULT_IMAGE_BASE_URL: &str = "https://image.tmdb.org/t/p";

/// Credential-store key for the metadata API key.
pub const CREDENTIAL_KEY: &str = "aurora-tmdb-api-key";

/// Smallest gap between requests. TMDB's published ceiling is far higher, but a 40,000
/// title library would hammer it, and being rate-limited mid-import costs more than
/// going a little slower does.
pub const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(25);

/// Poster and backdrop widths, matching what the UI actually renders (README §12).
/// Fetching `original` for a 342px card wastes bandwidth and disk for no visible gain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSize {
    Poster,
    PosterLarge,
    Backdrop,
    Logo,
    Profile,
}

impl ImageSize {
    pub fn as_path(self) -> &'static str {
        match self {
            ImageSize::Poster => "w342",
            ImageSize::PosterLarge => "w500",
            ImageSize::Backdrop => "w1280",
            ImageSize::Logo => "w300",
            ImageSize::Profile => "w185",
        }
    }
}

/// What a title is, for the endpoints that differ between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Movie,
    Series,
}

impl Kind {
    fn path(self) -> &'static str {
        match self {
            Kind::Movie => "movie",
            // TMDB calls series "tv" throughout its API.
            Kind::Series => "tv",
        }
    }
}

/// The seam enrichment is written against, so the pipeline can be tested without a key.
pub trait MetadataClient: Send + Sync {
    fn search(
        &self,
        kind: Kind,
        title: &str,
        year: Option<i32>,
    ) -> Result<Vec<Candidate>, NetFailure>;
    fn details(&self, kind: Kind, id: i64) -> Result<Metadata, NetFailure>;
    /// Absolute URL for an artwork path, or `None` when the path is absent.
    fn image_url(&self, path: Option<&str>, size: ImageSize) -> Option<String>;
}

pub struct TmdbClient {
    http: std::sync::Arc<HttpClient>,
    api_key: String,
    base_url: String,
    image_base_url: String,
    /// Two-letter language, which selects the overview and the localized title.
    language: String,
    last_request: Mutex<Option<Instant>>,
}

impl std::fmt::Debug for TmdbClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Deliberately no api_key field: a Debug line is a log line.
        f.debug_struct("TmdbClient")
            .field("base_url", &self.base_url)
            .field("language", &self.language)
            .finish_non_exhaustive()
    }
}

impl TmdbClient {
    pub fn new(http: std::sync::Arc<HttpClient>, api_key: impl Into<String>) -> Self {
        Self {
            http,
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.into(),
            image_base_url: DEFAULT_IMAGE_BASE_URL.into(),
            language: "en-US".into(),
            last_request: Mutex::new(None),
        }
    }

    /// Point at a different host. Tests use it; so would a mirror.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_image_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.image_base_url = base_url.into();
        self
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    /// Sleep just long enough that requests stay under the rate limit.
    fn throttle(&self) {
        let mut last = self.last_request.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = *last {
            let elapsed = previous.elapsed();
            if elapsed < MIN_REQUEST_INTERVAL {
                std::thread::sleep(MIN_REQUEST_INTERVAL - elapsed);
            }
        }
        *last = Some(Instant::now());
    }

    fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &str,
    ) -> Result<T, NetFailure> {
        self.throttle();
        let url = format!(
            "{}{path}?api_key={}&language={}{query}",
            self.base_url,
            urlencode(&self.api_key),
            urlencode(&self.language),
        );
        let body = self.http.fetch_string(&url)?;
        serde_json::from_str(&body).map_err(|e| NetFailure {
            code: ErrorCode::Unknown,
            message: "The metadata service sent something unreadable".into(),
            // Never the body: it is large, and on an auth failure TMDB echoes enough of
            // the request back to be worth not putting in front of anyone.
            cause: format!(
                "Expected JSON and could not parse it ({e}). A proxy or captive portal \
                 may be answering instead of the service."
            ),
            actions: vec![ErrorAction::Retry],
            retryable: true,
        })
    }
}

/// Percent-encode the characters that would break a query string.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// First four characters of a date, which is how TMDB gives release years.
fn year_of(date: Option<&str>) -> Option<i32> {
    date.filter(|d| d.len() >= 4)?.get(..4)?.parse().ok()
}

impl MetadataClient for TmdbClient {
    fn search(
        &self,
        kind: Kind,
        title: &str,
        year: Option<i32>,
    ) -> Result<Vec<Candidate>, NetFailure> {
        let mut query = format!("&query={}", urlencode(title));
        if let Some(year) = year {
            // The two endpoints spell the same filter differently.
            query.push_str(&match kind {
                Kind::Movie => format!("&year={year}"),
                Kind::Series => format!("&first_air_date_year={year}"),
            });
        }
        let page: SearchPage = self.get(&format!("/search/{}", kind.path()), &query)?;
        Ok(page.results.into_iter().map(Candidate::from).collect())
    }

    fn details(&self, kind: Kind, id: i64) -> Result<Metadata, NetFailure> {
        // One round trip for the lot: a per-title extra request for credits would triple
        // the import time and the rate-limit pressure.
        let raw: Details = self.get(
            &format!("/{}/{id}", kind.path()),
            "&append_to_response=credits,images,release_dates,content_ratings",
        )?;
        Ok(raw.into_metadata(kind))
    }

    fn image_url(&self, path: Option<&str>, size: ImageSize) -> Option<String> {
        let path = path?.trim();
        if path.is_empty() {
            return None;
        }
        // TMDB paths start with a slash; tolerate one that does not.
        let sep = if path.starts_with('/') { "" } else { "/" };
        Some(format!(
            "{}/{}{sep}{path}",
            self.image_base_url,
            size.as_path()
        ))
    }
}

/* ── Wire format ──────────────────────────────────────────────────────────────
Every field is optional. TMDB omits rather than nulls, forks and mirrors differ,
and a missing overview must not fail an import. ─────────────────────────────── */

#[derive(Debug, Deserialize)]
struct SearchPage {
    #[serde(default)]
    results: Vec<SearchResult>,
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    id: i64,
    #[serde(default)]
    title: Option<String>,
    /// Series use `name` where movies use `title`.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    original_title: Option<String>,
    #[serde(default)]
    original_name: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    first_air_date: Option<String>,
    #[serde(default)]
    popularity: Option<f32>,
}

impl From<SearchResult> for Candidate {
    fn from(r: SearchResult) -> Self {
        Candidate {
            id: r.id,
            title: r.title.or(r.name).unwrap_or_default(),
            original_title: r.original_title.or(r.original_name),
            year: year_of(r.release_date.as_deref()).or(year_of(r.first_air_date.as_deref())),
            popularity: r.popularity.unwrap_or(0.0),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Details {
    id: i64,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    backdrop_path: Option<String>,
    #[serde(default)]
    runtime: Option<u32>,
    /// Series report a list, one entry per episode length.
    #[serde(default)]
    episode_run_time: Vec<u32>,
    #[serde(default)]
    vote_average: Option<f32>,
    #[serde(default)]
    genres: Vec<Genre>,
    #[serde(default)]
    credits: Option<Credits>,
    #[serde(default)]
    images: Option<Images>,
    #[serde(default)]
    release_dates: Option<Paged<ReleaseDates>>,
    #[serde(default)]
    content_ratings: Option<Paged<ContentRating>>,
}

#[derive(Debug, Deserialize)]
struct Genre {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Credits {
    #[serde(default)]
    cast: Vec<CastMember>,
    #[serde(default)]
    crew: Vec<CrewMember>,
}

#[derive(Debug, Deserialize)]
struct CastMember {
    id: i64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    character: Option<String>,
    #[serde(default)]
    profile_path: Option<String>,
    #[serde(default)]
    order: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct CrewMember {
    id: i64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    job: Option<String>,
    #[serde(default)]
    profile_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Images {
    #[serde(default)]
    logos: Vec<Image>,
}

#[derive(Debug, Deserialize)]
struct Image {
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    vote_average: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct Paged<T> {
    #[serde(default = "Vec::new")]
    results: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct ReleaseDates {
    #[serde(default)]
    iso_3166_1: Option<String>,
    #[serde(default)]
    release_dates: Vec<ReleaseDate>,
}

#[derive(Debug, Deserialize)]
struct ReleaseDate {
    #[serde(default)]
    certification: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContentRating {
    #[serde(default)]
    iso_3166_1: Option<String>,
    #[serde(default)]
    rating: Option<String>,
}

/// Which country's certification to use. US ratings are what the parental-control
/// mapping in `aurora_core::parental` understands best; GB is the common fallback.
const CERTIFICATION_PREFERENCE: [&str; 3] = ["US", "GB", "CA"];

impl Details {
    fn into_metadata(self, kind: Kind) -> Metadata {
        let mut credits = Vec::new();
        if let Some(raw) = self.credits {
            for c in raw.cast {
                credits.push(Credit {
                    person: Person {
                        tmdb_id: c.id,
                        name: c.name.unwrap_or_default(),
                        profile_path: c.profile_path,
                    },
                    role: c.character,
                    is_cast: true,
                    order: c.order.unwrap_or(u16::MAX),
                });
            }
            // Crew carries no billing order, so preserve the order TMDB sent.
            for (i, c) in raw.crew.into_iter().enumerate() {
                credits.push(Credit {
                    person: Person {
                        tmdb_id: c.id,
                        name: c.name.unwrap_or_default(),
                        profile_path: c.profile_path,
                    },
                    role: c.job,
                    is_cast: false,
                    order: i.min(u16::MAX as usize) as u16,
                });
            }
        }

        // The best-voted logo, which is the one the hero billboard overlays.
        let logo_path = self.images.and_then(|images| {
            images
                .logos
                .into_iter()
                .filter(|l| l.file_path.is_some())
                .max_by(|a, b| {
                    a.vote_average
                        .unwrap_or(0.0)
                        .partial_cmp(&b.vote_average.unwrap_or(0.0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .and_then(|l| l.file_path)
        });

        Metadata {
            tmdb_id: self.id,
            overview: self.overview.filter(|o| !o.trim().is_empty()),
            poster_path: self.poster_path,
            backdrop_path: self.backdrop_path,
            logo_path,
            runtime_mins: match kind {
                Kind::Movie => self.runtime.filter(|r| *r > 0),
                Kind::Series => self.episode_run_time.first().copied().filter(|r| *r > 0),
            },
            // TMDB's 0.0 means "nobody has voted", not "everyone hated it".
            rating: self.vote_average.filter(|r| *r > 0.0),
            certification: pick_certification(
                self.release_dates.map(|p| p.results).unwrap_or_default(),
                self.content_ratings.map(|p| p.results).unwrap_or_default(),
            ),
            genres: self.genres.into_iter().filter_map(|g| g.name).collect(),
            credits,
        }
    }
}

fn pick_certification(movies: Vec<ReleaseDates>, series: Vec<ContentRating>) -> Option<String> {
    for country in CERTIFICATION_PREFERENCE {
        if let Some(found) = movies
            .iter()
            .find(|r| r.iso_3166_1.as_deref() == Some(country))
            .and_then(|r| {
                r.release_dates
                    .iter()
                    .find_map(|d| d.certification.as_deref().filter(|c| !c.trim().is_empty()))
            })
        {
            return Some(found.to_string());
        }
        if let Some(found) = series
            .iter()
            .find(|r| r.iso_3166_1.as_deref() == Some(country))
            .and_then(|r| r.rating.as_deref().filter(|c| !c.trim().is_empty()))
        {
            return Some(found.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpConfig;
    use crate::testserver::{Reply, TestServer};
    use std::sync::Arc;

    fn client(server: &TestServer) -> TmdbClient {
        let http = Arc::new(HttpClient::new(HttpConfig::default()).unwrap());
        TmdbClient::new(http, "secret-key").with_base_url(server.url(""))
    }

    #[test]
    fn a_movie_search_maps_onto_candidates() {
        let server = TestServer::always(Reply::ok(
            br#"{"results":[
                 {"id":603,"title":"The Matrix","original_title":"The Matrix",
                  "release_date":"1999-03-30","popularity":82.5},
                 {"id":604,"title":"The Matrix Reloaded","release_date":"2003-05-15"}
               ]}"#
            .to_vec(),
        ));
        let found = client(&server)
            .search(Kind::Movie, "The Matrix", Some(1999))
            .unwrap();

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, 603);
        assert_eq!(found[0].year, Some(1999));
        assert_eq!(found[0].popularity, 82.5);
        // A result with no popularity is zero, not a parse failure.
        assert_eq!(found[1].popularity, 0.0);
    }

    #[test]
    fn a_series_search_reads_name_and_first_air_date() {
        let server = TestServer::always(Reply::ok(
            br#"{"results":[{"id":1396,"name":"Breaking Bad",
                 "original_name":"Breaking Bad","first_air_date":"2008-01-20"}]}"#
                .to_vec(),
        ));
        let found = client(&server)
            .search(Kind::Series, "Breaking Bad", None)
            .unwrap();
        assert_eq!(found[0].title, "Breaking Bad");
        assert_eq!(found[0].year, Some(2008));
    }

    #[test]
    fn the_search_hits_the_right_endpoint_with_the_right_year_filter() {
        let server = TestServer::always(Reply::ok(br#"{"results":[]}"#.to_vec()));
        let c = client(&server);
        c.search(Kind::Movie, "Heat", Some(1995)).unwrap();
        c.search(Kind::Series, "Heat", Some(1995)).unwrap();

        let paths: Vec<String> = server.requests().iter().map(|r| r.path.clone()).collect();
        assert!(paths[0].contains("/search/movie"), "{paths:?}");
        assert!(paths[0].contains("year=1995"), "{paths:?}");
        // Series use TMDB's "tv" noun and a differently spelled year filter.
        assert!(paths[1].contains("/search/tv"), "{paths:?}");
        assert!(paths[1].contains("first_air_date_year=1995"), "{paths:?}");
    }

    #[test]
    fn a_title_with_spaces_and_punctuation_is_encoded() {
        let server = TestServer::always(Reply::ok(br#"{"results":[]}"#.to_vec()));
        client(&server)
            .search(Kind::Movie, "Am\u{e9}lie & Co / Part 2", None)
            .unwrap();
        let path = &server.requests()[0].path;
        assert!(
            !path.contains(' '),
            "a raw space would break the request: {path}"
        );
        assert!(
            !path.contains("& Co"),
            "the ampersand must not split the query: {path}"
        );
    }

    #[test]
    fn an_empty_result_set_is_not_an_error() {
        let server = TestServer::always(Reply::ok(br#"{"results":[]}"#.to_vec()));
        assert!(client(&server)
            .search(Kind::Movie, "Nothing", None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_response_missing_the_results_key_is_empty_rather_than_a_failure() {
        // Mirrors and forks omit keys; an import must not die on one.
        let server = TestServer::always(Reply::ok(br#"{}"#.to_vec()));
        assert!(client(&server)
            .search(Kind::Movie, "X", None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_bad_key_surfaces_as_a_readable_failure() {
        let server = TestServer::always(Reply::status(401));
        let err = client(&server).search(Kind::Movie, "X", None).unwrap_err();
        assert!(!err.message.is_empty());
        assert!(
            !err.message.contains("secret-key"),
            "the key must never be echoed"
        );
    }

    #[test]
    fn html_instead_of_json_is_explained_not_leaked() {
        let server = TestServer::always(Reply::ok(b"<html><body>gateway</body></html>".to_vec()));
        let err = client(&server).search(Kind::Movie, "X", None).unwrap_err();
        assert!(err.message.contains("metadata service"), "{}", err.message);
        // The body never reaches the user, in the message or the cause.
        assert!(!err.message.contains("<html>"), "{}", err.message);
        assert!(!err.cause.contains("<html>"), "{}", err.cause);
    }

    #[test]
    fn details_map_every_field_the_library_stores() {
        let server = TestServer::always(Reply::ok(
            br#"{"id":603,"overview":"A hacker learns the truth.",
                 "poster_path":"/p.jpg","backdrop_path":"/b.jpg","runtime":136,
                 "vote_average":8.2,
                 "genres":[{"name":"Action"},{"name":"Science Fiction"}],
                 "credits":{"cast":[
                     {"id":6384,"name":"Keanu Reeves","character":"Neo","order":0},
                     {"id":2975,"name":"Laurence Fishburne","character":"Morpheus","order":1}],
                   "crew":[{"id":9339,"name":"Lana Wachowski","job":"Director"}]},
                 "images":{"logos":[
                     {"file_path":"/low.png","vote_average":1.0},
                     {"file_path":"/best.png","vote_average":9.0}]},
                 "release_dates":{"results":[
                     {"iso_3166_1":"GB","release_dates":[{"certification":"15"}]},
                     {"iso_3166_1":"US","release_dates":[{"certification":"R"}]}]}}"#
                .to_vec(),
        ));
        let meta = client(&server).details(Kind::Movie, 603).unwrap();

        assert_eq!(meta.tmdb_id, 603);
        assert_eq!(meta.runtime_mins, Some(136));
        assert_eq!(meta.rating, Some(8.2));
        assert_eq!(meta.genres, vec!["Action", "Science Fiction"]);
        assert_eq!(meta.poster_path.as_deref(), Some("/p.jpg"));
        // The best-voted logo wins, not the first one listed.
        assert_eq!(meta.logo_path.as_deref(), Some("/best.png"));
        // US is preferred over GB, because that is what the parental mapping understands.
        assert_eq!(meta.certification.as_deref(), Some("R"));

        let cast: Vec<&str> = meta.cast().iter().map(|c| c.person.name.as_str()).collect();
        assert_eq!(cast, vec!["Keanu Reeves", "Laurence Fishburne"]);
        assert_eq!(
            meta.crew_by_job("Director")[0].person.name,
            "Lana Wachowski"
        );
    }

    #[test]
    fn a_series_takes_its_runtime_from_the_episode_list() {
        let server = TestServer::always(Reply::ok(
            br#"{"id":1396,"episode_run_time":[47,45],
                 "content_ratings":{"results":[{"iso_3166_1":"US","rating":"TV-MA"}]}}"#
                .to_vec(),
        ));
        let meta = client(&server).details(Kind::Series, 1396).unwrap();
        assert_eq!(meta.runtime_mins, Some(47));
        assert_eq!(meta.certification.as_deref(), Some("TV-MA"));
    }

    #[test]
    fn a_details_response_with_almost_nothing_in_it_still_parses() {
        let server = TestServer::always(Reply::ok(br#"{"id":1}"#.to_vec()));
        let meta = client(&server).details(Kind::Movie, 1).unwrap();
        assert_eq!(meta.tmdb_id, 1);
        assert_eq!(meta.overview, None);
        assert_eq!(meta.runtime_mins, None);
        assert_eq!(meta.certification, None);
        assert!(meta.credits.is_empty());
    }

    #[test]
    fn placeholder_values_are_treated_as_absent() {
        // Zero means "no votes" and zero runtime means "unknown"; storing either as a
        // real value puts "0.0 ★" and "0m" on a detail page.
        let server = TestServer::always(Reply::ok(
            br#"{"id":1,"vote_average":0.0,"runtime":0,"overview":"   ",
                 "release_dates":{"results":[
                    {"iso_3166_1":"US","release_dates":[{"certification":""}]}]}}"#
                .to_vec(),
        ));
        let meta = client(&server).details(Kind::Movie, 1).unwrap();
        assert_eq!(meta.rating, None);
        assert_eq!(meta.runtime_mins, None);
        assert_eq!(meta.overview, None);
        assert_eq!(
            meta.certification, None,
            "an empty certification is not a rating"
        );
    }

    #[test]
    fn image_urls_are_built_at_the_size_the_ui_renders() {
        let server = TestServer::always(Reply::ok(b"{}".to_vec()));
        let c = client(&server).with_image_base_url("https://img.example.com/t/p");

        assert_eq!(
            c.image_url(Some("/poster.jpg"), ImageSize::Poster)
                .as_deref(),
            Some("https://img.example.com/t/p/w342/poster.jpg")
        );
        // A path without the leading slash still produces a valid URL.
        assert_eq!(
            c.image_url(Some("poster.jpg"), ImageSize::Backdrop)
                .as_deref(),
            Some("https://img.example.com/t/p/w1280/poster.jpg")
        );
        assert_eq!(c.image_url(None, ImageSize::Poster), None);
        assert_eq!(c.image_url(Some("  "), ImageSize::Poster), None);
    }

    #[test]
    fn the_api_key_never_appears_in_a_debug_line() {
        let server = TestServer::always(Reply::ok(b"{}".to_vec()));
        let rendered = format!("{:?}", client(&server));
        assert!(!rendered.contains("secret-key"), "{rendered}");
    }

    #[test]
    fn requests_are_spaced_to_stay_under_the_rate_limit() {
        let server = TestServer::always(Reply::ok(br#"{"results":[]}"#.to_vec()));
        let c = client(&server);
        let started = Instant::now();
        for _ in 0..4 {
            c.search(Kind::Movie, "X", None).unwrap();
        }
        // Three gaps between four requests.
        assert!(
            started.elapsed() >= MIN_REQUEST_INTERVAL * 3,
            "four requests went out in {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_year_is_read_from_a_date_and_nothing_else() {
        assert_eq!(year_of(Some("1999-03-30")), Some(1999));
        assert_eq!(year_of(Some("1999")), Some(1999));
        // TMDB sends an empty string for an unknown release date.
        assert_eq!(year_of(Some("")), None);
        assert_eq!(year_of(Some("not-a-date")), None);
        assert_eq!(year_of(None), None);
    }
}
