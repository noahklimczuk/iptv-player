//! Turning a programme title into a filename Windows will actually accept.
//!
//! Every part of this exists because Windows refuses something Linux allows, and the
//! failure mode is a recording that silently never appears. `Ratched: Season 1` is a
//! perfectly ordinary show title and an illegal filename.

/// Characters Win32 rejects outright in a path component.
const RESERVED_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Device names that are reserved whatever the extension: `CON.ts` is still `CON`.
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Leave room for the directory, the episode suffix and the extension inside MAX_PATH.
const MAX_STEM_LEN: usize = 120;

/// Make `title` safe to use as one component of a Windows path.
///
/// Returns `"untitled"` rather than an empty string, so a title made entirely of
/// illegal characters still produces a file instead of a path ending in a separator.
pub fn safe_filename(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for ch in title.chars() {
        // Control characters are rejected by the filesystem, not just by convention.
        if RESERVED_CHARS.contains(&ch) || (ch as u32) < 0x20 {
            // A separator rather than nothing: "S01:E02" should not become "S01E02".
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
    }

    let mut out: String = out.split_whitespace().collect::<Vec<_>>().join(" ");

    // Truncate on a character boundary — byte slicing a multi-byte title would panic.
    if out.chars().count() > MAX_STEM_LEN {
        out = out.chars().take(MAX_STEM_LEN).collect();
        out = out.trim_end().to_string();
    }

    // Windows silently strips trailing dots and spaces, so a name ending in one opens a
    // different file than the one you created.
    let trimmed = out.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let mut out = trimmed.to_string();

    if out.is_empty() {
        return "untitled".into();
    }

    // A reserved device name is only reserved as the whole stem, so suffixing clears it.
    let stem_upper = out.split('.').next().unwrap_or(&out).to_ascii_uppercase();
    if RESERVED_NAMES.contains(&stem_upper.as_str()) {
        out.push('_');
    }
    out
}

/// The filename for one recording: title, then a season/episode or date suffix so two
/// airings of the same show never collide.
pub fn recording_filename(
    title: &str,
    season: Option<u16>,
    episode: Option<u16>,
    air_start: i64,
) -> String {
    let stem = safe_filename(title);
    match (season, episode) {
        (Some(s), Some(e)) => format!("{stem} - S{s:02}E{e:02} - {air_start}.ts"),
        // Without an episode number the airtime is the only thing that distinguishes
        // two recordings of the same title.
        _ => format!("{stem} - {air_start}.ts"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_characters_become_separators_not_deletions() {
        assert_eq!(safe_filename("Ratched: Season 1"), "Ratched Season 1");
        assert_eq!(safe_filename("Who? What!"), "Who What!");
        assert_eq!(safe_filename("AC/DC Live"), "AC DC Live");
    }

    #[test]
    fn runs_of_whitespace_collapse() {
        assert_eq!(safe_filename("A  ::  B"), "A B");
    }

    #[test]
    fn control_characters_are_removed() {
        assert_eq!(safe_filename("News\u{7}at\u{1}Ten"), "News at Ten");
    }

    #[test]
    fn trailing_dots_and_spaces_are_stripped() {
        // Windows would open "Show" when you asked for "Show..." — better not to create
        // a name whose meaning changes on the way to disk.
        assert_eq!(safe_filename("Show..."), "Show");
        assert_eq!(safe_filename("Show   "), "Show");
        assert_eq!(safe_filename(" .Show. "), "Show");
    }

    #[test]
    fn reserved_device_names_are_escaped() {
        assert_eq!(safe_filename("CON"), "CON_");
        assert_eq!(safe_filename("nul"), "nul_");
        // Reserved even with an extension attached.
        assert_eq!(safe_filename("COM1.ts"), "COM1.ts_");
        // Not reserved when it is only part of a longer name.
        assert_eq!(safe_filename("Contact"), "Contact");
    }

    #[test]
    fn a_title_of_only_illegal_characters_still_produces_a_name() {
        assert_eq!(safe_filename("///"), "untitled");
        assert_eq!(safe_filename(""), "untitled");
        assert_eq!(safe_filename("   "), "untitled");
    }

    #[test]
    fn long_titles_are_truncated_on_a_character_boundary() {
        let out = safe_filename(&"é".repeat(400));
        assert_eq!(out.chars().count(), MAX_STEM_LEN);
    }

    #[test]
    fn non_ascii_titles_survive() {
        assert_eq!(safe_filename("Züri Brännt"), "Züri Brännt");
        assert_eq!(safe_filename("日本のニュース"), "日本のニュース");
    }

    #[test]
    fn episode_numbers_disambiguate_recordings() {
        assert_eq!(
            recording_filename("The Show", Some(2), Some(7), 1_700_000_000),
            "The Show - S02E07 - 1700000000.ts"
        );
        assert_eq!(
            recording_filename("The News", None, None, 1_700_000_000),
            "The News - 1700000000.ts"
        );
    }

    #[test]
    fn two_airings_of_the_same_untitled_programme_do_not_collide() {
        let a = recording_filename("Film", None, None, 100);
        let b = recording_filename("Film", None, None, 200);
        assert_ne!(a, b);
    }
}
