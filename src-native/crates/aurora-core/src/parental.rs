//! Content-rating comparison for parental controls (README §11).
//!
//! Providers label content with whatever certification system their market uses, and
//! rarely consistently. Everything is therefore mapped onto a *minimum age*, which is
//! the only thing the systems have in common and the only thing a parent actually
//! wants to reason about.

use serde::{Deserialize, Serialize};

/// A ceiling on what a profile may watch, expressed as a minimum age.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgeLimit(pub u8);

impl AgeLimit {
    pub const EVERYONE: AgeLimit = AgeLimit(0);
    pub const ADULT: AgeLimit = AgeLimit(18);
}

/// Map a certification string onto the minimum age it implies.
///
/// Covers the systems that actually turn up in IPTV metadata: US film and TV, UK/BBFC,
/// Ireland, Germany (FSK), France, the Netherlands (Kijkwijzer), Australia, and bare
/// numbers. Returns `None` for anything unrecognised — which the caller must treat as
/// *unknown*, not as *safe*.
pub fn min_age(certification: &str) -> Option<u8> {
    // Uppercase first: "Rated R" and "rated r" both occur, and stripping the prefix
    // before normalising case would miss the ones that are capitalised.
    let c = certification.trim().to_ascii_uppercase();
    let c = c.trim_start_matches("RATED ").trim_start_matches("RATING ");
    let c = c.replace(['_', '-'], "").replace("  ", " ");
    let c = c.trim();

    // A bare number, a number with a suffix like "16+", or the "AGE 12" form.
    //
    // `AGE {digits}` was in this condition and could never match it: `digits` is taken
    // from the *start* of the string, so on "AGE 12" it is empty and the whole branch is
    // skipped -- then "AGE 12" matches no arm below either and the answer was `None`,
    // which the caller is required to read as *unknown*. The prefix comes off first now.
    // Found by writing the test for the dead arms below, which is the same defect one
    // line further up (R-16).
    let numeric = c.strip_prefix("AGE ").unwrap_or(c).trim();
    let digits: String = numeric
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if !digits.is_empty() && digits.len() <= 2 {
        // Guard against matching the "13" in "PG13" here; that is handled below.
        if numeric == digits || numeric == format!("{digits}+") {
            return digits.parse().ok();
        }
    }

    // Only the named systems below this line.
    //
    // Every bare number and every `n+` form has already returned above, so the arms
    // listing "0", "6", "12", "15", "16+", "18" and the rest were unreachable -- and so
    // was the `if c == "15"` inside the 15-or-16 arm, which was the one piece of this
    // function a reader had to stop and work out. Dead code that looks like a decision
    // is worse than no code: it is the first place somebody would go to change how "15"
    // is treated, and editing it would do nothing at all (R-16).
    Some(match c {
        // Universally suitable.
        "U" | "G" | "TVY" | "TVG" | "ALL" | "AL" | "E" | "EVERYONE" => 0,
        // Parental guidance.
        "PG" | "TVPG" | "TVY7" | "FSK6" => 6,
        "PG13" | "12A" | "TV14" | "FSK12" | "M" => 12,
        "TEEN" => 13,
        "MA15" => 15,
        "FSK16" => 16,
        "R" | "TVMA" | "MA" | "NC17" => 17,
        "X" | "XXX" | "FSK18" | "R18" | "ADULT" | "AO" => 18,
        "NR" | "UNRATED" | "UNKNOWN" | "" => return None,
        _ => return None,
    })
}

/// Whether a profile limited to `limit` may watch content certified `certification`.
///
/// `allow_unrated` decides the unknown case. Defaulting it to `false` is the safe
/// choice for a kids profile: IPTV metadata is patchy, and "no rating" is far more
/// often missing data than genuinely child-safe content.
pub fn is_allowed(
    certification: Option<&str>,
    limit: Option<AgeLimit>,
    allow_unrated: bool,
) -> bool {
    let Some(limit) = limit else {
        return true; // no restriction configured
    };
    match certification.and_then(min_age) {
        Some(age) => age <= limit.0,
        None => allow_unrated,
    }
}

/// Categories hidden unless the user explicitly opts in (README §11: "adult content is
/// hidden by default on a fresh install").
/// This and `rules::presets::hide_adult` answer the same question and have to agree.
/// The preset allows `18 +` as well as `18+`, because playlists write both, so the
/// spaces come out here before the markers are looked for -- otherwise a group one of
/// them hides is left visible by the other (R-18).
pub fn looks_adult(group: Option<&str>) -> bool {
    let Some(group) = group else { return false };
    let g = group.to_ascii_lowercase();
    // `+18` as well as `18+`: a playlist naming the category either way means the same
    // thing, and only one of them was listed (R-16).
    const MARKERS: &[&str] = &["adult", "xxx", "porn", "erotic", "18+", "+18", "for adults"];
    if MARKERS.iter().any(|m| g.contains(m)) {
        return true;
    }
    let tight: String = g.chars().filter(|c| !c.is_whitespace()).collect();
    tight.contains("18+") || tight.contains("+18")
}

#[cfg(test)]
mod tests {
    /// The numeric forms the dead arms claimed to handle, now that they are gone (R-16).
    ///
    /// Removing unreachable arms must not remove behaviour: each of these was listed in
    /// an arm that could never run, and is answered by the digit branch above it.
    #[test]
    fn the_bare_and_suffixed_numbers_still_map_to_themselves() {
        for (given, want) in [
            ("0", 0u8),
            ("0+", 0),
            ("6", 6),
            ("6+", 6),
            ("7", 7),
            ("12", 12),
            ("13", 13),
            ("13+", 13),
            ("14", 14),
            ("15", 15),
            ("16", 16),
            ("16+", 16),
            ("17", 17),
            ("17+", 17),
            ("18", 18),
            ("18+", 18),
            ("AGE 12", 12),
        ] {
            assert_eq!(super::min_age(given), Some(want), "{given:?}");
        }
        // The named systems, including the two that are not a bare number in disguise.
        assert_eq!(super::min_age("MA15"), Some(15));
        assert_eq!(super::min_age("FSK16"), Some(16));
        assert_eq!(super::min_age("PG13"), Some(12));
        assert_eq!(super::min_age("TVMA"), Some(17));
        assert_eq!(super::min_age("ADULT"), Some(18));
        // And unknown is still unknown rather than safe.
        assert_eq!(super::min_age("NR"), None);
        assert_eq!(super::min_age("wat"), None);
    }

    /// `+18` is the same category as `18+` (R-16).
    #[test]
    fn an_adult_group_is_recognised_whichever_side_the_plus_is_on() {
        for group in ["+18", "+18 Channels", "+ 18 VOD", "CANALES +18"] {
            assert!(
                super::looks_adult(Some(group)),
                "{group:?} was not recognised"
            );
        }
        // Still not matching something that merely contains the digits.
        assert!(!super::looks_adult(Some("Studio 18")));
    }

    use super::*;

    /// The spaced form, which `rules::presets::hide_adult` matches (R-18).
    #[test]
    fn an_adult_group_is_recognised_however_it_spaces_its_age() {
        for group in [
            "18+",
            "18+ Channels",
            "18 + VOD",
            "18  +  MOVIES",
            "FOR ADULTS",
        ] {
            assert!(looks_adult(Some(group)), "{group:?} was not recognised");
        }
        for group in ["Channel 18", "Documentaries", "Kids", "18"] {
            assert!(!looks_adult(Some(group)), "{group:?} was recognised");
        }
        assert!(!looks_adult(None));
    }

    #[test]
    fn maps_the_common_systems_onto_ages() {
        for (cert, age) in [
            ("U", 0),
            ("G", 0),
            ("TV-Y", 0),
            ("PG", 6),
            ("TV-PG", 6),
            ("PG-13", 12),
            ("12A", 12),
            ("TV-14", 12),
            ("15", 15),
            ("R", 17),
            ("TV-MA", 17),
            ("NC-17", 17),
            ("18", 18),
            ("FSK18", 18),
        ] {
            assert_eq!(min_age(cert), Some(age), "{cert}");
        }
    }

    #[test]
    fn accepts_bare_numbers_and_plus_forms() {
        assert_eq!(min_age("16"), Some(16));
        assert_eq!(min_age("16+"), Some(16));
        assert_eq!(min_age("6+"), Some(6));
    }

    #[test]
    fn is_case_and_punctuation_insensitive() {
        assert_eq!(min_age("pg-13"), min_age("PG13"));
        assert_eq!(min_age(" tv_ma "), min_age("TV-MA"));
        assert_eq!(min_age("Rated R"), Some(17));
    }

    #[test]
    fn unrated_is_unknown_not_safe() {
        for cert in ["NR", "Unrated", "", "   ", "completely made up"] {
            assert_eq!(min_age(cert), None, "{cert:?}");
        }
    }

    #[test]
    fn an_unset_limit_allows_everything() {
        assert!(is_allowed(Some("18"), None, false));
        assert!(is_allowed(None, None, false));
    }

    #[test]
    fn a_limit_blocks_content_above_it() {
        let kids = Some(AgeLimit(12));
        assert!(is_allowed(Some("PG"), kids, false));
        assert!(
            is_allowed(Some("PG-13"), kids, false),
            "12 is at the limit, not above it"
        );
        assert!(!is_allowed(Some("R"), kids, false));
        assert!(!is_allowed(Some("18"), kids, false));
    }

    #[test]
    fn unrated_content_is_blocked_by_default_under_a_limit() {
        let kids = Some(AgeLimit(12));
        assert!(
            !is_allowed(None, kids, false),
            "missing metadata is far more common than genuinely safe unrated content"
        );
        assert!(!is_allowed(Some("NR"), kids, false));
        // ...but the caller can opt into permitting it.
        assert!(is_allowed(None, kids, true));
    }

    #[test]
    fn everyone_rated_content_passes_the_strictest_limit() {
        assert!(is_allowed(Some("U"), Some(AgeLimit::EVERYONE), false));
        assert!(!is_allowed(Some("PG"), Some(AgeLimit::EVERYONE), false));
    }

    #[test]
    fn detects_adult_categories() {
        for group in [
            "XXX",
            "Adult Movies",
            "FOR ADULTS ONLY",
            "Erotic",
            "18+ Channels",
        ] {
            assert!(looks_adult(Some(group)), "{group}");
        }
        for group in ["Kids", "Documentary", "News", "Sports"] {
            assert!(!looks_adult(Some(group)), "{group}");
        }
        assert!(!looks_adult(None));
    }
}
