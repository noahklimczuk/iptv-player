//! Title cleaning and normalization.
//!
//! IPTV stream names are hostile: `US| CNN HD`, `Inception (2010) 1080p WEB-DL x265 MULTI`,
//! `FR - TF1 FHD`. Matching anything (TMDB, EPG channels, duplicate collapsing) requires
//! getting a clean title, a year, and a comparable key out of that.

use std::sync::OnceLock;

use regex::Regex;

/// Quality / source / codec noise stripped from VOD titles.
const RELEASE_TAGS: &[&str] = &[
    "2160p",
    "1080p",
    "1080i",
    "720p",
    "576p",
    "480p",
    "360p",
    "4k",
    "8k",
    "uhd",
    "fhd",
    "hd",
    "sd",
    "hq",
    "web-dl",
    "webdl",
    "webrip",
    "bluray",
    "blu-ray",
    "brrip",
    "bdrip",
    "dvdrip",
    "dvdscr",
    "hdtv",
    "hdrip",
    "tc",
    "remux",
    "x264",
    "x265",
    "h264",
    "h265",
    "hevc",
    "avc",
    "xvid",
    "divx",
    "aac",
    "ac3",
    "eac3",
    "dts",
    "dd5",
    "dd51",
    "ddp",
    "truehd",
    "atmos",
    "flac",
    "mp3",
    "10bit",
    "8bit",
    "hdr",
    "hdr10",
    "dovi",
    "dv",
    "sdr",
    "dubbed",
    "subbed",
    "vostfr",
    "vf",
    "vo",
    "vff",
    "subs",
    "unrated",
    "repack",
    "imax",
    "remastered",
    "3d",
    "sbs",
    "hsbs",
];

/// Release tags that are also ordinary English words.
///
/// These were in [`RELEASE_TAGS`], which is stripped word by word wherever a word
/// appears, so every one of them cut a real title short:
///
/// ```text
/// Uncut Gems       -> Gems          Extended Family -> Family
/// Internal Affairs -> Affairs       The Cut         -> The
/// Cut Throat City  -> Throat City   Web of Lies     -> of Lies
/// ```
///
/// A single-word title survived by accident -- `clean_movie_title` falls back to the raw
/// string when it has stripped everything -- and everything else was quietly truncated.
/// The damage is not only cosmetic: the cleaned title is what goes to TMDB, so these
/// films get the wrong artwork or none, and it is what `match_key` is built from, so
/// duplicate collapsing groups them with the wrong film (R-05).
///
/// A release tag is only a release tag in the places one appears. See
/// [`strip_release_tags`] for where that is.
const AMBIGUOUS_TAGS: &[&str] = &[
    "cam",
    "cut",
    "directors",
    "dual",
    "extended",
    "internal",
    "limited",
    "multi",
    "proper",
    "sub",
    "ts",
    "uncut",
    "web",
];

/// What one word of a VOD title is, before anything decides whether it is noise *here*.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Word {
    /// Part of the title.
    Title,
    /// A tag that is nothing else: `1080p`, `x265`, `webrip`.
    Tag,
    /// A tag that is also an English word: `cut`, `web`, `extended`.
    Maybe,
    /// Punctuation on its own -- a `-` or a `|` between tags. Always dropped, and
    /// transparent to the runs below, so `1080p - MULTI` is one run and not two.
    Punctuation,
}

/// Decide which words of a VOD title are release tags, and drop those.
///
/// Returns the words that are left, and the keys of the ones removed in the order they
/// were removed, which is where the quality comes from.
///
/// An unambiguous tag is noise wherever it appears. An ambiguous one -- a word that is
/// also English -- is noise only where a tag is what it can be: in a run of tags that
/// holds at least one unambiguous tag (`Inception 1080p WEB x265 MULTI`), or in a run of
/// two or more at the very end of the name (`Blade Runner DIRECTORS CUT`).
///
/// So `The Cut` keeps its `Cut`: a lone ambiguous word at the end is a word. That is the
/// conservative half of the trade, and deliberately so -- an `EXTENDED` left on a title
/// costs a slightly worse TMDB query, while an `Extended` taken off `Extended Family`
/// costs the right film.
fn strip_release_tags(s: &str) -> (Vec<&str>, Vec<String>) {
    let words: Vec<&str> = s.split_whitespace().collect();
    let keys: Vec<String> = words
        .iter()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_ascii_lowercase()
        })
        .collect();
    let kinds: Vec<Word> = keys
        .iter()
        .map(|k| {
            if k.is_empty() {
                Word::Punctuation
            } else if RELEASE_TAGS.contains(&k.as_str()) {
                Word::Tag
            } else if AMBIGUOUS_TAGS.contains(&k.as_str()) {
                Word::Maybe
            } else {
                Word::Title
            }
        })
        .collect();

    let mut dropped = vec![false; words.len()];
    let mut i = 0;
    while i < words.len() {
        if kinds[i] == Word::Title {
            i += 1;
            continue;
        }
        // The whole run of things that are not title words, punctuation included.
        let start = i;
        while i < words.len() && kinds[i] != Word::Title {
            i += 1;
        }
        let run = start..i;
        let tags = run
            .clone()
            .filter(|&j| kinds[j] != Word::Punctuation)
            .count();
        let certain = run.clone().any(|j| kinds[j] == Word::Tag);
        let at_end = i == words.len();
        let all = certain || (at_end && tags >= 2);
        for j in run {
            dropped[j] = all || kinds[j] != Word::Maybe;
        }
    }

    let kept = (0..words.len())
        .filter(|&i| !dropped[i])
        .map(|i| words[i])
        .collect();
    let removed = (0..words.len())
        .filter(|&i| dropped[i] && kinds[i] != Word::Punctuation)
        .map(|i| keys[i].clone())
        .collect();
    (kept, removed)
}

/// Channel-name noise stripped when building an EPG match key. Deliberately narrower
/// than [`RELEASE_TAGS`], and narrower still than [`AMBIGUOUS_TAGS`]: a channel name is
/// not a release name, and none of the words that are also English belong here at all.
const CHANNEL_TAGS: &[&str] = &[
    "uhd", "fhd", "hd", "sd", "4k", "8k", "hevc", "h265", "h264", "1080p", "1080", "720p", "720",
    "480p", "raw", "backup", "alt", "vip", "plus", "\u{ac}",
];

/// The year in a VOD name, with what may sit on either side of it.
///
/// The trailing set used to be `[\s\)\]\.\-_]|$`, which does not include an *opening*
/// bracket -- so `EN ★ The Academy - 2025[4K]`, with no space before the `[`, matched
/// nothing and the year stayed in the title. `re_bracketed` then took `[4K]` away and
/// left `EN The Academy 2025`, which is what was stored, searched and built into the
/// match key: `theacademy2025` scores 0.000 against TMDB's "The Academy", and the film
/// was marked `nomatch` after one attempt. Thirty films on this subscription share that
/// shape (R-59).
fn re_year() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // `*` among the trailing characters because this panel writes it: measured in
        // the real library, `EN Spider-Man Across the Spider-Verse 2023*` was the last
        // film whose year the cleaner could not see.
        Regex::new(r"(?:^|[\s\(\[\.\-_])((?:19|20)\d{2})(?:[\s\(\)\[\]\.\-_\*]|$)").unwrap()
    })
}

fn re_country_prefix() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // Two shapes: a bracketed code needs no separator ("[DE] RTL"), a bare code does
    // ("US| CNN", "FR - TF1") — otherwise "BBC One" would lose its "BBC".
    //
    // The star is not decoration. A real 22,305-channel subscription writes every
    // channel as "US ★ QVC HD": 18,460 of them carry a country code separated by
    // U+2605, and without it here the prefix was recognised on 2.1% of the playlist,
    // so the country went into the match key and "English only" had almost nothing to
    // go on. It costs what the other separators already cost — a channel actually
    // named "NBC ★ Sports" loses its "NBC", the same way "HBO - Max" already does.
    R.get_or_init(|| {
        Regex::new(
            r"^\s*(?:[\(\[]([A-Za-z]{2,4})[\)\]]|([A-Za-z]{2,4})\s*[:|\-\u{2013}\u{2605}])\s*",
        )
        .unwrap()
    })
}

fn re_separators() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\.\_\+]+").unwrap())
}

fn re_bracketed() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\(\[\{][^\)\]\}]*[\)\]\}]").unwrap())
}

fn re_ws() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\s{2,}").unwrap())
}

/// Fold common Latin diacritics so "Amélie" and "Amelie" compare equal.
/// Deliberately table-driven rather than pulling in a Unicode crate — this covers the
/// Latin-1/Latin-A range that provider titles actually use.
pub fn fold_diacritics(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'ī' | 'į' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' | 'ő' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ñ' | 'ń' | 'ň' => 'n',
            'ç' | 'ć' | 'č' => 'c',
            'š' | 'ś' => 's',
            'ž' | 'ź' | 'ż' => 'z',
            'ł' => 'l',
            'ď' | 'đ' => 'd',
            'ť' => 't',
            'ř' => 'r',
            'ğ' => 'g',
            other => other,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanTitle {
    pub title: String,
    pub year: Option<i32>,
    /// Quality token found, if any (`4K`, `1080p`, …) — used for duplicate collapsing.
    pub quality: Option<String>,
}

/// Clean a VOD stream name into a searchable title plus its year and quality.
///
/// ```
/// # use aurora_core::title::clean_movie_title;
/// let c = clean_movie_title("Inception.2010.1080p.WEB-DL.x265-MULTI");
/// assert_eq!(c.title, "Inception");
/// assert_eq!(c.year, Some(2010));
/// ```
pub fn clean_movie_title(raw: &str) -> CleanTitle {
    let mut s = raw.trim().to_string();

    // Strip a trailing container extension.
    for ext in [".mkv", ".mp4", ".avi", ".ts", ".m3u8", ".mov"] {
        if s.to_ascii_lowercase().ends_with(ext) {
            s.truncate(s.len() - ext.len());
        }
    }

    // Dots and underscores are word separators in scene naming.
    s = re_separators().replace_all(&s, " ").into_owned();

    // Quality has to be read from the whole string: truncating at the year would cut off the
    // "1080p" in "Inception.2010.1080p.WEB-DL".
    let mut quality = detect_quality(&s);

    // A year at position 0 is the title ("2012"), not a release year. Prefer the first year
    // that is preceded by something, so "2012 (2009)" resolves to 2009.
    let year_match = re_year()
        .captures_iter(&s)
        .find(|c| c.get(0).map(|m| m.start() > 0).unwrap_or(false));
    let year = year_match
        .as_ref()
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<i32>().ok());

    // Everything from the release year onward is almost always noise.
    if let Some(start) = year_match.and_then(|c| c.get(0)).map(|m| m.start()) {
        s.truncate(start);
    }

    s = re_bracketed().replace_all(&s, " ").into_owned();

    let (kept, removed) = strip_release_tags(&s);
    if quality.is_none() {
        quality = removed.iter().find_map(|key| normalize_quality(key));
    }

    let title = kept
        .join(" ")
        .trim_matches(|c: char| c == '-' || c == '|' || c.is_whitespace())
        .to_string();
    let title = re_ws().replace_all(&title, " ").trim().to_string();

    CleanTitle {
        title: if title.is_empty() {
            raw.trim().to_string()
        } else {
            title
        },
        year,
        quality,
    }
}

fn normalize_quality(key: &str) -> Option<String> {
    Some(
        match key {
            "2160p" | "4k" | "uhd" => "4K",
            "1080p" | "fhd" => "FHD",
            "720p" | "hd" => "HD",
            "sd" | "480p" | "360p" => "SD",
            _ => return None,
        }
        .to_string(),
    )
}

/// Split a leading country/language prefix off a channel name.
/// `"US| CNN HD"` → `("CNN HD", Some("US"))`.
pub fn split_country_prefix(name: &str) -> (String, Option<String>) {
    if let Some(c) = re_country_prefix().captures(name) {
        let code = c
            .get(1)
            .or_else(|| c.get(2))
            .map(|m| m.as_str().to_ascii_uppercase());
        let rest = name[c.get(0).unwrap().end()..].trim().to_string();
        if !rest.is_empty() {
            return (rest, code);
        }
    }
    (name.trim().to_string(), None)
}

/// Build the comparison key used for EPG matching and duplicate detection.
///
/// Lowercases, folds diacritics, drops a country prefix, removes quality tags, and strips
/// everything that is not alphanumeric — so `"US| CNN HD"`, `"CNN"`, and `"cnn.us"` all collapse
/// to `"cnn"`.
pub fn match_key(name: &str) -> String {
    let base = normalised_for_matching(name);

    let mut out = String::new();
    for word in base.split_whitespace() {
        let key: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
        if key.is_empty() || CHANNEL_TAGS.contains(&key.as_str()) {
            continue;
        }
        out.push_str(&key);
    }
    if out.is_empty() {
        // Never return an empty key — fall back to the raw alphanumerics.
        return base.chars().filter(|c| c.is_alphanumeric()).collect();
    }
    out
}

/// The normalisation [`match_key`] and [`key_words`] share, before either decides what a
/// word is.
///
/// Split out so there is one definition of it. What the two do afterwards genuinely
/// differs, and the comment on `key_words` says why.
fn normalised_for_matching(name: &str) -> String {
    let (base, _) = split_country_prefix(name);
    // And a bare code with no separator, which `split_country_prefix` leaves alone and
    // is right to.
    //
    // **This is what stops the same film appearing twice.** A real panel writes one film
    // both ways, and the two forms produced different keys. With the star (U+2605) glued
    // straight onto the title the separated form matched and the code came off; with a
    // space after it, nothing matched:
    //
    //     "EN *My Penguin Friend"  -> separator matched -> "mypenguinfriend"
    //     "EN My Penguin Friend"   -> no separator      -> "enmypenguinfriend"
    //
    // so duplicate collapsing never saw them as the same film and the library showed
    // both. Reading the bare form here brings 187 further groups together on the
    // subscription this was measured against. Only codes `lang` recognises, so "BBC One"
    // and "IT Crowd" keep their first word.
    let base = crate::lang::strip_bare_language_prefix(&base)
        .map(str::to_string)
        .unwrap_or(base);
    let base = fold_diacritics(&base).to_lowercase();
    let base = re_bracketed().replace_all(&base, " ");
    re_separators().replace_all(&base, " ").into_owned()
}

/// The same normalisation as [`match_key`], as a list of words.
///
/// Not `match_key(s).split_whitespace()`, which is what `tmdb::tokens` used to be.
/// `match_key` concatenates — "The Matrix Reloaded" comes back as "thematrixreloaded" —
/// so that split could only ever yield one element, and the token-set F1 built on it
/// collapsed to an equality test. Punctuation and spacing differences still matched,
/// which is why it appeared to work, but any difference of a *word* scored 0 instead of
/// the ~0.8 the F1 was designed to give: "Blade Runner" against "Blade Runner 2049",
/// "The Lord of the Rings" against "Lord of the Rings". Those titles then got no
/// metadata at all, and the three mechanisms built on a continuous score had an
/// eight-value step function underneath them (R-25).
///
/// The split is on anything that is not alphanumeric, rather than on whitespace, so
/// "spider-man" is two words and matches "Spider Man". `match_key` keeps its own
/// whitespace split on purpose: it concatenates, so where the boundaries fall cannot
/// change its answer, and changing which *characters* survive would change every
/// duplicate-collapsing key in the library.
pub fn key_words(name: &str) -> Vec<String> {
    let base = normalised_for_matching(name);
    base.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty() && !CHANNEL_TAGS.contains(word))
        .map(str::to_string)
        .collect()
}

/// Quality markers safe to remove from a title that is going on screen.
///
/// Deliberately a *shorter* list than [`CHANNEL_TAGS`], which is for building a match
/// key. A key can afford to drop an ambiguous word, because the only cost is two entries
/// grouping together; a title cannot, because the cost is a film called something else.
///
/// So the bare numbers are out — `1080` and `720` are plausible titles, and *1917* and
/// *2012* are real films — and so are the English words `plus`, `raw`, `alt`, `vip` and
/// `backup`: "Raw", "Plus One" and "Backup Plan" are all things a provider might list.
/// What is left cannot be anything but a quality tag.
const SHOWN_QUALITY_TAGS: &[&str] = &[
    "uhd", "fhd", "hd", "sd", "4k", "8k", "hevc", "h265", "h264", "1080p", "720p", "480p", "2160p",
    "4320p",
];

/// Take the quality marker off a title that is going to be displayed.
///
/// Two copies of one film are one film, and the grid says so now — the duplicate
/// collapser keeps the best copy, which is usually the 4K one, whose title is
/// `Breaking Bad [4K]`. Shown unchanged, the merged entry still reads like "the 4K
/// version" rather than like the film, which is the thing somebody collapsing duplicates
/// asked not to see. The quality is not lost: it is a column of its own and the card
/// already wears it as a badge.
///
/// Only a *trailing* marker, and only one, bracketed or bare. A tag sits at the end of a
/// provider's name — `Dune [4K]`, `Dune - 4K`, `Dune 4K` — and anything in the middle is
/// far likelier to be the title: *4K* could be a documentary about resolution, but
/// *Dune 4K Remastered* is not asking for its middle word to go.
pub fn strip_quality_tag(name: &str) -> String {
    let trimmed = name.trim_end();
    // A bracketed tag at the very end: `Dune [4K]`, `Dune (UHD)`.
    if let Some(open) = trimmed.rfind(['[', '(', '{']) {
        let close = trimmed.chars().last();
        let closes = matches!(close, Some(']') | Some(')') | Some('}'));
        if closes {
            let inner: String = trimmed[open + 1..trimmed.len() - 1]
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect();
            if SHOWN_QUALITY_TAGS.contains(&inner.to_ascii_lowercase().as_str()) {
                return trimmed[..open]
                    .trim_end()
                    .trim_end_matches(['-', '·', '|'])
                    .trim_end()
                    .to_string();
            }
        }
    }
    // A bare one: `Dune 4K`, `Dune - UHD`.
    let mut words = trimmed.rsplitn(2, char::is_whitespace);
    if let (Some(last), Some(rest)) = (words.next(), words.next()) {
        let key: String = last.chars().filter(|c| c.is_alphanumeric()).collect();
        if SHOWN_QUALITY_TAGS.contains(&key.to_ascii_lowercase().as_str()) {
            let rest = rest.trim_end().trim_end_matches(['-', '·', '|']).trim_end();
            // Never return nothing: a stream genuinely called "4K" keeps its name.
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    trimmed.to_string()
}

/// Detect the quality tier advertised in a channel or stream name.
pub fn detect_quality(name: &str) -> Option<String> {
    let lower = fold_diacritics(name).to_lowercase();
    for word in lower.split(|c: char| !c.is_alphanumeric()) {
        match word {
            "2160p" | "4k" | "uhd" => return Some("4K".into()),
            "4320p" | "8k" => return Some("8K".into()),
            "1080p" | "1080" | "fhd" => return Some("FHD".into()),
            "720p" | "720" | "hd" => return Some("HD".into()),
            "sd" | "480p" | "360p" => return Some("SD".into()),
            _ => {}
        }
    }
    None
}

/// Order quality tiers so duplicates can be collapsed onto the best copy
/// (README §7.3, "collapse quality duplicates").
///
/// Takes whatever is stored — the tags `detect_quality` returns, or a raw `1080p` out
/// of a provider's own field — and returns a number where bigger is better. Nothing
/// known ranks below `SD`, so a tagged copy always beats an untagged one, and two
/// untagged copies are left to the caller to break.
pub fn quality_rank(quality: Option<&str>) -> u8 {
    let Some(raw) = quality else { return 0 };
    let normalized = detect_quality(raw).unwrap_or_else(|| raw.trim().to_uppercase());
    match normalized.as_str() {
        "4K" | "8K" => 4,
        "FHD" => 3,
        "HD" => 2,
        "SD" => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_ranks_best_first_whatever_the_spelling() {
        assert!(quality_rank(Some("4K")) > quality_rank(Some("FHD")));
        assert!(quality_rank(Some("FHD")) > quality_rank(Some("HD")));
        assert!(quality_rank(Some("HD")) > quality_rank(Some("SD")));
        assert!(quality_rank(Some("SD")) > quality_rank(None));
        // Raw provider spellings rank the same as the tags we store.
        assert_eq!(quality_rank(Some("1080p")), quality_rank(Some("FHD")));
        assert_eq!(quality_rank(Some("2160p")), quality_rank(Some("4K")));
        assert_eq!(quality_rank(Some("uhd")), quality_rank(Some("4K")));
        assert_eq!(quality_rank(Some("720")), quality_rank(Some("HD")));
    }

    #[test]
    fn an_unrecognised_quality_ranks_as_unknown_not_as_best() {
        assert_eq!(quality_rank(Some("")), 0);
        assert_eq!(quality_rank(Some("VIP")), 0);
        assert_eq!(quality_rank(Some("H265")), 0);
    }

    #[test]
    fn strips_scene_release_noise() {
        let c = clean_movie_title("Inception.2010.1080p.WEB-DL.x265-MULTI");
        assert_eq!(c.title, "Inception");
        assert_eq!(c.year, Some(2010));
        assert_eq!(c.quality.as_deref(), Some("FHD"));
    }

    /// The thirteen tags that are also English words, in the titles they were cutting
    /// short. Every one of these lost a word before (R-05).
    #[test]
    fn an_english_word_that_is_also_a_release_tag_stays_in_the_title() {
        for (raw, want) in [
            ("Uncut Gems", "Uncut Gems"),
            ("Internal Affairs", "Internal Affairs"),
            ("Cut Throat City", "Cut Throat City"),
            ("Extended Family", "Extended Family"),
            ("The Cut", "The Cut"),
            ("Web of Lies", "Web of Lies"),
            ("Limited Partners", "Limited Partners"),
            ("Dual Survival", "Dual Survival"),
            ("Proper Manners", "Proper Manners"),
            ("Multi Facial", "Multi Facial"),
            ("The Sub", "The Sub"),
            ("Cam", "Cam"),
            ("Directors", "Directors"),
            ("TS Spivet", "TS Spivet"),
        ] {
            assert_eq!(clean_movie_title(raw).title, want, "from {raw:?}");
        }
    }

    /// And a tag is still a tag where a tag is what it can be: beside an unambiguous
    /// one, or as a pair of them at the end of the name.
    #[test]
    fn a_release_tag_beside_other_tags_is_still_stripped() {
        for (raw, want) in [
            // No year to truncate at, so the run is all this has to go on.
            ("Inception 1080p WEB-DL x265 MULTI", "Inception"),
            ("Arrival 720p WEB", "Arrival"),
            ("Heat UNCUT 1080p", "Heat"),
            // Punctuation between tags does not break the run.
            ("Sicario 1080p - MULTI", "Sicario"),
            // Two ambiguous tags at the end, which is a tag pair and not a sentence.
            ("Blade Runner DIRECTORS CUT", "Blade Runner"),
            ("Dune EXTENDED CUT", "Dune"),
            // A lone ambiguous word at the end is left alone, which is the half of the
            // trade that keeps "The Cut" whole.
            ("Some Film EXTENDED", "Some Film EXTENDED"),
        ] {
            assert_eq!(clean_movie_title(raw).title, want, "from {raw:?}");
        }
    }

    /// The quality still comes off a stripped tag, which is read from the removals
    /// rather than from the loop that used to do both jobs.
    #[test]
    fn quality_survives_the_split_between_certain_and_ambiguous_tags() {
        assert_eq!(
            clean_movie_title("Arrival 720p WEB").quality.as_deref(),
            Some("HD")
        );
        assert_eq!(
            clean_movie_title("Heat UNCUT 1080p").quality.as_deref(),
            Some("FHD")
        );
        // And a title that only looks like a tag reports no quality at all.
        assert_eq!(clean_movie_title("The Cut").quality, None);
    }

    /// A year pressed against a bracket, which is the shape that defeated this.
    ///
    /// `- 2025[4K]` with no space before the bracket matched no year, so the year stayed
    /// in the stored title and in the match key, and the film could not be matched
    /// against TMDB at all. Thirty films on the subscription this is measured against
    /// (R-59).
    #[test]
    fn a_year_against_a_bracket_is_still_a_year() {
        for (raw, title, year) in [
            // The star comes off too, which `splits_the_star_prefix_...` covers.
            ("EN \u{2605} The Academy - 2025[4K]", "EN The Academy", 2025),
            ("The Academy - 2025[4K]", "The Academy", 2025),
            ("Inception 2010[1080p]", "Inception", 2010),
            ("Arrival 2016(FHD)", "Arrival", 2016),
            // The trailing star this panel writes on a new release.
            (
                "Spider-Man Across the Spider-Verse 2023*",
                "Spider-Man Across the Spider-Verse",
                2023,
            ),
            // And the forms that already worked, which must not move.
            ("The Matrix (1999) [4K] REMUX", "The Matrix", 1999),
            ("Inception.2010.1080p", "Inception", 2010),
            ("Dune - 2021", "Dune", 2021),
        ] {
            let c = clean_movie_title(raw);
            assert_eq!(c.year, Some(year), "year from {raw:?}");
            assert_eq!(c.title, title, "title from {raw:?}");
        }

        // A numeric title is still a title: the year has to be preceded by something.
        assert_eq!(clean_movie_title("2012[4K]").title, "2012");
        assert_eq!(clean_movie_title("2012[4K]").year, None);
    }

    #[test]
    fn handles_parenthesised_year() {
        let c = clean_movie_title("The Matrix (1999) [4K] REMUX");
        assert_eq!(c.title, "The Matrix");
        assert_eq!(c.year, Some(1999));
    }

    #[test]
    fn keeps_numeric_titles_intact() {
        // "2012" is the title, not a stray year token.
        let c = clean_movie_title("2012");
        assert_eq!(c.title, "2012");
    }

    #[test]
    fn keeps_numeric_title_with_real_year() {
        let c = clean_movie_title("2012 (2009) 1080p");
        assert_eq!(c.title, "2012");
        assert_eq!(c.year, Some(2009));
    }

    #[test]
    fn never_returns_empty_title() {
        let c = clean_movie_title("1080p x265");
        assert!(!c.title.is_empty());
    }

    #[test]
    fn splits_country_prefixes() {
        assert_eq!(
            split_country_prefix("US| CNN HD"),
            ("CNN HD".into(), Some("US".into()))
        );
        assert_eq!(
            split_country_prefix("FR - TF1"),
            ("TF1".into(), Some("FR".into()))
        );
        assert_eq!(
            split_country_prefix("[DE] RTL"),
            ("RTL".into(), Some("DE".into()))
        );
    }

    #[test]
    fn splits_the_star_prefix_a_real_panel_writes_every_channel_with() {
        // Shapes taken from a 22,305-channel subscription, where 18,460 names are
        // exactly this. Two, three and four letter codes all appear.
        assert_eq!(
            split_country_prefix("US \u{2605} QVC HD"),
            ("QVC HD".into(), Some("US".into()))
        );
        assert_eq!(
            split_country_prefix("ALB \u{2605} Klan HD"),
            ("Klan HD".into(), Some("ALB".into()))
        );
        assert_eq!(
            split_country_prefix("EXYU \u{2605} RTS 1"),
            ("RTS 1".into(), Some("EXYU".into()))
        );
        // A star with no code in front of it is decoration, not a prefix.
        let (name, code) = split_country_prefix("\u{2605} Sports HD");
        assert_eq!(name, "\u{2605} Sports HD");
        assert_eq!(code, None);
    }

    #[test]
    fn a_star_prefixed_channel_matches_the_same_channel_without_one() {
        // The point of the previous test: the guide and duplicate detection key on
        // this, so a country prefix left in the key is an EPG match that never happens.
        assert_eq!(match_key("US \u{2605} CNN HD"), match_key("CNN"));
        assert_eq!(match_key("FR \u{2605} TF1"), match_key("TF1 FHD"));
    }

    #[test]
    fn does_not_eat_a_real_name_that_looks_like_a_prefix() {
        // No separator, so nothing should be stripped.
        let (name, code) = split_country_prefix("BBC One");
        assert_eq!(name, "BBC One");
        assert_eq!(code, None);
    }

    /// Two copies of one film are one film, and the title says so.
    #[test]
    fn a_quality_tag_comes_off_a_displayed_title() {
        for (raw, want) in [
            ("Breaking Bad [4k]", "Breaking Bad"),
            ("Dune [4K]", "Dune"),
            ("Dune (UHD)", "Dune"),
            ("Dune 4K", "Dune"),
            ("Dune - 4K", "Dune"),
            ("Dune | FHD", "Dune"),
            ("Some Show 1080p", "Some Show"),
            ("Trailing space 4K  ", "Trailing space"),
        ] {
            assert_eq!(strip_quality_tag(raw), want, "{raw}");
        }
    }

    /// And a title that merely looks like one keeps every word.
    ///
    /// This is why `SHOWN_QUALITY_TAGS` is shorter than `CHANNEL_TAGS`: a match key can
    /// afford to drop an ambiguous word, because the cost is two entries grouping
    /// together. A title cannot -- the cost is a film called something else.
    #[test]
    fn a_title_that_looks_like_a_tag_is_left_alone() {
        for raw in [
            "1917",
            "2012",
            "Plus One",
            "The Backup Plan",
            "Raw",
            "Dune 4K Remastered",
            "Blade Runner 2049",
            "1080",
            "720",
            "4K",
            "Æon Flux",
        ] {
            assert_eq!(strip_quality_tag(raw), raw, "{raw} was altered");
        }
    }

    #[test]
    fn match_key_collapses_channel_variants() {
        let k = match_key("CNN");
        assert_eq!(match_key("US| CNN HD"), k);
        assert_eq!(match_key("CNN FHD"), k);
        assert_eq!(match_key("cnn.us"), format!("{k}us"));
    }

    #[test]
    fn match_key_folds_diacritics() {
        assert_eq!(match_key("Télé 5"), match_key("Tele 5"));
    }

    #[test]
    fn match_key_is_never_empty() {
        assert!(!match_key("HD").is_empty());
        assert!(!match_key("4K").is_empty());
    }

    #[test]
    fn detects_quality_tiers() {
        assert_eq!(detect_quality("Sky Sports UHD").as_deref(), Some("4K"));
        assert_eq!(detect_quality("BBC One HD").as_deref(), Some("HD"));
        assert_eq!(detect_quality("BBC One"), None);
    }
}
