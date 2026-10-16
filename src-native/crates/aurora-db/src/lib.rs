//! SQLite persistence for Aurora TV.
//!
//! README §5. WAL mode, forward-only versioned migrations, FTS5 search, and batched writes —
//! the import path has to absorb 40,000 playlist entries and millions of EPG programmes
//! inside the §16 budget.

pub mod error;
pub mod migrate;
pub mod repo;
pub mod schema;

pub use error::{DbError, Result};
/// Re-exported so downstream crates share this crate's exact rusqlite version
/// rather than linking a second, incompatible one.
pub use rusqlite;

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

/// Open (or create) the library database with the pragmas the workload needs.
pub fn open(path: impl AsRef<Path>) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate::run(&conn)?;
    // Takes the credentials out of the three URL columns, once. In `open` rather than in
    // a migration because it has to parse a URL, which SQL does badly; it is guarded on
    // its own result, so the second launch does nothing. See `backfill_stream_ids`.
    let rewritten = backfill_stream_ids(&conn)?;
    if rewritten > 0 {
        tracing::info!(rows = rewritten, "rewrote stored stream URLs as stream ids");
        // **And then rewrite the file.** Blanking a column does not remove the bytes
        // from the database: SQLite leaves the old row in a freed page, and a freed page
        // keeps its contents until something reuses it. Measured on the real library
        // after the rewrite: 34,702 raw occurrences of the password still in the file
        // while every one of the three columns was clean (R-126).
        //
        // The whole point of this work is that `library.db` can be handed to somebody,
        // so a file that still has the password in its unallocated pages is not fixed.
        // `VACUUM` rebuilds it. Once, on the first launch after an upgrade, which is the
        // only time anything is rewritten; it needs temporary space about the size of
        // the library, and a failure is logged rather than fatal — the columns are
        // already clean, and refusing to start would be a worse answer than a file that
        // needs one more launch.
        match conn.execute_batch("VACUUM") {
            Ok(()) => tracing::info!("library compacted, so the old rows are gone from the file"),
            Err(e) => tracing::error!(
                "the library could not be compacted, so the previous stream URLs may                  still be recoverable from unused pages of the file: {e}"
            ),
        }
    }
    // A library that predates `analyze` being called at all — every library that
    // already exists — gets its statistics here, once. See `analyze`.
    if !has_statistics(&conn)? {
        analyze(&conn)?;
    }
    Ok(conn)
}

/// In-memory database, for tests.
pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate::run(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    // journal_mode returns a row, so it needs query_row rather than execute.
    let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -65536;", // 64 MiB page cache
    )?;
    Ok(())
}

/// Give the query planner the statistics it needs to choose between indexes.
///
/// **Without this the library is unusable at real scale, and nothing about the schema
/// or the queries looks wrong.** SQLite has no `sqlite_stat1` until `ANALYZE` has been
/// run, and with no statistics it guesses index selectivity from a fixed table of
/// assumptions. Those guesses were catastrophically wrong here.
///
/// The browse queries hide duplicates with
/// `NOT EXISTS (SELECT 1 FROM movies dup WHERE dup.match_key = movies.match_key AND …
/// (dup.quality_rank > … OR (dup.quality_rank = … AND dup.id < …)))`, and there is an
/// index built precisely for it: `idx_movies_dupe(match_key, quality_rank DESC, id)`.
/// Un-analysed, SQLite resolved the tie-break branch through `idx_movies_lang`
/// instead — `lang_code = 'en' AND rowid < ?`, which on an English-only filter means
/// scanning most of the English half of the table *once per candidate row*.
///
/// Measured on a real 117,510-film subscription:
///
/// | first page of Movies                    | time    |
/// |-----------------------------------------|---------|
/// | without duplicate hiding                |  0.01 s |
/// | with duplicate hiding, un-analysed      | 85.01 s |
/// | with duplicate hiding, after `ANALYZE`  |  0.04 s |
///
/// The heading's count query is the same shape and cost, so opening Movies was ~170
/// seconds of CPU for a page that renders in 40 ms. What a viewer saw was a blank grid
/// for over a minute, with no spinner, no error, and nothing in the log — because
/// nothing had failed.
///
/// `ANALYZE` itself takes about 0.1 s on that library. It is not a tuning knob; it is
/// the difference between the indexes being used and being ignored.
pub fn analyze(conn: &Connection) -> Result<()> {
    conn.execute_batch("ANALYZE")?;
    Ok(())
}

/// How a stream address should be stored for this provider: `(url, stream_id, container)`.
///
/// For a provider whose addresses can be rebuilt -- an Xtream panel, whose base address
/// is on the provider row and whose password is in Credential Manager -- the URL is
/// *not* stored: the credentials are in its path. For anything else, and for a URL this
/// does not recognise as a stream address, the URL is kept as it is.
///
/// Used at every write rather than only by `backfill_stream_ids`, because a pass that
/// cleans up afterwards leaves a window: the episode sweep runs in the background after
/// a refresh has finished, and on a real subscription it writes 42,493 rows -- every one
/// of which held the password until the next launch when the only conversion was
/// post-hoc (R-126).
pub fn stored_stream(rebuild: bool, url: &str) -> (String, Option<String>, Option<String>) {
    if !rebuild {
        return (url.to_string(), None, None);
    }
    match aurora_core::catchup::split_xtream_stream_ref(url) {
        Some((stream_id, container)) => (String::new(), Some(stream_id), container),
        // Not a stream address at all. An Xtream provider can still carry one of these —
        // a `get.php` playlist row, say — and it is kept rather than lost.
        None => (url.to_string(), None, None),
    }
}

/// Whether this provider's stream addresses are rebuilt rather than stored.
pub fn provider_rebuilds_urls(conn: &Connection, provider_id: i64) -> Result<bool> {
    let kind: Option<String> = conn
        .query_row(
            "SELECT kind FROM providers WHERE id = ?1",
            rusqlite::params![provider_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(kind.as_deref() == Some("xtream"))
}

/// Replace every stored Xtream stream URL with the id it can be rebuilt from.
///
/// An Xtream URL carries the credentials in its path, so storing it stored the viewer's
/// password -- 6,699 channel sources, 27,494 films and one row per episode of 8,086
/// series on the subscription this is measured against, several hundred thousand
/// plaintext copies in one file. `docs/PRIVACY.md` promised the opposite about the very
/// artefact somebody attaches to a bug report (R-126).
///
/// Run from `open` rather than as a migration because deciding whether a URL *is* an
/// Xtream stream address means parsing it, and SQL is the wrong tool for that. It is
/// idempotent by its own query: only rows that still hold a URL and have no `stream_id`
/// are considered, so the pass after the first finds nothing and costs one indexless
/// scan of three tables at startup.
///
/// Rows whose URL does not parse are left exactly as they are. That is an M3U provider,
/// whose addresses are whatever the playlist said and cannot be rebuilt from an id --
/// the honest half of this, and what the privacy note now says out loud.
///
/// Returns how many rows were rewritten, for the log line.
pub fn backfill_stream_ids(conn: &Connection) -> Result<usize> {
    // Only an Xtream provider's rows. An M3U provider's URLs are whatever the playlist
    // said -- including, very often, Xtream stream URLs, since that is what a panel's
    // `get.php` serves -- but there is no saved password to rebuild them from: an M3U
    // provider has no `credential_ref`, because its URL carries its own credentials. So
    // those rows keep theirs and `docs/PRIVACY.md` says so, which is the honest half of
    // this (R-126).
    let queries = [
        (
            "channel_sources",
            "SELECT s.id, s.url FROM channel_sources s
             JOIN channels c ON c.id = s.channel_id
             JOIN providers p ON p.id = c.provider_id
             WHERE s.stream_id IS NULL AND s.url <> '' AND p.kind = 'xtream'",
        ),
        (
            "movies",
            "SELECT m.id, m.url FROM movies m
             JOIN providers p ON p.id = m.provider_id
             WHERE m.stream_id IS NULL AND m.url <> '' AND p.kind = 'xtream'",
        ),
        (
            "episodes",
            "SELECT e.id, e.url FROM episodes e
             JOIN series r ON r.id = e.series_id
             JOIN providers p ON p.id = r.provider_id
             WHERE e.stream_id IS NULL AND e.url <> '' AND p.kind = 'xtream'",
        ),
    ];

    let mut rewritten = 0usize;
    for (table, select) in queries {
        let rows: Vec<(i64, String)> = {
            let mut stmt = conn.prepare(select)?;
            let found = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            found.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let sql =
            format!("UPDATE {table} SET stream_id = ?2, container = ?3, url = '' WHERE id = ?1");
        let mut update = conn.prepare(&sql)?;
        for (id, url) in rows {
            let Some((stream_id, container)) = aurora_core::catchup::split_xtream_stream_ref(&url)
            else {
                continue;
            };
            update.execute(rusqlite::params![id, stream_id, container])?;
            rewritten += 1;
        }
    }
    Ok(rewritten)
}

/// Whether `ANALYZE` has ever run on this database.
fn has_statistics(conn: &Connection) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'sqlite_stat1'",
        [],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// README §5: "automatic integrity check + repair on startup".
pub fn integrity_check(conn: &Connection) -> Result<bool> {
    let result: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    Ok(result == "ok")
}

/// What [`open_or_recover`] had to do to hand back a working database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    /// The file on disk was fine.
    Intact,
    /// It was not, so it was moved aside and a new one started. The library re-imports
    /// from the provider; the old file is kept in case anyone wants to look at it.
    Replaced { corrupt_copy: std::path::PathBuf },
}

/// Open the library, or start a new one if the file on disk cannot be used.
///
/// `open` alone propagates a corrupt file out of `Services::new`, which Tauri turns
/// into a start-up failure: no window, no dialog, and no console in a release build to
/// print to. The app simply did not launch and said nothing about why. A failed
/// `integrity_check` was worse — logged at error level and then used anyway.
///
/// The old file is renamed rather than deleted. It holds favourites, watch progress and
/// recordings metadata that somebody might want recovered, and deleting the one copy of
/// that to make an error message go away is not a trade this should make on its own.
pub fn open_or_recover(path: impl AsRef<Path>) -> Result<(Connection, Opened)> {
    let path = path.as_ref();

    let failure = match open(path) {
        Ok(conn) => match integrity_check(&conn) {
            Ok(true) => return Ok((conn, Opened::Intact)),
            Ok(false) => "failed its integrity check".to_string(),
            Err(e) => format!("could not be checked: {e}"),
        },
        Err(e) => e.to_string(),
    };

    // A database this build is too old for is a different problem, and replacing it
    // would silently throw away a newer library. Refuse instead, so the message says
    // to update rather than pretending nothing was there.
    if failure.contains("newer version of Aurora") || failure.contains("schema") {
        tracing::error!("the library database {failure}");
        return open(path).map(|c| (c, Opened::Intact));
    }

    tracing::error!("the library database {failure}; starting a new one");
    let corrupt_copy = aside(path);
    // Rename what is there, WAL sidecars included: leaving `-wal` and `-shm` behind
    // would have SQLite try to replay them into the fresh file.
    for suffix in ["", "-wal", "-shm"] {
        let from = with_suffix(path, suffix);
        if from.exists() {
            let _ = std::fs::rename(&from, with_suffix(&corrupt_copy, suffix));
        }
    }

    let conn = open(path)?;
    Ok((conn, Opened::Replaced { corrupt_copy }))
}

/// `library.db` → `library.corrupt-1760000000.db`, and never over something that is
/// already there.
fn aside(path: &Path) -> std::path::PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "library".to_string());
    let extension = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new("."));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    for n in 0..1000 {
        let name = if n == 0 {
            format!("{stem}.corrupt-{stamp}{extension}")
        } else {
            format!("{stem}.corrupt-{stamp}-{n}{extension}")
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}.corrupt{extension}"))
}

fn with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    if suffix.is_empty() {
        return path.to_path_buf();
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    std::path::PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aurora-db-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_healthy_database_is_opened_as_it_is() {
        let dir = scratch("intact");
        let path = dir.join("library.db");
        {
            let conn = open(&path).unwrap();
            conn.execute(
                "INSERT INTO providers (name, kind, base_url, created_at)
                 VALUES ('P','m3u','https://example.com',0)",
                [],
            )
            .unwrap();
        }

        let (conn, how) = open_or_recover(&path).unwrap();
        assert_eq!(how, Opened::Intact);
        let n: i64 = conn
            .query_row("SELECT count(*) FROM providers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "an intact library must not be replaced");
    }

    /// The password is not in the library after an import, and the row still plays.
    ///
    /// An Xtream stream URL carries the credentials in its path, and those strings were
    /// stored verbatim in three tables: 6,699 channel sources, 27,494 films and one row
    /// per episode of 8,086 series on the subscription this is measured against, which
    /// is several hundred thousand plaintext copies of a username and password in one
    /// SQLite file. `docs/PRIVACY.md` promised the opposite about the one artefact
    /// somebody attaches to a bug report (R-126).
    #[test]
    fn an_xtream_import_leaves_no_credentials_in_the_library() {
        let conn = open_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO providers (id,name,kind,base_url,username,credential_ref,created_at)
                 VALUES (1,'Panel','xtream','http://panel.example:8080','bob','ref',0);
             INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
                 VALUES (1,1,'c1','CNN','cnn',0);
             INSERT INTO channel_sources (channel_id,url)
                 VALUES (1,'http://panel.example:8080/live/bob/hunter2/1234.ts');
             INSERT INTO movies (id,provider_id,provider_key,title,match_key,url,last_seen_at)
                 VALUES (1,1,'m1','Heat','heat',
                         'http://panel.example:8080/movie/bob/hunter2/555.mkv',0);
             INSERT INTO series (id,provider_id,provider_key,title,match_key,last_seen_at)
                 VALUES (1,1,'s1','The Wire','the wire',0);
             INSERT INTO episodes (id,series_id,season,episode,url)
                 VALUES (1,1,1,1,'http://panel.example:8080/series/bob/hunter2/999.mp4');",
        )
        .unwrap();

        let rewritten = backfill_stream_ids(&conn).unwrap();
        assert_eq!(rewritten, 3, "every stored URL should have been rewritten");

        // Nothing anywhere in the three tables mentions the password or the username.
        for (table, column) in [
            ("channel_sources", "url"),
            ("movies", "url"),
            ("episodes", "url"),
        ] {
            let leaked: i64 = conn
                .query_row(
                    &format!(
                        "SELECT count(*) FROM {table}
                          WHERE {column} LIKE '%hunter2%' OR {column} LIKE '%bob%'"
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(leaked, 0, "{table}.{column} still carries the credentials");
        }

        // And what is kept is enough to rebuild the address: the id and the container.
        let (id, container): (String, Option<String>) = conn
            .query_row(
                "SELECT stream_id, container FROM movies WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(id, "555");
        assert_eq!(container.as_deref(), Some("mkv"));

        let (live_id, live_container): (String, Option<String>) = conn
            .query_row(
                "SELECT stream_id, container FROM channel_sources WHERE channel_id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(live_id, "1234");
        assert_eq!(live_container.as_deref(), Some("ts"));

        // Twice changes nothing: the pass runs at every launch.
        assert_eq!(backfill_stream_ids(&conn).unwrap(), 0);
    }

    /// Upgrading a library does not leave the password in the file.
    ///
    /// The half a column check cannot see. Blanking `url` does not remove the old row
    /// from a SQLite database: it stays in a freed page, and a freed page keeps its
    /// contents until something reuses it. Measured on the real 257 MB library after the
    /// first version of this fix: all three columns clean, and **34,702 raw occurrences
    /// of the password still in the file**. Since the whole point is that `library.db`
    /// can be handed to somebody, that was a false pass (R-126).
    ///
    /// On a real file rather than in memory, because this is a question about bytes on
    /// disk.
    #[test]
    fn upgrading_a_library_removes_the_password_from_the_file() {
        let dir = scratch("vacuum");
        let path = dir.join("library.db");

        // A library as an older build left it: migration 11, URLs with the password in
        // them, and no `stream_id` column at all.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            crate::configure(&conn).unwrap();
            for m in crate::schema::MIGRATIONS.iter().filter(|m| m.version <= 11) {
                conn.execute_batch(m.sql).unwrap();
            }
            conn.execute_batch("PRAGMA user_version = 11").unwrap();
            conn.execute_batch(
                "INSERT INTO providers (id,name,kind,base_url,username,credential_ref,created_at)
                     VALUES (1,'Panel','xtream','http://panel.example:8080','bob','ref',0);
                 INSERT INTO series (id,provider_id,provider_key,title,match_key,last_seen_at)
                     VALUES (1,1,'s1','Show','show',0);",
            )
            .unwrap();
            // Enough rows that the old ones cannot all sit in the same page as the new.
            let tx = conn.unchecked_transaction().unwrap();
            for i in 0..400 {
                tx.execute(
                    "INSERT INTO movies (provider_id,provider_key,title,match_key,url,last_seen_at)
                     VALUES (1,?1,?2,'m',?3,0)",
                    rusqlite::params![
                        format!("k{i}"),
                        format!("Film {i}"),
                        format!("http://panel.example:8080/movie/bob/hunter2/{i}.mkv")
                    ],
                )
                .unwrap();
                tx.execute(
                    "INSERT INTO episodes (series_id,season,episode,url)
                     VALUES (1,1,?1,?2)",
                    rusqlite::params![
                        i + 1,
                        format!("http://panel.example:8080/series/bob/hunter2/{i}.mp4")
                    ],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }

        let before = std::fs::read(&path).unwrap();
        assert!(
            count_bytes(&before, b"hunter2") > 100,
            "the fixture did not write the password often enough to be a real check"
        );

        // What a viewer's first launch on the new build does.
        let conn = open(&path).unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT count(*) FROM movies WHERE instr(url, 'hunter2') > 0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "the columns should be clean");
        drop(conn);

        let after = std::fs::read(&path).unwrap();
        assert_eq!(
            count_bytes(&after, b"hunter2"),
            0,
            "the password survives in the file's unused pages — VACUUM is what removes it"
        );
        // The username stays: it is on the `providers` row, which the app has to show.
        assert!(count_bytes(&after, b"bob") > 0);
    }

    fn count_bytes(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .filter(|w| *w == needle)
            .count()
    }

    /// An M3U provider keeps its URLs, because nothing could rebuild them.
    ///
    /// Its addresses are whatever the playlist said — including, very often, Xtream
    /// stream URLs, since that is what a panel's `get.php` serves — and there is no
    /// saved password to rebuild them from: an M3U provider has no `credential_ref`,
    /// because its URL carries its own credentials. `docs/PRIVACY.md` says so rather
    /// than implying the library is clean for everybody (R-126).
    #[test]
    fn an_m3u_providers_urls_are_left_alone() {
        let conn = open_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
                 VALUES (1,'List','m3u','http://host/get.php?username=bob',0);
             INSERT INTO movies (id,provider_id,provider_key,title,match_key,url,last_seen_at)
                 VALUES (1,1,'m1','Heat','heat','http://host/movie/bob/hunter2/555.mkv',0);",
        )
        .unwrap();

        assert_eq!(backfill_stream_ids(&conn).unwrap(), 0);
        let url: String = conn
            .query_row("SELECT url FROM movies WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(url, "http://host/movie/bob/hunter2/555.mkv");
    }

    /// The bug: a corrupt file propagated out of `Services::new`, Tauri turned that
    /// into a start-up failure, and a release build has no console to say so in. The
    /// app did not launch and gave no reason.
    #[test]
    fn a_corrupt_database_is_moved_aside_and_replaced() {
        let dir = scratch("corrupt");
        let path = dir.join("library.db");
        std::fs::write(&path, b"this is not a database at all, it is just bytes").unwrap();

        let (conn, how) = open_or_recover(&path).unwrap();
        let Opened::Replaced { corrupt_copy } = how else {
            panic!("a corrupt file must be replaced, not used");
        };

        // The new one works…
        assert_eq!(
            migrate::current_version(&conn).unwrap(),
            schema::LATEST_VERSION
        );
        assert!(integrity_check(&conn).unwrap());

        // …and the old one is kept rather than deleted.
        assert!(corrupt_copy.exists(), "{corrupt_copy:?}");
        assert_eq!(
            std::fs::read(&corrupt_copy).unwrap(),
            b"this is not a database at all, it is just bytes"
        );
    }

    #[test]
    fn a_truncated_header_is_recovered_too() {
        let dir = scratch("truncated");
        let path = dir.join("library.db");
        {
            let _ = open(&path).unwrap();
        }
        // Scribble over the page that holds the schema.
        let mut bytes = std::fs::read(&path).unwrap();
        for b in bytes.iter_mut().take(200) {
            *b = 0x41;
        }
        std::fs::write(&path, &bytes).unwrap();

        let (conn, how) = open_or_recover(&path).unwrap();
        assert!(matches!(how, Opened::Replaced { .. }));
        assert!(integrity_check(&conn).unwrap());
    }

    /// A stale `-wal` beside a replaced database is not a leftover, it is a hazard:
    /// SQLite would try to replay it into the fresh file.
    #[test]
    fn no_stale_sidecar_is_left_beside_the_new_database() {
        let dir = scratch("wal");
        let path = dir.join("library.db");
        std::fs::write(&path, b"not a database").unwrap();
        std::fs::write(dir.join("library.db-wal"), b"stale wal").unwrap();
        std::fs::write(dir.join("library.db-shm"), b"stale shm").unwrap();

        let (conn, how) = open_or_recover(&path).unwrap();
        assert!(matches!(how, Opened::Replaced { .. }));
        assert!(integrity_check(&conn).unwrap());
        drop(conn);

        for suffix in ["-wal", "-shm"] {
            let beside = with_suffix(&path, suffix);
            if beside.exists() {
                let bytes = std::fs::read(&beside).unwrap();
                assert!(
                    bytes != b"stale wal" && bytes != b"stale shm",
                    "the old {suffix} was left beside the new database"
                );
            }
        }
    }

    #[test]
    fn two_bad_launches_do_not_overwrite_the_first_copy() {
        let dir = scratch("twice");
        let path = dir.join("library.db");

        // Dropped between launches: the app opens one connection at startup, and a
        // live one holds a WAL the next open would recover the database from.
        std::fs::write(&path, b"first failure").unwrap();
        let (c, first) = open_or_recover(&path).unwrap();
        drop(c);
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(with_suffix(&path, suffix));
        }

        std::fs::write(&path, b"second failure").unwrap();
        let (c, second) = open_or_recover(&path).unwrap();
        drop(c);

        let (Opened::Replaced { corrupt_copy: a }, Opened::Replaced { corrupt_copy: b }) =
            (first, second)
        else {
            panic!("expected two replacements");
        };
        assert_ne!(a, b);
        assert_eq!(std::fs::read(&a).unwrap(), b"first failure");
        assert_eq!(std::fs::read(&b).unwrap(), b"second failure");
    }
}
