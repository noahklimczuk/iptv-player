//! User rule engine.
//!
//! README §7.3: "user-defined regex/keyword rules that auto-hide, auto-group, auto-rename, or
//! auto-favorite on every refresh", with presets for the jobs every 10,000-channel playlist
//! needs doing.

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::model::PlaylistEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Field {
    Name,
    Group,
    Url,
    Language,
    Country,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Match {
    Contains,
    Equals,
    StartsWith,
    EndsWith,
    Regex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "value")]
pub enum Action {
    Hide,
    Favorite,
    SetGroup(String),
    /// Replace the matched portion. With `Match::Regex`, `$1`-style captures are supported.
    Rename(String),
    /// Strip the matched text from the name, leaving the rest.
    StripFromName,
    SetNumber(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub enabled: bool,
    pub field: Field,
    #[serde(rename = "match")]
    pub match_kind: Match,
    pub pattern: String,
    pub case_sensitive: bool,
    pub action: Action,
}

impl Rule {
    pub fn new(field: Field, match_kind: Match, pattern: &str, action: Action) -> Self {
        Self {
            id: format!("{field:?}-{pattern}").to_lowercase(),
            enabled: true,
            field,
            match_kind,
            pattern: pattern.to_string(),
            case_sensitive: false,
            action,
        }
    }
}

/// What a rule pass did to an entry. Returned so the UI can preview a rule before applying it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub hidden: bool,
    pub favorite: bool,
    pub renamed: bool,
    pub regrouped: bool,
    /// Ids of the rules that fired, in order.
    pub fired: Vec<String>,
}

/// A compiled rule set. Compiling once and reusing it is what keeps a refresh over 40,000
/// entries fast.
#[derive(Debug)]
pub struct RuleSet {
    compiled: Vec<(Rule, Option<Regex>)>,
}

impl RuleSet {
    pub fn compile(rules: &[Rule]) -> Result<Self> {
        let mut compiled = Vec::with_capacity(rules.len());
        for rule in rules {
            let re = if rule.match_kind == Match::Regex {
                Some(
                    RegexBuilder::new(&rule.pattern)
                        .case_insensitive(!rule.case_sensitive)
                        .size_limit(1 << 20)
                        .build()
                        .map_err(|e| CoreError::Rule {
                            pattern: rule.pattern.clone(),
                            reason: e.to_string(),
                        })?,
                )
            } else {
                None
            };
            compiled.push((rule.clone(), re));
        }
        Ok(Self { compiled })
    }

    pub fn len(&self) -> usize {
        self.compiled.len()
    }

    pub fn is_empty(&self) -> bool {
        self.compiled.is_empty()
    }

    /// Apply every enabled rule to an entry, in order. Later rules see earlier edits.
    pub fn apply(&self, entry: &mut PlaylistEntry) -> Outcome {
        let mut outcome = Outcome::default();

        for (rule, re) in &self.compiled {
            if !rule.enabled {
                continue;
            }
            let Some(subject) = field_value(entry, rule.field) else {
                continue;
            };
            let subject = subject.to_string();
            if !matches(rule, re.as_ref(), &subject) {
                continue;
            }
            outcome.fired.push(rule.id.clone());

            match &rule.action {
                Action::Hide => outcome.hidden = true,
                Action::Favorite => outcome.favorite = true,
                Action::SetGroup(g) => {
                    entry.group = Some(g.clone());
                    outcome.regrouped = true;
                }
                Action::SetNumber(n) => entry.number = Some(*n),
                Action::Rename(replacement) => {
                    entry.name = match (rule.match_kind, re.as_ref()) {
                        (Match::Regex, Some(re)) => re
                            .replace_all(&entry.name, replacement.as_str())
                            .into_owned(),
                        _ => replacement.clone(),
                    };
                    outcome.renamed = true;
                }
                Action::StripFromName => {
                    entry.name = match (rule.match_kind, re.as_ref()) {
                        (Match::Regex, Some(re)) => re.replace_all(&entry.name, "").into_owned(),
                        _ => replace_ci(&entry.name, &rule.pattern, rule.case_sensitive),
                    }
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                    outcome.renamed = true;
                }
            }
        }
        outcome
    }
}

fn field_value(entry: &PlaylistEntry, field: Field) -> Option<&str> {
    match field {
        Field::Name => Some(entry.name.as_str()),
        Field::Group => entry.group.as_deref(),
        Field::Url => Some(entry.url.as_str()),
        Field::Language => entry.language.as_deref(),
        Field::Country => entry.country.as_deref(),
    }
}

fn matches(rule: &Rule, re: Option<&Regex>, subject: &str) -> bool {
    if rule.match_kind == Match::Regex {
        return re.map(|r| r.is_match(subject)).unwrap_or(false);
    }
    let (hay, needle) = if rule.case_sensitive {
        (subject.to_string(), rule.pattern.clone())
    } else {
        (subject.to_lowercase(), rule.pattern.to_lowercase())
    };
    match rule.match_kind {
        Match::Contains => hay.contains(&needle),
        Match::Equals => hay == needle,
        Match::StartsWith => hay.starts_with(&needle),
        Match::EndsWith => hay.ends_with(&needle),
        Match::Regex => unreachable!("handled above"),
    }
}

/// Does `hay` begin with `needle`, ignoring case? Returns how many of `hay`'s bytes the
/// match consumed.
///
/// Which is not always `needle.len()`, and that is the whole reason this exists.
/// Lowercasing is not length-preserving: `U+0130` Turkish dotted capital I lowercases
/// to `i` plus a combining dot, two bytes becoming three, and `U+023A` and `U+023E` do
/// the same. A search that lowercases the haystack and then uses the resulting byte
/// offsets to slice the *original* is off by one byte per such character before the
/// match -- which panics when it lands mid-character and silently eats a byte when it
/// does not (R-17).
///
/// Every character the haystack character lowercases to must be one the needle wants,
/// so a match can never end part-way through a character and strip half of one.
fn ci_match_len(hay: &str, needle: &str) -> Option<usize> {
    let mut wanted = needle.chars().flat_map(char::to_lowercase).peekable();
    let mut consumed = 0;
    for (idx, hc) in hay.char_indices() {
        if wanted.peek().is_none() {
            return Some(consumed);
        }
        for lc in hc.to_lowercase() {
            if wanted.next() != Some(lc) {
                return None;
            }
        }
        consumed = idx + hc.len_utf8();
    }
    wanted.peek().is_none().then_some(consumed)
}

fn replace_ci(haystack: &str, needle: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        return haystack.replace(needle, "");
    }
    if needle.is_empty() {
        return haystack.to_string();
    }
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    // Walks the original string a character at a time, so every index used to slice it
    // is a character boundary by construction and no offset is ever carried across from
    // a lowercased copy.
    while i < haystack.len() {
        match ci_match_len(&haystack[i..], needle) {
            Some(len) if len > 0 => i += len,
            _ => {
                let c = haystack[i..].chars().next().expect("i is a char boundary");
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}

/// The presets README §7.3 asks to ship.
pub mod presets {
    use super::*;

    /// Hide the groups a playlist files adult content under.
    ///
    /// The pattern was `\b(adult|xxx|porn|18\+)\b`, where the trailing `\b` can never
    /// match after `18\+`: `+` is not a word character, so the boundary needs one next
    /// to it, and in `18+` or `18+ Channels` what follows is a space or the end of the
    /// string. So the alternative written for the form playlists most often use was
    /// dead, and `adult` missed its own plural -- `FOR ADULTS` was not matched.
    ///
    /// The markers are now the ones `parental::looks_adult` uses, which is the other
    /// half of the same question and had `erotic` and `for adults` while this had
    /// neither. Two functions deciding "is this adult content" by different rules is how
    /// "adult content is hidden by default" becomes untrue for a playlist that writes it
    /// the other way (R-18).
    pub fn hide_adult() -> Rule {
        let mut r = Rule::new(
            Field::Group,
            Match::Regex,
            // `18\s*\+` ends at the `+` rather than asking for a boundary after it, and
            // allows the `18 +` some playlists write.
            r"(?i)\b(?:adults?|xxx|porn\w*|erotic\w*)\b|\b18\s*\+",
            Action::Hide,
        );
        r.id = "preset-hide-adult".into();
        r
    }

    pub fn hide_247() -> Rule {
        let mut r = Rule::new(
            Field::Name,
            Match::Regex,
            r"(?i)\b24[\s/\-]?7\b",
            Action::Hide,
        );
        r.id = "preset-hide-247".into();
        r
    }

    pub fn strip_quality_suffix() -> Rule {
        let mut r = Rule::new(
            Field::Name,
            Match::Regex,
            r"(?i)\s*\b(FHD|UHD|HD|SD|4K|1080p?|720p?)\b\s*$",
            Action::StripFromName,
        );
        r.id = "preset-strip-quality".into();
        r
    }

    pub fn all() -> Vec<Rule> {
        vec![hide_adult(), hide_247(), strip_quality_suffix()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, group: Option<&str>) -> PlaylistEntry {
        let mut e = PlaylistEntry::new(name, "https://example.com/s.ts");
        e.group = group.map(str::to_string);
        e
    }

    #[test]
    fn contains_is_case_insensitive_by_default() {
        let rs = RuleSet::compile(&[Rule::new(
            Field::Name,
            Match::Contains,
            "sport",
            Action::Favorite,
        )])
        .unwrap();
        let mut e = entry("Sky SPORTS Main Event", None);
        assert!(rs.apply(&mut e).favorite);
    }

    #[test]
    fn case_sensitivity_can_be_required() {
        let mut rule = Rule::new(Field::Name, Match::Contains, "sport", Action::Favorite);
        rule.case_sensitive = true;
        let rs = RuleSet::compile(&[rule]).unwrap();
        let mut e = entry("Sky SPORTS", None);
        assert!(!rs.apply(&mut e).favorite);
    }

    #[test]
    fn regroup_and_renumber() {
        let rs = RuleSet::compile(&[
            Rule::new(
                Field::Name,
                Match::Contains,
                "bbc",
                Action::SetGroup("UK".into()),
            ),
            Rule::new(
                Field::Name,
                Match::Equals,
                "BBC One",
                Action::SetNumber(101),
            ),
        ])
        .unwrap();
        let mut e = entry("BBC One", Some("Misc"));
        rs.apply(&mut e);
        assert_eq!(e.group.as_deref(), Some("UK"));
        assert_eq!(e.number, Some(101));
    }

    #[test]
    fn regex_rename_supports_captures() {
        let rs = RuleSet::compile(&[Rule::new(
            Field::Name,
            Match::Regex,
            r"^(\w+)\s*\|\s*(.+)$",
            Action::Rename("$2 ($1)".into()),
        )])
        .unwrap();
        let mut e = entry("US | CNN", None);
        rs.apply(&mut e);
        assert_eq!(e.name, "CNN (US)");
    }

    #[test]
    fn strip_removes_only_the_match() {
        let rs = RuleSet::compile(&[presets::strip_quality_suffix()]).unwrap();
        let mut e = entry("Discovery Channel FHD", None);
        rs.apply(&mut e);
        assert_eq!(e.name, "Discovery Channel");
    }

    #[test]
    fn strip_does_not_touch_mid_name_words() {
        let rs = RuleSet::compile(&[presets::strip_quality_suffix()]).unwrap();
        // "HD" here is not a trailing tag, so the anchored pattern must leave it alone.
        let mut e = entry("HD Cinema Classics", None);
        rs.apply(&mut e);
        assert_eq!(e.name, "HD Cinema Classics");
    }

    /// The three real titles on the subscription this is measured against that used to
    /// take the whole process down.
    ///
    /// `panic = "abort"` is set, so there was no error path: the viewer adds exactly the
    /// kind of house-keeping rule the preset list encourages -- strip " - 2026" from
    /// names -- and some hours later a background refresh kills the application while
    /// nobody is looking at it, with nothing in the log after it (R-17).
    #[test]
    fn stripping_around_a_turkish_dotted_capital_i_neither_panics_nor_eats_a_byte() {
        // U+0130 is 2 bytes and lowercases to 3, which is what shifted every later
        // offset by one.
        let real = "EN \u{2605} Bu \u{130}\u{15F}in \u{15E}akas\u{131} Yok - 2026";

        assert_eq!(
            replace_ci(real, "2026", false),
            "EN \u{2605} Bu \u{130}\u{15F}in \u{15E}akas\u{131} Yok - ",
            "this is the case that panicked"
        );
        assert_eq!(
            replace_ci(real, " - ", false),
            "EN \u{2605} Bu \u{130}\u{15F}in \u{15E}akas\u{131} Yok2026",
            "and this is the case that ate the 2"
        );
        // A match that precedes the character was always right, and still is.
        assert_eq!(
            replace_ci("ETR \u{2605} \u{130}yilik", "etr", false),
            " \u{2605} \u{130}yilik"
        );

        // The other two code points that lowercase longer than they are.
        assert_eq!(
            replace_ci("\u{23A}X\u{23E}Y", "x", false),
            "\u{23A}\u{23E}Y"
        );
        assert_eq!(replace_ci("\u{23E}tail", "TAIL", false), "\u{23E}");
    }

    /// A rule is reached through `apply`, not by calling `replace_ci` directly, so the
    /// path a refresh actually takes is covered too.
    #[test]
    fn a_strip_rule_survives_a_real_turkish_title() {
        let mut rule = Rule::new(
            Field::Name,
            Match::Contains,
            " - 2026",
            Action::StripFromName,
        );
        rule.case_sensitive = false;
        let rs = RuleSet::compile(&[rule]).unwrap();

        let mut e = entry(
            "EN \u{2605} Bu \u{130}\u{15F}in \u{15E}akas\u{131} Yok - 2026",
            None,
        );
        rs.apply(&mut e);
        assert_eq!(
            e.name,
            "EN \u{2605} Bu \u{130}\u{15F}in \u{15E}akas\u{131} Yok"
        );
    }

    /// Matching is per whole character: a needle that would cover only part of what a
    /// character lowercases to is not a match, so half of one is never stripped.
    #[test]
    fn a_match_never_ends_part_way_through_a_character() {
        // U+0130 lowercases to 'i' plus a combining dot. Asking for "i" must not take
        // the dotted capital and leave the dot behind.
        assert_eq!(replace_ci("\u{130}", "i", false), "\u{130}");
        // A plain lowercase 'i' is still matched.
        assert_eq!(replace_ci("mix", "i", false), "mx");
    }

    #[test]
    fn hide_adult_preset_fires_on_group() {
        let rs = RuleSet::compile(&[presets::hide_adult()]).unwrap();
        let mut e = entry("Some Channel", Some("XXX | Adult"));
        assert!(rs.apply(&mut e).hidden);

        let mut ok = entry("Kids TV", Some("Children"));
        assert!(!rs.apply(&mut ok).hidden);
    }

    /// The forms a playlist actually writes, two of which the preset could not match.
    ///
    /// `18+` is the common one and the trailing `\b` made it impossible; `FOR ADULTS`
    /// was missed by a singular `adult`. Both were hidden by `parental::looks_adult` at
    /// the same time, which is what made the disagreement invisible (R-18).
    #[test]
    fn hide_adult_preset_matches_the_forms_playlists_write() {
        let rs = RuleSet::compile(&[presets::hide_adult()]).unwrap();
        for group in [
            "18+",
            "18+ Channels",
            "18 + VOD",
            "ADULT 18+",
            "FOR ADULTS",
            "For Adults Only",
            "XXX",
            "Adult Movies",
            "PORNO HD",
            "Erotica",
            "EROTIC VOD",
        ] {
            let mut e = entry("Some Channel", Some(group));
            assert!(rs.apply(&mut e).hidden, "{group:?} was not hidden");
            // And the preset agrees with the other half of the same question.
            assert!(
                crate::parental::looks_adult(Some(group)),
                "{group:?} is hidden by the preset but not by looks_adult"
            );
        }

        // Still not matching what it never should: a number that is not an age rating,
        // and ordinary groups.
        for group in ["Channel 18", "Documentaries", "Kids"] {
            let mut e = entry("Some Channel", Some(group));
            assert!(!rs.apply(&mut e).hidden, "{group:?} should not be hidden");
        }
    }

    #[test]
    fn hide_247_preset_matches_common_spellings() {
        let rs = RuleSet::compile(&[presets::hide_247()]).unwrap();
        for name in ["24/7 Friends", "24-7 Movies", "24 7 Comedy"] {
            let mut e = entry(name, None);
            assert!(rs.apply(&mut e).hidden, "should hide {name}");
        }
        let mut keep = entry("Channel 24", None);
        assert!(!rs.apply(&mut keep).hidden);
    }

    #[test]
    fn rules_apply_in_order_and_compose() {
        let rs = RuleSet::compile(&[
            presets::strip_quality_suffix(),
            Rule::new(Field::Name, Match::Equals, "BBC One", Action::Favorite),
        ])
        .unwrap();
        let mut e = entry("BBC One HD", None);
        let out = rs.apply(&mut e);
        assert_eq!(e.name, "BBC One");
        // The second rule sees the renamed value.
        assert!(out.favorite);
        assert_eq!(out.fired.len(), 2);
    }

    #[test]
    fn disabled_rules_do_nothing() {
        let mut rule = presets::hide_adult();
        rule.enabled = false;
        let rs = RuleSet::compile(&[rule]).unwrap();
        let mut e = entry("X", Some("XXX"));
        assert!(!rs.apply(&mut e).hidden);
    }

    #[test]
    fn an_invalid_regex_is_a_clear_error_not_a_panic() {
        let err = RuleSet::compile(&[Rule::new(
            Field::Name,
            Match::Regex,
            "([unclosed",
            Action::Hide,
        )])
        .unwrap_err();
        assert!(matches!(err, CoreError::Rule { .. }));
    }

    #[test]
    fn missing_optional_fields_never_match() {
        let rs = RuleSet::compile(&[Rule::new(
            Field::Language,
            Match::Contains,
            "en",
            Action::Hide,
        )])
        .unwrap();
        let mut e = entry("No language set", None);
        assert!(!rs.apply(&mut e).hidden);
    }
}
