//! Import a real subscription into a throwaway database, and report what it cost.
//!
//! `probe` reads and prints; this one runs the import the app runs — the same
//! `sync::fetch` and `sync::apply` the host calls when somebody adds a provider — and
//! writes a real SQLite file. It is the only way to find out what 22,000 channels and
//! 122,000 films do to import time, memory and the reconciliation logic, none of which
//! a local test server can tell you (docs/ROADMAP.md, "Nearest useful next steps").
//!
//! ```text
//! AURORA_BASE=http://your-panel.example \
//! AURORA_USER=yourname \
//! AURORA_PASS=yourpassword \
//!   cargo run --release -p aurora-ingest --example import
//! ```
//!
//! It writes to a temporary directory and prints the path. The password comes from the
//! environment and is never printed; every URL goes through `http::redact`.

use std::time::Instant;

use aurora_core::rules::RuleSet;
use aurora_ingest::http::{redact, HttpClient, HttpConfig};
use aurora_ingest::source::SourceKind;
use aurora_ingest::sync::{self, Phase, SyncOptions};

fn main() {
    let (base, user, pass) = match (
        std::env::var("AURORA_BASE"),
        std::env::var("AURORA_USER"),
        std::env::var("AURORA_PASS"),
    ) {
        (Ok(b), Ok(u), Ok(p)) if !b.is_empty() && !u.is_empty() && !p.is_empty() => (b, u, p),
        _ => {
            eprintln!("Set AURORA_BASE, AURORA_USER and AURORA_PASS.");
            std::process::exit(2);
        }
    };
    let base = base.trim().trim_end_matches('/').to_string();

    let dir = std::env::temp_dir().join(format!("aurora-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join("library.db");
    println!("Importing {} into {}\n", redact(&base), db_path.display());

    let mut db = aurora_db::open(&db_path).expect("open database");
    db.execute(
        "INSERT INTO providers (id,name,kind,base_url,username,created_at)
         VALUES (1,'Real panel','xtream',?1,?2,0)",
        aurora_db::rusqlite::params![base, user],
    )
    .expect("provider row");
    db.execute(
        "INSERT OR IGNORE INTO profiles (id,name,created_at) VALUES (1,'Me',0)",
        [],
    )
    .expect("profile row");

    let http = HttpClient::new(HttpConfig::default()).expect("http client");
    let mut options = SyncOptions::new(
        1,
        SourceKind::Xtream {
            base_url: base.clone(),
            username: user,
        },
        now_unix(),
    );
    options.password = Some(pass);
    let rules = RuleSet::compile(&[]).expect("rules");

    // Fetch and apply separately, because they cost very different things: one is the
    // provider's network and the other is SQLite, and an import that feels slow is
    // worth blaming on the right half.
    let mut last_phase = None;
    let started = Instant::now();
    let fetched = match sync::fetch(&http, &options, &rules, |p| {
        if last_phase != Some(p.phase) {
            println!(
                "  [{:>6.1}s] {:?}",
                started.elapsed().as_secs_f64(),
                p.phase
            );
            last_phase = Some(p.phase);
        }
    }) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("\nFETCH FAILED: {}\n  {}", e.message, e.cause);
            std::process::exit(1);
        }
    };
    let fetch_secs = started.elapsed().as_secs_f64();
    println!("\nfetch      {fetch_secs:>7.1}s   nothing written yet");
    println!(
        "peak memory {:>6} MB  (the whole download is held between the halves — D20)",
        peak_rss_mb()
    );

    let applying = Instant::now();
    let mut seen_apply = None;
    let report = match sync::apply(&mut db, fetched, &options, |p| {
        if seen_apply != Some(p.phase) && p.phase != Phase::Done {
            println!(
                "  [{:>6.1}s] {:?}",
                applying.elapsed().as_secs_f64(),
                p.phase
            );
            seen_apply = Some(p.phase);
        }
    }) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("\nAPPLY FAILED: {}\n  {}", e.message, e.cause);
            std::process::exit(1);
        }
    };
    let apply_secs = applying.elapsed().as_secs_f64();

    println!("\napply      {apply_secs:>7.1}s");
    println!("total      {:>7.1}s", started.elapsed().as_secs_f64());
    println!("peak memory {:>6} MB", peak_rss_mb());
    println!(
        "database   {:>7.1} MB  {}",
        std::fs::metadata(&db_path)
            .map(|m| m.len() as f64 / 1_048_576.0)
            .unwrap_or(0.0),
        db_path.display()
    );

    println!("\nReport");
    println!("  channels          {}", report.channels);
    println!("  movies            {}", report.movies);
    println!("  series            {}", report.series);
    println!("  episodes          {}", report.episodes);
    println!("  EPG programmes    {}", report.epg_programmes);
    println!("  EPG channels      {}", report.epg_channels);
    println!("  EPG matched       {}", report.epg_matched);
    println!("  stopped listing   {}", report.channels_missing);
    if !report.warnings.is_empty() {
        println!("  warnings          {}", report.warnings.len());
        for w in report.warnings.iter().take(10) {
            println!("    {w}");
        }
    }

    println!("\nWhat landed in the database");
    for (label, sql) in [
        ("channels", "SELECT COUNT(*) FROM channels"),
        ("  with an EPG id", "SELECT COUNT(*) FROM channels WHERE epg_channel_id IS NOT NULL AND epg_channel_id <> ''"),
        ("  with a country", "SELECT COUNT(*) FROM channels WHERE country IS NOT NULL AND country <> ''"),
        ("  with a language", "SELECT COUNT(*) FROM channels WHERE language IS NOT NULL AND language <> ''"),
        ("  distinct match keys", "SELECT COUNT(DISTINCT match_key) FROM channels"),
        ("  hidden by rules", "SELECT COUNT(*) FROM channels WHERE hidden = 1"),
        ("channel sources", "SELECT COUNT(*) FROM channel_sources"),
        ("movies", "SELECT COUNT(*) FROM movies"),
        ("  with a year", "SELECT COUNT(*) FROM movies WHERE year IS NOT NULL"),
        ("series", "SELECT COUNT(*) FROM series"),
        ("episodes", "SELECT COUNT(*) FROM episodes"),
        ("epg programmes", "SELECT COUNT(*) FROM epg_programmes"),
        ("  matched to a channel", "SELECT COUNT(DISTINCT channel_id) FROM epg_programmes"),
    ] {
        let n: i64 = db.query_row(sql, [], |r| r.get(0)).unwrap_or(-1);
        println!("  {label:<24} {n}");
    }

    println!("\nA few channels as stored");
    {
        let mut stmt = db
            .prepare(
                "SELECT name, COALESCE(country,'—'), COALESCE(quality,'—'), match_key
             FROM channels LIMIT 6",
            )
            .expect("query");
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .expect("rows");
        for row in rows.flatten() {
            println!(
                "  {:<34} country={:<4} quality={:<5} key={}",
                row.0, row.1, row.2, row.3
            );
        }
    }

    // Classification is a separate pass the host runs at startup, not part of import,
    // so an example that stops here would report every channel as unclassified and
    // look like a bug in the filters.
    match aurora_db::repo::filtering::reclassify_if_stale(&mut db) {
        Ok(n) => println!("\nclassified {n} rows for the filters (the host does this at startup)"),
        Err(e) => println!("\nclassification failed: {e}"),
    }
    for (label, sql) in [
        (
            "channels with a country",
            "SELECT COUNT(*) FROM channels WHERE country IS NOT NULL AND country <> ''",
        ),
        (
            "channels with a language",
            "SELECT COUNT(*) FROM channels WHERE language IS NOT NULL AND language <> ''",
        ),
    ] {
        let n: i64 = db.query_row(sql, [], |r| r.get(0)).unwrap_or(-1);
        println!("  {label:<26} {n}");
    }

    println!("\nSearch, against what was just written");
    for term in ["news", "bbc", "inception"] {
        let started = Instant::now();
        let hits = aurora_db::repo::search::query(&db, term, 5)
            .map(|h| h.len())
            .unwrap_or(0);
        println!(
            "  {:<12} {hits} hits in {:.0} ms",
            term,
            started.elapsed().as_secs_f64() * 1000.0
        );
    }

    println!(
        "\nLeaving the database at {} — delete it when you are done.",
        db_path.display()
    );
}

fn peak_rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1).map(|k| k.to_string()))
        })
        .and_then(|kb| kb.parse::<u64>().ok())
        .map(|kb| kb / 1024)
        .unwrap_or(0)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
