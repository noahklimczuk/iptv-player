//! Version numbers, and what a change does to them.
//!
//! README §23 asks for real versions rather than a build counter: the third number
//! moves for a bug fix, the middle one for a feature. That decision is derived from
//! the commit subjects rather than asked of anyone, because the repository already
//! writes conventional commits (`fix(settings): …`, `feat(dvr): …`) and a rule nobody
//! has to remember is the only kind that survives.
//!
//! Two callers want different halves of this. The updater compares the running
//! version against whatever GitHub's latest release advertises, which needs parsing
//! and ordering. The release build needs the next number, which needs [`bump_for`].

use std::fmt;

use serde::{Deserialize, Serialize};

/// A plain `major.minor.patch` triple.
///
/// Field order matters: the derived `Ord` compares major, then minor, then patch,
/// which is exactly semver ordering for triples. Comparing the *strings* would put
/// `0.9.0` above `0.10.0`, and an updater that believes that stops offering updates
/// forever at `.9`.
///
/// Serialized as the string `"0.2.0"` rather than three fields: it crosses the IPC
/// boundary inside an update check, and the UI wants to print it, not rebuild it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse `1.2.3`, tolerating a leading `v` and surrounding whitespace.
    ///
    /// Anything else is `None`, including a pre-release suffix like `0.2.0-rc1`.
    /// Accepting those quietly would mean treating a release candidate as its final
    /// version, which is the one mistake this type exists to prevent; nothing in this
    /// project publishes them, so refusing is honest rather than restrictive.
    pub fn parse(raw: &str) -> Option<Self> {
        let text = raw.trim();
        let text = text
            .strip_prefix('v')
            .or_else(|| text.strip_prefix('V'))
            .unwrap_or(text);

        let mut parts = text.split('.');
        let major = number(parts.next()?)?;
        let minor = number(parts.next()?)?;
        let patch = number(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self::new(major, minor, patch))
    }

    /// The version this one becomes after `bump`.
    ///
    /// A minor bump resets the patch: 0.1.7 with a feature in it is 0.2.0, never
    /// 0.2.7.
    pub const fn bumped(self, bump: Bump) -> Self {
        match bump {
            Bump::Minor => Self::new(self.major, self.minor + 1, 0),
            Bump::Patch => Self::new(self.major, self.minor, self.patch + 1),
        }
    }
}

/// Digits only. `u32::from_str_radix` would accept a leading `+`.
fn number(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

impl From<Version> for String {
    fn from(v: Version) -> Self {
        v.to_string()
    }
}

impl TryFrom<String> for Version {
    type Error = String;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Version::parse(&raw).ok_or_else(|| format!("{raw:?} is not a major.minor.patch version"))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// How far a release moves the number.
///
/// There is deliberately no `Major`. Going to 1.0 is a judgement about whether the
/// thing is finished, which no commit message can make on anyone's behalf, so it
/// stays a decision someone takes by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bump {
    Minor,
    Patch,
}

/// The conventional-commit type of a subject line: `feat(dvr)!: …` is `("feat", true)`.
///
/// Returns `None` for a subject that is not a conventional commit at all, which is
/// treated the same as any non-feature: a patch.
fn commit_type(subject: &str) -> Option<(&str, bool)> {
    let colon = subject.find(':')?;
    let head = subject[..colon].trim();

    // `feat(scope)!:` puts the breaking marker after the scope, `feat!:` has no scope,
    // so the marker comes off first and either shape is left.
    let (head, breaking) = match head.strip_suffix('!') {
        Some(rest) => (rest, true),
        None => (head, false),
    };

    let ty = match head.find('(') {
        // An unclosed scope is not a conventional commit, it is prose with a bracket.
        Some(open) if head.ends_with(')') => &head[..open],
        Some(_) => return None,
        None => head,
    };

    if ty.is_empty() || !ty.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    Some((ty, breaking))
}

/// What the given commit subjects add up to.
///
/// One `feat` anywhere in the batch makes the whole release a feature release. Every
/// other shape — `fix`, `ci`, `docs`, a subject that is not a conventional commit at
/// all, or no commits — is a patch, because a build that ships must still get a
/// number of its own.
///
/// A breaking marker counts as a feature rather than a major bump. Below 1.0 that is
/// what semver itself prescribes, and above it the decision is not a script's to make.
pub fn bump_for<'a, I: IntoIterator<Item = &'a str>>(subjects: I) -> Bump {
    for subject in subjects {
        match commit_type(subject) {
            Some((_, true)) => return Bump::Minor,
            Some((ty, _)) if ty.eq_ignore_ascii_case("feat") => return Bump::Minor,
            _ => {}
        }
    }
    Bump::Patch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_triple_parses() {
        assert_eq!(Version::parse("0.1.0"), Some(Version::new(0, 1, 0)));
        assert_eq!(Version::parse("12.34.56"), Some(Version::new(12, 34, 56)));
    }

    #[test]
    fn a_tag_keeps_its_v_out_of_the_numbers() {
        assert_eq!(Version::parse("v0.2.0"), Some(Version::new(0, 2, 0)));
        assert_eq!(Version::parse("V0.2.0"), Some(Version::new(0, 2, 0)));
        assert_eq!(Version::parse("  v1.0.0\n"), Some(Version::new(1, 0, 0)));
    }

    #[test]
    fn anything_that_is_not_three_numbers_is_rejected() {
        for raw in [
            "",
            "v",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "1..2",
            "a.b.c",
            "1.2.-3",
            "1.2.+3",
            // A pre-release is not its final version, and must not be mistaken for one.
            "0.2.0-rc1",
            "0.2.0+build7",
        ] {
            assert_eq!(Version::parse(raw), None, "{raw:?} should not parse");
        }
    }

    #[test]
    fn ordering_is_numeric_not_lexicographic() {
        // The whole reason this is a struct and not a string.
        assert!(Version::parse("0.10.0").unwrap() > Version::parse("0.9.0").unwrap());
        assert!(Version::parse("0.2.0").unwrap() > Version::parse("0.1.9").unwrap());
        assert!(Version::parse("1.0.0").unwrap() > Version::parse("0.99.99").unwrap());
        assert!(Version::parse("0.1.10").unwrap() > Version::parse("0.1.9").unwrap());
        assert_eq!(
            Version::parse("0.1.0").unwrap(),
            Version::parse("v0.1.0").unwrap()
        );
    }

    #[test]
    fn a_patch_moves_only_the_last_number() {
        assert_eq!(
            Version::new(0, 1, 0).bumped(Bump::Patch),
            Version::new(0, 1, 1)
        );
    }

    #[test]
    fn a_feature_moves_the_middle_number_and_clears_the_last() {
        assert_eq!(
            Version::new(0, 1, 7).bumped(Bump::Minor),
            Version::new(0, 2, 0)
        );
    }

    #[test]
    fn a_feature_anywhere_in_the_batch_wins() {
        let subjects = [
            "fix(settings): wire Add provider to the wizard",
            "feat(dvr): record a whole series",
            "ci: cache the bundler",
        ];
        assert_eq!(bump_for(subjects), Bump::Minor);
    }

    #[test]
    fn fixes_and_chores_are_patches() {
        assert_eq!(bump_for(["fix: stop the crash"]), Bump::Patch);
        assert_eq!(bump_for(["ci: build faster"]), Bump::Patch);
        assert_eq!(bump_for(["docs: explain the updater"]), Bump::Patch);
        assert_eq!(
            bump_for(["refactor(db): split the repo module"]),
            Bump::Patch
        );
    }

    #[test]
    fn a_build_with_nothing_conventional_in_it_still_gets_a_number() {
        assert_eq!(bump_for(["merged main", "wip"]), Bump::Patch);
        assert_eq!(bump_for([]), Bump::Patch);
    }

    #[test]
    fn the_word_feature_in_prose_is_not_a_feature_commit() {
        // The trap a substring search would fall into: both of these are fixes.
        assert_eq!(
            bump_for(["fix: add the feature flag we forgot"]),
            Bump::Patch
        );
        assert_eq!(
            bump_for(["docs: describe the feat of strength"]),
            Bump::Patch
        );
        assert_eq!(bump_for(["defeat the flake"]), Bump::Patch);
    }

    #[test]
    fn a_scope_does_not_hide_the_type() {
        assert_eq!(bump_for(["feat(playlist): bulk edit"]), Bump::Minor);
        assert_eq!(bump_for(["feat(a-b/c 1): thing"]), Bump::Minor);
    }

    #[test]
    fn a_breaking_change_is_a_feature_not_a_major() {
        assert_eq!(bump_for(["feat!: rename every command"]), Bump::Minor);
        assert_eq!(bump_for(["fix(db)!: drop the legacy column"]), Bump::Minor);
        // Never a major: that is a person's call.
        assert_eq!(
            Version::new(0, 4, 2).bumped(bump_for(["feat!: x"])),
            Version::new(0, 5, 0)
        );
    }

    #[test]
    fn a_bracket_in_prose_is_not_a_scope() {
        assert_eq!(commit_type("feat(unclosed: thing"), None);
        assert_eq!(commit_type("we (finally) shipped: the thing"), None);
    }

    #[test]
    fn it_crosses_the_wire_as_a_string() {
        let v = Version::new(0, 12, 3);
        assert_eq!(serde_json::to_string(&v).unwrap(), "\"0.12.3\"");
        assert_eq!(serde_json::from_str::<Version>("\"0.12.3\"").unwrap(), v);
        assert!(serde_json::from_str::<Version>("\"nope\"").is_err());
    }

    #[test]
    fn display_round_trips_through_parse() {
        for v in [
            Version::new(0, 0, 0),
            Version::new(0, 1, 0),
            Version::new(1, 20, 300),
        ] {
            assert_eq!(Version::parse(&v.to_string()), Some(v));
        }
    }
}
