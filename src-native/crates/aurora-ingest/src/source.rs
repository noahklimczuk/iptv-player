//! What the user configured, and how a URL is derived from it.

use serde::{Deserialize, Serialize};

/// Credentials are never stored here — this holds only what is safe to persist.
/// The secret lives in the OS credential store (see `credentials`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SourceKind {
    /// An Xtream Codes panel: base URL plus a username/password pair.
    Xtream { base_url: String, username: String },
    /// A plain playlist URL or local file.
    M3u { url: String },
}

impl SourceKind {
    pub fn base_host(&self) -> &str {
        match self {
            SourceKind::Xtream { base_url, .. } => base_url,
            SourceKind::M3u { url } => url,
        }
    }
}

/// README §13: "if the user pastes a full Xtream `get.php` URL, parse out
/// host/username/password automatically."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastedXtream {
    pub base_url: String,
    pub username: String,
    pub password: String,
}

/// Recognise the shapes people actually paste.
///
/// Accepts `http://host:port/get.php?username=U&password=P&type=m3u_plus`, the
/// `player_api.php` equivalent, and a bare panel URL carrying the credentials as
/// query parameters. Returns `None` for anything that is just a playlist link.
pub fn parse_pasted_xtream(input: &str) -> Option<PastedXtream> {
    let trimmed = input.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return None;
    }

    let (before_query, query) = trimmed.split_once('?')?;
    let mut username = None;
    let mut password = None;
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key.to_ascii_lowercase().as_str() {
            "username" | "user" => username = Some(urldecode(value)),
            "password" | "pass" => password = Some(urldecode(value)),
            _ => {}
        }
    }

    let (username, password) = (username?, password?);
    if username.is_empty() || password.is_empty() {
        return None;
    }

    // Everything up to the last path segment is the panel root.
    let base_url = match before_query.rfind('/') {
        Some(idx) if idx > before_query.find("://").map(|i| i + 2).unwrap_or(0) => {
            before_query[..idx].to_string()
        }
        _ => before_query.to_string(),
    };

    Some(PastedXtream {
        base_url: base_url.trim_end_matches('/').to_string(),
        username,
        password,
    })
}

/// Whether an address looks like an Xtream panel root rather than a playlist link.
///
/// A provider that hands out a bare host with the username and password on a separate
/// line — which is how most of them do it — leaves nothing in the URL to detect. But the
/// shape is still a strong hint: a playlist link almost always carries a path, and
/// usually ends in `.m3u` or `.m3u8`. A bare host with nothing after it is a panel.
///
/// This only ever picks the *default* the wizard offers; the user can always say
/// otherwise. Getting it wrong costs one click, where offering no choice at all — as
/// this did — costs the whole first run.
pub fn looks_like_panel_root(input: &str) -> bool {
    let trimmed = input.trim().trim_end_matches('/');
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return false;
    }
    if trimmed.contains('?') {
        return false;
    }
    let after_scheme = match trimmed.find("://") {
        Some(i) => &trimmed[i + 3..],
        None => return false,
    };
    if after_scheme.is_empty() {
        return false;
    }
    // Nothing after host[:port] — no path at all.
    !after_scheme.contains('/')
}

fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { ' ' } else { b[i] as char });
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_get_php_url() {
        let got = parse_pasted_xtream(
            "http://example.com:8080/get.php?username=alice&password=hunter2&type=m3u_plus",
        )
        .unwrap();
        assert_eq!(got.base_url, "http://example.com:8080");
        assert_eq!(got.username, "alice");
        assert_eq!(got.password, "hunter2");
    }

    #[test]
    fn parses_a_player_api_url() {
        let got =
            parse_pasted_xtream("https://example.com/player_api.php?username=bob&password=s3cret")
                .unwrap();
        assert_eq!(got.base_url, "https://example.com");
        assert_eq!(got.username, "bob");
    }

    #[test]
    fn accepts_the_short_parameter_spellings() {
        let got = parse_pasted_xtream("http://example.com/get.php?user=carol&pass=pw123").unwrap();
        assert_eq!(got.username, "carol");
        assert_eq!(got.password, "pw123");
    }

    #[test]
    fn a_bare_host_is_recognised_as_a_panel_root() {
        // The shape a provider's credentials card actually has: a host, and the
        // username and password written underneath it.
        assert!(looks_like_panel_root("http://12345678.panel-host.example"));
        assert!(looks_like_panel_root("http://example.com:8080"));
        assert!(looks_like_panel_root("https://panel.example.com/"));
        assert!(looks_like_panel_root("  http://example.com  "));
    }

    #[test]
    fn a_playlist_link_is_not_a_panel_root() {
        assert!(!looks_like_panel_root("http://example.com/playlist.m3u"));
        assert!(!looks_like_panel_root(
            "http://example.com/get.php?username=a&password=b"
        ));
        assert!(!looks_like_panel_root("http://example.com/iptv/list.m3u8"));
        // Not a URL at all.
        assert!(!looks_like_panel_root("example.com"));
        assert!(!looks_like_panel_root("/home/me/list.m3u"));
        assert!(!looks_like_panel_root(""));
        assert!(!looks_like_panel_root("http://"));
    }

    #[test]
    fn url_decodes_credentials() {
        let got =
            parse_pasted_xtream("http://example.com/get.php?username=a%40b.com&password=p%20w")
                .unwrap();
        assert_eq!(got.username, "a@b.com");
        assert_eq!(got.password, "p w");
    }

    #[test]
    fn a_plain_playlist_link_is_not_an_xtream_paste() {
        assert!(parse_pasted_xtream("http://example.com/playlist.m3u").is_none());
        assert!(parse_pasted_xtream("http://example.com/list.m3u?token=abc").is_none());
    }

    #[test]
    fn non_urls_and_blanks_are_rejected() {
        for input in [
            "",
            "   ",
            "not a url",
            "ftp://example.com/get.php?username=a&password=b",
        ] {
            assert!(parse_pasted_xtream(input).is_none(), "{input:?}");
        }
    }

    #[test]
    fn empty_credentials_are_rejected() {
        assert!(parse_pasted_xtream("http://example.com/get.php?username=&password=").is_none());
    }

    #[test]
    fn a_trailing_slash_does_not_end_up_in_the_base_url() {
        let got =
            parse_pasted_xtream("http://example.com:8080/get.php?username=a&password=b").unwrap();
        assert!(!got.base_url.ends_with('/'), "{}", got.base_url);
    }
}
