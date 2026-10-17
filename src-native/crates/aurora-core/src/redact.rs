//! Stripping credentials out of a URL before it reaches a log line (README C10).
//!
//! In `aurora-core` rather than in `aurora-ingest`, where it used to live, because
//! `aurora-player` needs it and cannot depend on a crate that carries `reqwest`. That
//! was not a tidiness problem: `mpv.rs` logged mpv's own output, which names the URL it
//! failed to open, through no redaction at all, and `docs/PRIVACY.md` says plainly that
//! "every URL written to the log goes through `http::redact` first" (R-62).
//!
//! Pure string work, like everything else here. `http::redact` is re-exported from this
//! function so the call sites and the privacy note keep the name they use.

/// `1234.ts`, or just `1234` — the last segment of an Xtream stream URL.
///
/// **The extension is optional, and that is the whole point of this function.** It used
/// to be required, and the subscription this project is measured against serves every
/// one of its 6,699 live channels as `http://host/<user>/<pass>/<id>` with no extension
/// at all — so not one of them was redacted, while its films and series, which do carry
/// `.mkv` and `.mp4`, were. `docs/PRIVACY.md` names this exact form and writes it with a
/// `.ts` on the end, which is how a documented defence came to miss the commonest URL
/// the app handles.
///
/// An all-digit segment in the fourth position of a four-segment URL is specific enough:
/// `/a/b/c` and `/epg/guide/today.xml` are both left alone, and the cost of being wrong
/// here is a log line that says `***` where it could have said a path.
fn looks_like_stream_id(segment: &str) -> bool {
    let (id, ext) = match segment.rsplit_once('.') {
        Some((id, ext)) => (id, Some(ext)),
        None => (segment, None),
    };
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    match ext {
        Some(ext) => !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()),
        None => true,
    }
}

/// Strip credentials before a URL reaches a log line (README C10).
pub fn redact(url: &str) -> String {
    let mut out = url.to_string();
    // Xtream carries them as query parameters. Every occurrence, not the first: a
    // panel that repeats a parameter, or a URL assembled from two of them, would
    // otherwise leave the second copy in the log.
    for key in [
        "password", "pass", "token", "username", "user", "api_key", "apikey",
    ] {
        let needle = format!("{key}=");
        let mut from = 0usize;
        while let Some(offset) = out[from..].to_ascii_lowercase().find(&needle) {
            let start = from + offset;
            let value_start = start + needle.len();
            let end = out[value_start..]
                .find('&')
                .map(|i| value_start + i)
                .unwrap_or(out.len());
            out.replace_range(value_start..end, "***");
            from = value_start + 3;
            if from >= out.len() {
                break;
            }
        }
    }
    // ...and path segments carry them in stream URLs: /live/user/pass/123.ts
    if let Some(idx) = out.find("://") {
        let (scheme, rest) = out.split_at(idx + 3);
        let mut segments: Vec<&str> = rest.split('/').collect();
        let mut redacted = false;
        for marker in ["live", "movie", "series"] {
            if let Some(pos) = segments.iter().position(|s| *s == marker) {
                for seg in segments.iter_mut().skip(pos + 1).take(2) {
                    *seg = "***";
                }
                redacted = true;
            }
        }
        // Many panels serve `http://host/<user>/<pass>/<id>.ts` with no marker at all
        // — `aurora_core::catchup::split_xtream_stream_url` supports exactly that
        // form, so the codebase already knows it exists. Without this the commonest
        // stream URL there is went into the log intact.
        if !redacted && segments.len() == 4 && looks_like_stream_id(segments[3]) {
            segments[1] = "***";
            segments[2] = "***";
        }
        out = format!("{scheme}{}", segments.join("/"));
    }
    // userinfo form: http://user:pass@host
    if let Some(at) = out.find('@') {
        if let Some(scheme_end) = out.find("://") {
            if at > scheme_end {
                out.replace_range(scheme_end + 3..at, "***");
            }
        }
    }
    out
}

/// The same, for a line of prose that happens to contain a URL.
///
/// `redact` takes a URL and nothing else: it splits on the first `://` and treats
/// everything after it as path segments, so the trailing words of a sentence end up
/// inside the last segment and the all-digit test that identifies a stream id fails.
/// Measured on the exact line this was written for:
///
/// ```text
/// redact("Failed to open http://panel.example/bob/hunter2/1234: Server returned 403")
///   -> unchanged, password and all
/// ```
///
/// Which is the form mpv's log takes — never a bare URL, always ffmpeg's sentence about
/// one. So the URLs are picked out of the text first, each one redacted on its own, and
/// trailing punctuation is left where it was: a `:` or a `,` after the last path segment
/// is the sentence's, not the URL's, and leaving it attached is what stopped the id
/// being recognised (R-62).
pub fn redact_in_text(text: &str) -> String {
    /// What ends a URL in running text. Not `/` or `?` or `&`, which are part of one.
    fn is_boundary(c: char) -> bool {
        c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|' | '\\')
    }

    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        // Back up to the start of the scheme, which is the run of letters before `://`.
        let scheme_start = rest[..at]
            .rfind(|c: char| !c.is_ascii_alphabetic())
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = rest[at..]
            .find(is_boundary)
            .map(|i| at + i)
            .unwrap_or(rest.len());
        let token = &rest[scheme_start..end];
        // A sentence's punctuation, not the URL's.
        let trimmed = token.trim_end_matches(|c: char| ".,;:!?)]}".contains(c));
        let tail = &token[trimmed.len()..];

        out.push_str(&rest[..scheme_start]);
        out.push_str(&redact(trimmed));
        out.push_str(tail);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line ffmpeg writes when a stream refuses, which is prose and not a URL.
    ///
    /// `redact` on its own leaves this completely untouched: everything after `://` is
    /// read as path segments, so the last one is `1234: Server returned 403 Forbidden`,
    /// which is not an all-digit stream id. mpv's log is the only thing in the app that
    /// hands a URL over inside a sentence, which is why this went unnoticed.
    #[test]
    fn a_url_inside_a_sentence_is_redacted() {
        let line =
            "Failed to open http://panel.example/bob/hunter2/1234: Server returned 403 Forbidden";
        let out = redact_in_text(line);
        assert!(!out.contains("hunter2"), "{out}");
        assert!(!out.contains("/bob/"), "{out}");
        // The sentence survives, including the status that makes it worth logging.
        assert!(out.starts_with("Failed to open "), "{out}");
        assert!(out.ends_with(": Server returned 403 Forbidden"), "{out}");
    }

    #[test]
    fn several_urls_in_one_line_are_all_redacted() {
        let out = redact_in_text(
            "redirect http://a.example/bob/hunter2/1 -> http://b.example/bob/hunter2/2 failed",
        );
        assert_eq!(out.matches("hunter2").count(), 0, "{out}");
        assert!(out.contains(" -> "), "{out}");
    }

    #[test]
    fn a_line_with_no_url_is_returned_as_it_was() {
        for line in [
            "Using hardware decoding (d3d11va).",
            "AO: [wasapi] 48000Hz stereo 2ch float",
            "",
        ] {
            assert_eq!(redact_in_text(line), line);
        }
    }

    /// Quoted and bracketed forms, which is how mpv writes a URL about half the time.
    #[test]
    fn quotes_and_brackets_around_a_url_are_left_in_place() {
        let out = redact_in_text("cannot open \"http://panel.example/bob/hunter2/9\" (eof)");
        assert!(!out.contains("hunter2"), "{out}");
        assert!(out.contains("\"http://panel.example/***/***/9\""), "{out}");
    }

    #[test]
    fn redacts_xtream_query_credentials() {
        let got =
            redact("http://example.com/player_api.php?username=alice&password=hunter2&action=x");
        assert!(!got.contains("alice"), "{got}");
        assert!(!got.contains("hunter2"), "{got}");
        assert!(got.contains("action=x"), "non-secret params survive: {got}");
    }

    #[test]
    fn redacts_credentials_in_stream_paths() {
        let got = redact("http://example.com/live/alice/hunter2/123.ts");
        assert!(!got.contains("alice"), "{got}");
        assert!(!got.contains("hunter2"), "{got}");
        assert!(got.contains("123.ts"), "{got}");
    }

    /// The commonest stream URL a panel serves, and the one `redact` walked past.
    ///
    /// Path redaction only fired after a literal `live`, `movie` or `series` segment.
    /// Plenty of panels serve `http://host/<user>/<pass>/<id>.ts` with no marker —
    /// `aurora_core::catchup::split_xtream_stream_url` supports exactly that form —
    /// and every retry logs the URL it is retrying.
    #[test]
    fn a_markerless_user_pass_id_url_is_redacted() {
        let out = redact("http://panel.example/bob/hunter2/1234.ts");
        assert!(!out.contains("bob"), "{out}");
        assert!(!out.contains("hunter2"), "{out}");
        assert!(
            out.contains("1234.ts"),
            "the stream id is not a secret: {out}"
        );
    }

    /// The form this subscription actually serves, which carries no extension.
    ///
    /// Measured against the real playlist: 233,650 film and series URLs end in `.mkv`,
    /// `.mp4` or `.avi` and were redacted; all 6,699 live URLs are
    /// `http://host/<user>/<pass>/<id>` with nothing after the id, and none of them was.
    /// The difference was one character, and live is the kind a session opens repeatedly.
    #[test]
    fn a_markerless_url_with_no_extension_is_redacted() {
        let out = redact("http://panel.example/bob/hunter2/1234");
        assert!(!out.contains("bob"), "{out}");
        assert!(!out.contains("hunter2"), "{out}");
        assert!(out.contains("1234"), "the stream id is not a secret: {out}");
    }

    #[test]
    fn a_repeated_credential_parameter_is_redacted_every_time() {
        let out = redact(
            "http://panel.example/player_api.php?username=bob&password=hunter2\
             &action=get_series&password=hunter2",
        );
        assert!(!out.contains("hunter2"), "{out}");
        assert!(!out.contains("bob"), "{out}");
        assert!(out.contains("action=get_series"), "{out}");
    }

    /// The shape must be specific enough not to mangle an ordinary path.
    #[test]
    fn an_ordinary_three_segment_path_is_left_alone() {
        for url in [
            "http://example.com/epg/guide/today.xml",
            "http://example.com/a/b/c",
            "http://example.com/one/two/three.html",
        ] {
            assert_eq!(redact(url), url, "{url}");
        }
    }

    #[test]
    fn redacts_userinfo() {
        let got = redact("http://alice:hunter2@example.com/x");
        assert!(!got.contains("hunter2"), "{got}");
        assert!(got.contains("example.com"), "{got}");
    }

    #[test]
    fn a_metadata_api_key_is_redacted_too() {
        // TMDB carries its key in the query string, and a key in a log line is a leaked
        // credential exactly like a provider password is.
        let got = redact("https://api.themoviedb.org/3/search/movie?api_key=abcd1234&query=Heat");
        assert!(!got.contains("abcd1234"), "{got}");
        assert!(
            got.contains("query=Heat"),
            "the rest of the URL is still readable"
        );

        let got = redact("https://example.com/x?apikey=secret");
        assert!(!got.contains("secret"), "{got}");
    }

    #[test]
    fn redaction_leaves_a_clean_url_alone() {
        let url = "http://example.com/epg.xml.gz";
        assert_eq!(redact(url), url);
    }
}
