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
];

pub const LATEST_VERSION: u32 = 4;
