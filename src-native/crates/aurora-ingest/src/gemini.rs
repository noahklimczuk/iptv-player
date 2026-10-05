//! Recommendations from Gemini, resolved against the library.
//!
//! **Why a model rather than more arithmetic.** `aurora_core::recommend` scores the
//! library against a decayed profile of genres, categories and eras, and it is good at
//! what it can see: it knows this viewer watches Horror from "NL ✪ FILMS [SUB]" and finds
//! more of it. What it cannot know is that somebody who watched *Arrival* and *Primer*
//! would probably like *Coherence* — that is a fact about the films, not about their
//! metadata, and no amount of genre overlap recovers it. "Sci-Fi, 2013, rated 7.2" is
//! true of *Coherence* and of two thousand other things.
//!
//! **What is asked, and what is not.** The model is given the viewer's history and asked
//! for titles *by name*. It is not given the library and asked to rank it — partly
//! because 140,000 rows do not fit in a prompt, but mostly because reranking a shortlist
//! can only reorder what the retrieval already found, which throws away the one thing the
//! model is actually better at. Its answers are then looked up in the library by
//! `match_key` and anything missing is dropped, so every recommendation that reaches the
//! screen is a title the viewer can press play on.
//!
//! That lookup is also the honesty check. A model asked for films will happily invent
//! one; a title that does not exist cannot match a row, so it never arrives.

use serde::{Deserialize, Serialize};

use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure};

use crate::http::HttpClient;

/// The model to ask.
///
/// Flash rather than Pro: this is a list of film titles, it runs in the background while
/// somebody is browsing, and the cost difference is real on a library that asks again
/// whenever the taste profile moves.
pub const MODEL: &str = "gemini-flash-latest";

/// Where the API lives. Overridable so the tests can point it at a local server.
pub const DEFAULT_ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta";

/// The header the key travels in.
///
/// Google accepts the key either as `?key=` or in this header, and the query string is
/// the wrong one of the two. A URL is the part of a request that gets logged: `redact`
/// scrubs the parameters this codebase knows panels use — `password`, `token`, `api_key`
/// and the rest — and a bare `key` was not among them, so a retried generation wrote the
/// key into the debug log in full. A header is not in the URL, so it cannot be logged by
/// anything that logs URLs, and that holds for the next secret-bearing parameter too
/// without anyone having to remember to add it to a denylist.
/// Shared with `chat.rs`, which talks to the same API with the same key.
pub(crate) const API_KEY_HEADER: &str = "x-goog-api-key";

/// How many titles to ask for.
///
/// More than the rail shows, because some of them will not be in this library and are
/// dropped. Asking for forty to show twelve is the difference between a full rail and a
/// gappy one on a library that does not carry everything.
pub const ASK_FOR: usize = 40;

/// One thing the viewer has watched, as the model is told about it.
#[derive(Debug, Clone, Serialize)]
pub struct WatchedTitle {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,
    /// How much of it they watched, 0–1. A film finished says more than one sampled.
    pub watched: f32,
    /// Whether they marked it a favourite, which says more again.
    pub liked: bool,
}

/// What the model is asked to produce, one per suggestion.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Suggestion {
    pub title: String,
    #[serde(default)]
    pub year: Option<i32>,
    /// Why this viewer in particular would like it, in the model's words.
    #[serde(default)]
    pub reason: String,
    /// `movie` or `series`, so a suggestion is looked up in the right table.
    #[serde(default)]
    pub kind: Option<String>,
}

/// The seam, so the app can be tested without the network and without a key.
pub trait Recommender {
    fn suggest(
        &self,
        watched: &[WatchedTitle],
        avoid: &[String],
    ) -> Result<Vec<Suggestion>, NetFailure>;
}

pub struct GeminiClient<'a> {
    http: &'a HttpClient,
    api_key: String,
    endpoint: String,
    model: String,
}

impl<'a> GeminiClient<'a> {
    pub fn new(http: &'a HttpClient, api_key: &str) -> Self {
        Self {
            http,
            api_key: api_key.to_string(),
            endpoint: DEFAULT_ENDPOINT.to_string(),
            model: MODEL.to_string(),
        }
    }

    /// Point at a different endpoint. For tests, and for nothing else.
    pub fn with_endpoint(mut self, endpoint: &str) -> Self {
        self.endpoint = endpoint.trim_end_matches('/').to_string();
        self
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    /// No key in here: it goes in [`API_KEY_HEADER`].
    fn url(&self) -> String {
        format!("{}/models/{}:generateContent", self.endpoint, self.model)
    }
}

/// The instruction. Separate from the data so it can be read as prose and changed without
/// touching the plumbing.
///
/// Three things it insists on, each because of what happens without it:
///
///   * *Real titles only.* A model asked for recommendations will pad a short list with
///     plausible inventions. The library lookup drops those, but asking for forty and
///     having eight be fictional wastes most of the request.
///   * *Not the ones they have already watched.* Otherwise the most-watched genre comes
///     back as the films that produced it, which is a mirror rather than a recommendation.
///   * *A reason about this viewer.* "A sci-fi classic" is a fact about the film. "Shares
///     the slow dread of Arrival, which you finished" is a reason, and a rail that says
///     why is the difference between one somebody trusts and a row of posters.
const INSTRUCTION: &str = "\
You recommend films and television to one person, based on what they have watched.

Rules:
- Suggest only real, existing titles. Never invent one.
- Never suggest something in the watched list, or in the avoid list.
- Prefer titles that are well known enough to appear in a general catalogue; obscure \
festival shorts will not be available to them.
- Spread the suggestions: do not return ten films by one director or eight from one \
series.
- For `reason`, say in one short sentence why THIS viewer would like it, referring to \
what they watched. Not a plot summary, not a generic blurb.
- `kind` is \"movie\" or \"series\".
- `year` is the release year where you are confident of it, otherwise omit it.";

/// The shape the reply must take. Gemini enforces this server-side, so the answer either
/// parses or the request failed — there is no prose to strip, no fences to peel off, and
/// no "Sure! Here are some films:" to apologise for.
fn response_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "ARRAY",
        "items": {
            "type": "OBJECT",
            "properties": {
                "title": { "type": "STRING" },
                "year": { "type": "INTEGER" },
                "kind": { "type": "STRING", "enum": ["movie", "series"] },
                "reason": { "type": "STRING" },
            },
            "required": ["title", "reason", "kind"],
        },
    })
}

impl Recommender for GeminiClient<'_> {
    fn suggest(
        &self,
        watched: &[WatchedTitle],
        avoid: &[String],
    ) -> Result<Vec<Suggestion>, NetFailure> {
        if self.api_key.trim().is_empty() {
            return Err(NetFailure {
                code: ErrorCode::Unauthorized,
                message: "No Gemini key is set".into(),
                cause: "Recommendations from Gemini need an API key, which is entered in \
                        Settings."
                    .into(),
                actions: vec![ErrorAction::OpenSettings],
                retryable: false,
            });
        }

        let prompt = serde_json::json!({
            "instruction": INSTRUCTION,
            "watched": watched,
            "avoid": avoid,
            "how_many": ASK_FOR,
        });

        let body = serde_json::json!({
            "contents": [{ "parts": [{ "text": prompt.to_string() }] }],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": response_schema(),
                // Low, not zero. Zero makes every refresh return the same list, which on
                // a rail that is supposed to feel alive reads as broken; high enough and
                // it starts inventing titles that the library lookup then throws away.
                "temperature": 0.4,
            },
        });

        let raw = self.http.post_json(
            &self.url(),
            &body.to_string(),
            &[(API_KEY_HEADER, self.api_key.as_str())],
        )?;
        parse_reply(&raw)
    }
}

/// Dig the JSON array out of the envelope Gemini wraps it in.
///
/// The useful payload is `candidates[0].content.parts[0].text`, which is itself a JSON
/// string because `responseMimeType` asked for one. Everything that can be missing is
/// reported as a provider failure rather than unwrapped: a reply in an unexpected shape
/// is the one case where a panic would take the whole recommendation thread down.
pub fn parse_reply(raw: &str) -> Result<Vec<Suggestion>, NetFailure> {
    let envelope: serde_json::Value = serde_json::from_str(raw).map_err(|e| unreadable(&e))?;

    // An error reply carries its own message, which is far more useful than "could not
    // read": a bad key, a quota, or a model name that does not exist all land here.
    if let Some(message) = envelope
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
    {
        let lower = message.to_ascii_lowercase();
        let (code, action) = if lower.contains("api key") || lower.contains("permission") {
            (ErrorCode::Unauthorized, ErrorAction::OpenSettings)
        } else if lower.contains("quota") || lower.contains("rate") {
            (ErrorCode::RateLimited, ErrorAction::Retry)
        } else {
            (ErrorCode::ServerError, ErrorAction::Retry)
        };
        return Err(NetFailure {
            code,
            message: "Gemini refused the request".into(),
            cause: message.to_string(),
            actions: vec![action],
            retryable: !matches!(code, ErrorCode::Unauthorized),
        });
    }

    let text = envelope
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| NetFailure {
            code: ErrorCode::Unknown,
            message: "Gemini sent a reply Aurora could not read".into(),
            cause: "The response carried no content. This usually means the request was \
                    blocked by a safety filter, or the model returned nothing."
                .into(),
            actions: vec![ErrorAction::Retry],
            retryable: true,
        })?;

    let suggestions: Vec<Suggestion> = serde_json::from_str(text).map_err(|e| unreadable(&e))?;
    Ok(suggestions
        .into_iter()
        .filter(|s| !s.title.trim().is_empty())
        .collect())
}

fn unreadable(e: &serde_json::Error) -> NetFailure {
    NetFailure {
        code: ErrorCode::Unknown,
        message: "Gemini sent a reply Aurora could not read".into(),
        cause: format!("The response was not the JSON that was asked for. {e}"),
        actions: vec![ErrorAction::Retry],
        retryable: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpClient, HttpConfig};
    use crate::testserver::{Reply, TestServer};

    fn client() -> HttpClient {
        HttpClient::new(HttpConfig {
            max_attempts: 1,
            ..Default::default()
        })
        .unwrap()
    }

    fn envelope(inner: &str) -> Vec<u8> {
        serde_json::json!({
            "candidates": [{ "content": { "parts": [{ "text": inner }] } }]
        })
        .to_string()
        .into_bytes()
    }

    fn watched() -> Vec<WatchedTitle> {
        vec![WatchedTitle {
            title: "Arrival".into(),
            year: Some(2016),
            genres: vec!["Sci-Fi".into()],
            watched: 1.0,
            liked: true,
        }]
    }

    #[test]
    fn reads_the_suggestions_out_of_the_envelope() {
        let inner = r#"[
            {"title":"Coherence","year":2013,"kind":"movie","reason":"Shares Arrival's quiet dread."},
            {"title":"Dark","year":2017,"kind":"series","reason":"Time and language, slowly."}
        ]"#;
        let server = TestServer::always(Reply::ok(envelope(inner)));
        let http = client();
        let out = GeminiClient::new(&http, "key")
            .with_endpoint(&server.url(""))
            .suggest(&watched(), &[])
            .unwrap();

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "Coherence");
        assert_eq!(out[0].year, Some(2013));
        assert_eq!(out[0].kind.as_deref(), Some("movie"));
        assert!(out[0].reason.contains("Arrival"));
        assert_eq!(out[1].kind.as_deref(), Some("series"));
    }

    /// The key is a header, and the URL is clean.
    ///
    /// Not a style point. The URL is the part of a request that gets logged — on a retry
    /// `send_with_retry` writes it through `redact`, which scrubs the parameters panels
    /// use and never knew about a bare `key`. Asserting on the request target is what
    /// stops it going back there.
    #[test]
    fn the_key_travels_in_a_header_and_not_in_the_url() {
        let server = TestServer::always(Reply::ok(envelope("[]")));
        let http = client();
        GeminiClient::new(&http, "sekrit-123")
            .with_endpoint(&server.url(""))
            .suggest(&watched(), &[])
            .expect("an empty list is still a reply");

        let seen = server.requests();
        assert_eq!(seen.len(), 1, "one request, so one place to look");
        assert_eq!(seen[0].header(API_KEY_HEADER), Some("sekrit-123"));
        assert!(
            !seen[0].path.contains("sekrit-123"),
            "the key is not in the request target: {}",
            seen[0].path
        );
        assert!(
            !seen[0].path.contains("key="),
            "no key parameter at all, so there is nothing for a log to spill: {}",
            seen[0].path
        );
    }

    /// Asking with no key is a question about Settings, not a request to send.
    #[test]
    fn no_key_is_refused_before_anything_is_sent() {
        let http = client();
        let e = GeminiClient::new(&http, "   ")
            .suggest(&watched(), &[])
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Unauthorized);
        assert!(!e.retryable);
        assert!(e.actions.contains(&ErrorAction::OpenSettings));
    }

    /// Google's own error text says far more than "could not read" ever could.
    #[test]
    fn an_api_error_is_reported_in_googles_words() {
        let body = serde_json::json!({
            "error": { "message": "API key not valid. Please pass a valid API key." }
        })
        .to_string();
        let server = TestServer::always(Reply::ok(body.into_bytes()));
        let http = client();
        let e = GeminiClient::new(&http, "bad")
            .with_endpoint(&server.url(""))
            .suggest(&watched(), &[])
            .unwrap_err();

        assert_eq!(e.code, ErrorCode::Unauthorized);
        assert!(e.cause.contains("API key not valid"));
        assert!(!e.retryable, "a bad key does not improve on retry");
    }

    #[test]
    fn a_quota_error_is_retryable() {
        let body = serde_json::json!({
            "error": { "message": "Quota exceeded for quota metric 'Generate requests'" }
        })
        .to_string();
        let server = TestServer::always(Reply::ok(body.into_bytes()));
        let http = client();
        let e = GeminiClient::new(&http, "k")
            .with_endpoint(&server.url(""))
            .suggest(&watched(), &[])
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::RateLimited);
        assert!(e.retryable);
    }

    /// A safety block returns an envelope with no candidates at all.
    #[test]
    fn a_reply_with_no_content_is_an_error_not_an_empty_rail() {
        let server = TestServer::always(Reply::ok(b"{\"candidates\":[]}".to_vec()));
        let http = client();
        let e = GeminiClient::new(&http, "k")
            .with_endpoint(&server.url(""))
            .suggest(&watched(), &[])
            .unwrap_err();
        assert!(e.message.contains("could not read"));
        assert!(e.retryable);
    }

    /// Whatever the schema says, a title that is only whitespace cannot be looked up.
    #[test]
    fn blank_titles_are_dropped() {
        let inner = r#"[{"title":"  ","kind":"movie","reason":"x"},
                        {"title":"Primer","kind":"movie","reason":"y"}]"#;
        let out = parse_reply(std::str::from_utf8(&envelope(inner)).unwrap()).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Primer");
    }
}
