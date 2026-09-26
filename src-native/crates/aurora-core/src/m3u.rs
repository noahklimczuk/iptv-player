//! Streaming M3U / M3U8 playlist parser.
//!
//! README §4.2: real playlists are broken — BOMs, CRLF/LF mixes, unescaped quotes, duplicate
//! entries, missing commas, non-UTF-8 encodings, 400 MB files. This parser reads line by line,
//! never buffers the whole document, never panics, and degrades to a warning rather than losing
//! the rest of the file.

use std::collections::HashSet;
use std::io::BufRead;

use crate::classify;
use crate::model::{Catchup, HttpOptions, ParseWarning, PlaylistEntry, PlaylistParseResult};

/// Playlist-level metadata carried on the `#EXTM3U` header line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaylistHeader {
    /// `url-tvg` / `x-tvg-url` — the provider's own EPG endpoint.
    pub epg_urls: Vec<String>,
    pub name: Option<String>,
}

#[derive(Debug, Default)]
struct Pending {
    name: Option<String>,
    attrs: Vec<(String, String)>,
    group_override: Option<String>,
    http: HttpOptions,
    line: usize,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.name.is_none() && self.attrs.is_empty() && self.http.is_empty()
    }
}

#[derive(Debug)]
pub struct Parsed {
    pub header: PlaylistHeader,
    pub result: PlaylistParseResult,
}

/// Parse a playlist from any `BufRead`. Invalid UTF-8 is replaced rather than rejected, so a
/// Latin-1 playlist yields slightly mangled names instead of zero channels.
pub fn parse<R: BufRead>(reader: R) -> Parsed {
    let mut header = PlaylistHeader::default();
    let mut result = PlaylistParseResult::default();
    let mut pending = Pending::default();
    let mut seen_urls: HashSet<String> = HashSet::new();
    let mut saw_header = false;

    for (idx, raw) in reader.split(b'\n').enumerate() {
        let line_no = idx + 1;
        let bytes = match raw {
            Ok(b) => b,
            Err(e) => {
                result.warnings.push(ParseWarning {
                    line: line_no,
                    message: format!("unreadable line: {e}"),
                });
                continue;
            }
        };

        let mut line = String::from_utf8_lossy(&bytes).into_owned();
        // Strip a UTF-8 BOM on the first line, and any stray CR from CRLF files.
        if line_no == 1 {
            line = line.trim_start_matches('\u{feff}').to_string();
        }
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }

        if let Some(header_attrs) = line.strip_prefix("#EXTM3U") {
            saw_header = true;
            for (k, v) in parse_attributes(header_attrs) {
                match k.as_str() {
                    "url-tvg" | "x-tvg-url" | "tvg-url" => {
                        for u in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                            header.epg_urls.push(u.to_string());
                        }
                    }
                    "name" => header.name = Some(v),
                    _ => {}
                }
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            if !pending.is_empty() {
                result.warnings.push(ParseWarning {
                    line: pending.line,
                    message: "entry had no URL and was dropped".into(),
                });
                result.skipped += 1;
            }
            let (meta, name) = split_extinf(rest);
            pending = Pending {
                name: Some(name.trim().to_string()),
                attrs: parse_attributes(meta),
                group_override: None,
                http: HttpOptions::default(),
                line: line_no,
            };
            continue;
        }

        if let Some(group) = line.strip_prefix("#EXTGRP:") {
            pending.group_override = Some(group.trim().to_string());
            continue;
        }

        if let Some(opt) = line.strip_prefix("#EXTVLCOPT:") {
            let (k, v) = split_once_trim(opt, '=');
            match k.to_ascii_lowercase().as_str() {
                "http-user-agent" => pending.http.user_agent = Some(v.to_string()),
                "http-referrer" | "http-referer" => pending.http.referrer = Some(v.to_string()),
                "http-origin" => pending.http.origin = Some(v.to_string()),
                _ => {}
            }
            continue;
        }

        if let Some(prop) = line.strip_prefix("#KODIPROP:") {
            let (k, v) = split_once_trim(prop, '=');
            match k.to_ascii_lowercase().as_str() {
                "inputstream.adaptive.stream_headers" | "inputstream.adaptive.manifest_headers" => {
                    for part in v.split('&') {
                        let (hk, hv) = split_once_trim(part, '=');
                        match hk.to_ascii_lowercase().as_str() {
                            "user-agent" => pending.http.user_agent = Some(urldecode(hv)),
                            "referer" | "referrer" => pending.http.referrer = Some(urldecode(hv)),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            continue;
        }

        if let Some(name) = line.strip_prefix("#PLAYLIST:") {
            header.name = Some(name.trim().to_string());
            continue;
        }

        if line.starts_with('#') {
            continue; // comment or a directive we do not model
        }

        // A bare line is a URL.
        if !looks_like_url(line) {
            result.warnings.push(ParseWarning {
                line: line_no,
                message: format!("skipped unusable URL: {}", truncate(line, 80)),
            });
            result.skipped += 1;
            pending = Pending::default();
            continue;
        }

        if !seen_urls.insert(line.to_string()) {
            result.skipped += 1;
            pending = Pending::default();
            continue;
        }

        let entry = build_entry(std::mem::take(&mut pending), line, line_no);
        result.entries.push(entry);
    }

    if !pending.is_empty() {
        result.warnings.push(ParseWarning {
            line: pending.line,
            message: "trailing entry had no URL and was dropped".into(),
        });
        result.skipped += 1;
    }
    if !saw_header && result.entries.is_empty() {
        result.warnings.push(ParseWarning {
            line: 1,
            message: "no #EXTM3U header and no entries — is this a playlist?".into(),
        });
    }

    Parsed { header, result }
}

fn build_entry(pending: Pending, url: &str, line_no: usize) -> PlaylistEntry {
    let get = |key: &str| -> Option<String> {
        pending
            .attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.clone())
            .filter(|v| !v.trim().is_empty())
    };

    let name = pending
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| get("tvg-name"))
        .unwrap_or_else(|| "Unnamed".to_string());

    let group = pending
        .group_override
        .clone()
        .or_else(|| get("group-title"));

    let catchup = get("catchup")
        .or_else(|| get("catchup-type"))
        .map(|mode| Catchup {
            mode,
            source: get("catchup-source"),
            days: get("catchup-days")
                .and_then(|d| d.parse().ok())
                .unwrap_or(7),
        });

    let mut http = pending.http;
    if http.user_agent.is_none() {
        http.user_agent = get("user-agent");
    }

    let kind = classify::classify(&name, url, group.as_deref());

    PlaylistEntry {
        name,
        url: url.to_string(),
        kind,
        tvg_id: get("tvg-id"),
        tvg_name: get("tvg-name"),
        logo: get("tvg-logo").or_else(|| get("logo")),
        group,
        number: get("tvg-chno")
            .or_else(|| get("tvg-channel-number"))
            .or_else(|| get("channel-number"))
            .and_then(|n| n.trim().parse().ok()),
        shift_minutes: parse_shift(get("tvg-shift").or_else(|| get("timeshift"))),
        language: get("tvg-language"),
        country: get("tvg-country"),
        is_radio: get("radio").map(|v| is_truthy(&v)).unwrap_or(false),
        catchup,
        http,
        // An `#EXTINF` line has no agreed attribute for either, so a playlist in this
        // format contributes neither. They arrive from an Xtream panel instead, which
        // sends both for every VOD row.
        rating: None,
        added_at: None,
        source_line: line_no,
    }
}

/// `tvg-shift` is in hours and may be fractional or negative (`-1.5`).
fn parse_shift(value: Option<String>) -> i32 {
    value
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map(|h| (h * 60.0).round() as i32)
        .unwrap_or(0)
}

fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn looks_like_url(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    ["http://", "https://", "rtsp://", "rtmp://", "rtmps://", "udp://", "rtp://", "file://"]
        .iter()
        .any(|p| lower.starts_with(p))
        // Bare UNC and Windows drive paths, which playlists pointing at local files use.
        || line.starts_with("\\\\")
        || is_drive_path(line)
}

/// `C:\Videos\clip.mkv` — a drive letter, a colon, and a separator.
///
/// The separator is the part that was missing. Testing only for a colon in the
/// second byte accepted `a:b`, `1:30` and anything else whose second character
/// happens to be one, so a stray line in a playlist became an entry with a URL
/// nothing can play, rather than a warning naming the line.
fn is_drive_path(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// Split the `#EXTINF:` payload into its attribute section and the display name.
///
/// The display name follows the first comma that is *outside* a quoted attribute value —
/// which matters because `group-title="Movies, Action"` contains a comma.
fn split_extinf(rest: &str) -> (&str, &str) {
    let bytes = rest.as_bytes();
    let mut in_quotes = false;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => in_quotes = !in_quotes,
            b',' if !in_quotes => return (&rest[..i], &rest[i + 1..]),
            _ => {}
        }
    }
    // Malformed: no comma at all. Treat the whole thing as attributes with no name.
    (rest, "")
}

/// Parse `key="value"` / `key=value` pairs, tolerating unescaped junk between them.
fn parse_attributes(input: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Seek the start of a key.
        while i < chars.len() && !is_key_char(chars[i]) {
            i += 1;
        }
        let key_start = i;
        while i < chars.len() && is_key_char(chars[i]) {
            i += 1;
        }
        if key_start == i {
            break;
        }
        let key: String = chars[key_start..i].iter().collect();

        // Require an '=' to treat it as an attribute (skips the leading duration token).
        let save = i;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '=' {
            i = save;
            continue;
        }
        i += 1;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }

        let value: String = if i < chars.len() && chars[i] == '"' {
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            let v: String = chars[start..i].iter().collect();
            if i < chars.len() {
                i += 1; // closing quote
            }
            v
        } else {
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
            chars[start..i].iter().collect()
        };

        out.push((key.to_ascii_lowercase(), value));
    }
    out
}

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'
}

fn split_once_trim(s: &str, sep: char) -> (&str, &str) {
    match s.split_once(sep) {
        Some((a, b)) => (a.trim(), b.trim()),
        None => (s.trim(), ""),
    }
}

/// Percent-decode, over bytes.
///
/// Two things it has to get right, both of which the obvious `&str` version gets
/// wrong. Slicing `s[i + 1..i + 3]` to read the two hex digits panics whenever those
/// offsets land inside a multi-byte character — `"%a\u{e9}"` is enough — and this
/// parser's whole contract is that a broken playlist costs a warning, not the
/// process. And `byte as char` is a Latin-1 cast, so `%C3%A9` decoded to `Ã©`
/// instead of `é`, in a string that goes back out as an HTTP header.
///
/// Decoding into a `Vec<u8>` and interpreting the result as UTF-8 once at the end
/// removes both: there is no `&str` index to get wrong, and the bytes a provider
/// escaped are reassembled before anything decides what characters they are.
fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(hi), Some(lo)) = (hex_digit(b[i + 1]), hex_digit(b[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MediaKind;

    fn parse_str(s: &str) -> Parsed {
        parse(std::io::Cursor::new(s.as_bytes().to_vec()))
    }

    #[test]
    fn parses_a_normal_playlist() {
        let p = parse_str(
            r#"#EXTM3U url-tvg="https://example.com/epg.xml.gz"
#EXTINF:-1 tvg-id="cnn.us" tvg-name="CNN" tvg-logo="https://example.com/cnn.png" tvg-chno="202" group-title="News",CNN HD
https://example.com/live/cnn.ts
"#,
        );
        assert_eq!(p.header.epg_urls, vec!["https://example.com/epg.xml.gz"]);
        assert_eq!(p.result.entries.len(), 1);
        let e = &p.result.entries[0];
        assert_eq!(e.name, "CNN HD");
        assert_eq!(e.tvg_id.as_deref(), Some("cnn.us"));
        assert_eq!(e.number, Some(202));
        assert_eq!(e.group.as_deref(), Some("News"));
        assert_eq!(e.kind, MediaKind::Live);
    }

    #[test]
    fn comma_inside_a_quoted_attribute_does_not_split_the_name() {
        let p = parse_str(
            "#EXTM3U\n#EXTINF:-1 group-title=\"Movies, Action\",Die Hard\nhttps://example.com/m/1.mkv\n",
        );
        let e = &p.result.entries[0];
        assert_eq!(e.name, "Die Hard");
        assert_eq!(e.group.as_deref(), Some("Movies, Action"));
    }

    #[test]
    fn survives_a_utf8_bom_and_crlf() {
        let p = parse_str("\u{feff}#EXTM3U\r\n#EXTINF:-1,BBC One\r\nhttps://example.com/1.ts\r\n");
        assert_eq!(p.result.entries.len(), 1);
        assert_eq!(p.result.entries[0].name, "BBC One");
    }

    #[test]
    fn one_broken_line_does_not_lose_the_rest() {
        let p = parse_str(
            "#EXTM3U\n\
             #EXTINF:-1,Good One\nhttps://example.com/a.ts\n\
             this-is-not-a-url\n\
             #EXTINF:-1,Good Two\nhttps://example.com/b.ts\n",
        );
        assert_eq!(p.result.entries.len(), 2);
        assert_eq!(p.result.skipped, 1);
        assert!(!p.result.warnings.is_empty());
    }

    #[test]
    fn extinf_with_no_url_is_reported_not_merged() {
        let p =
            parse_str("#EXTM3U\n#EXTINF:-1,Orphan\n#EXTINF:-1,Real\nhttps://example.com/r.ts\n");
        assert_eq!(p.result.entries.len(), 1);
        assert_eq!(p.result.entries[0].name, "Real");
        assert!(p
            .result
            .warnings
            .iter()
            .any(|w| w.message.contains("no URL")));
    }

    #[test]
    fn deduplicates_identical_urls() {
        let p = parse_str(
            "#EXTM3U\n#EXTINF:-1,A\nhttps://example.com/x.ts\n#EXTINF:-1,A again\nhttps://example.com/x.ts\n",
        );
        assert_eq!(p.result.entries.len(), 1);
        assert_eq!(p.result.skipped, 1);
    }

    #[test]
    fn reads_extvlcopt_headers() {
        let p = parse_str(
            "#EXTM3U\n#EXTINF:-1,Ch\n#EXTVLCOPT:http-user-agent=AuroraAgent/1.0\n#EXTVLCOPT:http-referrer=https://example.com/\nhttps://example.com/s.ts\n",
        );
        let e = &p.result.entries[0];
        assert_eq!(e.http.user_agent.as_deref(), Some("AuroraAgent/1.0"));
        assert_eq!(e.http.referrer.as_deref(), Some("https://example.com/"));
    }

    #[test]
    fn extgrp_applies_when_group_title_is_absent() {
        let p = parse_str("#EXTM3U\n#EXTINF:-1,Ch\n#EXTGRP:Sports\nhttps://example.com/s.ts\n");
        assert_eq!(p.result.entries[0].group.as_deref(), Some("Sports"));
    }

    #[test]
    fn parses_catchup_and_shift() {
        let p = parse_str(
            "#EXTM3U\n#EXTINF:-1 catchup=\"shift\" catchup-days=\"5\" tvg-shift=\"-1.5\",Ch\nhttps://example.com/s.ts\n",
        );
        let e = &p.result.entries[0];
        let c = e.catchup.as_ref().unwrap();
        assert_eq!(c.mode, "shift");
        assert_eq!(c.days, 5);
        assert_eq!(e.shift_minutes, -90);
    }

    #[test]
    fn unquoted_attribute_values_are_accepted() {
        let p =
            parse_str("#EXTM3U\n#EXTINF:-1 tvg-chno=5 tvg-id=abc,Ch\nhttps://example.com/s.ts\n");
        let e = &p.result.entries[0];
        assert_eq!(e.number, Some(5));
        assert_eq!(e.tvg_id.as_deref(), Some("abc"));
    }

    #[test]
    fn empty_input_warns_instead_of_panicking() {
        let p = parse_str("");
        assert!(p.result.entries.is_empty());
        assert!(!p.result.warnings.is_empty());
    }

    #[test]
    fn invalid_utf8_is_replaced_not_fatal() {
        let bytes = b"#EXTM3U\n#EXTINF:-1,Caf\xe9 TV\nhttps://example.com/c.ts\n".to_vec();
        let p = parse(std::io::Cursor::new(bytes));
        assert_eq!(p.result.entries.len(), 1);
    }

    /// The crash this parser's own header said could not happen.
    ///
    /// `%a` followed by a two-byte character put byte index 3 inside `é`, and slicing
    /// a `&str` there panics. `[profile.release]` sets `panic = "abort"`, so a
    /// playlist line like this one did not raise an error — it ended the process
    /// mid-import.
    #[test]
    fn a_percent_escape_before_a_multibyte_character_does_not_panic() {
        let p = parse_str(
            "#EXTM3U\n#EXTINF:-1,Ch\n\
             #KODIPROP:inputstream.adaptive.stream_headers=User-Agent=%a\u{e9}\n\
             https://example.com/s.ts\n",
        );
        assert_eq!(p.result.entries.len(), 1);
        // Whatever it decoded to, it got here.
        assert!(p.result.entries[0].http.user_agent.is_some());
    }

    /// Every truncated escape a hostile file can end on, none of which may panic.
    #[test]
    fn truncated_and_invalid_escapes_are_left_alone() {
        for tail in ["%", "%2", "%zz", "%2z", "%\u{e9}", "\u{e9}%", "%%41"] {
            let p = parse_str(&format!(
                "#EXTM3U\n#EXTINF:-1,Ch\n\
                 #KODIPROP:inputstream.adaptive.stream_headers=User-Agent={tail}\n\
                 https://example.com/s.ts\n"
            ));
            assert_eq!(p.result.entries.len(), 1, "{tail:?}");
        }
    }

    #[test]
    fn percent_escapes_decode_to_utf8_not_latin1() {
        let p = parse_str(
            "#EXTM3U\n#EXTINF:-1,Ch\n\
             #KODIPROP:inputstream.adaptive.stream_headers=User-Agent=Caf%C3%A9%20TV\n\
             https://example.com/s.ts\n",
        );
        // `byte as char` gave "CafÃ© TV" here, which is what then went out as a header.
        assert_eq!(
            p.result.entries[0].http.user_agent.as_deref(),
            Some("Caf\u{e9} TV")
        );
    }

    /// `line.as_bytes()[1] == b':'` called every one of these a Windows path, so a
    /// stray line in a playlist became a channel with an unplayable URL instead of a
    /// warning somebody could read.
    #[test]
    fn a_bare_colon_line_is_not_mistaken_for_a_windows_path() {
        let p = parse_str(
            "#EXTM3U\n\
             #EXTINF:-1,Junk\n\
             a:b\n\
             #EXTINF:-1,Also junk\n\
             1:30\n\
             #EXTINF:-1,Real\n\
             C:\\Videos\\clip.mkv\n",
        );
        assert_eq!(p.result.entries.len(), 1);
        assert_eq!(p.result.entries[0].name, "Real");
        assert_eq!(p.result.skipped, 2);
    }

    #[test]
    fn missing_comma_does_not_abort_the_file() {
        let p = parse_str(
            "#EXTM3U\n#EXTINF:-1 tvg-id=\"x\"\nhttps://example.com/a.ts\n#EXTINF:-1,Fine\nhttps://example.com/b.ts\n",
        );
        assert_eq!(p.result.entries.len(), 2);
        // The nameless one falls back to tvg-name/Unnamed rather than being dropped.
        assert_eq!(p.result.entries[0].tvg_id.as_deref(), Some("x"));
    }
}
