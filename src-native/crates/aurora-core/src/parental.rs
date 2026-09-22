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

    // A bare number, or a number with a suffix like "16+".
    let digits: String = c.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if !digits.is_empty() && digits.len() <= 2 {
        // Guard against matching the "13" in "PG13" here; that is handled below.
        if c == digits || c == format!("{digits}+") || c == format!("AGE {digits}") {
            return digits.parse().ok();
        }
    }

    Some(match c {
        // Universally suitable.
        "U" | "G" | "TVY" | "TVG" | "ALL" | "AL" | "E" | "0" | "0+" | "EVERYONE" => 0,
        // Parental guidance.
        "PG" | "TVPG" | "TVY7" | "6" | "6+" | "7" | "7+" | "FSK6" => 6,
        "PG13" | "12A" | "12" | "TV14" | "FSK12" | "M" => 12,
        "13" | "13+" | "TEEN" => 13,
        "14" | "14+" => 14,
        "15" | "MA15" | "FSK16" | "16" | "16+" => {
            if c == "15" {
                15
            } else {
                16
            }
        }
        "R" | "TVMA" | "MA" | "17" | "17+" | "NC17" => 17,
        "18" | "18+" | "X" | "XXX" | "FSK18" | "R18" | "ADULT" | "AO" => 18,
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
pub fn looks_adult(group: Option<&str>) -> bool {
    let Some(group) = group else { return false };
    let g = group.to_ascii_lowercase();
    const MARKERS: &[&str] = &["adult", "xxx", "porn", "erotic", "18+", "for adults"];
    MARKERS.iter().any(|m| g.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

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
