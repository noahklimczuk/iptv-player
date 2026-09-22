//! Point Aurora's real ingestion code at a real provider and report what it finds.
//!
//! The parsers in `aurora-core` are written from the Xtream spec and tested against a
//! local server. Every panel is a fork of a fork, so the first real subscription is
//! where the assumptions get tested. This runs the shipping code paths — the same
//! client, the same models, the same title cleaning — and prints what came back, so a
//! disagreement shows up as a number that looks wrong rather than as a silently empty
//! library.
//!
//! ```text
//! AURORA_BASE=http://your-panel.example \
//! AURORA_USER=yourname \
//! AURORA_PASS=yourpassword \
//!   cargo run -p aurora-ingest --example probe
//! ```
//!
//! It only reads. Nothing is written to a database and nothing is downloaded. The
//! password is taken from the environment, is never printed, and every URL that reaches
//! the output goes through `http::redact` first.

use std::collections::BTreeMap;

use aurora_core::title;
use aurora_ingest::http::{redact, HttpClient, HttpConfig};
use aurora_ingest::xtream::XtreamClient;

fn main() {
    let (base, user, pass) = match (
        std::env::var("AURORA_BASE"),
        std::env::var("AURORA_USER"),
        std::env::var("AURORA_PASS"),
    ) {
        (Ok(b), Ok(u), Ok(p)) if !b.is_empty() && !u.is_empty() && !p.is_empty() => (b, u, p),
        _ => {
            eprintln!(
                "Set AURORA_BASE, AURORA_USER and AURORA_PASS.\n\n\
                 AURORA_BASE is the panel root, e.g. http://12345678.panel-host.example\n\
                 (no /get.php, no query string).\n\n\
                 Nothing is written and nothing is downloaded; this only reads."
            );
            std::process::exit(2);
        }
    };
    let base = base.trim().trim_end_matches('/').to_string();

    let http = match HttpClient::new(HttpConfig::default()) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Could not build an HTTP client: {}", e.message);
            std::process::exit(1);
        }
    };
    let client = XtreamClient::new(&http, &base, &user, &pass);

    println!("Probing {}\n", redact(&base));

    // ── Authentication ──────────────────────────────────────────────────────
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let account = match client.authenticate(now) {
        Ok(a) => a,
        Err(e) => {
            println!("AUTH FAILED\n  {}\n  {}", e.message, e.cause);
            println!("\nThis is what the first-run wizard would show. If the message does");
            println!("not match reality, the taxonomy in aurora_core::neterr needs a case.");
            std::process::exit(1);
        }
    };

    println!("Account");
    println!("  active            {}", account.active);
    println!("  trial             {}", account.is_trial);
    match account.days_until_expiry {
        Some(d) => println!("  expires in        {d} days"),
        None => println!("  expires in        (not reported)"),
    }
    match (account.active_connections, account.max_connections) {
        (Some(a), Some(m)) => println!("  connections       {a} of {m}"),
        (_, Some(m)) => println!("  connections       max {m}"),
        _ => println!("  connections       (not reported)"),
    }
    if account.max_connections.is_none() {
        println!("  ! no connection limit reported — the DVR will fall back to 2");
    }
    println!();

    // ── Catalogue ───────────────────────────────────────────────────────────
    let live = report("live streams", client.live_streams());
    let vod = report("VOD streams", client.vod_streams());
    let series = report("series", client.series());
    report("live categories", client.live_categories());
    report("VOD categories", client.vod_categories());
    report("series categories", client.series_categories());
    println!();

    // ── What the parsers make of it ─────────────────────────────────────────
    if let Some(live) = &live {
        let named = live.iter().filter(|s| s.name.is_some()).count();
        let with_epg = live
            .iter()
            .filter(|s| s.epg_channel_id.as_deref().is_some_and(|e| !e.is_empty()))
            .count();
        let with_id = live.iter().filter(|s| s.stream_id.is_some()).count();

        println!("Live channels");
        println!("  usable name       {named} of {}", live.len());
        println!("  stream id         {with_id} of {}", live.len());
        println!(
            "  EPG id            {with_epg} of {} ({}%)",
            live.len(),
            pct(with_epg, live.len())
        );
        if with_id < live.len() {
            println!("  ! channels without a stream id cannot be played or refreshed stably");
        }
        if pct(with_epg, live.len()) < 50 {
            println!("  ! low EPG coverage — the guide will be mostly empty");
        }

        // What the quality/country heuristics actually pull out of real names.
        let mut qualities: BTreeMap<String, usize> = BTreeMap::new();
        let mut countries: BTreeMap<String, usize> = BTreeMap::new();
        for s in live.iter().filter_map(|s| s.name.as_deref()) {
            *qualities
                .entry(title::detect_quality(s).unwrap_or_else(|| "(none)".into()))
                .or_default() += 1;
            let (_, country) = title::split_country_prefix(s);
            *countries
                .entry(country.unwrap_or_else(|| "(none)".into()))
                .or_default() += 1;
        }
        println!("  quality tags      {}", top(&qualities, 6));
        println!("  country prefixes  {}", top(&countries, 6));
        println!("  samples:");
        for s in live.iter().filter_map(|s| s.name.as_deref()).take(5) {
            println!("    {s}");
        }
        println!();
    }

    if let Some(vod) = &vod {
        let mut with_year = 0;
        let mut changed = 0;
        for name in vod.iter().filter_map(|v| v.name.as_deref()) {
            let clean = title::clean_movie_title(name);
            if clean.year.is_some() {
                with_year += 1;
            }
            if clean.title != name {
                changed += 1;
            }
        }
        println!("Movies");
        println!(
            "  year extracted    {with_year} of {} ({}%)",
            vod.len(),
            pct(with_year, vod.len())
        );
        println!("  title cleaned up  {changed} of {}", vod.len());
        if pct(with_year, vod.len()) < 40 {
            println!("  ! few years found — metadata matching will decline more often");
        }
        println!("  samples (raw -> cleaned):");
        for name in vod.iter().filter_map(|v| v.name.as_deref()).take(6) {
            let c = title::clean_movie_title(name);
            println!(
                "    {name}\n      -> {:?} year={:?} quality={:?}",
                c.title, c.year, c.quality
            );
        }
        println!();
    }

    if let Some(series) = &series {
        println!("Series");
        println!("  count             {}", series.len());
        println!("  samples:");
        for s in series.iter().take(5) {
            println!("    {:?}", s.name.as_deref().unwrap_or("(no name)"));
        }
        println!();
    }

    println!("EPG source: {}", redact(&client.xmltv_url()));
    println!("\nRead-only probe finished. Nothing was written or downloaded.");
}

/// Run one endpoint and say how it went, without letting a failure end the probe —
/// knowing which endpoints a panel does not implement is part of what this is for.
fn report<T>(
    what: &str,
    result: Result<Vec<T>, aurora_core::neterr::NetFailure>,
) -> Option<Vec<T>> {
    match result {
        Ok(items) => {
            println!("{what:<20} {}", items.len());
            Some(items)
        }
        Err(e) => {
            println!("{what:<20} FAILED — {}", e.message);
            println!("{:<20} {}", "", e.cause);
            None
        }
    }
}

fn pct(n: usize, total: usize) -> usize {
    (n * 100).checked_div(total).unwrap_or(0)
}

/// The most common values, so a real catalogue's shape is visible at a glance.
fn top(counts: &BTreeMap<String, usize>, limit: usize) -> String {
    let mut pairs: Vec<(&String, &usize)> = counts.iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(a.1));
    pairs
        .iter()
        .take(limit)
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("  ")
}
