//! Talking to Gemini in turns, with tools it can call (README §11).
//!
//! The recommender in `gemini.rs` asks one question and resolves the answer. This is the
//! other shape: a conversation, where the model can *ask the library questions* before it
//! answers, and where its answer can carry things the viewer can press play on.
//!
//! ## Why tools rather than a bigger prompt
//!
//! The library this was built against holds 24,658 films and 8,054 series. Listing them
//! is roughly a megabyte of titles, which is both too expensive to send on every turn and
//! the wrong shape — a model handed thirty thousand names will pattern-match on the list
//! instead of reasoning about what somebody asked for. So the model is given a digest
//! (counts, shelves, the genres that actually exist) and a set of functions it can call
//! to look things up. It decides what to ask; the host answers from SQLite.
//!
//! That also makes the answer honest by construction. Every title the viewer can press
//! play on came back from one of those lookups, so it is a row in their library with a
//! stream behind it. The model cannot invent a playable title, because the only ids that
//! reach the UI are ids the host handed out.
//!
//! ## What leaves the machine
//!
//! More than the one-shot recommender sends, and worth being exact about. Titles, years,
//! genres and shelf names from the viewer's library — whatever a tool call asks for and
//! the bound allows — plus the conversation itself. Never: credentials, the provider's
//! address, stream URLs, or anything that identifies the subscription or the person. The
//! host builds every tool result, so a tool cannot leak a field it was not written to
//! return.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::http::HttpClient;
use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure};

/// The ceiling on one reply, in tokens.
///
/// Three sentences and a few cards is a couple of hundred; this is several times that,
/// so it bounds a runaway rather than trimming an ordinary answer. Named because the
/// number is a judgement and belongs where somebody can find it.
pub const MAX_REPLY_TOKENS: u32 = 768;

/// One exchange in the conversation, in the order it happened.
///
/// `ToolCall` and `ToolResult` are kept in the history rather than collapsed away,
/// because Gemini requires the function call it made and the response to it to be
/// present as turns; dropping them is how a second round forgets what it just looked up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "camelCase")]
pub enum Turn {
    User {
        text: String,
    },
    Model {
        text: String,
    },
    ToolCall {
        name: String,
        args: serde_json::Value,
        /// Handed straight back to the API. See `ToolCall::thought_signature`.
        thought_signature: Option<String>,
    },
    ToolResult {
        name: String,
        result: serde_json::Value,
    },
}

/// A function the model may call, as it is declared to the API.
#[derive(Debug, Clone)]
pub struct ToolDecl {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON-Schema-ish parameter object, in the subset Gemini accepts.
    pub parameters: serde_json::Value,
}

/// What the model wants to call, and with what.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolCall {
    pub name: String,
    pub args: serde_json::Value,
    /// The model's opaque record of its own reasoning for this call.
    ///
    /// Gemini returns one beside every `functionCall` part, and **requires it back** on
    /// the next turn, in the same part it came from. Drop it and the next request is
    /// rejected outright:
    ///
    /// > Function call is missing a thought_signature in functionCall parts. This is
    /// > required for tools to work correctly…
    ///
    /// Which is a 400, and therefore was every tool-using question in the app failing on
    /// its second round. Opaque on purpose — it is not for reading, only for handing
    /// back. `None` for a model that does not send one, and for the scripted chatter in
    /// the tests.
    pub thought_signature: Option<String>,
}

/// One round's answer: either it is done talking, or it wants something looked up.
///
/// Both at once is possible and is kept that way — Gemini will happily narrate while it
/// calls a function ("let me check what you have…"), and throwing that text away makes
/// the pause before the real answer look like nothing happening.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatReply {
    pub text: Option<String>,
    pub calls: Vec<ToolCall>,
}

impl ChatReply {
    pub fn is_final(&self) -> bool {
        self.calls.is_empty()
    }
}

/// The seam, so the agent loop can be driven without a key or a network.
pub trait Chatter {
    fn turn(
        &self,
        system: &str,
        history: &[Turn],
        tools: &[ToolDecl],
    ) -> Result<ChatReply, NetFailure>;
}

pub struct ChatClient<'a> {
    http: &'a HttpClient,
    api_key: String,
    endpoint: String,
    model: String,
}

impl<'a> ChatClient<'a> {
    pub fn new(http: &'a HttpClient, api_key: &str) -> Self {
        Self {
            http,
            api_key: api_key.to_string(),
            endpoint: super::gemini::DEFAULT_ENDPOINT.to_string(),
            model: super::gemini::MODEL.to_string(),
        }
    }

    pub fn with_endpoint(mut self, endpoint: &str) -> Self {
        self.endpoint = endpoint.trim_end_matches('/').to_string();
        self
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    fn url(&self) -> String {
        format!("{}/models/{}:generateContent", self.endpoint, self.model)
    }
}

/// The history as Gemini's `contents` array.
///
/// A tool result is a `user` turn carrying a `functionResponse`, which reads oddly and is
/// what the API specifies: the function's output is something being *given to* the model,
/// so it arrives from the same side as the person's own words.
fn contents_of(history: &[Turn]) -> Vec<serde_json::Value> {
    // A call we cannot sign is told as prose instead, along with its result.
    //
    // Gemini requires `thoughtSignature` back on every `functionCall` part and rejects
    // the request outright without it — the error names the function and its position,
    // and the whole turn fails. `ToolCall::thought_signature` is `None` whenever the
    // model did not send one, and sending the call anyway is the one thing guaranteed
    // not to work.
    //
    // So those turns are degraded rather than dropped: the model still learns that it
    // asked and what came back, which is what the next round needs, and the request
    // stays one the API will accept. Done as a *pair* because a `functionResponse` is
    // only legal after the `functionCall` it answers — degrading one and not the other
    // trades a rejected request for a differently rejected request.
    let mut unsigned = vec![false; history.len()];
    for (i, turn) in history.iter().enumerate() {
        if let Turn::ToolCall {
            thought_signature: None,
            ..
        } = turn
        {
            unsigned[i] = true;
            if let Some(Turn::ToolResult { .. }) = history.get(i + 1) {
                unsigned[i + 1] = true;
            }
        }
    }

    history
        .iter()
        .enumerate()
        .map(|(i, turn)| match turn {
            _ if unsigned[i] => match turn {
                Turn::ToolCall { name, args, .. } => json!({
                    "role": "model",
                    "parts": [{ "text": format!("(Looked up {name} with {args})") }],
                }),
                Turn::ToolResult { name, result } => json!({
                    "role": "user",
                    "parts": [{ "text": format!("(Result of {name}: {result})") }],
                }),
                // Unreachable: only the two above are ever marked.
                other => turn_part(other),
            },
            other => turn_part(other),
        })
        .collect()
}

/// One turn as the API's `contents` entry, for a turn that needs no special handling.
fn turn_part(turn: &Turn) -> serde_json::Value {
    match turn {
        Turn::User { text } => json!({ "role": "user", "parts": [{ "text": text }] }),
        Turn::Model { text } => json!({ "role": "model", "parts": [{ "text": text }] }),
        Turn::ToolCall {
            name,
            args,
            thought_signature,
        } => {
            let mut part = json!({ "functionCall": { "name": name, "args": args } });
            // Beside `functionCall`, not inside it: the signature is a property of
            // the *part*, which is where the API looks for it.
            if let Some(signature) = thought_signature {
                part["thoughtSignature"] = json!(signature);
            }
            json!({ "role": "model", "parts": [part] })
        }
        Turn::ToolResult { name, result } => json!({
            "role": "user",
            "parts": [{
                "functionResponse": {
                    "name": name,
                    // The API wants an object here; a bare array or string is
                    // rejected, so anything that is not already a map is wrapped.
                    "response": match result {
                        serde_json::Value::Object(_) => result.clone(),
                        other => json!({ "result": other }),
                    },
                },
            }],
        }),
    }
}

impl Chatter for ChatClient<'_> {
    fn turn(
        &self,
        system: &str,
        history: &[Turn],
        tools: &[ToolDecl],
    ) -> Result<ChatReply, NetFailure> {
        if self.api_key.trim().is_empty() {
            return Err(NetFailure {
                code: ErrorCode::Unauthorized,
                message: "No Gemini key is set".into(),
                cause: "The assistant needs an API key, which is entered in Settings.".into(),
                actions: vec![ErrorAction::OpenSettings],
                retryable: false,
            });
        }

        let declarations: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect();

        let mut body = json!({
            "systemInstruction": { "parts": [{ "text": system }] },
            "contents": contents_of(history),
            "generationConfig": {
                // Warmer than the recommender's 0.4: this is conversation, and a model
                // held at near-zero answers the same way to "what else?" as it did to
                // the question before it.
                "temperature": 0.7,
                // The system prompt asks for "two or three sentences around the cards,
                // not an essay", and nothing made that true. Output tokens are the
                // expensive half, and a model that decides to be thorough was charged
                // for in full — the cap is what turns the instruction into a limit.
                // Generous against three sentences and a handful of cards; a reply that
                // needs more than this has misunderstood the question.
                "maxOutputTokens": MAX_REPLY_TOKENS,
                // Picking titles out of a list that has already been fetched does not
                // need deliberation, and on a 3.x model thinking is billed like any
                // other output. `thinkingBudget: 0` is refused by Flash Lite; `low` is
                // accepted, which is the floor this model offers.
                "thinkingConfig": { "thinkingLevel": "low" },
            },
        });
        if !declarations.is_empty() {
            body["tools"] = json!([{ "functionDeclarations": declarations }]);
        }

        let (status, raw) = self
            .http
            .post_json_with_status(
                &self.url(),
                &body.to_string(),
                &[(super::gemini::API_KEY_HEADER, self.api_key.as_str())],
            )
            // A transport failure here is the assistant's, not a provider's. Without
            // this it arrived as "Your provider didn't respond in time", with advice
            // about an Xtream port.
            .map_err(|e| super::gemini::unreachable(e, "The assistant"))?;
        if status >= 400 {
            return Err(super::gemini::rejected(status, &raw, "The assistant"));
        }
        parse_reply(&raw)
    }
}

/// Pull the text and any function calls out of the envelope.
///
/// Everything that can be absent is treated as absent rather than unwrapped: a reply in
/// an unexpected shape has to become a reported failure, because this runs on a thread
/// whose panic would take the app with it.
pub fn parse_reply(raw: &str) -> Result<ChatReply, NetFailure> {
    let envelope: serde_json::Value = serde_json::from_str(raw).map_err(|e| unreadable(&e))?;

    if let Some(message) = envelope
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
    {
        return Err(NetFailure {
            code: ErrorCode::Unknown,
            message: "The assistant could not answer".into(),
            cause: message.to_string(),
            actions: vec![ErrorAction::Retry],
            retryable: true,
        });
    }

    let parts = envelope
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array());

    // No parts at all is the shape a safety block takes, and it is worth saying so
    // rather than showing an empty bubble.
    let Some(parts) = parts else {
        let blocked = envelope
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("finishReason"))
            .and_then(|r| r.as_str())
            .unwrap_or("no reason given");
        return Err(NetFailure {
            code: ErrorCode::Unknown,
            message: "The assistant stopped without answering".into(),
            cause: format!("The model returned nothing ({blocked})."),
            actions: vec![ErrorAction::Retry],
            retryable: true,
        });
    };

    let mut text = String::new();
    let mut calls = Vec::new();
    for part in parts {
        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
            text.push_str(t);
        }
        if let Some(call) = part.get("functionCall") {
            if let Some(name) = call.get("name").and_then(|n| n.as_str()) {
                calls.push(ToolCall {
                    name: name.to_string(),
                    args: call.get("args").cloned().unwrap_or_else(|| json!({})),
                    thought_signature: part
                        .get("thoughtSignature")
                        .and_then(|t| t.as_str())
                        .map(str::to_string),
                });
            }
        }
    }

    let text = text.trim().to_string();
    Ok(ChatReply {
        text: (!text.is_empty()).then_some(text),
        calls,
    })
}

fn unreadable(e: &serde_json::Error) -> NetFailure {
    NetFailure {
        code: ErrorCode::Unknown,
        message: "The assistant's reply could not be read".into(),
        cause: format!("The response was not the shape this expects: {e}"),
        actions: vec![ErrorAction::Retry],
        retryable: true,
    }
}

#[cfg(test)]
mod tests {
    /// A call the model signed goes back exactly as it came.
    #[test]
    fn a_signed_call_keeps_its_signature() {
        let sent = contents_of(&[
            Turn::ToolCall {
                name: "search_library".into(),
                args: json!({ "text": "heat" }),
                thought_signature: Some("sig-abc".into()),
            },
            Turn::ToolResult {
                name: "search_library".into(),
                result: json!({ "hits": 2 }),
            },
        ]);
        assert_eq!(sent[0]["parts"][0]["thoughtSignature"], json!("sig-abc"));
        assert!(sent[0]["parts"][0]["functionCall"].is_object());
        assert!(sent[1]["parts"][0]["functionResponse"].is_object());
    }

    /// A call the model did **not** sign is never sent as a `functionCall`.
    ///
    /// Gemini requires `thoughtSignature` back on every one and rejects the request
    /// outright without it -- "Function call is missing a thought_signature in
    /// functionCall parts", naming the function and its position. Reported from the app,
    /// which is how this gap was found: the signature was being carried correctly and
    /// there were still two ways to emit a part that had none.
    #[test]
    fn an_unsigned_call_is_told_as_prose_instead() {
        let sent = contents_of(&[
            Turn::ToolCall {
                name: "search_library".into(),
                args: json!({ "text": "heat" }),
                thought_signature: None,
            },
            Turn::ToolResult {
                name: "search_library".into(),
                result: json!({ "hits": 2 }),
            },
        ]);
        // Neither half is a function part, so there is nothing for the API to reject.
        assert!(
            sent[0]["parts"][0].get("functionCall").is_none(),
            "{sent:?}"
        );
        assert!(
            sent[1]["parts"][0].get("functionResponse").is_none(),
            "{sent:?}"
        );
        // And the model is still told what happened, which is what the next round needs.
        let said = sent[0]["parts"][0]["text"].as_str().unwrap();
        assert!(said.contains("search_library"), "{said}");
        let back = sent[1]["parts"][0]["text"].as_str().unwrap();
        assert!(back.contains("hits"), "{back}");
        assert_eq!(sent[0]["role"], json!("model"));
        assert_eq!(sent[1]["role"], json!("user"));
    }

    /// Degrading is a pair, because a response is only legal after the call it answers.
    #[test]
    fn an_unsigned_call_does_not_leave_its_result_as_a_function_response() {
        let sent = contents_of(&[
            Turn::User { text: "hi".into() },
            Turn::ToolCall {
                name: "a".into(),
                args: json!({}),
                thought_signature: None,
            },
            Turn::ToolResult {
                name: "a".into(),
                result: json!({ "ok": true }),
            },
            // A signed one after it is untouched: the degradation is per pair, not a
            // mode the whole request falls into.
            Turn::ToolCall {
                name: "b".into(),
                args: json!({}),
                thought_signature: Some("sig-b".into()),
            },
            Turn::ToolResult {
                name: "b".into(),
                result: json!({ "ok": true }),
            },
        ]);
        assert!(sent[1]["parts"][0].get("functionCall").is_none());
        assert!(sent[2]["parts"][0].get("functionResponse").is_none());
        assert_eq!(sent[3]["parts"][0]["thoughtSignature"], json!("sig-b"));
        assert!(sent[4]["parts"][0]["functionResponse"].is_object());
    }

    /// Every `functionResponse` in a request answers a `functionCall` before it.
    ///
    /// The property rather than a case: an unpaired response is rejected whatever put it
    /// there, and the cap on lookups used to push one on its own.
    #[test]
    fn every_function_response_follows_its_call() {
        let history = vec![
            Turn::User { text: "hi".into() },
            Turn::ToolCall {
                name: "a".into(),
                args: json!({}),
                thought_signature: Some("s".into()),
            },
            Turn::ToolResult {
                name: "a".into(),
                result: json!({}),
            },
            Turn::Model {
                text: "done".into(),
            },
        ];
        let sent = contents_of(&history);
        for (i, entry) in sent.iter().enumerate() {
            if entry["parts"][0].get("functionResponse").is_some() {
                let before = &sent[i - 1];
                assert!(
                    before["parts"][0].get("functionCall").is_some(),
                    "entry {i} answers nothing: {sent:?}"
                );
            }
        }
    }

    use super::*;

    #[test]
    fn plain_text_is_a_final_answer() {
        let raw = json!({
            "candidates": [{ "content": { "parts": [{ "text": "Try Arrival." }] } }]
        })
        .to_string();

        let reply = parse_reply(&raw).unwrap();
        assert_eq!(reply.text.as_deref(), Some("Try Arrival."));
        assert!(reply.is_final());
    }

    #[test]
    fn a_function_call_is_not_final_and_carries_its_arguments() {
        let raw = json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "search_library",
                            "args": { "query": "heist", "limit": 10 }
                        }
                    }]
                }
            }]
        })
        .to_string();

        let reply = parse_reply(&raw).unwrap();
        assert!(!reply.is_final());
        assert_eq!(reply.calls.len(), 1);
        assert_eq!(reply.calls[0].name, "search_library");
        assert_eq!(reply.calls[0].args["query"], "heist");
        assert_eq!(reply.calls[0].args["limit"], 10);
    }

    /// Gemini narrates while it calls, and throwing the narration away makes the wait
    /// look like nothing happening.
    #[test]
    fn text_and_a_call_in_one_reply_both_survive() {
        let raw = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "Let me look at what you have." },
                        { "functionCall": { "name": "library_digest", "args": {} } }
                    ]
                }
            }]
        })
        .to_string();

        let reply = parse_reply(&raw).unwrap();
        assert_eq!(reply.text.as_deref(), Some("Let me look at what you have."));
        assert_eq!(reply.calls.len(), 1);
        assert!(!reply.is_final());
    }

    #[test]
    fn several_calls_in_one_turn_are_all_returned() {
        let raw = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "functionCall": { "name": "a", "args": {} } },
                        { "functionCall": { "name": "b", "args": { "x": 1 } } }
                    ]
                }
            }]
        })
        .to_string();

        let reply = parse_reply(&raw).unwrap();
        assert_eq!(reply.calls.len(), 2);
        assert_eq!(reply.calls[1].args["x"], 1);
    }

    #[test]
    fn an_api_error_is_reported_with_what_it_said() {
        let raw = json!({ "error": { "message": "API key not valid" } }).to_string();
        let err = parse_reply(&raw).unwrap_err();
        assert!(err.cause.contains("API key not valid"), "{err:?}");
    }

    /// A safety block comes back as a candidate with no parts, which would otherwise be
    /// an empty bubble with no explanation.
    #[test]
    fn a_reply_with_no_parts_says_so_rather_than_being_empty() {
        let raw = json!({ "candidates": [{ "finishReason": "SAFETY" }] }).to_string();
        let err = parse_reply(&raw).unwrap_err();
        assert!(err.message.contains("stopped without answering"), "{err:?}");
        assert!(err.cause.contains("SAFETY"), "{err:?}");
        assert!(err.retryable);
    }

    #[test]
    fn nonsense_is_a_failure_rather_than_a_panic() {
        assert!(parse_reply("not json").is_err());
        assert!(parse_reply("{}").is_err());
        assert!(parse_reply(r#"{"candidates":[]}"#).is_err());
    }

    /// The API rejects a `functionResponse` whose `response` is not an object, which is
    /// easy to hit because most of these tools naturally answer with a list.
    #[test]
    fn a_tool_result_that_is_a_list_is_wrapped_in_an_object() {
        let history = vec![Turn::ToolResult {
            name: "search_library".into(),
            result: json!([{ "id": 1 }, { "id": 2 }]),
        }];
        let contents = contents_of(&history);
        let response = &contents[0]["parts"][0]["functionResponse"]["response"];
        assert!(response.is_object(), "{response}");
        assert_eq!(response["result"][1]["id"], 2);

        // An object is passed through as it is.
        let history = vec![Turn::ToolResult {
            name: "library_digest".into(),
            result: json!({ "movies": 24658 }),
        }];
        let contents = contents_of(&history);
        assert_eq!(
            contents[0]["parts"][0]["functionResponse"]["response"]["movies"],
            24658
        );
    }

    /// The call and its result both have to stay in the history, in order, or the next
    /// round has no idea what it just looked up.
    #[test]
    fn the_history_keeps_calls_and_results_as_turns() {
        let history = vec![
            Turn::User {
                text: "something tense".into(),
            },
            Turn::ToolCall {
                name: "search_library".into(),
                args: json!({ "query": "thriller" }),
                thought_signature: Some("opaque-signature".into()),
            },
            Turn::ToolResult {
                name: "search_library".into(),
                result: json!({ "items": [] }),
            },
        ];
        let contents = contents_of(&history);
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(
            contents[1]["parts"][0]["functionCall"]["name"],
            "search_library"
        );
        // Beside `functionCall`, which is where the API looks for it. Without this the
        // next request is a 400: "Function call is missing a thought_signature in
        // functionCall parts." That was every tool-using question failing on its second
        // round, and it is the reason the assistant "gave errors".
        assert_eq!(
            contents[1]["parts"][0]["thoughtSignature"],
            "opaque-signature"
        );
        // The result arrives from the user's side, which is what the API specifies.
        assert_eq!(contents[2]["role"], "user");
    }

    #[test]
    fn no_key_is_refused_without_a_request() {
        let http = HttpClient::new(crate::http::HttpConfig::default()).unwrap();
        let client = ChatClient::new(&http, "   ");
        let err = client.turn("sys", &[], &[]).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unauthorized);
        assert!(!err.retryable);
    }
}
