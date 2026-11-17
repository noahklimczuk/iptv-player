//! A synced record and the rule for merging two copies of it.
//!
//! A record is one thing the viewer owns -- a favourite, a film's progress, a profile --
//! named by a key that means the same on every device (`favorite:{"pf":…,"item":…}`).
//! Each field carries the hybrid logical clock of the change that last set it, and so
//! does the record's existence (`alive`): a removal is a tombstone with a clock, not an
//! absence, which is what stops an older copy on another device resurrecting it.
//!
//! The merge is the same for every kind: per field, the later clock wins. That is
//! "last writer wins, field by field" (§3.4), and two concepts fall out of it with no
//! special casing:
//!
//! - **Sets with tombstones.** A favourite, a My List entry and a Not interested mark are
//!   each their own record, so add and remove are writes to `alive`.
//! - **Sticky `completed`.** A device only stamps a field it actually changed (see
//!   [`Record::capture`]). A stale device that saves a mid-film position never changed
//!   `completed`, so its write carries the old clock for that field and the "finished"
//!   from another device survives it. Only an explicit "mark unwatched" -- a real change
//!   of the field -- clears it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A record's field values, as read from or written to a local row.
pub type Fields = BTreeMap<String, Value>;

/// A value and the clock of the change that set it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clocked<T> {
    #[serde(rename = "v")]
    pub value: T,
    #[serde(rename = "t")]
    pub hlc: i64,
}

impl<T: Serialize> Clocked<T> {
    /// Whether `self` beats `other`. A later clock wins; an exact tie -- two devices
    /// stamping the same millisecond and counter -- is broken on the value so that every
    /// device picks the same winner.
    fn beats(&self, other: &Self) -> bool {
        match self.hlc.cmp(&other.hlc) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => {
                let a = serde_json::to_string(&self.value).unwrap_or_default();
                let b = serde_json::to_string(&other.value).unwrap_or_default();
                a > b
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// `{kind}:{ident}`, exactly as the capture triggers wrote it. Kept as written rather
    /// than rebuilt from `ident`, because JSON object order is part of the string.
    #[serde(rename = "k")]
    pub key: String,
    #[serde(rename = "n")]
    pub kind: String,
    /// The record's identity in device-independent terms.
    #[serde(rename = "i")]
    pub ident: Value,
    /// Whether the thing exists. False is a tombstone.
    #[serde(rename = "a")]
    pub alive: Clocked<bool>,
    #[serde(rename = "f", default)]
    pub fields: BTreeMap<String, Clocked<Value>>,
    /// The device that wrote this version.
    #[serde(rename = "w", default)]
    pub writer: String,
}

impl Record {
    /// Build the record for a local row as it is now.
    ///
    /// `shadow` is the last merged state this device knows. A field whose value is what
    /// the shadow already says keeps the shadow's clock; only a field this device actually
    /// changed is stamped with `hlc`. `row` is `None` when the row is gone, which makes
    /// the record a tombstone that keeps its last known field values.
    pub fn capture(
        shadow: Option<&Record>,
        key: &str,
        kind: &str,
        ident: &Value,
        row: Option<&Fields>,
        hlc: i64,
        writer: &str,
    ) -> Record {
        let alive_now = row.is_some();
        let alive = match shadow {
            Some(s) if s.alive.value == alive_now => s.alive.clone(),
            _ => Clocked {
                value: alive_now,
                hlc,
            },
        };
        let mut fields = shadow.map(|s| s.fields.clone()).unwrap_or_default();
        if let Some(row) = row {
            for (name, value) in row {
                let unchanged = fields.get(name).is_some_and(|f| &f.value == value);
                if !unchanged {
                    fields.insert(
                        name.clone(),
                        Clocked {
                            value: value.clone(),
                            hlc,
                        },
                    );
                }
            }
        }
        Record {
            key: key.to_string(),
            kind: kind.to_string(),
            ident: ident.clone(),
            alive,
            fields,
            writer: writer.to_string(),
        }
    }

    /// Merge two copies of the same record. Commutative, associative and idempotent, so
    /// devices that see the same writes in any order agree.
    pub fn merge(&self, other: &Record) -> Record {
        debug_assert_eq!(self.key, other.key);
        let alive = if other.alive.beats(&self.alive) {
            other.alive.clone()
        } else {
            self.alive.clone()
        };
        let mut fields = self.fields.clone();
        for (name, theirs) in &other.fields {
            match fields.get(name) {
                Some(ours) if !theirs.beats(ours) => {}
                _ => {
                    fields.insert(name.clone(), theirs.clone());
                }
            }
        }
        let writer = if other.max_clock() > self.max_clock() {
            other.writer.clone()
        } else {
            self.writer.clone()
        };
        Record {
            key: self.key.clone(),
            kind: self.kind.clone(),
            ident: self.ident.clone(),
            alive,
            fields,
            writer,
        }
    }

    /// The latest clock anywhere in the record.
    pub fn max_clock(&self) -> i64 {
        self.fields
            .values()
            .map(|f| f.hlc)
            .fold(self.alive.hlc, i64::max)
    }

    /// The values to write locally, or `None` for a tombstone.
    pub fn live_fields(&self) -> Option<Fields> {
        self.alive.value.then(|| {
            self.fields
                .iter()
                .map(|(k, v)| (k.clone(), v.value.clone()))
                .collect()
        })
    }

    /// Whether `self` says anything `other` does not, ignoring who wrote it.
    pub fn differs_from(&self, other: &Record) -> bool {
        self.alive != other.alive || self.fields != other.fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(pairs: &[(&str, Value)]) -> Fields {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn rec(row_: Option<&Fields>, hlc: i64, shadow: Option<&Record>) -> Record {
        Record::capture(shadow, "k", "progress", &json!({}), row_, hlc, "dev")
    }

    #[test]
    fn only_the_fields_that_changed_take_the_new_clock() {
        let first = rec(
            Some(&row(&[("pos", json!(10)), ("done", json!(0))])),
            100,
            None,
        );
        let second = rec(
            Some(&row(&[("pos", json!(20)), ("done", json!(0))])),
            200,
            Some(&first),
        );
        assert_eq!(second.fields["pos"].hlc, 200);
        assert_eq!(second.fields["done"].hlc, 100);
        assert_eq!(second.alive.hlc, 100);
    }

    #[test]
    fn a_finished_film_is_not_unfinished_by_a_stale_position() {
        // Both devices start from the same state.
        let base = rec(
            Some(&row(&[("pos", json!(10)), ("done", json!(0))])),
            100,
            None,
        );
        // The TV finishes the film.
        let tv = rec(
            Some(&row(&[("pos", json!(7000)), ("done", json!(1))])),
            300,
            Some(&base),
        );
        // The PC, offline, saved a mid-film position *later* by the clock.
        let pc = rec(
            Some(&row(&[("pos", json!(50)), ("done", json!(0))])),
            400,
            Some(&base),
        );
        let merged = tv.merge(&pc);
        assert_eq!(merged.fields["done"].value, json!(1), "completed is sticky");
        assert_eq!(merged.fields["pos"].value, json!(50));
        assert_eq!(pc.merge(&tv), merged, "merge is commutative");
    }

    #[test]
    fn an_explicit_unwatch_does_clear_it() {
        let done = rec(Some(&row(&[("done", json!(1))])), 100, None);
        let unwatched = rec(Some(&row(&[("done", json!(0))])), 200, Some(&done));
        assert_eq!(done.merge(&unwatched).fields["done"].value, json!(0));
    }

    #[test]
    fn a_removal_is_not_resurrected_by_an_older_copy() {
        let added = rec(Some(&row(&[("at", json!(1))])), 100, None);
        let removed = rec(None, 200, Some(&added));
        assert!(!removed.alive.value);
        assert_eq!(added.merge(&removed).live_fields(), None);
        assert_eq!(removed.merge(&added).live_fields(), None);
    }

    #[test]
    fn re_adding_after_a_removal_wins_over_the_removal() {
        let added = rec(Some(&row(&[("at", json!(1))])), 100, None);
        let removed = rec(None, 200, Some(&added));
        let again = rec(Some(&row(&[("at", json!(3))])), 300, Some(&removed));
        assert!(removed.merge(&again).alive.value);
    }

    #[test]
    fn concurrent_edits_to_different_fields_both_survive() {
        let base = rec(
            Some(&row(&[("name", json!("Kid")), ("pin", Value::Null)])),
            100,
            None,
        );
        let renamed = rec(
            Some(&row(&[("name", json!("Sam")), ("pin", Value::Null)])),
            200,
            Some(&base),
        );
        let pinned = rec(
            Some(&row(&[("name", json!("Kid")), ("pin", json!("hash"))])),
            150,
            Some(&base),
        );
        let merged = renamed.merge(&pinned);
        assert_eq!(merged.fields["name"].value, json!("Sam"));
        assert_eq!(merged.fields["pin"].value, json!("hash"));
    }

    #[test]
    fn an_exact_tie_resolves_the_same_way_on_every_device() {
        let a = rec(Some(&row(&[("x", json!("a"))])), 100, None);
        let b = rec(Some(&row(&[("x", json!("b"))])), 100, None);
        assert_eq!(a.merge(&b).fields["x"], b.merge(&a).fields["x"]);
    }

    #[test]
    fn merge_is_idempotent_and_associative() {
        let a = rec(Some(&row(&[("x", json!(1))])), 100, None);
        let b = rec(Some(&row(&[("y", json!(2))])), 200, None);
        let c = rec(None, 300, None);
        assert_eq!(a.merge(&a), a);
        assert_eq!(a.merge(&b).merge(&c), a.merge(&b.merge(&c)));
    }

    #[test]
    fn a_record_round_trips_through_json() {
        let a = rec(
            Some(&row(&[("x", json!(1)), ("y", Value::Null)])),
            100,
            None,
        );
        let back: Record = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(a, back);
    }
}
