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

/// The gap between requests this aims for, and the floor it will not go below.
///
/// 10 ms is about a hundred requests a second. That is above what TMDB suggests and
/// below what it refuses, which is only a safe place to sit because the pacer below
/// *reacts*: the moment the service says no, the gap widens and stays widened until
/// requests are succeeding again.
///
/// It used to be a flat 25 ms — 40 a second — chosen because nothing here could tell the
/// difference between "fine" and "about to be throttled", so the only safe answer was to
/// stay well under. That cost real time on a large library: the pacer, not the machine,
/// was what decided a pass took 22 minutes.
pub const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(10);

/// How far the gap is allowed to open when the service pushes back. Two seconds between
/// requests is a crawl, and deliberately: it is what the far end has asked for.
const MAX_REQUEST_INTERVAL: Duration = Duration::from_millis(2000);

/// What a refusal costs, and what a success gives back.
///
/// Multiplicative up, gentle down — the standard shape, because the two mistakes are not
/// equally expensive. Backing off too slowly means a run of rejected requests; recovering
/// too slowly only means finishing later.
const BACKOFF_FACTOR: u32 = 4;
const RECOVERY_NUMERATOR: u32 = 4;
const RECOVERY_DENOMINATOR: u32 = 5;

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
    /// When the last request was let through, and how far apart they are being spaced
    /// right now. One lock, because the two are only ever read and written together.
    pace: Mutex<Pace>,
}

/// The state of the pacer: what it is aiming for and when it last let something past.
#[derive(Debug)]
struct Pace {
    last_request: Option<Instant>,
    interval: Duration,
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
            pace: Mutex::new(Pace {
                last_request: None,
                interval: MIN_REQUEST_INTERVAL,
            }),
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

    /// Wait for this request's turn.
    ///
    /// The lock is deliberately held across the sleep. That is what makes this a pacer
    /// rather than a free-for-all: with several lookups in flight, each one takes its
    /// turn at the front and the *waits* overlap, so the starts stay one interval apart
    /// however many threads are asking.
    fn throttle(&self) {
        let mut pace = self.pace.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = pace.last_request {
            let elapsed = previous.elapsed();
            if elapsed < pace.interval {
                std::thread::sleep(pace.interval - elapsed);
            }
        }
        pace.last_request = Some(Instant::now());
    }

    /// The service refused: space requests further apart.
    ///
    /// Without this the rate is a guess made once, at compile time, by someone who cannot
    /// see the connection it will run on. With it the guess only has to be a starting
    /// point, which is why the starting point can be an ambitious one.
    fn slow_down(&self) {
        let mut pace = self.pace.lock().unwrap_or_else(|e| e.into_inner());
        let widened = (pace.interval * BACKOFF_FACTOR).min(MAX_REQUEST_INTERVAL);
        if widened > pace.interval {
            tracing::debug!(
                from_ms = pace.interval.as_millis() as u64,
                to_ms = widened.as_millis() as u64,
                "rate limited; spacing metadata requests further apart"
            );
            pace.interval = widened;
        }
    }

    /// That one worked: edge back towards the target.
    fn speed_up(&self) {
        let mut pace = self.pace.lock().unwrap_or_else(|e| e.into_inner());
        if pace.interval > MIN_REQUEST_INTERVAL {
            pace.interval = (pace.interval * RECOVERY_NUMERATOR / RECOVERY_DENOMINATOR)
                .max(MIN_REQUEST_INTERVAL);
        }
    }

    /// The interval in force, for tests and diagnostics.
    pub fn current_interval(&self) -> Duration {
        self.pace.lock().unwrap_or_else(|e| e.into_inner()).interval
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
        // The pacer only earns its ambitious starting interval by reacting to the
        // answer. `fetch_string` has already retried a 429 through its own backoff, so
        // reaching here with one means the service is genuinely pushing back rather than
        // hiccupping, and the spacing should change for every request after it — not
        // just this one.
        let body = match self.http.fetch_string(&url) {
            Ok(body) => {
                self.speed_up();
                body
            }
            Err(failure) => {
                if failure.code == ErrorCode::RateLimited {
                    self.slow_down();
                }
                return Err(failure);
            }
        };
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
    /// A film is a `title`, a show is a `name`. Both shapes arrive here.
    #[serde(default)]
    title: Option<String>,
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
    tagline: Option<String>,
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

        // Before the fields are moved into the struct below.
        let year = year_of(self.release_date.as_deref())
            .or_else(|| year_of(self.first_air_date.as_deref()));
        let said = |v: Option<String>| v.filter(|s| !s.trim().is_empty());

        Metadata {
            tmdb_id: self.id,
            title: said(self.title.or(self.name)),
            original_title: said(self.original_title.or(self.original_name)),
            year,
            tagline: said(self.tagline),
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

    /// The pacer has to react, or its starting interval is just a faster guess.
    ///
    /// 10 ms between requests is only defensible because being refused changes it. If
    /// this stops working the client keeps hammering at a hundred a second into a
    /// service that has already said no, which is worse than the 25 ms it replaced.
    #[test]
    fn being_refused_spaces_requests_further_apart() {
        let server = TestServer::always(Reply::status(429));
        // One attempt: this is about what the pacer does with the answer, not about the
        // HTTP layer's own retries, and three rounds of backoff would make it slow.
        let http = Arc::new(
            HttpClient::new(HttpConfig {
                max_attempts: 1,
                ..Default::default()
            })
            .unwrap(),
        );
        let client = TmdbClient::new(http, "secret-key").with_base_url(server.url(""));

        assert_eq!(
            client.current_interval(),
            MIN_REQUEST_INTERVAL,
            "a fresh client should start at the target"
        );

        let first = client.search(Kind::Movie, "anything", None);
        assert!(
            first.is_err(),
            "the server answered 429; that is not a success"
        );
        let after_one = client.current_interval();
        assert!(
            after_one > MIN_REQUEST_INTERVAL,
            "a refusal did not widen the gap: still {after_one:?}"
        );

        // And it keeps widening rather than settling after one step.
        let _ = client.search(Kind::Movie, "anything", None);
        assert!(
            client.current_interval() > after_one,
            "a second refusal did not widen it further"
        );

        // But not without limit: something has to stop a run of refusals turning into
        // a request every few minutes. Driven directly rather than through twenty more
        // requests — each of those would first *wait* the interval being tested, which
        // is half a minute of sleeping to assert one number.
        for _ in 0..20 {
            client.slow_down();
        }
        assert!(
            client.current_interval() <= MAX_REQUEST_INTERVAL,
            "the gap opened past its ceiling: {:?}",
            client.current_interval()
        );
    }

    /// And recovers, or one bad minute would cost the rest of the pass.
    #[test]
    fn requests_that_work_bring_the_pace_back() {
        let server = TestServer::always(Reply::ok(r#"{"results":[],"total_results":0}"#));
        let client = client(&server);

        // Start it somewhere slow, as a run of refusals would have left it.
        client.slow_down();
        client.slow_down();
        let slowed = client.current_interval();
        assert!(slowed > MIN_REQUEST_INTERVAL);

        for _ in 0..40 {
            client.search(Kind::Movie, "anything", None).unwrap();
        }
        assert_eq!(
            client.current_interval(),
            MIN_REQUEST_INTERVAL,
            "successful requests never brought the pace back to the target"
        );
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
            br#"{"id":603,"title":"The Matrix","original_title":"The Matrix",
                 "release_date":"1999-03-30","tagline":"Welcome to the Real World.",
                 "overview":"A hacker learns the truth.",
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
        // What it is actually called, which is the point of asking TMDB at all.
        assert_eq!(meta.title.as_deref(), Some("The Matrix"));
        assert_eq!(meta.original_title.as_deref(), Some("The Matrix"));
        assert_eq!(meta.tagline.as_deref(), Some("Welcome to the Real World."));
        assert_eq!(meta.year, Some(1999));
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
