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
                        (Match::Regex, Some(re)) => {
                            re.replace_all(&entry.name, replacement.as_str()).into_owned()
                        }
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

fn replace_ci(haystack: &str, needle: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        return haystack.replace(needle, "");
    }
    if needle.is_empty() {
        return haystack.to_string();
    }
    let lower_hay = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while let Some(pos) = lower_hay[i..].find(&lower_needle) {
        let abs = i + pos;
        out.push_str(&haystack[i..abs]);
        i = abs + needle.len();
    }
    out.push_str(&haystack[i..]);
    out
}

/// The presets README §7.3 asks to ship.
pub mod presets {
    use super::*;

    pub fn hide_adult() -> Rule {
        let mut r = Rule::new(
            Field::Group,
            Match::Regex,
            r"(?i)\b(adult|xxx|porn|18\+)\b",
            Action::Hide,
        );
        r.id = "preset-hide-adult".into();
        r
    }

    pub fn hide_247() -> Rule {
        let mut r = Rule::new(Field::Name, Match::Regex, r"(?i)\b24[\s/\-]?7\b", Action::Hide);
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
            Rule::new(Field::Name, Match::Contains, "bbc", Action::SetGroup("UK".into())),
            Rule::new(Field::Name, Match::Equals, "BBC One", Action::SetNumber(101)),
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

    #[test]
    fn hide_adult_preset_fires_on_group() {
        let rs = RuleSet::compile(&[presets::hide_adult()]).unwrap();
        let mut e = entry("Some Channel", Some("XXX | Adult"));
        assert!(rs.apply(&mut e).hidden);

        let mut ok = entry("Kids TV", Some("Children"));
        assert!(!rs.apply(&mut ok).hidden);
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
