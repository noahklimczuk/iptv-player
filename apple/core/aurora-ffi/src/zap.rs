//! Changing channel from inside the player, as a set-top box does: Windows' `useZapper`
//! and `zap.ts` (README §7.2). Channel up and down, a number typed on the keypad, and
//! back to the last channel.
//!
//! The lookups are the same calls Windows makes: `channels.byNumber` for a typed number
//! and, for up and down, a step through `channels.list` with no arguments, the list
//! Windows' `App` zaps through. The timing and the digit buffer are Windows' too, and
//! `mirrors.rs` checks both against `src-ui`.

use aurora_db::repo::channels::{self, ChannelFilter};
use aurora_db::repo::filtering::LibraryFilter;

use crate::error::Result;
use crate::library::Channel;
use crate::AuroraCore;

/// How long after the last digit a typed number is tuned (`DIGIT_TIMEOUT_MS`).
pub const DIGIT_TIMEOUT_MS: u32 = 1800;
/// How many digits the buffer keeps; more are ignored, as Windows' `pushDigit` slices.
pub const MAX_DIGITS: u32 = 4;

/// Windows' timings for number entry, for the player to keep.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct ZapRules {
    pub digit_timeout_ms: u32,
    pub max_digits: u32,
}

#[uniffi::export]
pub fn zap_rules() -> ZapRules {
    ZapRules {
        digit_timeout_ms: DIGIT_TIMEOUT_MS,
        max_digits: MAX_DIGITS,
    }
}

/// Add a digit to what has been typed so far, as Windows' `pushDigit` does: past
/// `MAX_DIGITS` the extra digits are dropped, not the earlier ones.
#[uniffi::export]
pub fn push_digit(typed: String, digit: String) -> String {
    let mut all = typed;
    all.push_str(&digit);
    all.chars().take(MAX_DIGITS as usize).collect()
}

/// Where Ch+ or Ch− lands in a list of `count`, from position `current`. From outside the
/// list, Ch+ is the first channel and Ch− the last; at either end it wraps
/// (`stepIndex`).
fn step_index(count: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match current {
        None if forward => 0,
        None => count - 1,
        Some(i) if forward => (i + 1) % count,
        Some(i) => (i + count - 1) % count,
    })
}

#[uniffi::export]
impl AuroraCore {
    /// The channel a typed number reaches (`channels.byNumber`): the viewer's own
    /// number first, then the provider's, among channels not hidden; on a tie, the one
    /// imported first, so the same digits always reach the same channel.
    pub fn channel_by_number(&self, number: u32) -> Result<Option<Channel>> {
        let db = self.db.lock();
        let Some(id) = channels::find_by_number(&db, number)? else {
            return Ok(None);
        };
        // `get`, as Windows does: a radio station or a channel the library filters keep
        // off the lists is still reached by its number.
        Ok(channels::get(&db, id)?.map(Channel::from))
    }

    /// One channel by its id (`channels.get`): the player names the channel it opened
    /// on, number and all, so "last channel" can say where it goes back to.
    pub fn channel(&self, id: i64) -> Result<Option<Channel>> {
        let db = self.db.lock();
        Ok(channels::get(&db, id)?.map(Channel::from))
    }

    /// The channel after (or before) `from` in the order Windows zaps through: every
    /// channel the lists show, with the library filters applied, wrapping at the ends.
    /// `from` that isn't in that list (or none) starts from outside it.
    pub fn channel_step(&self, from: Option<i64>, forward: bool) -> Result<Option<Channel>> {
        let db = self.db.lock();
        let filter = ChannelFilter {
            library: LibraryFilter::load(&db)?,
            ..Default::default()
        };
        let rows = channels::list(&db, &filter)?;
        let current = from.and_then(|id| rows.iter().position(|r| r.id == id));
        Ok(step_index(rows.len(), current, forward)
            .and_then(|at| rows.into_iter().nth(at))
            .map(Channel::from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::model::PlaylistEntry;

    fn seeded(
        numbers: &[Option<u32>],
    ) -> (std::sync::Arc<AuroraCore>, tempfile::TempDir, Vec<i64>) {
        let (core, dir) = crate::testing::core();
        {
            let mut db = core.db.lock();
            db.execute(
                "INSERT INTO providers (id,name,kind,base_url,created_at)
                 VALUES (1,'P','m3u','https://example.com',0)",
                [],
            )
            .unwrap();
            let entries: Vec<PlaylistEntry> = numbers
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    let mut e = PlaylistEntry::new(
                        &format!("Channel {i}"),
                        &format!("https://example.com/{i}.ts"),
                    );
                    e.number = *n;
                    e
                })
                .collect();
            let batch: Vec<(String, &PlaylistEntry)> = entries
                .iter()
                .enumerate()
                .map(|(i, e)| (format!("k{i}"), e))
                .collect();
            channels::upsert_batch(&mut db, 1, &batch, 0).unwrap();
        }
        // In the listed order, which is what up and down walk.
        let ids = channels::list(&core.db.lock(), &ChannelFilter::default())
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        (core, dir, ids)
    }

    #[test]
    fn steps_wrap_and_start_from_outside_the_list() {
        assert_eq!(step_index(0, None, true), None);
        assert_eq!(step_index(3, None, true), Some(0));
        assert_eq!(step_index(3, None, false), Some(2));
        assert_eq!(step_index(3, Some(2), true), Some(0));
        assert_eq!(step_index(3, Some(0), false), Some(2));
        assert_eq!(step_index(3, Some(1), true), Some(2));
    }

    #[test]
    fn the_digit_buffer_keeps_the_first_four() {
        assert_eq!(push_digit("".into(), "1".into()), "1");
        assert_eq!(push_digit("123".into(), "4".into()), "1234");
        assert_eq!(push_digit("1234".into(), "5".into()), "1234");
    }

    #[test]
    fn up_and_down_go_through_the_listed_order_and_wrap() {
        let (core, _dir, ids) = seeded(&[Some(1), Some(2), Some(3)]);
        assert_eq!(ids.len(), 3);
        let next = core.channel_step(Some(ids[0]), true).unwrap().unwrap();
        assert_eq!(next.id, ids[1]);
        let wrapped = core.channel_step(Some(ids[2]), true).unwrap().unwrap();
        assert_eq!(wrapped.id, ids[0]);
        let back = core.channel_step(Some(ids[0]), false).unwrap().unwrap();
        assert_eq!(back.id, ids[2]);
        // From nothing: up is the first, down the last.
        assert_eq!(core.channel_step(None, true).unwrap().unwrap().id, ids[0]);
        assert_eq!(core.channel_step(None, false).unwrap().unwrap().id, ids[2]);
    }

    #[test]
    fn a_number_reaches_its_channel_and_a_missing_one_reaches_nothing() {
        let (core, _dir, _ids) = seeded(&[Some(101), Some(205)]);
        let hit = core.channel_by_number(205).unwrap().unwrap();
        assert_eq!(hit.number, Some(205));
        assert_eq!(core.channel(hit.id).unwrap().unwrap().number, Some(205));
        assert!(core.channel(-1).unwrap().is_none());
        assert!(core.channel_by_number(999).unwrap().is_none());
    }

    #[test]
    fn an_empty_library_steps_nowhere() {
        let (core, _dir) = crate::testing::core();
        assert!(core.channel_step(None, true).unwrap().is_none());
        assert!(core.channel_by_number(1).unwrap().is_none());
    }
}
