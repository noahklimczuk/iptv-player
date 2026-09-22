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
];

pub const LATEST_VERSION: u32 = 8;
