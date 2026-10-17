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

    /// Bytes that are not text survive as replacement characters rather than ending
    /// the parse.
    #[test]
    fn invalid_utf8_is_replaced_not_rejected() {
        let got = decode("%FF%FE");
        assert!(!got.is_empty());
        assert!(got.contains('\u{fffd}'), "{got:?}");
    }
}
