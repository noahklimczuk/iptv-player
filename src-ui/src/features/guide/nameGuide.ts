/**
 * What a channel with no guide says about itself, read from its name.
 *
 * Most of a real line has no schedule anywhere. Of 6,700 channels on the line this was
 * built against, 3,642 had none, and they were not quiet channels: event slots
 * ("US ★ Flo 547 : [Hockey:2025 Rochester Americans vs Syracuse Crunch Away]
 * (2025-10-18 19:04:10)"), game feeds ("US ★ NHL GAME 06: LIGHTNING @ BLUE JACKETS
 * 7PM ET") and 24/7 loops ("24/7 ★ The Kardashians"). No guide publishes them -- the
 * provider writes what is on into the name instead. So the guide reads it there.
 *
 * Never presented as a schedule: the row says it came from the channel's name, and
 * nothing here is a programme a reminder or a recording could be set on.
 */

export type NameGuide =
  /** A fixture. `at` is when the name says it starts, in Unix seconds. */
  | { kind: 'event'; title: string; at: number | null; detail: string | null }
  /** A loop of one show or theme, all day. */
  | { kind: 'loop'; title: string }
  /** An event slot with nothing in it right now. */
  | { kind: 'idle'; title: string }
  /** Nothing to read but the channel's own name. */
  | { kind: 'name'; title: string };

/** "US ★ ", "CA | ", "24/7 ★ " and the like, which say where, not what. */
const PREFIX = /^\s*[A-Z0-9/+]{2,6}\s*[★|:\-–]\s*/u;
/** Quality tags that say nothing about what is on. */
const QUALITY = /\b(FHD|UHD|HD|SD|4K|HEVC|H\.?265|1080p|720p|RAW)\b/gi;
/** Whether bracketed text describes a fixture rather than, say, a city. */
const FIXTURE = /^[^:]{2,24}:|\s(vs\.?|v|@)\s/i;

/**
 * Quality tags out, the provider's separators made readable. Case is left alone: the
 * names are full of acronyms (AMC, NCAAF, BTN) that no recasing gets right.
 */
function tidy(s: string): string {
  return s
    .replace(QUALITY, ' ')
    .replace(/\s_\s/g, ' · ')
    .replace(/`/g, '’')
    .replace(/\s{2,}/g, ' ')
    .trim()
    .replace(/[\s:|\-–]+$/u, '');
}

/** "2025-10-18 19:05:30", in the viewer's local time, which is what the panel means. */
function parseStamp(s: string): number | null {
  const m = /^(\d{4})-(\d\d)-(\d\d)[ T](\d\d):(\d\d)(?::(\d\d))?/.exec(s.trim());
  if (!m) return null;
  const d = new Date(+m[1]!, +m[2]! - 1, +m[3]!, +m[4]!, +m[5]!, +(m[6] ?? 0));
  return Number.isNaN(d.getTime()) ? null : Math.floor(d.getTime() / 1000);
}

export function nameGuide(rawName: string): NameGuide {
  const name = rawName.trim();

  // "[Hockey:2025 Des Moines Buccaneers vs Fargo Force Home] (2025-10-18 19:05:30)".
  // The closing parenthesis is often cut off by the provider, so it is optional.
  const bracket = /\[([^\]]+)\]\s*(?:\(([^)]+)\)?)?/.exec(name);
  if (bracket && FIXTURE.test(bracket[1]!)) {
    const inner = bracket[1]!.trim();
    // "Hockey:2025 Title" -> "Hockey: Title", the season number dropped.
    const sport = /^([^:]{2,24}):\s*(?:\d{4}\s+)?(.+)$/.exec(inner);
    const title = sport ? `${sport[1]!.trim()}: ${sport[2]!.trim()}` : inner;
    return {
      kind: 'event',
      title: tidy(title),
      at: bracket[2] ? parseStamp(bracket[2]) : null,
      detail: null,
    };
  }

  const bare = name.replace(PREFIX, '');

  // "24/7 ★ The Kardashians"
  if (/^\s*24\/7\b/i.test(name)) {
    const show = tidy(bare.replace(/\b24\/7\b/gi, ''));
    if (show) return { kind: 'loop', title: show };
  }

  // "NHL GAME 06: LIGHTNING @ BLUE JACKETS 7PM ET", and "Flo 068 : " with nothing after.
  const colon = /^(.*?\S)\s*:\s*(.*)$/.exec(bare);
  if (colon) {
    const after = tidy(colon[2]!);
    if (!after) return { kind: 'idle', title: 'Nothing scheduled on this event channel' };
    if (/\s(@|vs\.?|v)\s/i.test(after) || /\d\s?(AM|PM)\b/i.test(after)) {
      const time = /\b(\d{1,2}(?::\d\d)?\s?(?:AM|PM)(?:\s?[ECMP][SD]?T)?)\s*$/i.exec(after);
      const title = time ? after.slice(0, time.index).trim() : after;
      return { kind: 'event', title, at: null, detail: time ? time[1]!.toUpperCase() : null };
    }
  }

  return { kind: 'name', title: tidy(bare) || name };
}
