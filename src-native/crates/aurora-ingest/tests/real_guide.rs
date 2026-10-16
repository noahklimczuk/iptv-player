//! EPG matching, replayed over a library a real import produced.
//!
//! The unit tests for `EpgIndex` use a handful of invented channels, which is how the
//! fuzzy pass came to hand 655 real channels another channel's schedule: every one of
//! those is a pair of long names that no fixture contains, and the rule that was
//! supposed to prevent it (`key.len() >= 5`) only ever excluded *short* ones (R-22).
//!
//! `#[ignore]` because it needs a real library on disk. After a host run:
//!
//! ```text
//! cargo test -p aurora-ingest --test real_guide -- --ignored --nocapture
//! ```
//!
//! Point `AURORA_REAL_LIBRARY` at a different `library.db` to use another one.

use aurora_core::epg_match::{EpgIndex, MatchMethod};
use aurora_core::model::EpgChannel;
use aurora_core::title;
use aurora_db::rusqlite;

const DEFAULT_LIBRARY: &str = r"C:\aurora-dev\target\release\data\library.db";

fn library() -> rusqlite::Connection {
    let path = std::env::var("AURORA_REAL_LIBRARY").unwrap_or_else(|_| DEFAULT_LIBRARY.into());
    assert!(
        std::path::Path::new(&path).exists(),
        "no library at {path}. Run a host scenario first, or set AURORA_REAL_LIBRARY."
    );
    rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .expect("the library should open read-only")
}

/// Are these two keys one *digit* apart?
///
/// Derived here from the public `match_key` rather than from the matcher's own private
/// helper, so the assertion is independent of the code it is checking.
fn differs_by_one_digit(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    match a.len().cmp(&b.len()) {
        std::cmp::Ordering::Equal => {
            let mut diffs = a.iter().zip(&b).filter(|(x, y)| x != y);
            match (diffs.next(), diffs.next()) {
                (Some((x, y)), None) => x.is_ascii_digit() || y.is_ascii_digit(),
                _ => false,
            }
        }
        _ => {
            let (short, long) = if a.len() < b.len() {
                (&a, &b)
            } else {
                (&b, &a)
            };
            if long.len() - short.len() != 1 {
                return false;
            }
            let at = (0..short.len())
                .find(|&i| short[i] != long[i])
                .unwrap_or(short.len());
            // The rest must line up after the insertion for this to be one edit.
            let shifted = (at..short.len()).all(|i| short[i] == long[i + 1]);
            shifted && long.get(at).is_some_and(|c| c.is_ascii_digit())
        }
    }
}

/// No channel is shown a schedule belonging to a channel with a different number.
///
/// Measured before the fix, on this subscription: 655 fuzzy matches, 639 of them (97%)
/// differing by a single digit -- `espn156` handed `espn15`, `spectrumxcast3` handed
/// `spectrumxcast` -- and 624 where several candidates were tied at the same distance,
/// so the answer came down to the order the provider wrote its `<channel>` elements in.
/// Each of those channels showed another channel's programmes at confidence 60, which is
/// worse than a blank row: a blank row is understood and a plausible wrong title is
/// believed.
#[test]
#[ignore = "real library: run with --ignored"]
fn no_channel_is_handed_a_schedule_that_differs_only_by_a_number() {
    let db = library();

    let guide: Vec<EpgChannel> = db
        .prepare("SELECT id, display_name FROM epg_channels")
        .unwrap()
        .query_map([], |r| {
            let id: String = r.get(0)?;
            let name: Option<String> = r.get(1)?;
            Ok(EpgChannel {
                id,
                display_names: name.into_iter().collect(),
                icon: None,
            })
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        guide.len() > 100,
        "only {} guide channels -- is this a real library?",
        guide.len()
    );

    let names: Vec<String> = db
        .prepare("SELECT name FROM channels")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(names.len() > 100, "only {} channels", names.len());

    // What the index itself was built from, so a matched id can be named back.
    let index = EpgIndex::build(&guide);
    let key_of_id: std::collections::HashMap<&str, String> = guide
        .iter()
        .map(|c| {
            let key = c
                .display_names
                .first()
                .map(|n| title::match_key(n))
                .filter(|k| !k.is_empty())
                .unwrap_or_else(|| title::match_key(&c.id));
            (c.id.as_str(), key)
        })
        .collect();

    let mut fuzzy = 0;
    let mut by_a_digit = Vec::new();
    for name in &names {
        let Some(hit) = index.resolve(None, name, None) else {
            continue;
        };
        if hit.method != MatchMethod::Fuzzy {
            continue;
        }
        fuzzy += 1;
        let ours = title::match_key(name);
        let theirs = key_of_id
            .get(hit.epg_channel_id.as_str())
            .cloned()
            .unwrap_or_default();
        if differs_by_one_digit(&ours, &theirs) {
            by_a_digit.push(format!("{name:?} key {ours:?} -> {theirs:?}"));
        }
    }

    println!(
        "   {} channels, {} guide channels, {fuzzy} fuzzy matches, {} of them by a digit",
        names.len(),
        guide.len(),
        by_a_digit.len()
    );
    for line in by_a_digit.iter().take(10) {
        println!("      {line}");
    }
    assert!(
        by_a_digit.is_empty(),
        "{} channels are being shown the schedule of a channel that differs from them \
         only by a number",
        by_a_digit.len()
    );
}

/// No channel is matched to a guide channel that has nothing scheduled against it.
///
/// Builds the index the way `sync::apply` does -- from the ids the guide actually
/// schedules programmes against, not from every id it declares. On this library 4,827
/// ids are declared and 2,937 carry a programme, and before the two fixes in this
/// change 909 channels were pointed at one of the other 1,890: 360 by an exact
/// `tvg-id`, 67 by normalized name (R-21) and 482 by the fuzzy pass (R-22). Each one
/// got an empty guide row while the diagnostics counted it as covered.
#[test]
#[ignore = "real library: run with --ignored"]
fn no_channel_is_matched_to_a_guide_channel_with_no_programmes() {
    let db = library();

    let scheduled: std::collections::HashSet<String> = db
        .prepare("SELECT DISTINCT channel_id FROM epg_programmes")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        !scheduled.is_empty(),
        "no programmes at all -- is this a real library?"
    );

    let declared: Vec<EpgChannel> = db
        .prepare("SELECT id, display_name FROM epg_channels")
        .unwrap()
        .query_map([], |r| {
            let id: String = r.get(0)?;
            let name: Option<String> = r.get(1)?;
            Ok(EpgChannel {
                id,
                display_names: name.into_iter().collect(),
                icon: None,
            })
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    // The pruning `sync::apply` performs before building the index.
    let offered: Vec<EpgChannel> = declared
        .iter()
        .filter(|c| scheduled.contains(&c.id))
        .cloned()
        .collect();
    println!(
        "   {} guide channels declared, {} with programmes, {} offered for matching",
        declared.len(),
        scheduled.len(),
        offered.len()
    );

    let index = EpgIndex::build(&offered);
    let channels: Vec<(String, Option<String>)> = db
        .prepare("SELECT name, tvg_id FROM channels")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    let mut matched = 0;
    let mut empty = Vec::new();
    for (name, tvg_id) in &channels {
        let Some(hit) = index.resolve(tvg_id.as_deref(), name, None) else {
            continue;
        };
        matched += 1;
        if !scheduled.contains(&hit.epg_channel_id) {
            empty.push(format!(
                "{name:?} -> {:?} ({:?})",
                hit.epg_channel_id, hit.method
            ));
        }
    }

    println!(
        "   {} channels, {matched} matched, {} of them to a guide channel with nothing in it",
        channels.len(),
        empty.len()
    );
    for line in empty.iter().take(10) {
        println!("      {line}");
    }
    assert!(
        empty.is_empty(),
        "{} channels are reported as having a guide that holds no programme for them",
        empty.len()
    );
}
