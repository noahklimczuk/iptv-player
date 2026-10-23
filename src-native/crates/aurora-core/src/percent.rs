//! Percent-decoding, in one place.
//!
//! There were two copies of this, and only one of them was right. The M3U parser's
//! version decodes over bytes; the one in `aurora-ingest::source`, which reads the
//! username and password out of a pasted `get.php` URL, sliced a `&str` and cast each
//! byte to `char`. Both of those are wrong in ways that reached a viewer:
//!
//!   * **Slicing panics.** `s[i + 1..i + 3]` lands inside a multi-byte character
//!     whenever one follows a `%` — `"%\u{2013}"` is enough, and a URL pasted out of a
//!     chat client that smart-quoted it contains exactly that. `[profile.release]` sets
//!     `panic = "abort"`, and paste detection runs on every keystroke of the setup
//!     wizard's first field, so the window simply closed.
//!   * **`byte as char` is Latin-1.** `%C3%A9` decoded to `Ã©` rather than `é`. In the
//!     M3U parser that went back out as an HTTP header; in the paste parser it was the
//!     viewer's password, saved to Credential Manager in that form and rejected by the
//!     panel, with a masked field showing five dots where four belonged.
//!
//! Decoding into a `Vec<u8>` and interpreting the result as UTF-8 once at the end
//! removes both: there is no `&str` index to get wrong, and the bytes a provider
//! escaped are reassembled before anything decides what characters they are.

/// Percent-decode, over bytes. `+` becomes a space, as in a query string.
///
/// Anything that is not a complete, valid escape is left exactly as it was found: a
/// trailing `%`, `%2`, `%zz` and `%%41` all pass through untouched rather than being
/// guessed at. Invalid UTF-8 in the decoded bytes is replaced rather than rejected,
/// because a playlist is not a document this project gets to refuse.
pub fn decode(s: &str) -> String {
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

/// Percent-encode one path segment or one query value.
///
/// Everything outside the unreserved set of RFC 3986 — `A-Z a-z 0-9 - . _ ~` — goes out
/// as `%XX` over the UTF-8 bytes. That is deliberately more than either context strictly
/// requires, and it is the only rule that is correct in both: a password goes into
/// `/live/<user>/<pass>/<id>.ts` and into `timeshift.php?password=…`, and the characters
/// that break those two are not the same set.
///
/// What this is for: panels do issue passwords with punctuation in them, and the
/// credentials went into both forms verbatim. A `/` added a path segment, `#` truncated
/// everything after it into a fragment, `?` started a query, and in the query form `&`
/// or `=` rewrote the parameters. The provider then answered 401 or 404 for an address
/// that did not name what was meant — indistinguishable, from inside the app, from a
/// wrong password, which is the one diagnosis a viewer acts on by retyping a password
/// that was already right (R-33).
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_escapes_decode() {
        assert_eq!(decode("hunter2"), "hunter2");
        assert_eq!(decode("hunter%20two"), "hunter two");
        assert_eq!(decode("a+b"), "a b");
        assert_eq!(decode("%41%42%43"), "ABC");
        assert_eq!(decode("%4a%4A"), "JJ");
    }

    /// The Latin-1 cast, which silently corrupted a password.
    #[test]
    fn multibyte_escapes_reassemble_as_utf8() {
        assert_eq!(decode("caf%C3%A9"), "caf\u{e9}");
        assert_eq!(decode("p%C3%A4ss"), "p\u{e4}ss");
        assert_eq!(decode("%E2%82%AC"), "\u{20ac}");
        assert_eq!(decode("%F0%9F%98%80"), "\u{1f600}");
        // Four characters, not five. The byte-at-a-time version gave "cafÃ©".
        assert_eq!(decode("caf%C3%A9").chars().count(), 4);
    }

    /// A `%` with a multi-byte character after it. Slicing a `&str` here panicked, and
    /// `panic = "abort"` made that the end of the process.
    #[test]
    fn a_percent_before_a_multibyte_character_does_not_panic() {
        for s in [
            "%\u{e9}",     // 2-byte: the offsets happened to align
            "%\u{2013}",   // 3-byte en dash — this one panicked
            "%\u{2019}",   // 3-byte curly apostrophe, what a chat client produces
            "%\u{20ac}x",  // 3-byte euro, with something after it
            "%\u{1f600}",  // 4-byte emoji
            "a%\u{2013}b", // and not only at the start
        ] {
            let _ = decode(s);
        }
    }

    /// Every truncated or invalid escape a hostile string can end on. None may panic,
    /// and none may be guessed at.
    #[test]
    fn truncated_and_invalid_escapes_are_left_alone() {
        assert_eq!(decode("%"), "%");
        assert_eq!(decode("%2"), "%2");
        assert_eq!(decode("%zz"), "%zz");
        assert_eq!(decode("%2z"), "%2z");
        assert_eq!(decode("%%41"), "%A");
        assert_eq!(decode("abc%"), "abc%");
        assert_eq!(decode(""), "");
    }

    /// The characters that rewrote the address they were pasted into.
    #[test]
    fn the_punctuation_a_password_can_contain_is_escaped() {
        assert_eq!(encode("hunter2"), "hunter2");
        // Unreserved, and left alone: escaping these would also work, but it would stop
        // the URLs being comparable with the ones already stored.
        assert_eq!(encode("a-b.c_d~e"), "a-b.c_d~e");
        // A path segment's terminator, a fragment, a query, and the two that rewrite a
        // parameter list.
        assert_eq!(encode("p/w"), "p%2Fw");
        assert_eq!(encode("p#w"), "p%23w");
        assert_eq!(encode("p?w"), "p%3Fw");
        assert_eq!(encode("p&w=x"), "p%26w%3Dx");
        // `+` is a space once something decodes the query, so it cannot go out as `+`.
        assert_eq!(encode("p+w"), "p%2Bw");
        assert_eq!(encode("p w"), "p%20w");
        assert_eq!(encode("100%"), "100%25");
    }

    /// Non-ASCII is escaped per UTF-8 byte, which is what the decoder reassembles.
    #[test]
    fn what_is_encoded_decodes_back_to_itself() {
        for s in [
            "hunter2",
            "p/w",
            "p#w",
            "p?w",
            "p&w=x",
            "p+w",
            "p w",
            "100%",
            "caf\u{e9}",
            "p\u{e4}ss",
            "\u{20ac}5",
            "\u{1f600}",
            "a:b@c",
            "[]{}",
        ] {
            assert_eq!(
                decode(&encode(s)),
                s,
                "{s:?} did not survive the round trip"
            );
        }
    }

    /// Bytes that are not text survive as replacement characters rather than ending
    /// the parse.
    #[test]
    fn invalid_utf8_is_replaced_not_rejected() {
        let got = decode("%FF%FE");
        assert!(!got.is_empty());
        assert!(got.contains('\u{fffd}'), "{got:?}");
    }
}
