//! What the guide shows: which groups, and which kinds of channel.
//!
//! Windows' `src-ui/src/features/guide/guideFilters.ts`, rule for rule: any number of
//! groups shown only or hidden, on top of "with listings", favourites, catch-up, radio
//! and quality. Windows fetches the whole channel list and narrows it in the page; here
//! the narrowing happens in the core and the guide is handed a page of what is left, so
//! a filter that empties a whole page of 200 never leaves the guide waiting on a row
//! that will not appear to ask for the next one.
//!
//! The choice is kept on the device, as Windows keeps it in `localStorage`: how somebody
//! likes to browse, not part of the library. Swift stores the text [`guide_filters_write`]
//! gives, and [`guide_filters_read`] checks it field by field on the way back, because
//! a store is editable text (Windows' `readFilters`).

use aurora_db::repo::channels::{self, ChannelFilter, ChannelRow};
use aurora_db::repo::filtering::LibraryFilter;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;
use crate::library::Channel;
use crate::AuroraCore;

/// Whether the chosen groups are the only ones shown, or the ones left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum GuideGroupMode {
    #[default]
    Only,
    Hide,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct GuideFilters {
    /// The groups chosen. Empty means none chosen: every group is shown.
    pub groups: Vec<String>,
    pub group_mode: GuideGroupMode,
    /// Only channels the guide has a schedule for.
    pub with_listings: bool,
    pub favorites_only: bool,
    pub catchup_only: bool,
    pub hide_radio: bool,
    /// Empty means any quality.
    pub qualities: Vec<String>,
}

/// The qualities a channel can be tagged with, best first: Windows' `QUALITIES`.
pub const QUALITIES: [&str; 4] = ["4K", "FHD", "HD", "SD"];

/// A page of the guide's channels, and how many the filters leave in all.
#[derive(Debug, Clone, uniffi::Record)]
pub struct GuideChannels {
    pub channels: Vec<Channel>,
    pub shown: u32,
}

/// The one group to ask the database for, when the filter comes down to exactly that:
/// Windows' `hostGroup`. Anything else is read whole and narrowed by [`passes`].
pub fn host_group(f: &GuideFilters) -> Option<String> {
    match (f.group_mode, f.groups.as_slice()) {
        (GuideGroupMode::Only, [one]) => Some(one.clone()),
        _ => None,
    }
}

/// Whether one channel gets through: Windows' `applyFilters`, for one row.
pub fn passes(c: &ChannelRow, f: &GuideFilters) -> bool {
    if !f.groups.is_empty() {
        let group = c.group.as_deref().unwrap_or("");
        let in_chosen = f.groups.iter().any(|g| g == group);
        let shown = match f.group_mode {
            GuideGroupMode::Only => in_chosen,
            GuideGroupMode::Hide => !in_chosen,
        };
        if !shown {
            return false;
        }
    }
    if f.with_listings && c.epg_channel_id.is_none() {
        return false;
    }
    if f.favorites_only && !c.favorite {
        return false;
    }
    if f.catchup_only && !c.has_catchup {
        return false;
    }
    if f.hide_radio && c.is_radio {
        return false;
    }
    if !f.qualities.is_empty()
        && !c
            .quality
            .as_deref()
            .is_some_and(|q| f.qualities.iter().any(|x| x == q))
    {
        return false;
    }
    true
}

/// The channels the filters let through, in the order given.
pub fn apply(rows: Vec<ChannelRow>, f: &GuideFilters) -> Vec<ChannelRow> {
    rows.into_iter().filter(|c| passes(c, f)).collect()
}

/// What was chosen last time, from the text [`guide_filters_write`] gave. Anything
/// unreadable is no filters at all, and each field is taken only if it is the right
/// kind: Windows' `readFilters`.
#[uniffi::export]
pub fn guide_filters_read(text: String) -> GuideFilters {
    let Ok(Value::Object(v)) = serde_json::from_str::<Value>(&text) else {
        return GuideFilters::default();
    };
    let flag = |key: &str| v.get(key) == Some(&Value::Bool(true));
    let strings = |key: &str| -> Vec<String> {
        v.get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    GuideFilters {
        groups: strings("groups"),
        group_mode: if v.get("groupMode").and_then(Value::as_str) == Some("hide") {
            GuideGroupMode::Hide
        } else {
            GuideGroupMode::Only
        },
        with_listings: flag("withListings"),
        favorites_only: flag("favoritesOnly"),
        catchup_only: flag("catchupOnly"),
        hide_radio: flag("hideRadio"),
        qualities: strings("qualities")
            .into_iter()
            .filter(|q| QUALITIES.contains(&q.as_str()))
            .collect(),
    }
}

/// The text to keep, in Windows' own shape.
#[uniffi::export]
pub fn guide_filters_write(filters: GuideFilters) -> String {
    serde_json::to_string(&filters).unwrap_or_default()
}

/// The qualities to offer, best first.
#[uniffi::export]
pub fn guide_qualities() -> Vec<String> {
    QUALITIES.iter().map(|q| q.to_string()).collect()
}

#[uniffi::export]
impl AuroraCore {
    /// A page of the channels the guide's filters leave, for this profile (whose
    /// favourites "Favourites only" means), and the count of all of them.
    ///
    /// The whole list is read each time, as Windows reads it: 22,000 rows is a few
    /// milliseconds of SQLite, and a count that disagreed with the rows would be worse.
    pub fn guide_channels(
        &self,
        profile_id: i64,
        filters: GuideFilters,
        limit: u32,
        offset: u32,
    ) -> Result<GuideChannels> {
        let db = self.db.lock();
        let rows = channels::list(
            &db,
            &ChannelFilter {
                group: host_group(&filters),
                limit: None,
                offset: 0,
                library: LibraryFilter::load(&db)?,
                profile_id: Some(profile_id),
                ..Default::default()
            },
        )?;
        let left = apply(rows, &filters);
        let shown = left.len() as u32;
        Ok(GuideChannels {
            channels: left
                .into_iter()
                .skip(offset as usize)
                .take(limit as usize)
                .map(Channel::from)
                .collect(),
            shown,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(id: i64, f: impl FnOnce(&mut ChannelRow)) -> ChannelRow {
        let mut c = ChannelRow {
            id,
            name: format!("Channel {id}"),
            group: Some("News".into()),
            quality: Some("HD".into()),
            ..Default::default()
        };
        f(&mut c);
        c
    }

    /// Windows' `guideFilters.test.ts` list.
    fn list() -> Vec<ChannelRow> {
        vec![
            ch(1, |c| {
                c.group = Some("UK News".into());
                c.epg_channel_id = Some("bbc".into());
                c.quality = Some("FHD".into());
                c.favorite = true;
            }),
            ch(2, |c| {
                c.group = Some("UK Sport".into());
                c.has_catchup = true;
                c.quality = Some("4K".into());
            }),
            ch(3, |c| {
                c.group = Some("US News".into());
                c.is_radio = true;
                c.quality = Some("SD".into());
            }),
            ch(4, |c| c.group = Some("Adult".into())),
            ch(5, |c| c.group = None),
        ]
    }

    fn ids(f: &GuideFilters) -> Vec<i64> {
        apply(list(), f).into_iter().map(|c| c.id).collect()
    }

    fn none() -> GuideFilters {
        GuideFilters::default()
    }

    fn groups(g: &[&str]) -> Vec<String> {
        g.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn lets_everything_through_with_nothing_chosen() {
        assert_eq!(ids(&none()), [1, 2, 3, 4, 5]);
    }

    #[test]
    fn shows_only_several_chosen_groups_at_once() {
        let f = GuideFilters {
            groups: groups(&["UK News", "UK Sport"]),
            ..none()
        };
        assert_eq!(ids(&f), [1, 2]);
    }

    #[test]
    fn hides_the_chosen_groups_instead_keeping_channels_with_no_group() {
        let f = GuideFilters {
            groups: groups(&["Adult", "US News"]),
            group_mode: GuideGroupMode::Hide,
            ..none()
        };
        assert_eq!(ids(&f), [1, 2, 5]);
    }

    #[test]
    fn narrows_by_what_the_channel_is_on_top_of_its_group() {
        assert_eq!(
            ids(&GuideFilters {
                with_listings: true,
                ..none()
            }),
            [1]
        );
        assert_eq!(
            ids(&GuideFilters {
                favorites_only: true,
                ..none()
            }),
            [1]
        );
        assert_eq!(
            ids(&GuideFilters {
                catchup_only: true,
                ..none()
            }),
            [2]
        );
        assert_eq!(
            ids(&GuideFilters {
                hide_radio: true,
                ..none()
            }),
            [1, 2, 4, 5]
        );
        let q = GuideFilters {
            qualities: groups(&["4K", "FHD"]),
            ..none()
        };
        assert_eq!(ids(&q), [1, 2]);
    }

    #[test]
    fn asks_the_database_for_one_group_only_when_the_filter_is_exactly_that() {
        let one = GuideFilters {
            groups: groups(&["UK News"]),
            ..none()
        };
        assert_eq!(host_group(&one).as_deref(), Some("UK News"));
        let two = GuideFilters {
            groups: groups(&["UK News", "UK Sport"]),
            ..none()
        };
        assert_eq!(host_group(&two), None);
        let hide = GuideFilters {
            group_mode: GuideGroupMode::Hide,
            ..one.clone()
        };
        assert_eq!(host_group(&hide), None);
        assert_eq!(host_group(&none()), None);
    }

    #[test]
    fn what_is_kept_reads_back_the_same() {
        let f = GuideFilters {
            groups: groups(&["UK News"]),
            group_mode: GuideGroupMode::Hide,
            with_listings: true,
            favorites_only: false,
            catchup_only: true,
            hide_radio: true,
            qualities: groups(&["4K"]),
        };
        let text = guide_filters_write(f.clone());
        assert!(text.contains("\"groupMode\":\"hide\""), "{text}");
        assert_eq!(guide_filters_read(text), f);
    }

    #[test]
    fn a_damaged_store_reads_field_by_field_or_not_at_all() {
        assert_eq!(guide_filters_read(String::new()), none());
        assert_eq!(guide_filters_read("[1,2]".into()), none());
        let odd = r#"{"groups":["A",3,null],"groupMode":"sideways","withListings":"yes",
                      "catchupOnly":true,"qualities":["8K","HD"]}"#;
        assert_eq!(
            guide_filters_read(odd.into()),
            GuideFilters {
                groups: groups(&["A"]),
                catchup_only: true,
                qualities: groups(&["HD"]),
                ..none()
            }
        );
    }

    #[test]
    fn the_guide_pages_what_the_filters_leave_and_counts_all_of_it() {
        use aurora_core::model::PlaylistEntry;
        let (core, _dir) = crate::testing::core();
        assert_eq!(core.guide_channels(1, none(), 50, 0).unwrap().shown, 0);
        {
            let mut db = core.db.lock();
            db.execute(
                "INSERT INTO providers (id,name,kind,base_url,created_at)
                 VALUES (1,'P','m3u','https://example.com',0)",
                [],
            )
            .unwrap();
            let entries: Vec<PlaylistEntry> = (0..6)
                .map(|i| {
                    let mut e = PlaylistEntry::new(
                        format!("Channel {i}"),
                        format!("https://example.com/{i}.ts"),
                    );
                    e.number = Some(i + 1);
                    e.group = Some(if i % 2 == 0 { "Even" } else { "Odd" }.into());
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
        let hide_odd = GuideFilters {
            groups: groups(&["Odd"]),
            group_mode: GuideGroupMode::Hide,
            ..none()
        };
        let first = core.guide_channels(1, hide_odd.clone(), 2, 0).unwrap();
        let rest = core.guide_channels(1, hide_odd, 2, 2).unwrap();
        assert_eq!(first.shown, 3);
        let names: Vec<_> = first
            .channels
            .iter()
            .chain(&rest.channels)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, ["Channel 0", "Channel 2", "Channel 4"]);
        let only_odd = GuideFilters {
            groups: groups(&["Odd"]),
            ..none()
        };
        assert_eq!(core.guide_channels(1, only_odd, 50, 0).unwrap().shown, 3);
    }
}
