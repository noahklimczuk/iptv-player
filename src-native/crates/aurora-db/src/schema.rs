//! Schema definition, as an ordered list of forward-only migrations.
//!
//! Never edit a shipped migration — append a new one. `migrate::run` applies whatever is
//! missing and records the version in `user_version`.

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial",
        sql: r#"
CREATE TABLE providers (
    id               INTEGER PRIMARY KEY,
    name             TEXT    NOT NULL,
    kind             TEXT    NOT NULL CHECK (kind IN ('xtream','m3u','stalker')),
    base_url         TEXT    NOT NULL,
    -- Credentials live in Windows Credential Manager (README C10); this is only the key.
    credential_ref   TEXT,
    user_agent       TEXT,
    referrer         TEXT,
    enabled          INTEGER NOT NULL DEFAULT 1,
    max_connections  INTEGER,
    expires_at       INTEGER,
    last_refresh_at  INTEGER,
    sort_order       INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL
);

CREATE TABLE channels (
    id               INTEGER PRIMARY KEY,
    provider_id      INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    provider_key     TEXT    NOT NULL,      -- stream id / URL hash, stable across refreshes
    name             TEXT    NOT NULL,
    custom_name      TEXT,
    match_key        TEXT    NOT NULL,      -- normalized, for dedupe and EPG matching
    number           INTEGER,
    custom_number    INTEGER,
    logo             TEXT,
    custom_logo      TEXT,
    group_title      TEXT,
    custom_group     TEXT,
    tvg_id           TEXT,
    epg_channel_id   TEXT,                  -- resolved EPG channel
    epg_match_method TEXT,
    shift_minutes    INTEGER NOT NULL DEFAULT 0,
    quality          TEXT,
    language         TEXT,
    country          TEXT,
    is_radio         INTEGER NOT NULL DEFAULT 0,
    hidden           INTEGER NOT NULL DEFAULT 0,
    catchup_mode     TEXT,
    catchup_source   TEXT,
    catchup_days     INTEGER NOT NULL DEFAULT 0,
    sort_order       INTEGER NOT NULL DEFAULT 0,
    last_seen_at     INTEGER NOT NULL,
    UNIQUE (provider_id, provider_key)
);
CREATE INDEX idx_channels_number     ON channels(custom_number, number);
CREATE INDEX idx_channels_group      ON channels(group_title);
CREATE INDEX idx_channels_matchkey   ON channels(match_key);
CREATE INDEX idx_channels_visible    ON channels(hidden, sort_order);

-- README §7.14: one logical channel can have several URLs for failover.
CREATE TABLE channel_sources (
    id           INTEGER PRIMARY KEY,
    channel_id   INTEGER NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    url          TEXT    NOT NULL,
    priority     INTEGER NOT NULL DEFAULT 0,
    quality      TEXT,
    last_ok_at   INTEGER,
    last_fail_at INTEGER,
    fail_count   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_sources_channel ON channel_sources(channel_id, priority);

CREATE TABLE epg_channels (
    id            TEXT PRIMARY KEY,
    display_name  TEXT,
    icon          TEXT,
    source_id     INTEGER
);

CREATE TABLE epg_programmes (
    id            INTEGER PRIMARY KEY,
    channel_id    TEXT    NOT NULL,
    start         INTEGER NOT NULL,
    stop          INTEGER NOT NULL,
    title         TEXT    NOT NULL,
    sub_title     TEXT,
    description   TEXT,
    categories    TEXT,
    season        INTEGER,
    episode       INTEGER,
    icon          TEXT,
    rating        TEXT,
    star_rating   REAL,
    flags         INTEGER NOT NULL DEFAULT 0,   -- bit 0 new, 1 live, 2 premiere
    credits       TEXT
);
-- The hottest index in the app: every guide paint is a range scan over this.
CREATE INDEX idx_prog_channel_time ON epg_programmes(channel_id, start, stop);
CREATE INDEX idx_prog_time         ON epg_programmes(start);

CREATE TABLE movies (
    id             INTEGER PRIMARY KEY,
    provider_id    INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    provider_key   TEXT    NOT NULL,
    title          TEXT    NOT NULL,
    match_key      TEXT    NOT NULL,
    year           INTEGER,
    quality        TEXT,
    group_title    TEXT,
    url            TEXT    NOT NULL,
    poster         TEXT,
    backdrop       TEXT,
    logo_art       TEXT,
    overview       TEXT,
    runtime_mins   INTEGER,
    rating         REAL,
    certification  TEXT,
    genres         TEXT,
    tmdb_id        INTEGER,
    added_at       INTEGER,
    last_seen_at   INTEGER NOT NULL,
    UNIQUE (provider_id, provider_key)
);
CREATE INDEX idx_movies_matchkey ON movies(match_key, year);
CREATE INDEX idx_movies_added    ON movies(added_at DESC);

CREATE TABLE series (
    id             INTEGER PRIMARY KEY,
    provider_id    INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    provider_key   TEXT    NOT NULL,
    title          TEXT    NOT NULL,
    match_key      TEXT    NOT NULL,
    year           INTEGER,
    poster         TEXT,
    backdrop       TEXT,
    logo_art       TEXT,
    overview       TEXT,
    rating         REAL,
    certification  TEXT,
    genres         TEXT,
    tmdb_id        INTEGER,
    added_at       INTEGER,
    last_seen_at   INTEGER NOT NULL,
    UNIQUE (provider_id, provider_key)
);
CREATE INDEX idx_series_matchkey ON series(match_key, year);

CREATE TABLE episodes (
    id            INTEGER PRIMARY KEY,
    series_id     INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
    season        INTEGER NOT NULL,
    episode       INTEGER NOT NULL,
    title         TEXT,
    overview      TEXT,
    still         TEXT,
    runtime_mins  INTEGER,
    air_date      INTEGER,
    url           TEXT    NOT NULL,
    added_at      INTEGER,
    UNIQUE (series_id, season, episode)
);
CREATE INDEX idx_episodes_series ON episodes(series_id, season, episode);

CREATE TABLE profiles (
    id            INTEGER PRIMARY KEY,
    name          TEXT    NOT NULL,
    avatar        TEXT,
    is_kids       INTEGER NOT NULL DEFAULT 0,
    pin_hash      TEXT,
    max_rating    TEXT,
    created_at    INTEGER NOT NULL
);

CREATE TABLE watch_progress (
    profile_id    INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    item_kind     TEXT    NOT NULL CHECK (item_kind IN ('movie','episode','channel','recording')),
    item_id       INTEGER NOT NULL,
    position_secs INTEGER NOT NULL DEFAULT 0,
    duration_secs INTEGER NOT NULL DEFAULT 0,
    completed     INTEGER NOT NULL DEFAULT 0,
    updated_at    INTEGER NOT NULL,
    PRIMARY KEY (profile_id, item_kind, item_id)
);
CREATE INDEX idx_progress_recent ON watch_progress(profile_id, updated_at DESC);

CREATE TABLE favorites (
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    list_name   TEXT    NOT NULL DEFAULT 'Favorites',
    item_kind   TEXT    NOT NULL,
    item_id     INTEGER NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    added_at    INTEGER NOT NULL,
    PRIMARY KEY (profile_id, list_name, item_kind, item_id)
);

CREATE TABLE my_list (
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    item_kind   TEXT    NOT NULL,
    item_id     INTEGER NOT NULL,
    added_at    INTEGER NOT NULL,
    PRIMARY KEY (profile_id, item_kind, item_id)
);

CREATE TABLE settings (
    scope       TEXT    NOT NULL,          -- 'global' or a profile id
    key         TEXT    NOT NULL,
    value       TEXT    NOT NULL,          -- JSON
    PRIMARY KEY (scope, key)
);

CREATE TABLE rules (
    id          TEXT    PRIMARY KEY,
    enabled     INTEGER NOT NULL DEFAULT 1,
    definition  TEXT    NOT NULL,          -- JSON, aurora_core::rules::Rule
    sort_order  INTEGER NOT NULL DEFAULT 0
);

-- README §10: sub-100ms search across everything.
CREATE VIRTUAL TABLE search_index USING fts5(
    title,
    subtitle,
    kind      UNINDEXED,
    ref_id    UNINDEXED,
    tokenize  = 'unicode61 remove_diacritics 2'
);
"#,
    },
    Migration {
        version: 2,
        name: "epg_manual_mappings",
        sql: r#"
-- README §4.4: user-pinned EPG mappings must survive every refresh.
CREATE TABLE epg_manual_map (
    channel_id      INTEGER PRIMARY KEY REFERENCES channels(id) ON DELETE CASCADE,
    epg_channel_id  TEXT NOT NULL,
    created_at      INTEGER NOT NULL
);
"#,
    },
    Migration {
        version: 3,
        name: "skip_markers_and_series_prefs",
        sql: r#"
-- README §9: Skip Intro / Skip Recap / Skip Credits.
--
-- One row per (episode, kind, source): a chapter-derived marker and a user's own
-- skip can coexist, and aurora_core::markers::merge decides which one the button
-- uses. Keeping the user's row even when chapters win is what lets the series
-- learn from it.
CREATE TABLE skip_markers (
    id          INTEGER PRIMARY KEY,
    episode_id  INTEGER NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    kind        TEXT    NOT NULL CHECK (kind IN ('intro','recap','credits')),
    source      TEXT    NOT NULL CHECK (source IN ('chapters','user','learned')),
    start_secs  REAL    NOT NULL,
    end_secs    REAL    NOT NULL,
    created_at  INTEGER NOT NULL,
    UNIQUE (episode_id, kind, source)
);
CREATE INDEX idx_markers_episode ON skip_markers(episode_id, kind);

-- Per-profile, per-show playback preferences (README §9: "remember per-show
-- whether the user always skips").
CREATE TABLE series_prefs (
    profile_id        INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    series_id         INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
    always_skip_intro INTEGER NOT NULL DEFAULT 0,
    always_skip_recap INTEGER NOT NULL DEFAULT 0,
    autoplay_next     INTEGER NOT NULL DEFAULT 1,
    updated_at        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, series_id)
);
"#,
    },
    Migration {
        version: 4,
        name: "provider_username",
        sql: r#"
-- An Xtream line has a username, which is not the same thing as what the user
-- chose to call the provider in the sidebar. Overloading `name` for both meant a
-- provider renamed to "My IPTV" would try to authenticate as "My IPTV".
ALTER TABLE providers ADD COLUMN username TEXT;
"#,
    },
    Migration {
        version: 5,
        name: "default_profile_and_parental_controls",
        sql: r#"
-- Every per-profile table has a foreign key onto `profiles`, and nothing ever
-- created a row. On a real install the first attempt to save watch progress failed
-- with a FOREIGN KEY violation; the tests all created their own profile and so
-- never saw it.
INSERT INTO profiles (id, name, avatar, is_kids, created_at)
SELECT 1, 'Me', 'default', 0, 0
WHERE NOT EXISTS (SELECT 1 FROM profiles);

-- README §11: kids profiles, certification ceilings, and daily limits.
ALTER TABLE profiles ADD COLUMN allow_unrated   INTEGER NOT NULL DEFAULT 1;
ALTER TABLE profiles ADD COLUMN daily_limit_min INTEGER;
ALTER TABLE profiles ADD COLUMN sort_order      INTEGER NOT NULL DEFAULT 0;

-- The master PIN guarding Settings and adult content, distinct from any individual
-- profile's own PIN. Single row.
CREATE TABLE parental (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    master_pin     TEXT,
    hide_adult     INTEGER NOT NULL DEFAULT 1,
    lock_settings  INTEGER NOT NULL DEFAULT 0,
    updated_at     INTEGER NOT NULL DEFAULT 0
);
INSERT INTO parental (id) VALUES (1);

-- Per-channel and per-category locks.
CREATE TABLE parental_locks (
    kind   TEXT NOT NULL CHECK (kind IN ('channel','category')),
    value  TEXT NOT NULL,
    PRIMARY KEY (kind, value)
);

-- Failed PIN attempts, so guessing can be throttled. A 4-digit PIN is only ten
-- thousand possibilities; the throttle is what makes that impractical, not the hash.
CREATE TABLE pin_attempts (
    scope        TEXT PRIMARY KEY,
    failures     INTEGER NOT NULL DEFAULT 0,
    locked_until INTEGER NOT NULL DEFAULT 0
);
"#,
    },
    Migration {
        version: 6,
        name: "dvr",
        sql: r#"
-- README §5 listed `recordings`, `recording_rules` and `reminders` among the core
-- tables, and migration 1 never created them. Nothing noticed because nothing had
-- tried to record anything yet.
CREATE TABLE recording_rules (
    id                  INTEGER PRIMARY KEY,
    title               TEXT    NOT NULL,   -- as shown to the user
    title_key           TEXT    NOT NULL,   -- normalized, what matching compares
    channel_id          INTEGER REFERENCES channels(id) ON DELETE CASCADE,
    new_only            INTEGER NOT NULL DEFAULT 0,
    weekdays            TEXT,               -- JSON array, 0 = Monday, NULL = any
    around_local_minute INTEGER,            -- minutes past local midnight, NULL = any
    time_slack_secs     INTEGER NOT NULL DEFAULT 900,
    pre_padding_secs    INTEGER NOT NULL DEFAULT 60,
    post_padding_secs   INTEGER NOT NULL DEFAULT 300,
    keep_episodes       INTEGER,            -- prune oldest beyond this, NULL = keep all
    priority            INTEGER NOT NULL DEFAULT 0,
    enabled             INTEGER NOT NULL DEFAULT 1,
    created_at          INTEGER NOT NULL
);
CREATE INDEX idx_rules_active ON recording_rules(enabled, title_key);

CREATE TABLE recordings (
    id             INTEGER PRIMARY KEY,
    channel_id     INTEGER NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    rule_id        INTEGER REFERENCES recording_rules(id) ON DELETE SET NULL,
    programme_id   INTEGER,                -- guide row it came from, if any
    title          TEXT    NOT NULL,
    sub_title      TEXT,
    description    TEXT,
    season         INTEGER,
    episode        INTEGER,
    -- Airtime as the guide gave it, kept so the library can show what was actually on.
    air_start      INTEGER NOT NULL,
    air_stop       INTEGER NOT NULL,
    -- Airtime plus padding: what the recorder actually opens and closes on.
    start          INTEGER NOT NULL,
    stop           INTEGER NOT NULL,
    state          TEXT    NOT NULL DEFAULT 'scheduled'
                   CHECK (state IN ('scheduled','recording','completed','failed','skipped')),
    reason         TEXT,                   -- why a failed/skipped row ended that way
    priority       INTEGER NOT NULL DEFAULT 0,
    file_path      TEXT,
    bytes          INTEGER NOT NULL DEFAULT 0,
    duration_secs  INTEGER NOT NULL DEFAULT 0,
    keep           INTEGER NOT NULL DEFAULT 0,   -- exempt from quota pruning
    watched        INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL
);
-- Rule expansion runs on every EPG refresh and must not schedule the same airing
-- twice. This is what makes it idempotent.
CREATE UNIQUE INDEX idx_recordings_unique ON recordings(channel_id, start, title);
CREATE INDEX idx_recordings_due   ON recordings(state, start);
CREATE INDEX idx_recordings_start ON recordings(start DESC);

-- "Remind me when this starts" — the guide's other button. No recorder involved.
CREATE TABLE reminders (
    id            INTEGER PRIMARY KEY,
    channel_id    INTEGER NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    programme_id  INTEGER,
    title         TEXT    NOT NULL,
    start         INTEGER NOT NULL,
    lead_secs     INTEGER NOT NULL DEFAULT 120,
    fired         INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_reminders_unique ON reminders(channel_id, start, title);
CREATE INDEX idx_reminders_due ON reminders(fired, start);
"#,
    },
    Migration {
        version: 7,
        name: "enrichment",
        sql: r#"
-- `people` and `credits` are the third pair of tables README §5 listed from the start
-- and migration 1 never created — after `recordings`, `recording_rules` and `reminders`.
-- The table list in migrate.rs's test now names every one of them, so a fourth cannot
-- go unnoticed the same way.
CREATE TABLE people (
    tmdb_id      INTEGER PRIMARY KEY,
    name         TEXT    NOT NULL,
    profile_path TEXT
);

-- Polymorphic over movies and series, following `watch_progress`. That means no
-- cascade, so deleting a title has to clear its credits — see repo::enrichment::forget.
CREATE TABLE credits (
    item_kind  TEXT    NOT NULL CHECK (item_kind IN ('movie','series')),
    item_id    INTEGER NOT NULL,
    person_id  INTEGER NOT NULL REFERENCES people(tmdb_id) ON DELETE CASCADE,
    -- Character for cast, job for crew. NOT NULL because a NULL in a composite primary
    -- key compares distinct from itself in SQLite, which would let duplicates through.
    role       TEXT    NOT NULL DEFAULT '',
    is_cast    INTEGER NOT NULL DEFAULT 1,
    ord        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (item_kind, item_id, person_id, is_cast, role)
);
CREATE INDEX idx_credits_item   ON credits(item_kind, item_id, is_cast, ord);
CREATE INDEX idx_credits_person ON credits(person_id);

-- What enrichment has already tried.
--
-- Without this, a title that genuinely has no match is searched again on every refresh
-- for ever: a few hundred unmatched titles would spend the whole rate-limit budget
-- re-asking questions already answered.
CREATE TABLE enrichment (
    item_kind    TEXT    NOT NULL CHECK (item_kind IN ('movie','series')),
    item_id      INTEGER NOT NULL,
    state        TEXT    NOT NULL CHECK (state IN ('matched','nomatch','failed')),
    tmdb_id      INTEGER,
    confidence   REAL,
    attempted_at INTEGER NOT NULL,
    PRIMARY KEY (item_kind, item_id)
);
CREATE INDEX idx_enrichment_state ON enrichment(state, attempted_at);
"#,
    },
    Migration {
        version: 8,
        name: "filtering",
        sql: r#"
-- README §7.3: "Hide non-[language]" and "collapse quality duplicates". Both are
-- answered per row at query time, so both need a column that SQL can read.
--
-- `lang_code` is ISO 639-1 as `aurora_core::lang` worked it out, or NULL when the name
-- said nothing. NULL is not "English" — a filter that treated it as non-English would
-- empty the library of every provider that tags nothing, which is most of them.
--
-- `quality_rank` orders duplicates onto the best copy. 0 is unknown, which ranks below
-- SD, so a tagged copy always wins over an untagged one.
--
-- `hidden` and `custom_title` mirror what `channels` already has, so the playlist
-- editor works the same on all three lists and a refresh never discards either
-- (README §4.6).
ALTER TABLE channels ADD COLUMN lang_code    TEXT;
ALTER TABLE channels ADD COLUMN quality_rank INTEGER NOT NULL DEFAULT 0;

ALTER TABLE movies ADD COLUMN lang_code    TEXT;
ALTER TABLE movies ADD COLUMN quality_rank INTEGER NOT NULL DEFAULT 0;
ALTER TABLE movies ADD COLUMN hidden       INTEGER NOT NULL DEFAULT 0;
ALTER TABLE movies ADD COLUMN custom_title TEXT;

-- Series never carried a quality or a category at all: the provider advertises a
-- quality per episode stream, and the show is the thing being collapsed and grouped.
ALTER TABLE series ADD COLUMN group_title  TEXT;
ALTER TABLE series ADD COLUMN lang_code    TEXT;
ALTER TABLE series ADD COLUMN quality      TEXT;
ALTER TABLE series ADD COLUMN quality_rank INTEGER NOT NULL DEFAULT 0;
ALTER TABLE series ADD COLUMN hidden       INTEGER NOT NULL DEFAULT 0;
ALTER TABLE series ADD COLUMN custom_title TEXT;

-- The duplicate query groups by match key and picks the best rank, on every list paint.
CREATE INDEX idx_channels_dupe ON channels(match_key, quality_rank DESC, id);
CREATE INDEX idx_movies_dupe   ON movies(match_key, quality_rank DESC, id);
CREATE INDEX idx_series_dupe   ON series(match_key, quality_rank DESC, id);
CREATE INDEX idx_channels_lang ON channels(lang_code);
CREATE INDEX idx_movies_lang   ON movies(lang_code);
CREATE INDEX idx_series_lang   ON series(lang_code);
"#,
    },
    Migration {
        version: 9,
        name: "canonical_titles",
        sql: r#"
-- What a title is actually called, as TMDB has it.
--
-- A provider's title is a filing string, not a name: a real subscription carries
-- "EN Final Spain Vs Argentina", "NL * FILMS [SUB] Bad Neighbours" and
-- "AR ..." with the language, the group and the quality all folded in. Enrichment
-- already fetches the canonical one and threw it away, writing only the overview, the
-- artwork and the ratings.
--
-- Kept *beside* the provider's title rather than replacing it, because the original is
-- still what `match_key` is built from, what duplicate collapsing compares, and what
-- the playlist editor is editing. Display order is `custom_title` (what the viewer
-- renamed it to), then `tmdb_title`, then `title`.
ALTER TABLE movies ADD COLUMN tmdb_title     TEXT;
ALTER TABLE movies ADD COLUMN original_title TEXT;
ALTER TABLE movies ADD COLUMN tagline        TEXT;
ALTER TABLE series ADD COLUMN tmdb_title     TEXT;
ALTER TABLE series ADD COLUMN original_title TEXT;
ALTER TABLE series ADD COLUMN tagline        TEXT;
"#,
    },
    Migration {
        version: 10,
        name: "trailers",
        sql: r#"
-- The trailer, as a YouTube key.
--
-- The hero billboard has claimed to be playing one since it was drawn: it prints "NOW
-- PLAYING TRAILER", slowly zooms the backdrop, and offers a mute button wired to a
-- state variable that drives no audio. Nothing fetched a trailer and nothing played
-- one. The same label sits on the rail cards as "Preview playing".
--
-- A key rather than a URL: TMDB gives `site` and `key` separately, the embed address is
-- built from the key, and storing a URL would mean reparsing it to get the key back for
-- the iframe API. Nothing but YouTube is stored — TMDB lists Vimeo occasionally and
-- there is no second player here to send it to.
ALTER TABLE movies ADD COLUMN trailer_key TEXT;
ALTER TABLE series ADD COLUMN trailer_key TEXT;
"#,
    },
    Migration {
        version: 11,
        name: "mosaic_layouts",
        sql: r#"
-- A named multi-view layout (README §7.4).
--
-- The channels are a JSON array of ids rather than rows in a child table, because
-- what is being saved is an *ordered, positional* list where the index is the tile and
-- a gap is meaningful: a 2x2 with three channels chosen has tile 3 empty, and that is
-- a layout somebody deliberately saved rather than a row that failed to insert. A
-- child table would make the order a sort column and the gap unrepresentable.
--
-- No foreign key on the ids for the same reason favourites survive a refresh: a
-- provider dropping a channel for a day should leave the layout alone and show that
-- tile as unavailable, not silently rewrite what the viewer saved. `mosaic::open`
-- resolves ids at open time and says which ones are gone.
CREATE TABLE mosaic_layouts (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL,
    layout      TEXT    NOT NULL,
    -- JSON array of channel ids, tile order, `null` for an empty tile.
    channels    TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
);

-- Names are how the viewer picks one, so two with the same name is a bug rather than
-- a choice. Saving over an existing name replaces it (`upsert_layout`).
CREATE UNIQUE INDEX idx_mosaic_layouts_name ON mosaic_layouts(name);
"#,
    },
    Migration {
        version: 12,
        name: "stream_ids_instead_of_urls",
        sql: r#"
-- Stop keeping the provider's password in the library.
--
-- An Xtream stream URL carries the credentials in its path, by design of the protocol:
-- `{base}/live/{user}/{pass}/{id}.ts`, and `/movie/` and `/series/` the same. Those
-- strings were stored verbatim in three tables, which on the subscription this project
-- is measured against is 6,699 channel sources, 27,494 films and one row per episode of
-- 8,086 series -- several hundred thousand plaintext copies of the username and password
-- in a single SQLite file.
--
-- `docs/PRIVACY.md` said the opposite, and said it as a feature: that a copy of
-- `library.db` "contains no credentials", which is the specific reassurance somebody
-- reads before attaching it to a bug report. It was true of the `providers` table, which
-- keeps only a `credential_ref`, and false of the library (R-126).
--
-- The URL is derivable: the provider row has `base_url`, the password is in Credential
-- Manager, and `aurora_core::xtream::stream_url` already builds the address from a
-- stream id. So the id and the container are what is stored, and the URL is rebuilt at
-- play time.
--
-- `url` stays, and stays NOT NULL, for two reasons. An M3U provider's URLs are whatever
-- the playlist said and cannot be reconstructed from an id, so those rows keep theirs --
-- which `docs/PRIVACY.md` now says out loud rather than implying otherwise. And the
-- backfill that empties the Xtream ones is Rust (`backfill_stream_ids`), because it has
-- to parse a URL, which SQL does badly; an empty string is how a row says "rebuild me".
ALTER TABLE channel_sources ADD COLUMN stream_id TEXT;
ALTER TABLE channel_sources ADD COLUMN container TEXT;
ALTER TABLE movies          ADD COLUMN stream_id TEXT;
ALTER TABLE movies          ADD COLUMN container TEXT;
ALTER TABLE episodes        ADD COLUMN stream_id TEXT;
ALTER TABLE episodes        ADD COLUMN container TEXT;
"#,
    },
    Migration {
        version: 13,
        name: "series_listing_attempts",
        sql: r#"
-- "We already asked, and the answer was no."
--
-- The episode sweep's queue was "shows with no episodes stored", which cannot tell a
-- show nobody has asked about from one the provider will not list. A show whose listing
-- comes back empty, or fails, writes no episodes, stays in the queue, and is returned by
-- the very next batch -- and the ordering is deterministic, so the tail of a sweep
-- re-asked the same unanswerable remainder on every pass until SWEEP_MAX_BATCHES (200)
-- ran out. With N unanswerable shows that is up to 200 x N requests to the viewer's own
-- panel: the same host their video comes from, on a line that answers HTTP 555 when it
-- has one connection too many (R-83).
--
-- Metadata enrichment has had the answer to this since migration 7: the `enrichment`
-- table records `matched` / `nomatch` / `failed` with an `attempted_at` precisely so a
-- "no" is not re-asked for ever. This is the same thing for episode listings, and
-- deliberately the same shape.
--
-- Only unsuccessful attempts are recorded. A listing that produced episodes leaves the
-- queue because the episodes are there, which is the condition that was always right.
CREATE TABLE series_listing_attempts (
    series_id    INTEGER PRIMARY KEY REFERENCES series(id) ON DELETE CASCADE,
    -- `empty`: the panel answered and listed nothing usable. `failed`: it would not
    -- answer. Both are reasons to wait; they are kept apart because only one of them
    -- says anything is wrong.
    state        TEXT    NOT NULL CHECK (state IN ('empty','failed')),
    attempted_at INTEGER NOT NULL
);
CREATE INDEX idx_series_listing_attempts_at
    ON series_listing_attempts(attempted_at);
"#,
    },
    Migration {
        version: 14,
        name: "recordings_keyed_on_airtime",
        sql: r#"
-- Key a scheduled recording on the airing, not on the padded start.
--
-- `idx_recordings_unique` was `(channel_id, start, title)`, and the comment above it is
-- right that this is what makes rule expansion idempotent across EPG refreshes -- as
-- long as `start` does not move. It does: `start` is the *padded* start and
-- `pre_padding_secs` is a per-rule setting. Raise a rule's pre-padding from 60 to 120
-- seconds and every airing it has already scheduled gets a new `start`, so the next
-- refresh inserts a second row for each rather than leaving it alone. Two recordings of
-- the same programme, two connections wanted, and on a one-connection line (R-32) one of
-- them failing (R-47).
--
-- `air_start` is the unpadded airtime, which is the stable identity of "this airing on
-- this channel" and does not move when padding changes. The row already carries it.
--
-- Duplicates that the old index allowed are collapsed first, keeping the lowest id --
-- the one scheduled first, which is also the one any `rule_id` reference and any
-- recording already on disk belongs to.
DELETE FROM recordings WHERE id NOT IN (
    SELECT MIN(id) FROM recordings GROUP BY channel_id, air_start, title
);
DROP INDEX IF EXISTS idx_recordings_unique;
CREATE UNIQUE INDEX idx_recordings_unique ON recordings(channel_id, air_start, title);

-- `MarkerSource` has four variants and this CHECK had three.
--
-- `Convention` -- the credits-from-the-clock marker, tier 4 -- was missing, while
-- `markers::record` inserts `marker.source.as_str()` without restriction and
-- `MarkerSource::parse` accepts "convention" specifically so the value can round-trip
-- through this column. Nothing writes one today, so the constraint and the enum
-- disagreed where the compiler could not see it and the first caller to record one would
-- have failed at runtime (R-44).
--
-- SQLite cannot alter a CHECK, so the table is rebuilt. It is small -- one row per
-- marker per episode -- and `PRAGMA foreign_keys` is off during a migration, so the
-- episode references survive the swap.
CREATE TABLE skip_markers_new (
    id          INTEGER PRIMARY KEY,
    episode_id  INTEGER NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    kind        TEXT    NOT NULL CHECK (kind IN ('intro','recap','credits')),
    source      TEXT    NOT NULL CHECK (source IN ('chapters','user','learned','convention')),
    start_secs  REAL    NOT NULL,
    end_secs    REAL    NOT NULL,
    created_at  INTEGER NOT NULL,
    UNIQUE (episode_id, kind, source)
);
INSERT INTO skip_markers_new
    (id, episode_id, kind, source, start_secs, end_secs, created_at)
    SELECT id, episode_id, kind, source, start_secs, end_secs, created_at
      FROM skip_markers;
DROP TABLE skip_markers;
ALTER TABLE skip_markers_new RENAME TO skip_markers;
CREATE INDEX idx_markers_episode ON skip_markers(episode_id, kind);

-- Every display name a guide channel has, not only the first.
--
-- XMLTV allows several `<display-name>` elements and the alternative spellings are
-- exactly what fuzzy name matching needs, but the column was singular so all but the
-- first were dropped at the boundary. It does not affect matching today -- `sync.rs`
-- builds the `EpgIndex` from the in-memory parse, which has all of them -- but anything
-- *later* could only ever see one spelling: a re-match without a re-fetch, or a "which
-- EPG channel is this?" picker in Settings (R-50).
--
-- A JSON array rather than a child table: this is read as a whole, per channel, and
-- never queried across channels.
ALTER TABLE epg_channels ADD COLUMN display_names TEXT;
"#,
    },
    Migration {
        version: 15,
        name: "seek_is_its_own_marker_source",
        sql: r#"
-- `MarkerSource::Seek`: a forward jump the viewer made that nothing has corroborated.
--
-- It used to be recorded as `user`, which is the source a *press of the Skip button*
-- has. The two are not the same evidence. A press acts on a region already on screen;
-- a drag is an inference about why somebody moved their thumb. Recorded as `user` it
-- outranked everything the rest of the show had to say and -- because
-- `may_auto_skip()` was `!matches!(self, Convention)` -- the per-show "always skip"
-- preference was allowed to fire it without asking. One twenty-second nudge, applied
-- silently to every episode of a series.
--
-- `seek` ranks below `learned` and never auto-skips, so a single jump earns a button
-- and nothing more.
--
-- The rows already stored as `user` are left as `user`. They cannot be told apart --
-- both paths wrote the same string -- and demoting all of them would throw away the
-- deliberate button presses along with the accidents. They are bounded in number and
-- the agreement rule in `learn_from_series` now contains the damage: a lone
-- observation no longer reaches another episode whatever its source.
--
-- SQLite cannot alter a CHECK, so the table is rebuilt, exactly as migration 14 did
-- for the same column. It is small -- one row per marker per episode -- and
-- `PRAGMA foreign_keys` is off during a migration, so the episode references survive
-- the swap.
CREATE TABLE skip_markers_new (
    id          INTEGER PRIMARY KEY,
    episode_id  INTEGER NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    kind        TEXT    NOT NULL CHECK (kind IN ('intro','recap','credits')),
    source      TEXT    NOT NULL
                CHECK (source IN ('chapters','user','learned','seek','convention')),
    start_secs  REAL    NOT NULL,
    end_secs    REAL    NOT NULL,
    created_at  INTEGER NOT NULL,
    UNIQUE (episode_id, kind, source)
);
INSERT INTO skip_markers_new
    (id, episode_id, kind, source, start_secs, end_secs, created_at)
    SELECT id, episode_id, kind, source, start_secs, end_secs, created_at
      FROM skip_markers;
DROP TABLE skip_markers;
ALTER TABLE skip_markers_new RENAME TO skip_markers;
CREATE INDEX idx_markers_episode ON skip_markers(episode_id, kind);
"#,
    },
    Migration {
        version: 16,
        name: "refresh_stamp_matches_the_rows",
        sql: r#"
-- Every channel was wearing the "Dropped" badge.
--
-- `channels::list` shows it when `last_seen_at < providers.last_refresh_at`, meaning
-- "your provider no longer lists this channel". Every row a refresh touches is stamped
-- with `options.now_unix`, taken before the download begins -- but `last_refresh_at` was
-- written from a *second* call to the clock, after the import had finished. On a panel
-- with 22,121 channels and a guide that is minutes later, so the comparison read
-- "stamped at the start < finished at the end" for the entire library.
--
-- The code now stores the stamp, so the next refresh is correct by itself. This is for
-- the libraries that already have the wrong number in the column: without it they go on
-- showing the badge on everything until somebody refreshes.
--
-- The repair is the largest `last_seen_at` the provider's own rows carry, which *is* the
-- stamp the last refresh used -- the rows are the record of it. Channels first, because
-- they are what the badge is about, and `MAX` over movies and series too so a provider
-- whose channels were all genuinely dropped is not given a stamp from before them.
--
-- `WHERE last_refresh_at IS NOT NULL` so a provider that has never been refreshed keeps
-- its NULL, which is what makes "a library nobody has refreshed has not lost anything"
-- true. And the value is only lowered, never raised: `MIN(last_refresh_at, ...)` would
-- be wrong for a provider whose rows legitimately predate a refresh that found nothing
-- new, so the repair applies only where the stamp is *ahead* of every row it wrote.
UPDATE providers
   SET last_refresh_at = (
         SELECT MAX(seen) FROM (
           SELECT MAX(last_seen_at) AS seen FROM channels WHERE provider_id = providers.id
           UNION ALL
           SELECT MAX(last_seen_at) FROM movies WHERE provider_id = providers.id
           UNION ALL
           SELECT MAX(last_seen_at) FROM series WHERE provider_id = providers.id
         )
       )
 WHERE last_refresh_at IS NOT NULL
   AND EXISTS (
         SELECT 1 FROM channels
          WHERE provider_id = providers.id
            AND last_seen_at < providers.last_refresh_at
       )
   AND (
         SELECT MAX(seen) FROM (
           SELECT MAX(last_seen_at) AS seen FROM channels WHERE provider_id = providers.id
           UNION ALL
           SELECT MAX(last_seen_at) FROM movies WHERE provider_id = providers.id
           UNION ALL
           SELECT MAX(last_seen_at) FROM series WHERE provider_id = providers.id
         )
       ) IS NOT NULL;
"#,
    },
    Migration {
        version: 17,
        name: "home_feed_signals",
        sql: r#"
-- Two things the home feed learns from that nothing stored before.
--
-- `not_interested` is the viewer saying "not for me" about a title. It is the only
-- explicit negative signal there is: abandonment is inferred, and a like or a list is
-- the other direction. The title leaves every rail at once, and its genres, shelf and
-- people count against what is recommended next. Per profile, like everything else
-- about taste.
CREATE TABLE not_interested (
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    item_kind   TEXT    NOT NULL CHECK (item_kind IN ('movie','series')),
    item_id     INTEGER NOT NULL,
    added_at    INTEGER NOT NULL,
    PRIMARY KEY (profile_id, item_kind, item_id)
);

-- Lists TMDB ranks for us, cached: what is trending this week, and for a title the
-- viewer watched, what people who liked it also liked. Keyed `trending` or
-- `recs:movie:603` / `recs:series:1399`; `items` is a JSON array of [kind, tmdb_id]
-- pairs, best first. TMDB ids rather than library ids, because the library is
-- rebuilt by every refresh and a title's TMDB id is the thing that survives it.
-- A cache in the plain sense: deleting every row loses nothing but a fetch.
CREATE TABLE tmdb_lists (
    list_key    TEXT    PRIMARY KEY,
    items       TEXT    NOT NULL,
    fetched_at  INTEGER NOT NULL
);

-- The feed resolves those ids against the library.
CREATE INDEX idx_movies_tmdb ON movies(tmdb_id) WHERE tmdb_id IS NOT NULL;
CREATE INDEX idx_series_tmdb ON series(tmdb_id) WHERE tmdb_id IS NOT NULL;
"#,
    },
    Migration {
        version: 18,
        name: "sync_capture",
        sql: SYNC_CAPTURE,
    },
    Migration {
        version: 19,
        name: "parental_gate",
        sql: PARENTAL_GATE,
    },
    Migration {
        version: 20,
        name: "watch_time",
        sql: WATCH_TIME,
    },
    Migration {
        version: 21,
        name: "adult_needs_pin",
        sql: ADULT_NEEDS_PIN,
    },
    Migration {
        version: 22,
        name: "sync_unreadable",
        sql: SYNC_UNREADABLE,
    },
    // The search palette's people section asks `name LIKE '%…%' ORDER BY name LIMIT n`.
    // With nothing on `name`, SQLite read all of `people` -- 583,341 rows on a real
    // library -- and sorted the matches, 250-490 ms on every keystroke. Walking this
    // index in name order stops at the limit: under 10 ms for a common fragment, and no
    // worse than before for one that matches nothing.
    Migration {
        version: 23,
        name: "people_name_index",
        sql: "CREATE INDEX idx_people_name ON people(name);",
    },
    Migration {
        version: 24,
        name: "restore_pending",
        sql: RESTORE_PENDING,
    },
];

pub const LATEST_VERSION: u32 = 24;

/// Migration 24: what a restored backup named that this library does not have yet
/// (`aurora_sync::backup`).
///
/// A backup restored onto a fresh install arrives before the library does: a favourite
/// names a channel that is only imported once the provider it also restored has been
/// refreshed. It waits here and is tried again after each import. Its own table rather
/// than `sync_pending`, because signing out of sync empties that -- and a backup is for
/// the people who do not use sync at all.
const RESTORE_PENDING: &str = r#"
CREATE TABLE restore_pending (
    sync_key   TEXT    PRIMARY KEY,
    kind       TEXT    NOT NULL,
    ident      TEXT    NOT NULL,
    fields     TEXT    NOT NULL,
    parked_at  INTEGER NOT NULL
) WITHOUT ROWID;
"#;

/// Migration 21: adult categories show only while there is a parental PIN (README §11:
/// "requires explicit opt-in plus the master PIN to reveal"; `AUDIT/findings.md` F-39).
///
/// "Hide adult categories" was a switch anybody could turn off with no PIN set, and
/// clearing the PIN left them showing. The trigger holds the rule wherever the row is
/// written from: the settings command, clearing the PIN, or another device's row arriving
/// through sync. It puts the switch back rather than refusing, because sync has nobody to
/// refuse to; the command checks first and says why. Recursive triggers are off, so the
/// update inside does not fire it again, and `sync_parental_update` sends the corrected
/// row on.
///
/// Migration 22: sync records this device could not read (`docs/APPLE_PORT_AND_SYNC.md`
/// §3.8, `AUDIT/findings.md` F-41).
///
/// A record written by a newer Aurora, or damaged, is skipped and the pull cursor moves
/// past it, so without a note of it the record is never seen again: an update that could
/// read it would not know to ask. `reader` is what this build can read
/// (`aurora_sync::engine::reader`), and a different one is the sign to fetch the record
/// again. Device-local, like the rest of sync's own state, and with no capture triggers.
const SYNC_UNREADABLE: &str = r#"
CREATE TABLE sync_unreadable (
    doc_id   TEXT    PRIMARY KEY,
    reader   TEXT    NOT NULL,
    seen_at  INTEGER NOT NULL
) WITHOUT ROWID;
"#;

/// The `UPDATE` applies the rule to a library that already broke it.
const ADULT_NEEDS_PIN: &str = r#"
UPDATE parental SET hide_adult = 1 WHERE id = 1 AND hide_adult = 0 AND master_pin IS NULL;

CREATE TRIGGER parental_adult_needs_pin AFTER UPDATE OF hide_adult, master_pin ON parental
    WHEN NEW.hide_adult = 0 AND NEW.master_pin IS NULL
BEGIN
    UPDATE parental SET hide_adult = 1 WHERE id = 1;
END;
"#;

/// Migration 20: how long each profile has watched today, for the daily limit
/// (README §11, `AUDIT/findings.md` F-37).
///
/// Per device and unsynced, like `session`: the host counts what this device plays.
/// `profile_id` 0 is nobody chosen. `day` is the viewer's local date, as SQLite's
/// `localtime` reads it, so a day ends at the viewer's midnight, not at UTC's.
const WATCH_TIME: &str = r#"
CREATE TABLE watch_time (
    profile_id INTEGER NOT NULL,
    day        TEXT    NOT NULL,
    seconds    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, day)
) WITHOUT ROWID;
"#;

/// Migration 19: parental controls that actually apply (README §11).
///
/// Before this, every parental setting was stored and none was read. The age limit on
/// a kids profile, "hide adult categories" (on by default) and the per-channel and
/// per-category locks reached no list query and no playback, so a nine-year-old's
/// profile showed and played everything the owner's did.
///
/// **`session`** is who is watching on this device now: the profile last chosen in
/// "Who's watching?", and until when the parental PIN has opened locked content and the
/// parental settings. One row, no sync triggers, and the host empties it at every launch,
/// so a restart is never still unlocked from yesterday.
///
/// **`parental_gate`** is what that profile may see, folded into one row: an age limit,
/// whether unrated titles pass it, whether adult categories are hidden, and whether
/// locked content is hidden or only asks for the PIN. `LibraryFilter::where_sql` reads it
/// in every list query, and playback asks the same row (`repo::parental::check`), so a
/// list and the player cannot disagree about what is allowed. A view rather than values
/// the host keeps up to date, because the inputs change from four places -- the picker,
/// the parental settings, a profile's limits and a sync pulling any of them in -- and a
/// view cannot be forgotten by one of them.
///
/// With no profile chosen, or the chosen one deleted, the gate is the **strictest of
/// every profile's**: the lowest age limit, unrated blocked if any profile blocks it. A
/// device with one profile is therefore that profile, and one with a kids profile shows
/// nothing above the kid's limit until somebody has picked who is watching.
///
/// Locks apply only while there is a parental PIN to open them with. A lock set and then
/// left behind when the PIN was removed would otherwise be a channel nobody can watch.
///
/// `rule` packs the content half into one integer for `parental_allows`: -1 when nothing
/// is restricted, otherwise the age limit (255 for none) in the low byte, "unrated
/// passes" in bit 8 and "hide adult categories" in bit 9. An age limit below 18 always
/// hides adult categories, whatever the household setting says.
const PARENTAL_GATE: &str = r#"
CREATE TABLE session (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    profile_id     INTEGER,
    unlocked_until INTEGER NOT NULL DEFAULT 0
);
INSERT INTO session (id) VALUES (1);

CREATE VIEW parental_gate AS
SELECT
    CASE WHEN g.age >= 255 AND g.adult = 0 THEN -1
         ELSE g.age | (g.unrated << 8) | (g.adult << 9) END AS rule,
    g.hide_locked AS hide_locked,
    g.unlocked    AS unlocked
FROM (
    SELECT
        c.age     AS age,
        c.unrated AS unrated,
        CASE WHEN c.age < 18
               OR COALESCE((SELECT hide_adult FROM parental WHERE id = 1), 1) != 0
             THEN 1 ELSE 0 END AS adult,
        CASE WHEN COALESCE((SELECT value FROM settings
                             WHERE scope = 'global' AND key = 'parental.hide_locked'),
                           'false') = 'true'
             THEN 1 ELSE 0 END AS hide_locked,
        CASE WHEN (SELECT master_pin FROM parental WHERE id = 1) IS NULL
               OR COALESCE((SELECT unlocked_until FROM session WHERE id = 1), 0)
                  > CAST(strftime('%s', 'now') AS INTEGER)
             THEN 1 ELSE 0 END AS unlocked
    FROM (
        SELECT COALESCE(MIN(parental_age(p.max_rating)), 255) AS age,
               COALESCE(MIN(CASE WHEN parental_age(p.max_rating) < 255
                                 THEN p.allow_unrated END), 1) AS unrated
          FROM profiles p
         WHERE p.id = (SELECT profile_id FROM session WHERE id = 1)
            OR NOT EXISTS (SELECT 1 FROM profiles q
                            WHERE q.id = (SELECT profile_id FROM session WHERE id = 1))
    ) c
) g;
"#;

/// Migration 18: what sync needs from the local database (`docs/APPLE_PORT_AND_SYNC.md`
/// §3.3 and §3.6).
///
/// Three parts.
///
/// **Identity.** Row ids are local: movie 4,812 here is not movie 4,812 on the TV. Rows
/// that sync are named by *sync keys* built from things that mean the same on every
/// device -- a UUID minted once per provider and profile, and the provider's own stream
/// key. `sync_item_keys` builds them, so there is one definition of "which film is this"
/// rather than one per table.
///
/// **Capture.** Every user mutation of synced data lands in `sync_outbox` *in the same
/// transaction as the change*, by trigger. Triggers rather than calls at each write site
/// because there are more than thirty write sites across three crates, and a sync that
/// misses one is a sync that silently forgets an edit; a trigger cannot be forgotten by a
/// new command. The outbox is coalesced on the key: a film played for an hour is one
/// pending row, not seven hundred.
///
/// Capture is off until sync is turned on (`sync_state.enabled`), so a device that never
/// signs in carries nothing but an idle trigger, and it is suspended while the sync
/// engine applies a pulled change (`sync_state.applying`), so applying a remote edit does
/// not echo it straight back.
///
/// **Clock.** Each capture stamps a hybrid logical clock: wall-clock milliseconds in the
/// high bits and a counter in the low sixteen, never going backwards. The engine merges
/// remote clocks into the same `sync_state.hlc` on pull, so a PC whose clock runs two
/// minutes fast cannot win every conflict for two minutes.
///
/// The outbox holds *which record* changed and when, not its contents. The engine reads
/// the row as it is at push time, which is both smaller and always current. The `ident`
/// is the record's identity in device-independent terms -- the same JSON the receiving
/// device resolves back to its own rows -- and is captured at change time, because after
/// a delete there is no row left to read it from.
const SYNC_CAPTURE: &str = r#"
-- Identity --------------------------------------------------------------------------

ALTER TABLE providers       ADD COLUMN sync_id TEXT;
ALTER TABLE profiles        ADD COLUMN sync_id TEXT;
ALTER TABLE recording_rules ADD COLUMN sync_id TEXT;

-- `randomblob` is evaluated per row, so every existing row gets its own id.
UPDATE providers       SET sync_id = lower(hex(randomblob(16))) WHERE sync_id IS NULL;
UPDATE profiles        SET sync_id = lower(hex(randomblob(16))) WHERE sync_id IS NULL;
UPDATE recording_rules SET sync_id = lower(hex(randomblob(16))) WHERE sync_id IS NULL;

CREATE UNIQUE INDEX idx_providers_sync_id       ON providers(sync_id);
CREATE UNIQUE INDEX idx_profiles_sync_id        ON profiles(sync_id);
CREATE UNIQUE INDEX idx_recording_rules_sync_id ON recording_rules(sync_id);

-- The sync key of every catalogue item, by local kind and id.
--
-- `provider_key` is the provider's own stream id or URL hash, already stable across
-- refreshes, so `mv:{provider}:{provider_key}` is the same film on every device that
-- imported the same line. An episode is named by its series and its place in it, which
-- is the `UNIQUE (series_id, season, episode)` the table already enforces.
--
-- A lookup by (kind, id) touches one arm: SQLite pushes the WHERE into each branch of a
-- UNION ALL view and the other three fail on the constant `kind` at once.
-- `sync_lookups_use_the_primary_keys` checks that stays true.
CREATE VIEW sync_item_keys (kind, id, key) AS
    SELECT 'channel', c.id, 'ch:' || p.sync_id || ':' || c.provider_key
      FROM channels c JOIN providers p ON p.id = c.provider_id
    UNION ALL
    SELECT 'movie', m.id, 'mv:' || p.sync_id || ':' || m.provider_key
      FROM movies m JOIN providers p ON p.id = m.provider_id
    UNION ALL
    SELECT 'series', s.id, 'sr:' || p.sync_id || ':' || s.provider_key
      FROM series s JOIN providers p ON p.id = s.provider_id
    UNION ALL
    SELECT 'episode', e.id,
           'ep:sr:' || p.sync_id || ':' || s.provider_key
                    || ':s' || e.season || 'e' || e.episode
      FROM episodes e
      JOIN series s    ON s.id = e.series_id
      JOIN providers p ON p.id = s.provider_id;

-- Sync's own state --------------------------------------------------------------------

-- Small key/value store: `enabled`, `applying`, `hlc`, the pull cursor, this device's id.
CREATE TABLE sync_state (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);

-- Records changed here and not yet pushed. One row per record, however often it changed.
CREATE TABLE sync_outbox (
    sync_key  TEXT    PRIMARY KEY,
    kind      TEXT    NOT NULL,
    ident     TEXT    NOT NULL,     -- JSON, device-independent
    hlc       INTEGER NOT NULL,     -- when it last changed here
    attempts  INTEGER NOT NULL DEFAULT 0
);

-- The last merged state of every record this device knows, with a clock per field.
-- What a field-by-field merge compares against, and how a push tells which fields of a
-- row this device actually changed.
CREATE TABLE sync_shadow (
    sync_key  TEXT PRIMARY KEY,
    record    TEXT NOT NULL         -- JSON
);

-- Records that refer to something not imported here yet -- a favourite of a channel from
-- a provider this device has not refreshed. Parked, never dropped, and retried after the
-- next import (§3.3).
CREATE TABLE sync_pending (
    sync_key   TEXT    PRIMARY KEY,
    record     TEXT    NOT NULL,    -- JSON
    parked_at  INTEGER NOT NULL
);

-- The next clock value: never behind the wall clock, never behind the last value issued.
CREATE VIEW sync_next_hlc (hlc) AS
SELECT MAX(
    COALESCE((SELECT CAST(value AS INTEGER) FROM sync_state WHERE key = 'hlc'), 0) + 1,
    CAST(unixepoch('subsec') * 1000 AS INTEGER) * 65536
);

-- The one place a change is recorded. Table triggers insert (kind, ident) here, and this
-- decides whether sync is on, stamps the clock and coalesces into the outbox. An ident
-- with a null in it names something that cannot be identified across devices -- an item
-- whose provider is gone, a row whose profile was deleted in the same statement -- and
-- is skipped; the deletion of the parent is what other devices will act on.
CREATE VIEW sync_enqueue (kind, ident) AS SELECT NULL, NULL WHERE 0;

CREATE TRIGGER sync_enqueue_capture INSTEAD OF INSERT ON sync_enqueue
WHEN EXISTS (SELECT 1 FROM sync_state WHERE key = 'enabled')
 AND NOT EXISTS (SELECT 1 FROM sync_state WHERE key = 'applying')
 AND NEW.ident IS NOT NULL
 AND NOT EXISTS (SELECT 1 FROM json_each(NEW.ident) WHERE type = 'null')
BEGIN
    INSERT INTO sync_state (key, value)
        SELECT 'hlc', hlc FROM sync_next_hlc WHERE true
        ON CONFLICT (key) DO UPDATE SET value = excluded.value;
    INSERT INTO sync_outbox (sync_key, kind, ident, hlc)
        VALUES (NEW.kind || ':' || NEW.ident, NEW.kind, NEW.ident,
                (SELECT CAST(value AS INTEGER) FROM sync_state WHERE key = 'hlc'))
        ON CONFLICT (sync_key) DO UPDATE SET hlc = excluded.hlc, attempts = 0;
END;

-- Providers ---------------------------------------------------------------------------

CREATE TRIGGER sync_providers_insert AFTER INSERT ON providers
BEGIN
    UPDATE providers SET sync_id = lower(hex(randomblob(16)))
     WHERE id = NEW.id AND sync_id IS NULL;
    INSERT INTO sync_enqueue VALUES ('provider',
        json_object('p', (SELECT sync_id FROM providers WHERE id = NEW.id)));
END;

-- Not `last_refresh_at`, `expires_at` or `credential_ref`: those are this device's.
CREATE TRIGGER sync_providers_update AFTER UPDATE OF
    name, kind, base_url, username, user_agent, referrer, enabled, max_connections, sort_order
    ON providers
BEGIN
    INSERT INTO sync_enqueue VALUES ('provider', json_object('p', NEW.sync_id));
END;

CREATE TRIGGER sync_providers_delete AFTER DELETE ON providers
BEGIN
    INSERT INTO sync_enqueue VALUES ('provider', json_object('p', OLD.sync_id));
END;

-- Profiles and parental controls ------------------------------------------------------

CREATE TRIGGER sync_profiles_insert AFTER INSERT ON profiles
BEGIN
    UPDATE profiles SET sync_id = lower(hex(randomblob(16)))
     WHERE id = NEW.id AND sync_id IS NULL;
    INSERT INTO sync_enqueue VALUES ('profile',
        json_object('pf', (SELECT 'pf:' || sync_id FROM profiles WHERE id = NEW.id)));
END;

CREATE TRIGGER sync_profiles_update AFTER UPDATE OF
    name, avatar, is_kids, pin_hash, max_rating, allow_unrated, daily_limit_min, sort_order
    ON profiles
BEGIN
    INSERT INTO sync_enqueue VALUES ('profile', json_object('pf', 'pf:' || NEW.sync_id));
END;

CREATE TRIGGER sync_profiles_delete AFTER DELETE ON profiles
BEGIN
    INSERT INTO sync_enqueue VALUES ('profile', json_object('pf', 'pf:' || OLD.sync_id));
END;

CREATE TRIGGER sync_parental_update AFTER UPDATE OF master_pin, hide_adult, lock_settings
    ON parental
BEGIN
    INSERT INTO sync_enqueue VALUES ('parental', json_object());
END;

CREATE TRIGGER sync_parental_locks_insert AFTER INSERT ON parental_locks
BEGIN
    INSERT INTO sync_enqueue VALUES ('parental_lock', json_object('kind', NEW.kind,
        'value', CASE NEW.kind
            WHEN 'channel' THEN (SELECT key FROM sync_item_keys
                                  WHERE kind = 'channel' AND id = CAST(NEW.value AS INTEGER))
            ELSE NEW.value END));
END;

CREATE TRIGGER sync_parental_locks_delete AFTER DELETE ON parental_locks
BEGIN
    INSERT INTO sync_enqueue VALUES ('parental_lock', json_object('kind', OLD.kind,
        'value', CASE OLD.kind
            WHEN 'channel' THEN (SELECT key FROM sync_item_keys
                                  WHERE kind = 'channel' AND id = CAST(OLD.value AS INTEGER))
            ELSE OLD.value END));
END;

-- Watch progress ----------------------------------------------------------------------
--
-- Recordings are files on this PC and their progress means nothing elsewhere.

CREATE TRIGGER sync_progress_insert AFTER INSERT ON watch_progress
WHEN NEW.item_kind <> 'recording'
BEGIN
    INSERT INTO sync_enqueue VALUES ('progress', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = NEW.profile_id),
        'item', (SELECT key FROM sync_item_keys WHERE kind = NEW.item_kind AND id = NEW.item_id)));
END;

CREATE TRIGGER sync_progress_update AFTER UPDATE ON watch_progress
WHEN NEW.item_kind <> 'recording'
BEGIN
    INSERT INTO sync_enqueue VALUES ('progress', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = NEW.profile_id),
        'item', (SELECT key FROM sync_item_keys WHERE kind = NEW.item_kind AND id = NEW.item_id)));
END;

CREATE TRIGGER sync_progress_delete AFTER DELETE ON watch_progress
WHEN OLD.item_kind <> 'recording'
BEGIN
    INSERT INTO sync_enqueue VALUES ('progress', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = OLD.profile_id),
        'item', (SELECT key FROM sync_item_keys WHERE kind = OLD.item_kind AND id = OLD.item_id)));
END;

-- Favourites, My List, Not interested ------------------------------------------------
--
-- Each membership is its own record, and a removal is a tombstone of that record. That
-- is what keeps a removal on the TV from being resurrected by the PC's older copy.

CREATE TRIGGER sync_favorites_insert AFTER INSERT ON favorites
BEGIN
    INSERT INTO sync_enqueue VALUES ('favorite', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = NEW.profile_id),
        'list', NEW.list_name,
        'item', (SELECT key FROM sync_item_keys WHERE kind = NEW.item_kind AND id = NEW.item_id)));
END;

-- A rename moves rows between lists, which is a removal from one and an addition to the
-- other; a reorder changes one field. Enqueueing both sides covers both.
CREATE TRIGGER sync_favorites_update AFTER UPDATE ON favorites
BEGIN
    INSERT INTO sync_enqueue VALUES ('favorite', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = OLD.profile_id),
        'list', OLD.list_name,
        'item', (SELECT key FROM sync_item_keys WHERE kind = OLD.item_kind AND id = OLD.item_id)));
    INSERT INTO sync_enqueue VALUES ('favorite', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = NEW.profile_id),
        'list', NEW.list_name,
        'item', (SELECT key FROM sync_item_keys WHERE kind = NEW.item_kind AND id = NEW.item_id)));
END;

CREATE TRIGGER sync_favorites_delete AFTER DELETE ON favorites
BEGIN
    INSERT INTO sync_enqueue VALUES ('favorite', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = OLD.profile_id),
        'list', OLD.list_name,
        'item', (SELECT key FROM sync_item_keys WHERE kind = OLD.item_kind AND id = OLD.item_id)));
END;

CREATE TRIGGER sync_my_list_insert AFTER INSERT ON my_list
BEGIN
    INSERT INTO sync_enqueue VALUES ('my_list', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = NEW.profile_id),
        'item', (SELECT key FROM sync_item_keys WHERE kind = NEW.item_kind AND id = NEW.item_id)));
END;

CREATE TRIGGER sync_my_list_delete AFTER DELETE ON my_list
BEGIN
    INSERT INTO sync_enqueue VALUES ('my_list', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = OLD.profile_id),
        'item', (SELECT key FROM sync_item_keys WHERE kind = OLD.item_kind AND id = OLD.item_id)));
END;

CREATE TRIGGER sync_not_interested_insert AFTER INSERT ON not_interested
BEGIN
    INSERT INTO sync_enqueue VALUES ('not_interested', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = NEW.profile_id),
        'item', (SELECT key FROM sync_item_keys WHERE kind = NEW.item_kind AND id = NEW.item_id)));
END;

CREATE TRIGGER sync_not_interested_delete AFTER DELETE ON not_interested
BEGIN
    INSERT INTO sync_enqueue VALUES ('not_interested', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = OLD.profile_id),
        'item', (SELECT key FROM sync_item_keys WHERE kind = OLD.item_kind AND id = OLD.item_id)));
END;

-- Series preferences ------------------------------------------------------------------

CREATE TRIGGER sync_series_prefs_insert AFTER INSERT ON series_prefs
BEGIN
    INSERT INTO sync_enqueue VALUES ('series_prefs', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = NEW.profile_id),
        'item', (SELECT key FROM sync_item_keys WHERE kind = 'series' AND id = NEW.series_id)));
END;

CREATE TRIGGER sync_series_prefs_update AFTER UPDATE ON series_prefs
BEGIN
    INSERT INTO sync_enqueue VALUES ('series_prefs', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = NEW.profile_id),
        'item', (SELECT key FROM sync_item_keys WHERE kind = 'series' AND id = NEW.series_id)));
END;

CREATE TRIGGER sync_series_prefs_delete AFTER DELETE ON series_prefs
BEGIN
    INSERT INTO sync_enqueue VALUES ('series_prefs', json_object(
        'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = OLD.profile_id),
        'item', (SELECT key FROM sync_item_keys WHERE kind = 'series' AND id = OLD.series_id)));
END;

-- Playlist edits ----------------------------------------------------------------------
--
-- Only the viewer's override columns. The importer's upsert never names them (see
-- `repo::channels::upsert_batch`), and the WHEN skips a write that changed nothing, so a
-- refresh of 117k films enqueues nothing at all.

CREATE TRIGGER sync_channel_override AFTER UPDATE OF
    custom_name, custom_number, custom_logo, custom_group, hidden ON channels
WHEN OLD.custom_name   IS NOT NEW.custom_name
  OR OLD.custom_number IS NOT NEW.custom_number
  OR OLD.custom_logo   IS NOT NEW.custom_logo
  OR OLD.custom_group  IS NOT NEW.custom_group
  OR OLD.hidden        IS NOT NEW.hidden
BEGIN
    INSERT INTO sync_enqueue VALUES ('override', json_object(
        'item', (SELECT key FROM sync_item_keys WHERE kind = 'channel' AND id = NEW.id)));
END;

CREATE TRIGGER sync_movie_override AFTER UPDATE OF custom_title, hidden ON movies
WHEN OLD.custom_title IS NOT NEW.custom_title OR OLD.hidden IS NOT NEW.hidden
BEGIN
    INSERT INTO sync_enqueue VALUES ('override', json_object(
        'item', (SELECT key FROM sync_item_keys WHERE kind = 'movie' AND id = NEW.id)));
END;

CREATE TRIGGER sync_series_override AFTER UPDATE OF custom_title, hidden ON series
WHEN OLD.custom_title IS NOT NEW.custom_title OR OLD.hidden IS NOT NEW.hidden
BEGIN
    INSERT INTO sync_enqueue VALUES ('override', json_object(
        'item', (SELECT key FROM sync_item_keys WHERE kind = 'series' AND id = NEW.id)));
END;

-- Rules and manual guide mappings -----------------------------------------------------

CREATE TRIGGER sync_rules_insert AFTER INSERT ON rules
BEGIN
    INSERT INTO sync_enqueue VALUES ('rule', json_object('id', NEW.id));
END;

CREATE TRIGGER sync_rules_update AFTER UPDATE ON rules
BEGIN
    INSERT INTO sync_enqueue VALUES ('rule', json_object('id', OLD.id));
    INSERT INTO sync_enqueue VALUES ('rule', json_object('id', NEW.id));
END;

CREATE TRIGGER sync_rules_delete AFTER DELETE ON rules
BEGIN
    INSERT INTO sync_enqueue VALUES ('rule', json_object('id', OLD.id));
END;

CREATE TRIGGER sync_epg_map_insert AFTER INSERT ON epg_manual_map
BEGIN
    INSERT INTO sync_enqueue VALUES ('epg_map', json_object(
        'item', (SELECT key FROM sync_item_keys WHERE kind = 'channel' AND id = NEW.channel_id)));
END;

CREATE TRIGGER sync_epg_map_update AFTER UPDATE ON epg_manual_map
BEGIN
    INSERT INTO sync_enqueue VALUES ('epg_map', json_object(
        'item', (SELECT key FROM sync_item_keys WHERE kind = 'channel' AND id = NEW.channel_id)));
END;

CREATE TRIGGER sync_epg_map_delete AFTER DELETE ON epg_manual_map
BEGIN
    INSERT INTO sync_enqueue VALUES ('epg_map', json_object(
        'item', (SELECT key FROM sync_item_keys WHERE kind = 'channel' AND id = OLD.channel_id)));
END;

-- Recording rules, reminders and mosaic layouts ---------------------------------------
--
-- Recording rules sync so a rule made on the phone records on the PC; only Windows acts
-- on them. A reminder's `fired` is this device's, so it is not a trigger column.

CREATE TRIGGER sync_recording_rules_insert AFTER INSERT ON recording_rules
BEGIN
    UPDATE recording_rules SET sync_id = lower(hex(randomblob(16)))
     WHERE id = NEW.id AND sync_id IS NULL;
    INSERT INTO sync_enqueue VALUES ('recording_rule',
        json_object('id', (SELECT sync_id FROM recording_rules WHERE id = NEW.id)));
END;

CREATE TRIGGER sync_recording_rules_update AFTER UPDATE OF
    title, channel_id, new_only, weekdays, around_local_minute, time_slack_secs,
    pre_padding_secs, post_padding_secs, keep_episodes, priority, enabled
    ON recording_rules
BEGIN
    INSERT INTO sync_enqueue VALUES ('recording_rule', json_object('id', NEW.sync_id));
END;

CREATE TRIGGER sync_recording_rules_delete AFTER DELETE ON recording_rules
BEGIN
    INSERT INTO sync_enqueue VALUES ('recording_rule', json_object('id', OLD.sync_id));
END;

CREATE TRIGGER sync_reminders_insert AFTER INSERT ON reminders
BEGIN
    INSERT INTO sync_enqueue VALUES ('reminder', json_object(
        'item',  (SELECT key FROM sync_item_keys WHERE kind = 'channel' AND id = NEW.channel_id),
        'start', NEW.start,
        'title', NEW.title));
END;

CREATE TRIGGER sync_reminders_delete AFTER DELETE ON reminders
BEGIN
    INSERT INTO sync_enqueue VALUES ('reminder', json_object(
        'item',  (SELECT key FROM sync_item_keys WHERE kind = 'channel' AND id = OLD.channel_id),
        'start', OLD.start,
        'title', OLD.title));
END;

CREATE TRIGGER sync_mosaic_insert AFTER INSERT ON mosaic_layouts
BEGIN
    INSERT INTO sync_enqueue VALUES ('mosaic', json_object('name', NEW.name));
END;

CREATE TRIGGER sync_mosaic_update AFTER UPDATE ON mosaic_layouts
BEGIN
    INSERT INTO sync_enqueue VALUES ('mosaic', json_object('name', OLD.name));
    INSERT INTO sync_enqueue VALUES ('mosaic', json_object('name', NEW.name));
END;

CREATE TRIGGER sync_mosaic_delete AFTER DELETE ON mosaic_layouts
BEGIN
    INSERT INTO sync_enqueue VALUES ('mosaic', json_object('name', OLD.name));
END;

-- Settings ----------------------------------------------------------------------------
--
-- Every key is captured; the engine drops the device-local ones at push time against
-- `aurora_core::sync_settings`, which is where the allowlist lives. A profile-scoped
-- setting is named by the profile's sync key, since profile 2 here is not profile 2 there.

CREATE TRIGGER sync_settings_insert AFTER INSERT ON settings
BEGIN
    INSERT INTO sync_enqueue VALUES ('setting', json_object(
        'scope', CASE NEW.scope WHEN 'global' THEN 'global'
                 ELSE (SELECT 'pf:' || sync_id FROM profiles WHERE id = CAST(NEW.scope AS INTEGER))
                 END,
        'key', NEW.key));
END;

CREATE TRIGGER sync_settings_update AFTER UPDATE ON settings
BEGIN
    INSERT INTO sync_enqueue VALUES ('setting', json_object(
        'scope', CASE NEW.scope WHEN 'global' THEN 'global'
                 ELSE (SELECT 'pf:' || sync_id FROM profiles WHERE id = CAST(NEW.scope AS INTEGER))
                 END,
        'key', NEW.key));
END;

CREATE TRIGGER sync_settings_delete AFTER DELETE ON settings
BEGIN
    INSERT INTO sync_enqueue VALUES ('setting', json_object(
        'scope', CASE OLD.scope WHEN 'global' THEN 'global'
                 ELSE (SELECT 'pf:' || sync_id FROM profiles WHERE id = CAST(OLD.scope AS INTEGER))
                 END,
        'key', OLD.key));
END;
"#;
