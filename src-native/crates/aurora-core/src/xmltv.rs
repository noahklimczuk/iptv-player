//! Streaming XMLTV parser.
//!
//! README §4.4 requires a pull parser: a 7-day EPG for 3,000 channels can exceed 1 GB
//! uncompressed, so nothing is buffered beyond the programme currently being built.
//! Results are pushed to a [`XmltvSink`] so the importer can batch straight into SQLite.

use std::io::BufRead;

use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

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
    ///
    /// Programmes only. A `<channel>` dropped for an empty id used to increment this one
    /// too, which made the number mean neither thing: on the real guide it stood at
    /// 2,235 before a single programme had been considered, so an import reporting
    /// "2,489 skipped" was reporting mostly channels under a name that says programmes
    /// (R-29).
    pub skipped: usize,
    /// Channels discarded for having no id, which is the only thing that disqualifies one.
    pub channels_skipped: usize,
}

#[derive(Default)]
struct ProgBuilder {
    channel_id: String,
    start: Option<i64>,
    stop: Option<i64>,
    title: String,
    /// `title_rank` of the title currently held. Only meaningful once `title` is set,
    /// and written every time `title` is.
    title_rank: usize,
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

/// Resolve an entity reference to the text it stands for, or `None` if it is neither a
/// character reference nor one of XML's five predefined entities.
///
/// XMLTV documents in the wild use exactly these; anything else would have to come from
/// a DTD, which this parser does not read and which a provider has no business relying
/// on in a feed.
fn resolve_entity(r: &quick_xml::events::BytesRef<'_>) -> Option<String> {
    // `&#233;` and `&#xE9;`. Malformed ones (`&#zz;`) resolve to nothing, not to an
    // error that would take the surrounding programme down with it.
    if r.is_char_ref() {
        return r.resolve_char_ref().ok().flatten().map(String::from);
    }
    match r.xml10_content().as_ref() {
        "amp" => Some("&".to_string()),
        "lt" => Some("<".to_string()),
        "gt" => Some(">".to_string()),
        "quot" => Some("\"".to_string()),
        "apos" => Some("'".to_string()),
        _ => None,
    }
}

/// What the caller knows that the document does not.
#[derive(Debug, Clone, Default)]
pub struct XmltvOptions {
    /// Language codes for `<title lang=...>`, best first, matched as a prefix so `en`
    /// accepts `en-GB`. Empty keeps the first title the document offers.
    pub title_langs: Vec<String>,
}

/// How well a `<title>`'s `lang` matches the preference. Lower is better.
///
/// XMLTV allows one `<title>` per language and defines no precedence between them, so
/// there is no answer here that comes from the document alone — which is why this takes
/// the preference rather than guessing one. A listed language wins, in the order it is
/// listed. An untagged title comes next, because a feed that labels nothing is the
/// ordinary case and its single title is the title. A title tagged as some other
/// language comes last, so `lang="es"` cannot displace an untagged one.
fn title_rank(lang: Option<&str>, preferred: &[String]) -> usize {
    match lang {
        Some(l) => {
            let l = l.to_ascii_lowercase();
            preferred
                .iter()
                .position(|p| l.starts_with(&p.to_ascii_lowercase()))
                .unwrap_or(usize::MAX)
        }
        None => usize::MAX - 1,
    }
}

/// Parse an XMLTV document, pushing items into `sink` as they complete.
///
/// Keeps the first `<title>`, which is what a document with one title per programme
/// means and what every caller wanted before there was a preference to express.
pub fn parse<R: BufRead, S: XmltvSink>(reader: R, sink: &mut S) -> Result<XmltvStats> {
    parse_with(reader, sink, &XmltvOptions::default())
}

/// Parse an XMLTV document, resolving multi-language titles against `options`.
pub fn parse_with<R: BufRead, S: XmltvSink>(
    reader: R,
    sink: &mut S,
    options: &XmltvOptions,
) -> Result<XmltvStats> {
    let mut xml = Reader::from_reader(reader);
    // `trim_text` is deliberately off, where it used to be on. quick-xml 0.42 ends a
    // text run at every entity and reports the entity separately, so `Tom &amp; Jerry`
    // arrives as three pieces — and trimming each piece turns it into `Tom&Jerry`. The
    // whole accumulated value is trimmed once at the closing tag instead, which is what
    // `trim_text` used to do back when a run was never split.
    xml.config_mut().check_end_names = false;

    let mut buf = Vec::new();
    let mut stats = XmltvStats::default();

    // Element-scoped state.
    let mut in_channel: Option<EpgChannel> = None;
    let mut in_prog: Option<ProgBuilder> = None;
    let mut in_credits = false;
    let mut current = String::new();
    let mut text = String::new();
    // `<rating>` and `<star-rating>` both wrap a `<value>`; remember which we are inside.
    let mut rating_scope: Option<&'static str> = None;
    let mut episode_system = String::new();
    // The `lang` of the `<title>` being read: on the opening tag, wanted at the closing
    // one where the text has finished arriving.
    let mut title_lang: Option<String> = None;

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
                let name = e.name().as_ref().to_string();
                text.clear();
                match name.as_str() {
                    "channel" => {
                        let mut c = EpgChannel::default();
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == "id" {
                                c.id = a
                                    .normalized_value(XmlVersion::Implicit1_0)
                                    .unwrap_or_default()
                                    .into_owned();
                            }
                        }
                        in_channel = Some(c);
                    }
                    "programme" => {
                        let mut p = ProgBuilder::default();
                        for a in e.attributes().flatten() {
                            let v = a
                                .normalized_value(XmlVersion::Implicit1_0)
                                .unwrap_or_default()
                                .into_owned();
                            match a.key.as_ref() {
                                "channel" => p.channel_id = v,
                                "start" => p.start = parse_time(&v),
                                "stop" => p.stop = parse_time(&v),
                                _ => {}
                            }
                        }
                        in_prog = Some(p);
                    }
                    "title" => {
                        title_lang = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == "lang")
                            .and_then(|a| {
                                a.normalized_value(XmlVersion::Implicit1_0)
                                    .ok()
                                    .map(|v| v.into_owned())
                            })
                            .filter(|l| !l.is_empty());
                    }
                    "credits" => in_credits = true,
                    "rating" => rating_scope = Some("rating"),
                    "star-rating" => rating_scope = Some("star"),
                    "episode-num" => {
                        episode_system.clear();
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == "system" {
                                episode_system = a
                                    .normalized_value(XmlVersion::Implicit1_0)
                                    .unwrap_or_default()
                                    .into_owned();
                            }
                        }
                    }
                    "icon" => {
                        let src = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == "src")
                            .and_then(|a| {
                                a.normalized_value(XmlVersion::Implicit1_0)
                                    .ok()
                                    .map(|v| v.into_owned())
                            });
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
                    "new" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.is_new = true;
                        }
                    }
                    "live" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.is_live = true;
                        }
                    }
                    "premiere" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.is_premiere = true;
                        }
                    }
                    "icon" => {
                        let src = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == "src")
                            .and_then(|a| {
                                a.normalized_value(XmlVersion::Implicit1_0)
                                    .ok()
                                    .map(|v| v.into_owned())
                            });
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
                // `xml10_content` normalises end-of-line and nothing else. Entities are
                // no longer in this event at all — they arrive as `GeneralRef` below.
                text.push_str(t.xml10_content().as_ref());
            }
            // An entity reference: `&amp;`, or `&#233;`. In quick-xml 0.36 these were
            // resolved inside the text event and never seen; 0.42 reports each one and
            // ends the text run around it, which is why this arm has to exist for a
            // title as ordinary as `Tom &amp; Jerry` to survive.
            Ok(Event::GeneralRef(r)) => match resolve_entity(&r) {
                Some(resolved) => text.push_str(&resolved),
                // Not one of the five XML predefines and not a character reference —
                // an XMLTV file is not carrying a DTD we could resolve it against.
                // Kept as it was written rather than dropped: inventing a character
                // would be worse, and silently losing one hides a malformed feed.
                None => {
                    text.push('&');
                    text.push_str(r.xml10_content().as_ref());
                    text.push(';');
                }
            },
            Ok(Event::CData(t)) => {
                text.push_str(t.as_ref());
            }
            Ok(Event::End(e)) => {
                let name = e.name().as_ref().to_string();
                // Trimmed here rather than by the reader, because the reader would trim
                // each piece of a run split around an entity. Once, at the end, over
                // the whole assembled value, is what `trim_text(true)` used to mean.
                let value = std::mem::take(&mut text).trim().to_string();

                match name.as_str() {
                    "channel" => {
                        if let Some(c) = in_channel.take() {
                            if !c.id.is_empty() {
                                stats.channels += 1;
                                sink.channel(c);
                            } else {
                                stats.channels_skipped += 1;
                            }
                        }
                    }
                    "programme" => {
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
                    "credits" => in_credits = false,
                    "display-name" => {
                        if let Some(c) = in_channel.as_mut() {
                            if !value.is_empty() {
                                c.display_names.push(value);
                            }
                        }
                    }
                    "title" => {
                        let lang = title_lang.take();
                        if let Some(p) = in_prog.as_mut() {
                            let rank = title_rank(lang.as_deref(), &options.title_langs);
                            // With no preference the first title wins, exactly as it
                            // always has: ranking only decides between titles when the
                            // viewer has said which language they want.
                            let better = !options.title_langs.is_empty() && rank < p.title_rank;
                            // An empty `<title/>` never displaces a real one, however
                            // well its `lang` scores.
                            if (p.title.is_empty() || better) && !value.is_empty() {
                                p.title = value;
                                p.title_rank = rank;
                            }
                        }
                    }
                    "sub-title" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.sub_title.get_or_insert(value);
                        }
                    }
                    "desc" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.description.get_or_insert(value);
                        }
                    }
                    "category" => {
                        if let Some(p) = in_prog.as_mut() {
                            if !value.is_empty() && !p.categories.contains(&value) {
                                p.categories.push(value);
                            }
                        }
                    }
                    "episode-num" => {
                        if let Some(p) = in_prog.as_mut() {
                            apply_episode_num(p, &episode_system, &value);
                        }
                    }
                    "value" => {
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
                    "rating" | "star-rating" => rating_scope = None,
                    "new" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.is_new = true;
                        }
                    }
                    "live" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.is_live = true;
                        }
                    }
                    "premiere" => {
                        if let Some(p) = in_prog.as_mut() {
                            p.is_premiere = true;
                        }
                    }
                    role if in_credits => {
                        let role_s = role.to_string();
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
    // A stop that is not *after* the start is the same kind of provider mistake and gets
    // the same answer, which it did not: `if stop <= start { return None }` discarded it.
    //
    // Measured over the real guide, the 254 skipped programmes were 174 with an empty
    // `<title>` -- rightly dropped, there is nothing to show -- and 80 with
    // `stop == start`, each with a real title on a real channel:
    //
    // ```text
    // channel='Historia.es' start='20261008101000 +0200' stop='20261008101000 +0200'
    //   title='Augusto Pinochet: el golpe, la tortura y Occidente'
    // ```
    //
    // So eighty real programmes were lost to a typo the line above already knows how to
    // forgive (R-34).
    //
    // Only equality is forgiven. A stop *before* the start is contradictory rather than
    // incomplete -- neither timestamp can be trusted when they disagree about which way
    // time runs -- and `stop_before_start_is_rejected` is where that decision lives.
    const ASSUMED_SECS: i64 = 1800;
    let stop = match p.stop {
        Some(stop) if stop > start => stop,
        // The same timestamp twice: one obvious reading, and the same one a missing stop
        // gets.
        Some(stop) if stop == start => start + ASSUMED_SECS,
        Some(_) => return None,
        None => start + ASSUMED_SECS,
    };
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
        //
        // `checked_add`, because `xmltv_ns` is zero-based and the `+ 1` that corrects it
        // was applied to a number already parsed as `u16`: a guide carrying `65535.0.0`
        // wrapped to season 0 in a release build, where overflow checks are off, and
        // panicked in a debug one. A number that cannot be incremented is not a season
        // anybody can use, so it is dropped rather than guessed at -- which leaves
        // `None`, the same answer as a guide that says nothing (R-28).
        "xmltv_ns" => {
            let mut parts = value.split('.');
            if let Some(s) = parts.next() {
                if let Ok(n) = s.split('/').next().unwrap_or("").trim().parse::<u16>() {
                    p.season = n.checked_add(1);
                }
            }
            if let Some(e) = parts.next() {
                if let Ok(n) = e.split('/').next().unwrap_or("").trim().parse::<u16>() {
                    p.episode = n.checked_add(1);
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

    /// Wrap a title in just enough XMLTV to be parsed.
    /// A programme carrying several titles, each with the `lang` given (or none).
    fn multi_title(titles: &[(Option<&str>, &str)], preferred: &[&str]) -> String {
        let body: String = titles
            .iter()
            .map(|(lang, text)| match lang {
                Some(l) => format!("<title lang=\"{l}\">{text}</title>"),
                None => format!("<title>{text}</title>"),
            })
            .collect();
        let xml = format!(
            "<tv><programme channel=\"a\" start=\"20240115120000 +0000\">{body}</programme></tv>"
        );
        let mut sink = CollectSink::default();
        let options = XmltvOptions {
            title_langs: preferred.iter().map(|s| (*s).to_string()).collect(),
        };
        parse_with(std::io::Cursor::new(xml.into_bytes()), &mut sink, &options).unwrap();
        sink.programmes
            .first()
            .map(|p| p.title.clone())
            .unwrap_or_default()
    }

    /// XMLTV allows one `<title>` per language and gives no precedence, so the parser
    /// cannot choose without being told. It used to keep the first and never read `lang`,
    /// which is right for the common one-title guide and arbitrary for any other (R-30).
    ///
    /// The default still keeps the first, so a guide that was imported one way yesterday
    /// imports the same way today. Only a stated preference reorders anything.
    #[test]
    fn with_no_preference_the_documents_first_title_is_kept() {
        let both = [(Some("en"), "The Simpsons"), (Some("es"), "Los Simpson")];
        assert_eq!(multi_title(&both, &[]), "The Simpsons");
        // The same document the other way round: still the first, not the English one,
        // because nothing has asked for English.
        let reversed = [(Some("es"), "Los Simpson"), (Some("en"), "The Simpsons")];
        assert_eq!(multi_title(&reversed, &[]), "Los Simpson");
    }

    #[test]
    fn a_preference_finds_its_language_wherever_it_sits() {
        let reversed = [(Some("es"), "Los Simpson"), (Some("en"), "The Simpsons")];
        assert_eq!(multi_title(&reversed, &["en"]), "The Simpsons");
        // And honours which preference was listed first, not which title came first.
        assert_eq!(multi_title(&reversed, &["es", "en"]), "Los Simpson");
    }

    #[test]
    fn a_region_tag_matches_the_language_it_belongs_to() {
        let regional = [(Some("pt"), "Os Simpsons"), (Some("en-GB"), "The Simpsons")];
        assert_eq!(multi_title(&regional, &["en"]), "The Simpsons");
    }

    /// An untagged title outranks a language nobody asked for. A feed that labels one
    /// title and leaves another bare is saying the bare one is the default.
    #[test]
    fn an_untagged_title_beats_an_unwanted_language() {
        let mixed = [(Some("pt"), "Os Simpsons"), (None, "The Simpsons")];
        assert_eq!(multi_title(&mixed, &["en"]), "The Simpsons");
    }

    #[test]
    fn the_first_of_two_equally_good_titles_wins() {
        let twice = [(Some("en"), "The Simpsons"), (Some("en"), "Simpsons, The")];
        assert_eq!(multi_title(&twice, &["en"]), "The Simpsons");
    }

    /// The preference must not be able to empty a title that was there. A `<title/>` in
    /// the wanted language ranks best and carries nothing.
    #[test]
    fn an_empty_title_never_displaces_a_real_one() {
        let hollow = [(Some("pt"), "Os Simpsons"), (Some("en"), "")];
        assert_eq!(multi_title(&hollow, &["en"]), "Os Simpsons");
    }

    fn title_of(title: &str) -> String {
        let mut sink = CollectSink::default();
        let xml = format!(
            "<tv><programme channel=\"a\" start=\"20240115120000 +0000\">\
             <title>{title}</title></programme></tv>"
        );
        parse(std::io::Cursor::new(xml.into_bytes()), &mut sink).unwrap();
        sink.programmes
            .first()
            .map(|p| p.title.clone())
            .unwrap_or_default()
    }

    /// quick-xml 0.42 ends a text run at every entity and reports the entity as its own
    /// event, where 0.36 resolved them inside the text and never mentioned it. That is
    /// invisible until a title contains an ampersand — and then the title is either
    /// right, or silently cut off at the first one. These pin each way it can go.
    #[test]
    fn entities_in_a_title_are_resolved_and_the_text_around_them_survives() {
        // The predefined five, each splitting the run it sits in.
        assert_eq!(title_of("Tom &amp; Jerry"), "Tom & Jerry");
        assert_eq!(title_of("&lt;live&gt;"), "<live>");
        assert_eq!(
            title_of("&quot;Quoted&quot; &apos;so&apos;"),
            "\"Quoted\" 'so'"
        );

        // The spaces on either side of an entity are the thing that breaks if the
        // reader is left trimming each piece: this would come back as `Tom&Jerry`.
        assert_eq!(title_of("  Tom &amp; Jerry  "), "Tom & Jerry");

        // Character references, decimal and hexadecimal.
        assert_eq!(title_of("Caf&#233;"), "Café");
        assert_eq!(title_of("Caf&#xE9;"), "Café");
    }

    /// An entity that is not predefined and not a character reference could only be
    /// declared in a DTD, which this parser does not read. Keeping the source text is
    /// the one option that neither invents a character nor loses one silently.
    #[test]
    fn an_unresolvable_entity_is_kept_as_written_rather_than_guessed_at() {
        assert_eq!(title_of("Tom &nbsp; Jerry"), "Tom &nbsp; Jerry");
        // Malformed character references resolve to nothing, and must not take the
        // rest of the title with them.
        assert_eq!(title_of("Bad &#zz; reference"), "Bad &#zz; reference");
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

    /// A number that cannot be incremented is not a season (R-28).
    ///
    /// `xmltv_ns` is zero-based, so the parser adds one -- to a value already parsed as
    /// `u16`. `65535.0.0` wrapped to season 0 in a release build, where overflow checks
    /// are off, and panicked in a debug one.
    #[test]
    fn an_episode_number_that_does_not_fit_is_dropped_rather_than_wrapped() {
        let (_, sink) = collect(
            r#"<tv><programme start="20240115120000" channel="a"><title>T</title>
            <episode-num system="xmltv_ns">65535.65535.0</episode-num></programme></tv>"#,
        );
        assert_eq!(sink.programmes[0].season, None, "wrapped to a season");
        assert_eq!(sink.programmes[0].episode, None, "wrapped to an episode");

        // One below the ceiling still counts, so this is a bound and not a refusal.
        let (_, ok) = collect(
            r#"<tv><programme start="20240115120000" channel="a"><title>T</title>
            <episode-num system="xmltv_ns">65534.1.0</episode-num></programme></tv>"#,
        );
        assert_eq!(ok.programmes[0].season, Some(65535));
        assert_eq!(ok.programmes[0].episode, Some(2));
    }

    /// A dropped channel and a dropped programme are counted separately (R-29).
    ///
    /// Both used to increment `skipped`, which is documented as programmes. On the real
    /// guide that counter stood at 2,235 before a single programme had been considered,
    /// so the number an import reported was mostly channels under a name that says
    /// otherwise.
    #[test]
    fn a_channel_with_no_id_is_not_counted_as_a_skipped_programme() {
        let (stats, _) = collect(
            r#"<tv>
                <channel id=""><display-name>Nameless</display-name></channel>
                <channel id="a"><display-name>Real</display-name></channel>
                <programme start="20240115120000" channel="a"><title>T</title></programme>
                <programme start="20240115130000" channel="a"></programme>
            </tv>"#,
        );
        assert_eq!(stats.channels, 1);
        assert_eq!(stats.channels_skipped, 1, "the id-less channel");
        assert_eq!(stats.programmes, 1);
        assert_eq!(stats.skipped, 1, "the title-less programme, and only that");
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

    /// A stop equal to the start is a typo, and is forgiven the way a missing one is.
    ///
    /// 80 of the real guide's 254 skipped programmes were this, each with a real title
    /// on a real channel -- `stop="20261008101000 +0200"` the same as its start -- while
    /// the line above already assumed 30 minutes for a provider that omitted the stop
    /// entirely (R-34).
    #[test]
    fn a_zero_length_slot_is_given_the_benefit_of_the_doubt() {
        let (stats, sink) = collect(
            r#"<tv><programme start="20240115120000 +0000" stop="20240115120000 +0000" channel="a">
            <title>Augusto Pinochet</title></programme></tv>"#,
        );
        assert_eq!(stats.programmes, 1, "a real programme was discarded");
        assert_eq!(stats.skipped, 0);
        let p = &sink.programmes[0];
        assert_eq!(p.title, "Augusto Pinochet");
        assert_eq!(
            p.stop - p.start,
            1800,
            "the assumed length is the same 30 minutes"
        );
    }

    /// And a programme with no title is still dropped, because there is nothing to show.
    #[test]
    fn an_untitled_programme_is_still_dropped() {
        let (stats, _) = collect(
            r#"<tv><programme start="20240115120000 +0000" stop="20240115130000 +0000" channel="a">
            <title></title></programme></tv>"#,
        );
        assert_eq!(stats.programmes, 0);
        assert_eq!(stats.skipped, 1);
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
