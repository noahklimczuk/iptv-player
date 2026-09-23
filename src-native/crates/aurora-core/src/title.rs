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
    "web",
    "bluray",
    "blu-ray",
    "brrip",
    "bdrip",
    "dvdrip",
    "dvdscr",
    "hdtv",
    "hdrip",
    "cam",
    "ts",
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
    "multi",
    "dual",
    "dubbed",
    "subbed",
    "vostfr",
    "vf",
    "vo",
    "vff",
    "subs",
    "sub",
    "extended",
    "unrated",
    "proper",
    "repack",
    "internal",
    "limited",
    "imax",
    "remastered",
    "directors",
    "cut",
    "uncut",
    "3d",
    "sbs",
    "hsbs",
];

/// Channel-name noise stripped when building an EPG match key. Deliberately narrower than
/// [`RELEASE_TAGS`] — stripping "web" or "cut" from a channel name would be wrong.
const CHANNEL_TAGS: &[&str] = &[
    "uhd", "fhd", "hd", "sd", "4k", "8k", "hevc", "h265", "h264", "1080p", "1080", "720p", "720",
    "480p", "raw", "backup", "alt", "vip", "plus", "\u{ac}",
];

fn re_year() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?:^|[\s\(\[\.\-_])((?:19|20)\d{2})(?:[\s\)\]\.\-_]|$)").unwrap())
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

    let mut kept: Vec<&str> = Vec::new();
    for word in s.split_whitespace() {
        let key = word
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_ascii_lowercase();
        if key.is_empty() {
            continue;
        }
        if RELEASE_TAGS.contains(&key.as_str()) {
            if quality.is_none() {
                quality = normalize_quality(&key);
            }
            continue;
        }
        kept.push(word);
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
    let (base, _) = split_country_prefix(name);
    let base = fold_diacritics(&base).to_lowercase();
    let base = re_bracketed().replace_all(&base, " ");
    let base = re_separators().replace_all(&base, " ");

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
