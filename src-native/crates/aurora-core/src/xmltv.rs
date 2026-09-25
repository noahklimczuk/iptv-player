//! Streaming XMLTV parser.
//!
//! README §4.4 requires a pull parser: a 7-day EPG for 3,000 channels can exceed 1 GB
//! uncompressed, so nothing is buffered beyond the programme currently being built.
//! Results are pushed to a [`XmltvSink`] so the importer can batch straight into SQLite.

use std::io::BufRead;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{CoreError, Result};
use crate::model::{Credit, EpgChannel, Programme};

/// Receives parsed items as they are produced.
pub trait XmltvSink {
    fn channel(&mut self, channel: EpgChannel);
    fn programme(&mut self, programme: Programme);
}

/// Convenience sink that keeps everything in memory. Fine for tests and small EPGs.
#[derive(Debug, Default)]
pub struct CollectSink {
    pub channels: Vec<EpgChannel>,
    pub programmes: Vec<Programme>,
}

impl XmltvSink for CollectSink {
    fn channel(&mut self, channel: EpgChannel) {
        self.channels.push(channel);
    }
    fn programme(&mut self, programme: Programme) {
        self.programmes.push(programme);
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct XmltvStats {
    pub channels: usize,
    pub programmes: usize,
    /// Programmes discarded for having no channel, no title, or an unparseable start time.
    pub skipped: usize,
}

#[derive(Default)]
struct ProgBuilder {
    channel_id: String,
    start: Option<i64>,
    stop: Option<i64>,
    title: String,
    sub_title: Option<String>,
    description: Option<String>,
    categories: Vec<String>,
    season: Option<u16>,
    episode: Option<u16>,
    icon: Option<String>,
    rating: Option<String>,
    star_rating: Option<f32>,
    is_new: bool,
    is_live: bool,
    is_premiere: bool,
    credits: Vec<Credit>,
}

const CREDIT_ROLES: &[&str] = &[
    "director",
    "actor",
    "writer",
    "adapter",
    "producer",
    "composer",
    "editor",
    "presenter",
    "commentator",
    "guest",
];

/// Parse an XMLTV document, pushing items into `sink` as they complete.
pub fn parse<R: BufRead, S: XmltvSink>(reader: R, sink: &mut S) -> Result<XmltvStats> {
    let mut xml = Reader::from_reader(reader);
    xml.config_mut().trim_text(true);
    xml.config_mut().check_end_names = false;

    let mut buf = Vec::new();
    let mut stats = XmltvStats::default();

    // Element-scoped state.
    let mut in_channel: Option<EpgChannel> = None;
    let mut in_prog: Option<ProgBuilder> = None;
    let mut in_credits = false;
    let mut current: Vec<u8> = Vec::new();
    let mut text = String::new();
    // `<rating>` and `<star-rating>` both wrap a `<value>`; remember which we are inside.
    let mut rating_scope: Option<&'static str> = None;
    let mut episode_system = String::new();

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(CoreError::Xmltv(format!(
                    "at byte {}: {e}",
                    xml.buffer_position()
                )))
            }
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_vec();
                text.clear();
                match name.as_slice() {
                    b"channel" => {
                        let mut c = EpgChannel::default();
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"id" {
                                c.id = a.unescape_value().unwrap_or_default().into_owned();
                            }
                        }
                        in_channel = Some(c);
                    }
                    b"programme" => {
                        let mut p = ProgBuilder::default();
                        for a in e.attributes().flatten() {
                            let v = a.unescape_value().unwrap_or_default().into_owned();
                            match a.key.as_ref() {
                                b"channel" => p.channel_id = v,
                                b"start" => p.start = parse_time(&v),
                                b"stop" => p.stop = parse_time(&v),
                                _ => {}
                            }
                        }
                        in_prog = Some(p);
                    }
                    b"credits" => in_credits = true,
                    b"rating" => rating_scope = Some("rating"),
                    b"star-rating" => rating_scope = Some("star"),
                    b"episode-num" => {
                        episode_system.clear();
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"system" {
                                episode_system =
                                    a.unescape_value().unwrap_or_default().into_owned();
                            }
                        }
                    }
                    b"icon" => {
                        let src = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"src")
                            .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()));
                        if let Some(src) = src {
                            if let Some(p) = in_prog.as_mut() {
                                p.icon.get_or_insert(src);
                            } else if let Some(c) = in_channel.as_mut() {
                                c.icon.get_or_insert(src);
                            }
                        }
                    }
                    _ => {}
                }
                current = name;
            }
            Ok(Event::Empty(e)) => {
                // Self-closing flags and icons.
                match e.name().as_ref() {
                    b"new" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.is_new = true;
                        }
                    }
                    b"live" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.is_live = true;
                        }
                    }
                    b"premiere" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.is_premiere = true;
                        }
                    }
                    b"icon" => {
                        let src = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"src")
                            .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()));
                        if let Some(src) = src {
                            if let Some(p) = in_prog.as_mut() {
                                p.icon.get_or_insert(src);
                            } else if let Some(c) = in_channel.as_mut() {
                                c.icon.get_or_insert(src);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if let Ok(v) = t.unescape() {
                    text.push_str(v.as_ref());
                }
            }
            Ok(Event::CData(t)) => {
                text.push_str(&String::from_utf8_lossy(t.as_ref()));
            }
            Ok(Event::End(e)) => {
                let name = e.name().as_ref().to_vec();
                let value = std::mem::take(&mut text);

                match name.as_slice() {
                    b"channel" => {
                        if let Some(c) = in_channel.take() {
                            if !c.id.is_empty() {
                                stats.channels += 1;
                                sink.channel(c);
                            } else {
                                stats.skipped += 1;
                            }
                        }
                    }
                    b"programme" => {
                        if let Some(p) = in_prog.take() {
                            match finish(p) {
                                Some(prog) => {
                                    stats.programmes += 1;
                                    sink.programme(prog);
                                }
                                None => stats.skipped += 1,
                            }
                        }
                    }
                    b"credits" => in_credits = false,
                    b"display-name" => {
                        if let Some(c) = in_channel.as_mut() {
                            if !value.is_empty() {
                                c.display_names.push(value);
                            }
                        }
                    }
                    b"title" => {
                        if let Some(p) = in_prog.as_mut() {
                            if p.title.is_empty() {
                                p.title = value;
                            }
                        }
                    }
                    b"sub-title" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.sub_title.get_or_insert(value);
                        }
                    }
                    b"desc" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.description.get_or_insert(value);
                        }
                    }
                    b"category" => {
                        if let Some(p) = in_prog.as_mut() {
                            if !value.is_empty() && !p.categories.contains(&value) {
                                p.categories.push(value);
                            }
                        }
                    }
                    b"episode-num" => {
                        if let Some(p) = in_prog.as_mut() {
                            apply_episode_num(p, &episode_system, &value);
                        }
                    }
                    b"value" => {
                        if let Some(p) = in_prog.as_mut() {
                            match rating_scope {
                                Some("rating") => {
                                    p.rating.get_or_insert(value);
                                }
                                Some("star") => {
                                    p.star_rating = parse_star(&value);
                                }
                                _ => {}
                            }
                        }
                    }
                    b"rating" | b"star-rating" => rating_scope = None,
                    b"new" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.is_new = true;
                        }
                    }
                    b"live" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.is_live = true;
                        }
                    }
                    b"premiere" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.is_premiere = true;
                        }
                    }
                    role if in_credits => {
                        let role_s = String::from_utf8_lossy(role).to_string();
                        if CREDIT_ROLES.contains(&role_s.as_str()) && !value.is_empty() {
                            if let Some(p) = in_prog.as_mut() {
                                p.credits.push(Credit {
                                    role: role_s,
                                    name: value,
                                });
                            }
                        }
                    }
                    _ => {}
                }
                current.clear();
            }
            Ok(_) => {}
        }
        buf.clear();
    }

    let _ = current;
    Ok(stats)
}

fn finish(p: ProgBuilder) -> Option<Programme> {
    let start = p.start?;
    if p.channel_id.is_empty() || p.title.is_empty() {
        return None;
    }
    // A missing stop is common; assume 30 minutes rather than discarding the programme.
    let stop = p.stop.unwrap_or(start + 1800);
    if stop <= start {
        return None;
    }
    Some(Programme {
        channel_id: p.channel_id,
        start,
        stop,
        title: p.title,
        sub_title: p.sub_title,
        description: p.description,
        categories: p.categories,
        season: p.season,
        episode: p.episode,
        icon: p.icon,
        rating: p.rating,
        star_rating: p.star_rating,
        is_new: p.is_new,
        is_live: p.is_live,
        is_premiere: p.is_premiere,
        credits: p.credits,
    })
}

fn apply_episode_num(p: &mut ProgBuilder, system: &str, value: &str) {
    match system {
        // "0.1.0/1" — zero-based season.episode.part
        "xmltv_ns" => {
            let mut parts = value.split('.');
            if let Some(s) = parts.next() {
                if let Ok(n) = s.split('/').next().unwrap_or("").trim().parse::<u16>() {
                    p.season = Some(n + 1);
                }
            }
            if let Some(e) = parts.next() {
                if let Ok(n) = e.split('/').next().unwrap_or("").trim().parse::<u16>() {
                    p.episode = Some(n + 1);
                }
            }
        }
        // "S01E02" or "1x02"
        _ => {
            if let Some(m) = crate::series::parse_episode_marker(value) {
                p.season = Some(m.season);
                p.episode = Some(m.episode);
            }
        }
    }
}

fn parse_star(value: &str) -> Option<f32> {
    // "7/10" or "3.5"
    if let Some((num, den)) = value.split_once('/') {
        let n: f32 = num.trim().parse().ok()?;
        let d: f32 = den.trim().parse().ok()?;
        if d > 0.0 {
            return Some((n / d) * 10.0);
        }
        return None;
    }
    value.trim().parse().ok()
}

/// How long a month actually is.
///
/// `1..=31` accepted 31 February, and `days_from_civil` answers for it rather than
/// refusing — so a guide with a typo in it silently produced a programme three days
/// later, in the middle of real ones, rather than a skipped entry and a count that
/// says something went wrong.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days since the Unix epoch for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Parse an XMLTV timestamp into Unix seconds UTC.
///
/// Accepts `YYYYMMDDHHMMSS +0100`, `YYYYMMDDHHMMSS`, `YYYYMMDDHHMM`, and `YYYYMMDD`,
/// with or without a space before the offset. A missing offset is treated as UTC.
pub fn parse_time(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if s.len() < 8 {
        return None;
    }
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return None;
    }
    let num = |a: usize, b: usize| -> Option<i64> { digits.get(a..b)?.parse().ok() };

    let year = num(0, 4)?;
    let month = num(4, 6)?;
    let day = num(6, 8)?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let hour = if digits.len() >= 10 { num(8, 10)? } else { 0 };
    let minute = if digits.len() >= 12 { num(10, 12)? } else { 0 };
    let second = if digits.len() >= 14 { num(12, 14)? } else { 0 };
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    let mut unix = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;

    // Optional trailing offset, e.g. " +0100" / "-0530" / "+01:00".
    let rest = s[digits.len()..].trim();
    if let Some(sign) = rest.chars().next() {
        if sign == '+' || sign == '-' {
            let off: String = rest[1..].chars().filter(|c| c.is_ascii_digit()).collect();
            if off.len() >= 4 {
                let oh: i64 = off[0..2].parse().ok()?;
                let om: i64 = off[2..4].parse().ok()?;
                let total = oh * 3600 + om * 60;
                unix += if sign == '+' { -total } else { total };
            }
        }
    }
    Some(unix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(xml: &str) -> (XmltvStats, CollectSink) {
        let mut sink = CollectSink::default();
        let stats = parse(std::io::Cursor::new(xml.as_bytes().to_vec()), &mut sink).unwrap();
        (stats, sink)
    }

    #[test]
    fn epoch_is_correct() {
        assert_eq!(parse_time("19700101000000 +0000"), Some(0));
        assert_eq!(parse_time("20240115120000 +0000"), Some(1_705_320_000));
    }

    /// `1..=31` accepted 31 February, and `days_from_civil` happily answers for it —
    /// so a guide with a typo silently produced a programme three days later, sitting
    /// in the middle of real ones rather than being skipped and counted.
    #[test]
    fn an_impossible_date_is_rejected_rather_than_rolled_over() {
        for bad in [
            "20250231000000 +0000", // February has 28 days in 2025
            "20250230000000 +0000",
            "20250431000000 +0000", // April has 30
            "20250631000000 +0000",
            "20250931000000 +0000",
            "20251131000000 +0000",
            "20250100000000 +0000", // day zero
            "20251301000000 +0000", // month thirteen
            "20250001000000 +0000", // month zero
        ] {
            assert_eq!(parse_time(bad), None, "{bad} should not parse");
        }
    }

    #[test]
    fn leap_years_are_worked_out_rather_than_guessed() {
        // Divisible by four: a leap year.
        assert!(parse_time("20240229120000 +0000").is_some());
        // Not divisible by four.
        assert_eq!(parse_time("20250229120000 +0000"), None);
        // Divisible by 100 but not 400: not a leap year.
        assert_eq!(parse_time("19000229120000 +0000"), None);
        // Divisible by 400: a leap year.
        assert!(parse_time("20000229120000 +0000").is_some());
    }

    #[test]
    fn the_last_day_of_every_month_still_parses() {
        for (date, _) in [
            ("20250131", 31),
            ("20250228", 28),
            ("20250331", 31),
            ("20250430", 30),
            ("20250630", 30),
            ("20250930", 30),
            ("20251130", 30),
            ("20251231", 31),
        ] {
            assert!(
                parse_time(&format!("{date}120000 +0000")).is_some(),
                "{date} is a real date"
            );
        }
    }

    #[test]
    fn applies_the_utc_offset() {
        let utc = parse_time("20240115120000 +0000").unwrap();
        assert_eq!(parse_time("20240115130000 +0100"), Some(utc));
        assert_eq!(parse_time("20240115070000 -0500"), Some(utc));
    }

    #[test]
    fn accepts_short_and_colon_forms() {
        assert!(parse_time("20240115").is_some());
        assert!(parse_time("202401151200").is_some());
        assert_eq!(
            parse_time("20240115130000 +01:00"),
            parse_time("20240115120000 +0000")
        );
    }

    #[test]
    fn rejects_nonsense_timestamps() {
        assert_eq!(parse_time(""), None);
        assert_eq!(parse_time("not-a-time"), None);
        assert_eq!(parse_time("20241332000000"), None);
    }

    #[test]
    fn parses_channels_and_programmes() {
        let (stats, sink) = collect(
            r#"<?xml version="1.0"?>
<tv>
  <channel id="cnn.us">
    <display-name>CNN</display-name>
    <display-name>CNN International</display-name>
    <icon src="https://example.com/cnn.png"/>
  </channel>
  <programme start="20240115120000 +0000" stop="20240115130000 +0000" channel="cnn.us">
    <title>World News</title>
    <sub-title>Midday</sub-title>
    <desc>The day so far.</desc>
    <category>News</category>
    <category>Current Affairs</category>
    <episode-num system="xmltv_ns">0.1.0/1</episode-num>
    <rating><value>PG</value></rating>
    <star-rating><value>7/10</value></star-rating>
    <credits><presenter>Someone</presenter><actor>Another</actor></credits>
    <new/>
  </programme>
</tv>"#,
        );
        assert_eq!(stats.channels, 1);
        assert_eq!(stats.programmes, 1);

        let c = &sink.channels[0];
        assert_eq!(c.id, "cnn.us");
        assert_eq!(c.display_names.len(), 2);
        assert_eq!(c.icon.as_deref(), Some("https://example.com/cnn.png"));

        let p = &sink.programmes[0];
        assert_eq!(p.title, "World News");
        assert_eq!(p.sub_title.as_deref(), Some("Midday"));
        assert_eq!(p.categories, vec!["News", "Current Affairs"]);
        assert_eq!(p.season, Some(1));
        assert_eq!(p.episode, Some(2));
        assert_eq!(p.rating.as_deref(), Some("PG"));
        assert_eq!(p.star_rating, Some(7.0));
        assert!(p.is_new);
        assert_eq!(p.credits.len(), 2);
        assert_eq!(p.duration_secs(), 3600);
    }

    #[test]
    fn onscreen_episode_numbers_work() {
        let (_, sink) = collect(
            r#"<tv><programme start="20240115120000" channel="a"><title>T</title>
            <episode-num system="onscreen">S03E07</episode-num></programme></tv>"#,
        );
        assert_eq!(sink.programmes[0].season, Some(3));
        assert_eq!(sink.programmes[0].episode, Some(7));
    }

    #[test]
    fn missing_stop_gets_a_default_duration() {
        let (_, sink) = collect(
            r#"<tv><programme start="20240115120000 +0000" channel="a"><title>T</title></programme></tv>"#,
        );
        assert_eq!(sink.programmes[0].duration_secs(), 1800);
    }

    #[test]
    fn programmes_without_a_title_or_channel_are_skipped_not_fatal() {
        let (stats, sink) = collect(
            r#"<tv>
              <programme start="20240115120000" channel="a"></programme>
              <programme start="20240115120000"><title>No channel</title></programme>
              <programme start="20240115120000" channel="b"><title>Good</title></programme>
            </tv>"#,
        );
        assert_eq!(stats.programmes, 1);
        assert_eq!(stats.skipped, 2);
        assert_eq!(sink.programmes[0].title, "Good");
    }

    #[test]
    fn cdata_descriptions_survive() {
        let (_, sink) = collect(
            r#"<tv><programme start="20240115120000" channel="a"><title>T</title>
            <desc><![CDATA[Ampersands & <angle> brackets]]></desc></programme></tv>"#,
        );
        assert!(sink.programmes[0]
            .description
            .as_deref()
            .unwrap()
            .contains("Ampersands &"));
    }

    #[test]
    fn stop_before_start_is_rejected() {
        let (stats, _) = collect(
            r#"<tv><programme start="20240115130000 +0000" stop="20240115120000 +0000" channel="a">
            <title>Backwards</title></programme></tv>"#,
        );
        assert_eq!(stats.programmes, 0);
        assert_eq!(stats.skipped, 1);
    }
}
