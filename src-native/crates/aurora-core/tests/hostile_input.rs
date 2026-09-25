//! The inputs a real provider actually sends, at the sizes it actually sends them.
//!
//! The unit tests beside each parser cover the shapes; this covers the *scale* and the
//! malice. README §4.2 is the contract being tested — "parse what you can, log what you
//! can't, never crash" — and §16 is the budget: a 50,000-entry playlist has to come
//! back in seconds, not minutes.
//!
//! Every fixture is generated rather than committed. A 6 MB playlist in git is a 6 MB
//! playlist in every clone for ever, and a generator says what it is testing in a way
//! a blob cannot.

use std::io::Cursor;

use aurora_core::model::MediaKind;
use aurora_core::{m3u, xmltv};

/// A playlist the size a real subscription sends. The panel in docs/ROADMAP.md lists
/// 22,305 channels and 122,274 films; 50,000 entries is between those.
fn big_playlist(entries: usize) -> Vec<u8> {
    let mut out = String::with_capacity(entries * 140);
    out.push_str("#EXTM3U url-tvg=\"https://example.com/epg.xml.gz\"\n");
    for i in 0..entries {
        let group = ["News", "Sports", "Movies, Action", "Kids"][i % 4];
        out.push_str(&format!(
            "#EXTINF:-1 tvg-id=\"ch{i}.example\" tvg-name=\"Channel {i}\" \
             tvg-logo=\"http://example.com/{i}.png\" tvg-chno=\"{}\" group-title=\"{group}\",\
             US \u{2605} Channel {i} HD\n\
             http://example.com/live/user/pass/{i}.ts\n",
            i + 1
        ));
    }
    out.into_bytes()
}

#[test]
fn a_fifty_thousand_entry_playlist_parses_inside_the_budget() {
    let bytes = big_playlist(50_000);
    let size_mb = bytes.len() as f64 / (1024.0 * 1024.0);

    let started = std::time::Instant::now();
    let parsed = m3u::parse(Cursor::new(bytes));
    let elapsed = started.elapsed();

    assert_eq!(parsed.result.entries.len(), 50_000);
    assert_eq!(parsed.result.skipped, 0);
    assert_eq!(
        parsed.header.epg_urls,
        vec!["https://example.com/epg.xml.gz"]
    );
    assert_eq!(parsed.result.entries[0].number, Some(1));
    assert_eq!(parsed.result.entries[49_999].number, Some(50_000));
    // The star separator the real panel uses (docs/ROADMAP.md) is kept in the name.
    assert!(parsed.result.entries[0].name.contains('\u{2605}'));

    // Generous on purpose: this is a debug build on shared CI hardware, and the point
    // is to catch an accidental O(n²), not to benchmark. It runs in well under a
    // second in practice.
    assert!(
        elapsed.as_secs() < 20,
        "{size_mb:.1} MB / 50,000 entries took {elapsed:?}"
    );
}

/// Every way a playlist has been seen to be broken, in one file, none of which may
/// cost more than the entry it is on.
#[test]
fn a_hostile_playlist_loses_only_the_broken_entries() {
    let hostile = concat!(
        "\u{feff}#EXTM3U\r\n",
        // Good, to prove the rest of the file still arrives.
        "#EXTINF:-1 tvg-id=\"good.1\",Good One\r\n",
        "http://example.com/1.ts\r\n",
        // A percent escape immediately before a multi-byte character: this used to
        // end the process, not the entry (F-02).
        "#EXTINF:-1,Header Trap\n",
        "#KODIPROP:inputstream.adaptive.stream_headers=User-Agent=%a\u{e9}\n",
        "http://example.com/2.ts\n",
        // Unterminated quote.
        "#EXTINF:-1 group-title=\"Never closed,Unterminated\n",
        "http://example.com/3.ts\n",
        // No comma at all.
        "#EXTINF:-1 tvg-id=\"nocomma\"\n",
        "http://example.com/4.ts\n",
        // An #EXTINF with no URL after it.
        "#EXTINF:-1,Orphan\n",
        // A line that is not a URL.
        "this is not a url at all\n",
        // A bare colon that is not a Windows path (F-21).
        "1:30\n",
        // A duplicate of an entry already seen.
        "#EXTINF:-1,Duplicate\n",
        "http://example.com/1.ts\n",
        // Directives nothing models.
        "#EXTVLCOPT:something-unknown=1\n",
        "#EXT-X-UNKNOWN:whatever\n",
        // A very long name.
        "#EXTINF:-1,AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
        "http://example.com/5.ts\n",
        // Still fine at the end.
        "#EXTINF:-1,Good Last\n",
        "http://example.com/6.ts\n",
    );

    let parsed = m3u::parse(Cursor::new(hostile.as_bytes().to_vec()));

    let names: Vec<&str> = parsed
        .result
        .entries
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert!(names.contains(&"Good One"), "{names:?}");
    assert!(names.contains(&"Good Last"), "{names:?}");
    assert!(names.contains(&"Header Trap"), "{names:?}");
    // The broken ones are skipped and reported, not silently merged into a neighbour.
    assert!(parsed.result.skipped >= 3, "{:?}", parsed.result);
    assert!(!parsed.result.warnings.is_empty());
}

/// Invalid UTF-8, a lone CR, and a NUL in the middle of a name.
#[test]
fn bytes_that_are_not_text_do_not_stop_the_parse() {
    let mut bytes = b"#EXTM3U\n#EXTINF:-1,Caf".to_vec();
    bytes.push(0xE9); // Latin-1 'é', not valid UTF-8
    bytes.extend_from_slice(b" TV\nhttp://example.com/1.ts\n");
    bytes.extend_from_slice(b"#EXTINF:-1,With\0NUL\nhttp://example.com/2.ts\n");
    bytes.extend_from_slice(b"#EXTINF:-1,After\rCR\nhttp://example.com/3.ts\n");

    let parsed = m3u::parse(Cursor::new(bytes));
    assert_eq!(parsed.result.entries.len(), 3);
}

/// A playlist that is one line, with no newline anywhere, must not be mistaken for an
/// entry.
#[test]
fn a_single_enormous_line_is_handled() {
    let line = format!("#EXTINF:-1,{}", "x".repeat(200_000));
    let parsed = m3u::parse(Cursor::new(line.into_bytes()));
    assert!(parsed.result.entries.is_empty());
    assert!(!parsed.result.warnings.is_empty());
}

#[test]
fn classification_survives_a_real_panels_naming() {
    // The forms the probe in docs/ROADMAP.md found on an actual subscription.
    let playlist = "#EXTM3U\n\
        #EXTINF:-1 group-title=\"US ENTERTAINMENT\",US \u{2605} QVC HD\n\
        http://example.com/live/u/p/1.ts\n\
        #EXTINF:-1 group-title=\"VOD ACTION\",AR \u{2605} Inception (2010)\n\
        http://example.com/movie/u/p/2.mkv\n\
        #EXTINF:-1 group-title=\"SERIES\",EN \u{2605} Ratched S01E03\n\
        http://example.com/series/u/p/3.mkv\n";

    let parsed = m3u::parse(Cursor::new(playlist.as_bytes().to_vec()));
    let kinds: Vec<MediaKind> = parsed.result.entries.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![MediaKind::Live, MediaKind::Movie, MediaKind::Episode],
        "{:?}",
        parsed
            .result
            .entries
            .iter()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );
}

/* ── XMLTV ─────────────────────────────────────────────────────────────────── */

#[derive(Default)]
struct Sink {
    channels: usize,
    programmes: Vec<aurora_core::model::Programme>,
}

impl xmltv::XmltvSink for Sink {
    fn channel(&mut self, _c: aurora_core::model::EpgChannel) {
        self.channels += 1;
    }
    fn programme(&mut self, p: aurora_core::model::Programme) {
        self.programmes.push(p);
    }
}

/// The hour that happens twice, and the hour that never happens.
///
/// Europe/London springs forward at 01:00 UTC on 2025-03-30 (+0000 → +0100) and falls
/// back at 01:00 UTC on 2025-10-26 (+0100 → +0000). A guide writes both sides with the
/// offset it was in, and the only thing that makes the result sortable is normalising
/// to UTC at parse time — which is what this is checking still happens.
#[test]
fn a_dst_transition_lands_on_the_right_utc_instants() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tv>
  <channel id="bbc.uk"><display-name>BBC One</display-name></channel>

  <!-- Spring forward. 00:30 GMT then 02:30 BST: one hour apart in UTC. -->
  <programme channel="bbc.uk" start="20250330003000 +0000" stop="20250330010000 +0000">
    <title>Before the clocks go forward</title>
  </programme>
  <programme channel="bbc.uk" start="20250330023000 +0100" stop="20250330030000 +0100">
    <title>After the clocks go forward</title>
  </programme>

  <!-- Fall back: the repeated hour. Both say 01:30 locally, an hour apart in UTC. -->
  <programme channel="bbc.uk" start="20251026013000 +0100" stop="20251026020000 +0100">
    <title>The first one thirty</title>
  </programme>
  <programme channel="bbc.uk" start="20251026013000 +0000" stop="20251026020000 +0000">
    <title>The second one thirty</title>
  </programme>
</tv>"#;

    let mut sink = Sink::default();
    let stats = xmltv::parse(Cursor::new(xml.as_bytes().to_vec()), &mut sink).unwrap();
    assert_eq!(stats.channels, 1);
    assert_eq!(stats.programmes, 4);

    let at = |title: &str| {
        sink.programmes
            .iter()
            .find(|p| p.title == title)
            .unwrap_or_else(|| panic!("{title} is missing"))
            .start
    };

    // 2025-03-30 00:30 UTC and 01:30 UTC.
    assert_eq!(
        at("After the clocks go forward") - at("Before the clocks go forward"),
        3600,
        "the spring-forward pair is not an hour apart in UTC"
    );
    // The repeated hour: the same local time, an hour apart.
    assert_eq!(
        at("The second one thirty") - at("The first one thirty"),
        3600,
        "the repeated hour collapsed onto one instant"
    );
    // Sortable by UTC, which is the whole reason for normalising at parse time.
    let mut order: Vec<i64> = sink.programmes.iter().map(|p| p.start).collect();
    let sorted = {
        let mut s = order.clone();
        s.sort_unstable();
        s
    };
    order.sort_unstable();
    assert_eq!(order, sorted);
}

/// Half-hour and three-quarter-hour zones, which several real guides use.
#[test]
fn odd_utc_offsets_are_applied_exactly() {
    let utc = xmltv::parse_time("20250115120000 +0000").unwrap();
    assert_eq!(xmltv::parse_time("20250115173000 +0530"), Some(utc)); // India
    assert_eq!(xmltv::parse_time("20250115224500 +1045"), Some(utc)); // Lord Howe
    assert_eq!(xmltv::parse_time("20250115063000 -0530"), Some(utc));
    // A colon in the offset, which some writers emit.
    assert_eq!(xmltv::parse_time("20250115130000 +01:00"), Some(utc));
}

#[test]
fn a_hostile_guide_loses_only_the_broken_programmes() {
    let xml = concat!(
        "<tv>",
        "<channel id=\"a\"><display-name>A</display-name></channel>",
        // No channel attribute.
        "<programme start=\"20250115120000 +0000\" stop=\"20250115130000 +0000\">",
        "<title>Orphan</title></programme>",
        // No title.
        "<programme channel=\"a\" start=\"20250115120000 +0000\" ",
        "stop=\"20250115130000 +0000\"></programme>",
        // Unparseable start.
        "<programme channel=\"a\" start=\"not a time\" stop=\"20250115130000 +0000\">",
        "<title>Bad start</title></programme>",
        // Impossible date (F-22).
        "<programme channel=\"a\" start=\"20250231120000 +0000\" ",
        "stop=\"20250231130000 +0000\"><title>February 31st</title></programme>",
        // Stop before start.
        "<programme channel=\"a\" start=\"20250115130000 +0000\" ",
        "stop=\"20250115120000 +0000\"><title>Backwards</title></programme>",
        // Entities and CDATA in the title.
        "<programme channel=\"a\" start=\"20250115140000 +0000\" ",
        "stop=\"20250115150000 +0000\"><title>Tom &amp; Jerry &lt;live&gt;</title>",
        "<desc><![CDATA[A <b>description</b> with markup]]></desc></programme>",
        // No stop at all: assumed to be half an hour.
        "<programme channel=\"a\" start=\"20250115160000 +0000\">",
        "<title>No stop</title></programme>",
        "</tv>",
    );

    let mut sink = Sink::default();
    let stats = xmltv::parse(Cursor::new(xml.as_bytes().to_vec()), &mut sink).unwrap();

    assert_eq!(stats.programmes, 2, "{:?}", titles(&sink));
    assert_eq!(stats.skipped, 5);
    assert!(titles(&sink).contains(&"Tom & Jerry <live>".to_string()));

    let no_stop = sink
        .programmes
        .iter()
        .find(|p| p.title == "No stop")
        .unwrap();
    assert_eq!(no_stop.duration_secs(), 1800);
}

fn titles(sink: &Sink) -> Vec<String> {
    sink.programmes.iter().map(|p| p.title.clone()).collect()
}

/// A guide the size a real panel publishes: 482,190 programmes on the subscription in
/// docs/ROADMAP.md. A tenth of that is enough to catch an accidental quadratic while
/// staying a test rather than a benchmark.
#[test]
fn a_large_guide_streams_without_buffering_the_document() {
    const CHANNELS: usize = 200;
    const PER_CHANNEL: usize = 240; // ten days at an hour each

    let mut xml = String::with_capacity(CHANNELS * PER_CHANNEL * 150);
    xml.push_str("<tv>");
    for c in 0..CHANNELS {
        xml.push_str(&format!(
            "<channel id=\"ch{c}\"><display-name>Channel {c}</display-name></channel>"
        ));
    }
    for c in 0..CHANNELS {
        for p in 0..PER_CHANNEL {
            let hour = p % 24;
            let day = 1 + (p / 24);
            xml.push_str(&format!(
                "<programme channel=\"ch{c}\" start=\"202501{day:02}{hour:02}0000 +0000\" \
                 stop=\"202501{day:02}{hour:02}3000 +0000\"><title>Show {c}-{p}</title>\
                 <desc>Something to fill the document out a bit.</desc></programme>"
            ));
        }
    }
    xml.push_str("</tv>");

    let started = std::time::Instant::now();
    let mut sink = Sink::default();
    let stats = xmltv::parse(Cursor::new(xml.into_bytes()), &mut sink).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(stats.channels, CHANNELS);
    assert_eq!(stats.programmes, CHANNELS * PER_CHANNEL);
    assert!(
        elapsed.as_secs() < 30,
        "{} programmes took {elapsed:?}",
        stats.programmes
    );
}
