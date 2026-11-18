//! Parental controls as they apply (README §11): who is watching on this device, what
//! that profile may see and play, and the per-channel and per-category locks.
//!
//! The settings themselves -- the master PIN, "hide adult categories", each profile's age
//! limit -- are kept by `profiles`. This module is where they take effect, through the
//! `parental_gate` view (schema migration 19): [`where_sql`] puts it into every list query
//! by way of `LibraryFilter::where_sql`, and [`check`] asks it before anything plays. One
//! view, so a list and the player cannot disagree about what is allowed.
//!
//! Two different refusals, because they need different answers on screen:
//!
//! - **Not allowed** -- over the profile's age limit, or an adult category on a profile
//!   that hides them. Hidden from lists, refused at playback, and the PIN does not open
//!   it: the answer is to switch to a profile that may watch it.
//! - **Locked** -- a channel or category the parent locked. Shown with a padlock and
//!   asking for the parental PIN, or hidden entirely, as the household chooses
//!   (README §11: "hide locked content entirely" vs. "show but require PIN"). The PIN
//!   opens locked content for [`UNLOCK_SECS`], until somebody picks a profile again, or
//!   until the app is closed.

use aurora_core::parental::{self, AgeLimit};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{DbError, Result};
use crate::repo::filtering::Kind;
use crate::repo::settings;

/// How long the parental PIN keeps locked content and the parental settings open.
///
/// Long enough to zap between two locked channels or change several settings without
/// typing the PIN at each step, short enough that a parent who walks away does not leave
/// the evening unlocked. Choosing a profile closes it straight away.
pub const UNLOCK_SECS: i64 = 30 * 60;

/// "Hide locked content entirely" rather than showing it with a padlock. Off by default:
/// a locked channel that has vanished looks like a channel the provider dropped.
pub const KEY_HIDE_LOCKED: &str = "parental.hide_locked";

/// No age limit, in the gate's encoding.
const NO_LIMIT: u8 = 255;
const UNRATED_PASSES: i64 = 1 << 8;
const HIDE_ADULT: i64 = 1 << 9;

/// A profile's stored limit as the gate reads it: [`NO_LIMIT`] when none was set, the
/// strictest limit (0) when one was set and cannot be read. The same reading as
/// `profiles::stored_age_limit`, without its warning, because the view asks this on every
/// list query.
pub(crate) fn age_of(max_rating: Option<&str>) -> u8 {
    let Some(raw) = max_rating.map(str::trim).filter(|s| !s.is_empty()) else {
        return NO_LIMIT;
    };
    parental::min_age(raw).unwrap_or(0)
}

/// Whether the gate's `rule` lets this through. `rated` is false for a channel, which has
/// no certification to judge: a channel is held back by its category and its locks, not
/// by an age limit it cannot be measured against.
pub(crate) fn allows(
    rule: i64,
    certification: Option<&str>,
    group: Option<&str>,
    rated: bool,
) -> bool {
    if rule < 0 {
        return true;
    }
    if rule & HIDE_ADULT != 0 && parental::looks_adult(group) {
        return false;
    }
    let age = (rule & 0xFF) as u8;
    if !rated || age == NO_LIMIT {
        return true;
    }
    parental::is_allowed(
        certification,
        Some(AgeLimit(age)),
        rule & UNRATED_PASSES != 0,
    )
}

/// Whether `rule` holds back every title with no rating: an age limit that does not let
/// unrated titles through.
fn unrated_hidden(rule: i64) -> bool {
    rule >= 0 && (rule & 0xFF) as u8 != NO_LIMIT && rule & UNRATED_PASSES == 0
}

/// The category a row of `kind` is filed under, as SQL over the unaliased table. A
/// channel's is the viewer's own regrouping if there is one: a lock is on what the
/// viewer sees.
fn group_sql(kind: Kind, alias: &str) -> String {
    match kind {
        Kind::Live => format!("COALESCE({alias}.custom_group, {alias}.group_title)"),
        Kind::Movies | Kind::Series => format!("{alias}.group_title"),
    }
}

/// Whether a lock in `parental_locks pl` covers the row `alias` of `kind`.
fn lock_match_sql(kind: Kind, alias: &str) -> String {
    let category = format!(
        "(pl.kind = 'category' AND pl.value = {})",
        group_sql(kind, alias)
    );
    match kind {
        Kind::Live => {
            format!("((pl.kind = 'channel' AND pl.value = CAST({alias}.id AS TEXT)) OR {category})")
        }
        Kind::Movies | Kind::Series => category,
    }
}

/// Conditions to `AND` onto a list query over `kind`'s table, by its own name.
///
/// Each `(SELECT … FROM parental_gate)` is uncorrelated, so SQLite works it out once per
/// statement, and when nothing is restricted the `OR` stops before any per-row work.
pub(crate) fn where_sql(kind: Kind) -> String {
    where_sql_as(kind, kind.table())
}

/// [`where_sql`] for a query that names the table `t` -- the playlist editor's.
pub(crate) fn where_sql_as(kind: Kind, t: &str) -> String {
    let (certification, rated) = match kind {
        Kind::Live => ("NULL".to_string(), 0),
        Kind::Movies | Kind::Series => (format!("{t}.certification"), 1),
    };
    format!(
        " AND ((SELECT rule FROM parental_gate) < 0
               OR parental_allows((SELECT rule FROM parental_gate), {certification}, {group}, {rated}))
          AND ((SELECT hide_locked AND NOT unlocked FROM parental_gate) = 0
               OR NOT EXISTS (SELECT 1 FROM parental_locks pl WHERE {locks}))",
        group = group_sql(kind, t),
        locks = lock_match_sql(kind, t),
    )
}

/// The gate as it stands, for a cache key: anything that changes what a list shows
/// changes this string.
pub fn fingerprint(conn: &Connection) -> Result<String> {
    Ok(conn.query_row(
        "SELECT g.rule || ':' || g.hide_locked || ':' || g.unlocked || ':' ||
                COALESCE((SELECT group_concat(kind || '=' || value, '|')
                            FROM (SELECT kind, value FROM parental_locks ORDER BY kind, value)), '')
           FROM parental_gate g",
        [],
        |r| r.get(0),
    )?)
}

/// What playback is about to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Channel(i64),
    Movie(i64),
    /// An episode, judged by its show.
    Episode(i64),
    /// A recording, judged by the channel it was recorded from.
    Recording(i64),
}

/// The answer for one thing about to play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    /// Over this profile's limit. The PIN does not open it.
    NotAllowed,
    /// Locked by a parent, and the PIN has not been entered.
    Locked,
}

/// Whether the profile watching may play `target` now.
///
/// Something that is not in the library at all is `Allowed`: the resolver is the one that
/// can say it has gone, in words that help.
pub fn check(conn: &Connection, target: Target) -> Result<Verdict> {
    let (kind, table, id) = match target {
        Target::Channel(id) => (Kind::Live, "channels", id),
        Target::Recording(id) => {
            let channel: Option<i64> = conn
                .query_row(
                    "SELECT channel_id FROM recordings WHERE id = ?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?;
            match channel {
                Some(channel) => (Kind::Live, "channels", channel),
                None => return Ok(Verdict::Allowed),
            }
        }
        Target::Movie(id) => (Kind::Movies, "movies", id),
        Target::Episode(id) => {
            let series: Option<i64> = conn
                .query_row("SELECT series_id FROM episodes WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            match series {
                Some(series) => (Kind::Series, "series", series),
                None => return Ok(Verdict::Allowed),
            }
        }
    };
    let (certification, rated) = match kind {
        Kind::Live => ("NULL".to_string(), 0),
        Kind::Movies | Kind::Series => ("x.certification".to_string(), 1),
    };
    let row: Option<(bool, bool, bool)> = conn
        .query_row(
            &format!(
                "SELECT parental_allows(g.rule, {certification}, {group}, {rated}),
                        EXISTS (SELECT 1 FROM parental_locks pl WHERE {locks}),
                        g.unlocked
                   FROM {table} x, parental_gate g
                  WHERE x.id = ?1",
                group = group_sql(kind, "x"),
                locks = lock_match_sql(kind, "x"),
            ),
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(match row {
        None => Verdict::Allowed,
        Some((false, _, _)) => Verdict::NotAllowed,
        Some((true, true, false)) => Verdict::Locked,
        Some(_) => Verdict::Allowed,
    })
}

/* ── Who is watching ─────────────────────────────────────────────────────── */

/// Choose who is watching. Closes anything the PIN had opened: the next person to sit
/// down has not typed it.
pub fn select_profile(conn: &Connection, profile_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE session SET profile_id = ?1, unlocked_until = 0 WHERE id = 1",
        [profile_id],
    )?;
    Ok(())
}

/// Nobody chosen, nothing unlocked: how every launch starts.
pub fn clear_session(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE session SET profile_id = NULL, unlocked_until = 0 WHERE id = 1",
        [],
    )?;
    Ok(())
}

/// The parental PIN was just entered: open locked content and the parental settings.
/// Returns until when.
pub fn unlock(conn: &Connection, now: i64) -> Result<i64> {
    let until = now + UNLOCK_SECS;
    conn.execute(
        "UPDATE session SET unlocked_until = ?1 WHERE id = 1",
        [until],
    )?;
    Ok(until)
}

/// Close what the PIN opened, before it would expire by itself.
pub fn relock(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE session SET unlocked_until = 0 WHERE id = 1", [])?;
    Ok(())
}

/// Whether the parental PIN has opened things, as of `now`.
pub fn is_unlocked(conn: &Connection, now: i64) -> Result<bool> {
    Ok(unlocked_until(conn)? > now)
}

fn unlocked_until(conn: &Connection) -> Result<i64> {
    Ok(conn
        .query_row("SELECT unlocked_until FROM session WHERE id = 1", [], |r| {
            r.get(0)
        })
        .optional()?
        .unwrap_or(0))
}

/// The profile chosen in "Who's watching?" on this device, if any.
pub fn active_profile(conn: &Connection) -> Result<Option<i64>> {
    Ok(conn
        .query_row("SELECT profile_id FROM session WHERE id = 1", [], |r| {
            r.get(0)
        })
        .optional()?
        .flatten())
}

/// What the screens need to know about the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentalState {
    /// The profile chosen on this device, if any.
    pub profile_id: Option<i64>,
    /// Until when the PIN has opened locked content and the parental settings; `None`
    /// when it has not.
    pub unlocked_until: Option<i64>,
    /// Locked content is hidden rather than shown with a padlock.
    pub hide_locked: bool,
    /// Whether anything at all is held back for the profile watching.
    pub restricted: bool,
    /// The profile watching has an age limit and does not allow titles with no rating,
    /// so every film and series without one is held back.
    pub unrated_hidden: bool,
    /// Films and series in the library that carry a rating, and how many there are in
    /// all. Panels rarely send ratings; they come from fetching metadata. Until they are
    /// fetched, a profile that blocks unrated titles sees an empty film library, and the
    /// screens need these to say why.
    pub rated_titles: i64,
    pub titles: i64,
}

pub fn state(conn: &Connection, now: i64) -> Result<ParentalState> {
    let until = unlocked_until(conn)?;
    let rule: i64 = conn.query_row("SELECT rule FROM parental_gate", [], |r| r.get(0))?;
    let any_lock: bool =
        conn.query_row("SELECT EXISTS (SELECT 1 FROM parental_locks)", [], |r| {
            r.get(0)
        })?;
    let (rated_titles, titles): (i64, i64) = conn.query_row(
        "SELECT
             (SELECT count(*) FROM movies WHERE hidden = 0 AND trim(COALESCE(certification, '')) != '')
           + (SELECT count(*) FROM series WHERE hidden = 0 AND trim(COALESCE(certification, '')) != ''),
             (SELECT count(*) FROM movies WHERE hidden = 0)
           + (SELECT count(*) FROM series WHERE hidden = 0)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(ParentalState {
        profile_id: active_profile(conn)?,
        unlocked_until: (until > now).then_some(until),
        hide_locked: hide_locked(conn)?,
        restricted: rule >= 0 || any_lock,
        unrated_hidden: unrated_hidden(rule),
        rated_titles,
        titles,
    })
}

/* ── Locks ───────────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LockKind {
    Channel,
    Category,
}

impl LockKind {
    fn as_str(self) -> &'static str {
        match self {
            LockKind::Channel => "channel",
            LockKind::Category => "category",
        }
    }
}

/// One lock, as the settings screen lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lock {
    pub kind: LockKind,
    /// A channel's id, or a category's name.
    pub value: String,
    /// What to call it: the channel's name as the viewer sees it, or the category.
    pub label: String,
}

pub fn locks(conn: &Connection) -> Result<Vec<Lock>> {
    let mut stmt = conn.prepare(
        "SELECT l.kind, l.value,
                CASE l.kind
                  WHEN 'channel' THEN COALESCE(
                      (SELECT COALESCE(c.custom_name, c.name) FROM channels c
                        WHERE CAST(c.id AS TEXT) = l.value),
                      'A channel no longer in the playlist')
                  ELSE l.value END
           FROM parental_locks l
          ORDER BY l.kind DESC, 3 COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        let kind: String = r.get(0)?;
        Ok(Lock {
            kind: if kind == "channel" {
                LockKind::Channel
            } else {
                LockKind::Category
            },
            value: r.get(1)?,
            label: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Lock or unlock one channel or category. A channel is named by its id, and must exist:
/// a lock on an id that names nothing would sit in the list as an unexplained row.
pub fn set_lock(conn: &Connection, kind: LockKind, value: &str, locked: bool) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        return Err(DbError::Rejected("Nothing was named to lock.".into()));
    }
    if locked && kind == LockKind::Channel {
        let exists: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM channels WHERE CAST(id AS TEXT) = ?1)",
            [value],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(DbError::Rejected(
                "That channel is not in the playlist.".into(),
            ));
        }
    }
    if locked {
        conn.execute(
            "INSERT OR IGNORE INTO parental_locks (kind, value) VALUES (?1, ?2)",
            params![kind.as_str(), value],
        )?;
    } else {
        conn.execute(
            "DELETE FROM parental_locks WHERE kind = ?1 AND value = ?2",
            params![kind.as_str(), value],
        )?;
    }
    Ok(())
}

pub fn hide_locked(conn: &Connection) -> Result<bool> {
    settings::get_or(conn, KEY_HIDE_LOCKED, false)
}

pub fn set_hide_locked(conn: &Connection, hide: bool) -> Result<()> {
    settings::set(conn, KEY_HIDE_LOCKED, &hide)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::filtering::LibraryFilter;
    use crate::repo::profiles::{self, NewProfile};

    const NOW: i64 = 1_800_000_000;

    /// A library with something at every rating, an adult category, and two channels.
    fn library() -> Connection {
        let conn = crate::open_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
                  VALUES (1,'Line','xtream','http://panel.example',0);
             INSERT INTO movies (id,provider_id,provider_key,title,match_key,url,certification,group_title,last_seen_at)
                  VALUES (1,1,'1','Toy Story','toystory','http://x/1','U','Family',0),
                         (2,1,'2','Heat','heat','http://x/2','15','Action',0),
                         (3,1,'3','Untitled','untitled','http://x/3',NULL,'Action',0),
                         (4,1,'4','Night','night','http://x/4','U','XXX Adults',0);
             INSERT INTO series (id,provider_id,provider_key,title,match_key,certification,group_title,last_seen_at)
                  VALUES (1,1,'1','Bluey','bluey','G','Kids',0),
                         (2,1,'2','The Wire','thewire','TV-MA','Drama',0);
             INSERT INTO episodes (id,series_id,season,episode,title,url)
                  VALUES (1,1,1,1,'Magic Xylophone','http://x/e1'),
                         (2,2,1,1,'The Target','http://x/e2');
             INSERT INTO channels (id,provider_id,provider_key,name,match_key,group_title,last_seen_at)
                  VALUES (10,1,'10','BBC One','bbcone','UK',0),
                         (11,1,'11','Hot TV','hottv','Adult 18+',0),
                         (12,1,'12','Cartoons','cartoons','Kids',0);",
        )
        .unwrap();
        // Locks apply only while there is a PIN to open them with.
        profiles::set_master_pin(&conn, Some("2468"), 0).unwrap();
        conn
    }

    fn ids(conn: &Connection, kind: Kind) -> Vec<i64> {
        let t = kind.table();
        let filter = LibraryFilter::default().where_sql(kind);
        let mut stmt = conn
            .prepare(&format!(
                "SELECT id FROM {t} WHERE hidden = 0{filter} ORDER BY id"
            ))
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<i64>, _>>()
            .unwrap()
    }

    fn kid(conn: &Connection) -> i64 {
        profiles::create(
            conn,
            &NewProfile {
                name: "Sam".into(),
                is_kids: true,
                max_age: Some(12),
                ..Default::default()
            },
            0,
        )
        .unwrap()
    }

    #[test]
    fn one_profile_sees_everything_but_the_adult_categories_hidden_by_default() {
        let conn = library();
        assert_eq!(
            ids(&conn, Kind::Movies),
            [1, 2, 3],
            "README §11: adult hidden on a fresh install"
        );
        assert_eq!(ids(&conn, Kind::Live), [10, 12]);
        assert_eq!(
            check(&conn, Target::Channel(11)).unwrap(),
            Verdict::NotAllowed
        );

        profiles::set_parental_flags(&conn, false, false, 0).unwrap();
        assert_eq!(ids(&conn, Kind::Movies), [1, 2, 3, 4]);
        assert_eq!(ids(&conn, Kind::Live), [10, 11, 12]);
        assert_eq!(check(&conn, Target::Channel(11)).unwrap(), Verdict::Allowed);
        assert!(!state(&conn, NOW).unwrap().restricted);
    }

    #[test]
    fn a_kids_profile_sees_only_what_its_age_limit_allows_and_plays_nothing_else() {
        let conn = library();
        profiles::set_parental_flags(&conn, false, false, 0).unwrap();
        let sam = kid(&conn);
        select_profile(&conn, sam).unwrap();

        // Unrated is out too: a kids profile defaults to blocking it, and the state says
        // so with how much of the library carries a rating, for the screens to explain
        // an empty film list.
        assert_eq!(ids(&conn, Kind::Movies), [1]);
        let seen = state(&conn, NOW).unwrap();
        assert!(seen.unrated_hidden);
        assert_eq!((seen.rated_titles, seen.titles), (5, 6));
        assert_eq!(ids(&conn, Kind::Series), [1]);
        // An age limit hides adult categories whatever the household setting says.
        assert_eq!(ids(&conn, Kind::Live), [10, 12]);

        assert_eq!(check(&conn, Target::Movie(1)).unwrap(), Verdict::Allowed);
        assert_eq!(check(&conn, Target::Movie(2)).unwrap(), Verdict::NotAllowed);
        assert_eq!(check(&conn, Target::Movie(3)).unwrap(), Verdict::NotAllowed);
        assert_eq!(check(&conn, Target::Episode(1)).unwrap(), Verdict::Allowed);
        assert_eq!(
            check(&conn, Target::Episode(2)).unwrap(),
            Verdict::NotAllowed
        );
        assert_eq!(
            check(&conn, Target::Channel(11)).unwrap(),
            Verdict::NotAllowed
        );

        // The PIN opens locks, not limits.
        unlock(&conn, NOW).unwrap();
        assert_eq!(check(&conn, Target::Movie(2)).unwrap(), Verdict::NotAllowed);

        // Back to the owner: everything again.
        select_profile(&conn, 1).unwrap();
        assert_eq!(ids(&conn, Kind::Movies), [1, 2, 3, 4]);
        assert!(!state(&conn, NOW).unwrap().unrated_hidden);
        assert_eq!(check(&conn, Target::Movie(2)).unwrap(), Verdict::Allowed);
    }

    #[test]
    fn until_somebody_is_chosen_the_strictest_profile_applies() {
        let conn = library();
        profiles::set_parental_flags(&conn, false, false, 0).unwrap();
        kid(&conn);
        clear_session(&conn).unwrap();
        assert_eq!(ids(&conn, Kind::Movies), [1]);
        assert_eq!(check(&conn, Target::Movie(2)).unwrap(), Verdict::NotAllowed);

        // A chosen profile that has since been deleted is nobody chosen.
        select_profile(&conn, 99).unwrap();
        assert_eq!(ids(&conn, Kind::Movies), [1]);
    }

    #[test]
    fn a_locked_channel_asks_for_the_pin_and_the_pin_opens_it_for_a_while() {
        let conn = library();
        set_lock(&conn, LockKind::Channel, "10", true).unwrap();

        // Shown with a padlock by default, refused until the PIN.
        assert_eq!(ids(&conn, Kind::Live), [10, 12]);
        assert_eq!(check(&conn, Target::Channel(10)).unwrap(), Verdict::Locked);
        unlock(&conn, NOW).unwrap();
        assert!(is_unlocked(&conn, NOW + UNLOCK_SECS - 1).unwrap());
        assert!(!is_unlocked(&conn, NOW + UNLOCK_SECS).unwrap());
        relock(&conn).unwrap();

        // Hidden entirely, when the household asks for that, until the PIN.
        set_hide_locked(&conn, true).unwrap();
        assert_eq!(ids(&conn, Kind::Live), [12]);
        let real_now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        unlock(&conn, real_now).unwrap();
        assert_eq!(ids(&conn, Kind::Live), [10, 12]);
        assert_eq!(check(&conn, Target::Channel(10)).unwrap(), Verdict::Allowed);

        // Choosing a profile closes it again.
        select_profile(&conn, 1).unwrap();
        assert_eq!(check(&conn, Target::Channel(10)).unwrap(), Verdict::Locked);
    }

    #[test]
    fn a_locked_category_covers_its_films_its_shows_and_its_channels() {
        let conn = library();
        set_hide_locked(&conn, true).unwrap();
        set_lock(&conn, LockKind::Category, "Kids", true).unwrap();
        set_lock(&conn, LockKind::Category, "Action", true).unwrap();
        assert_eq!(ids(&conn, Kind::Movies), [1]);
        assert_eq!(ids(&conn, Kind::Series), [2]);
        assert_eq!(ids(&conn, Kind::Live), [10]);
        assert_eq!(check(&conn, Target::Episode(1)).unwrap(), Verdict::Locked);

        // The viewer's own regrouping is what a lock sees.
        conn.execute(
            "UPDATE channels SET custom_group = 'Kids' WHERE id = 10",
            [],
        )
        .unwrap();
        assert_eq!(ids(&conn, Kind::Live), Vec::<i64>::new());
    }

    #[test]
    fn without_a_parental_pin_nothing_stays_locked() {
        let conn = library();
        set_lock(&conn, LockKind::Channel, "10", true).unwrap();
        set_hide_locked(&conn, true).unwrap();
        assert_eq!(check(&conn, Target::Channel(10)).unwrap(), Verdict::Locked);
        profiles::set_master_pin(&conn, None, 0).unwrap();
        assert_eq!(check(&conn, Target::Channel(10)).unwrap(), Verdict::Allowed);
        assert_eq!(ids(&conn, Kind::Live), [10, 12]);
    }

    #[test]
    fn a_recording_is_judged_by_its_channel() {
        let conn = library();
        set_lock(&conn, LockKind::Channel, "10", true).unwrap();
        conn.execute_batch(
            "INSERT INTO recordings (id, channel_id, title, air_start, air_stop, start, stop, state, created_at)
                  VALUES (1, 10, 'News', 0, 60, 0, 60, 'completed', 0);",
        )
        .unwrap();
        assert_eq!(check(&conn, Target::Recording(1)).unwrap(), Verdict::Locked);
        assert_eq!(
            check(&conn, Target::Recording(2)).unwrap(),
            Verdict::Allowed
        );
    }

    #[test]
    fn locks_list_by_name_and_a_channel_must_exist_to_be_locked() {
        let conn = library();
        set_lock(&conn, LockKind::Channel, "12", true).unwrap();
        set_lock(&conn, LockKind::Category, "UK", true).unwrap();
        assert!(set_lock(&conn, LockKind::Channel, "999", true).is_err());
        let all = locks(&conn).unwrap();
        assert_eq!(
            all.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(),
            ["Cartoons", "UK"],
            "channels first, then categories"
        );
        set_lock(&conn, LockKind::Channel, "12", false).unwrap();
        assert_eq!(locks(&conn).unwrap().len(), 1);
    }

    #[test]
    fn the_fingerprint_moves_with_anything_that_changes_a_list() {
        let conn = library();
        let a = fingerprint(&conn).unwrap();
        set_lock(&conn, LockKind::Category, "UK", true).unwrap();
        let b = fingerprint(&conn).unwrap();
        assert_ne!(a, b);
        kid(&conn);
        assert_ne!(
            b,
            fingerprint(&conn).unwrap(),
            "a stricter profile appeared"
        );
    }

    /// What the gate costs on a library the size of the one this was built against
    /// (117,510 films): `cargo test -p aurora-db --release gate_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement, not a check"]
    fn gate_cost() {
        let conn = crate::open_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
                  VALUES (1,'Line','xtream','http://panel.example',0);
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 117510)
             INSERT INTO movies (id,provider_id,provider_key,title,match_key,url,certification,group_title,last_seen_at)
             SELECT i, 1, i, 'Film ' || i, 'film' || i, 'http://x/' || i,
                    CASE i % 5 WHEN 0 THEN 'U' WHEN 1 THEN 'PG' WHEN 2 THEN '15' WHEN 3 THEN 'R' ELSE NULL END,
                    CASE i % 50 WHEN 0 THEN 'XXX Adults' ELSE 'Group ' || (i % 40) END, 0
               FROM n;
             ANALYZE;",
        )
        .unwrap();
        let filter = LibraryFilter {
            english_only: false,
            hide_duplicates: true,
            hide_untagged: false,
        };
        let time = |label: &str| {
            let sql = format!(
                "SELECT count(*) FROM movies WHERE hidden = 0{}",
                filter.where_sql(Kind::Movies)
            );
            let start = std::time::Instant::now();
            let n: i64 = conn.query_row(&sql, [], |r| r.get(0)).unwrap();
            println!(
                "{label:<40} {n:>7} films  {:>7.1} ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
        };
        profiles::set_parental_flags(&conn, false, false, 0).unwrap();
        time("nothing restricted");
        profiles::set_parental_flags(&conn, true, false, 0).unwrap();
        time("adult categories hidden (the default)");
        let sam = kid(&conn);
        select_profile(&conn, sam).unwrap();
        time("a kids profile, limit 12");
        set_hide_locked(&conn, true).unwrap();
        for g in 0..10 {
            set_lock(&conn, LockKind::Category, &format!("Group {g}"), true).unwrap();
        }
        time("... and ten categories locked and hidden");
    }

    #[test]
    fn an_unreadable_age_limit_is_the_strictest_and_none_is_no_limit() {
        assert_eq!(age_of(None), NO_LIMIT);
        assert_eq!(age_of(Some("  ")), NO_LIMIT);
        assert_eq!(age_of(Some("12")), 12);
        assert_eq!(age_of(Some("what")), 0);
    }
}
