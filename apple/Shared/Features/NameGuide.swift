import Foundation

/// What a channel's name says it is showing, for the guide's rows with no listings:
/// Windows' `nameGuide.ts`, rule for rule. Event channels carry the fixture in their
/// name ("US ★ NFL 01: Bears @ Packers 8:15 PM ET"), 24/7 channels the show they loop,
/// and saying so beats an empty strip that reads as the guide having failed to load.
enum NameGuide: Equatable {
    /// A fixture, with its start if the name gives one: a full stamp (`at`, Unix
    /// seconds) or a bare time of day as written (`detail`).
    case event(title: String, at: Int64?, detail: String?)
    /// A channel that plays one show round the clock.
    case loop(title: String)
    /// An event channel with nothing on it now.
    case idle(title: String)
    /// Only the name, tidied.
    case name(title: String)

    var title: String {
        switch self {
        case .event(let t, _, _), .loop(let t), .idle(let t), .name(let t): t
        }
    }

    init(_ rawName: String) {
        let name = rawName.trimmingCharacters(in: .whitespaces)

        if let m = Self.bracket.firstMatch(name), Self.fixture.matches(m[1] ?? "") {
            let inner = (m[1] ?? "").trimmingCharacters(in: .whitespaces)
            let title: String
            if let s = Self.sport.firstMatch(inner), let a = s[1], let b = s[2] {
                title = "\(a.trimmingCharacters(in: .whitespaces)): \(b.trimmingCharacters(in: .whitespaces))"
            } else {
                title = inner
            }
            self = .event(title: Self.tidy(title), at: m[2].flatMap(Self.parseStamp), detail: nil)
            return
        }

        let bare = Self.prefix.replacing(in: name, with: "")

        if Self.loopStart.matches(name) {
            let show = Self.tidy(Self.twentyFourSeven.replacing(in: bare, with: ""))
            if !show.isEmpty { self = .loop(title: show); return }
        }

        if let c = Self.colon.firstMatch(bare) {
            let after = Self.tidy(c[2] ?? "")
            if after.isEmpty { self = .idle(title: "Nothing scheduled on this event channel"); return }
            if Self.versus.matches(after) || Self.clock.matches(after) {
                if let t = Self.trailingTime.firstMatch(after), let range = t.range {
                    let title = String(after[..<range.lowerBound]).trimmingCharacters(in: .whitespaces)
                    self = .event(title: title, at: nil, detail: t[1]?.uppercased())
                } else {
                    self = .event(title: after, at: nil, detail: nil)
                }
                return
            }
        }

        let tidied = Self.tidy(bare)
        self = .name(title: tidied.isEmpty ? name : tidied)
    }

    // The patterns, as written in `nameGuide.ts`.
    private static let prefix = Pattern(#"^\s*[A-Z0-9/+]{2,6}\s*[★|:\-–]\s*"#)
    private static let quality = Pattern(#"\b(FHD|UHD|HD|SD|4K|HEVC|H\.?265|1080p|720p|RAW)\b"#, caseless: true)
    private static let fixture = Pattern(#"^[^:]{2,24}:|\s(vs\.?|v|@)\s"#, caseless: true)
    private static let bracket = Pattern(#"\[([^\]]+)\]\s*(?:\(([^)]+)\)?)?"#)
    private static let sport = Pattern(#"^([^:]{2,24}):\s*(?:\d{4}\s+)?(.+)$"#)
    private static let loopStart = Pattern(#"^\s*24/7\b"#, caseless: true)
    private static let twentyFourSeven = Pattern(#"\b24/7\b"#, caseless: true)
    private static let colon = Pattern(#"^(.*?\S)\s*:\s*(.*)$"#)
    private static let versus = Pattern(#"\s(@|vs\.?|v)\s"#, caseless: true)
    private static let clock = Pattern(#"\d\s?(AM|PM)\b"#, caseless: true)
    private static let trailingTime = Pattern(#"\b(\d{1,2}(?::\d\d)?\s?(?:AM|PM)(?:\s?[ECMP][SD]?T)?)\s*$"#, caseless: true)
    private static let stamp = Pattern(#"^(\d{4})-(\d\d)-(\d\d)[ T](\d\d):(\d\d)(?::(\d\d))?"#)

    static func tidy(_ s: String) -> String {
        var out = quality.replacing(in: s, with: " ")
        out = out.replacingOccurrences(of: " _ ", with: " · ")
        out = out.replacingOccurrences(of: "`", with: "’")
        out = Pattern(#"\s{2,}"#).replacing(in: out, with: " ")
        out = out.trimmingCharacters(in: .whitespaces)
        return Pattern(#"[\s:|\-–]+$"#).replacing(in: out, with: "")
    }

    /// "2026-11-20 20:15" (seconds optional), in the device's time zone as Windows reads it.
    static func parseStamp(_ s: String) -> Int64? {
        guard let m = stamp.firstMatch(s.trimmingCharacters(in: .whitespaces)) else { return nil }
        var parts = DateComponents()
        parts.year = Int(m[1] ?? "")
        parts.month = Int(m[2] ?? "")
        parts.day = Int(m[3] ?? "")
        parts.hour = Int(m[4] ?? "")
        parts.minute = Int(m[5] ?? "")
        parts.second = Int(m[6] ?? "0") ?? 0
        guard let date = Calendar.current.date(from: parts) else { return nil }
        return Int64(date.timeIntervalSince1970)
    }
}

/// A JavaScript-style regular expression, enough of one for the rules above.
private struct Pattern {
    let regex: NSRegularExpression

    init(_ pattern: String, caseless: Bool = false) {
        // The patterns are constants: a bad one is a bug to find at once.
        // swiftlint:disable:next force_try
        regex = try! NSRegularExpression(pattern: pattern, options: caseless ? [.caseInsensitive] : [])
    }

    struct Match {
        let groups: [String?]
        let range: Range<String.Index>?
        subscript(i: Int) -> String? { i < groups.count ? groups[i] : nil }
    }

    func matches(_ s: String) -> Bool { firstMatch(s) != nil }

    func firstMatch(_ s: String) -> Match? {
        guard let m = regex.firstMatch(in: s, range: NSRange(s.startIndex..., in: s)) else { return nil }
        let groups = (0..<m.numberOfRanges).map { i in Range(m.range(at: i), in: s).map { String(s[$0]) } }
        return Match(groups: groups, range: Range(m.range, in: s))
    }

    func replacing(in s: String, with template: String) -> String {
        regex.stringByReplacingMatches(in: s, range: NSRange(s.startIndex..., in: s), withTemplate: template)
    }
}
