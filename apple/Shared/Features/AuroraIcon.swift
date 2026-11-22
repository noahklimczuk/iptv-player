import SwiftUI

/// Windows' icon set (src-ui/src/components/Icon.tsx), drawn from the same SVG paths:
/// 24-point outlines stroked with round caps and joins, or filled.
///
/// Generated from `ICON_PATHS`; when Windows adds or redraws an icon, copy its path here.
struct AuroraIcon: View {
    enum Name: String, CaseIterable {
    case play
    case pause
    case stop
    case plus
    case check
    case filter
    case info
    case chevronRight
    case chevronLeft
    case chevronDown
    case search
    case refresh
    case home
    case tv
    case grid
    case film
    case stack
    case settings
    case volume
    case volumeOff
    case fullscreen
    case record
    case lock
    case clock
    case bell
    case star
    case thumbUp
    case thumbDown
    case close
    case skip
    case back10
    case forward10
    case subtitles
    case audio
    case pip
    case cast
    case layers
    case sparkle
    case heart
    case person
    case keyboard
    case alert
    case sun
    case moon
    case more

        /// The path, in Windows' 24-point view box.
        var path: String {
            switch self {
        case .play: "M5 3v18l14-9z"
        case .pause: "M6 5h4v14H6zm8 0h4v14h-4z"
        case .stop: "M6 6h12v12H6z"
        case .plus: "M12 5v14M5 12h14"
        case .check: "M4 12l5 5L20 6"
        case .filter: "M3 5h18M6 12h12M10 19h4"
        case .info: "M12 8h.01M11 12h1v5h1M12 3a9 9 0 100 18 9 9 0 000-18z"
        case .chevronRight: "M9 6l6 6-6 6"
        case .chevronLeft: "M15 6l-6 6 6 6"
        case .chevronDown: "M6 9l6 6 6-6"
        case .search: "M11 4a7 7 0 100 14 7 7 0 000-14zM20 20l-4-4"
        case .refresh: "M20 11a8 8 0 10-.6 4M20 5v6h-6"
        case .home: "M3 11l9-8 9 8M6 10v10h12V10"
        case .tv: "M3 5h18v11H3zM8 20h8M12 16v4"
        case .grid: "M3 4h18v4H3zM3 10h8v4H3zM13 10h8v4h-8zM3 16h18v4H3z"
        case .film: "M3 4h18v16H3zM7 4v16M17 4v16M3 9h4M3 15h4M17 9h4M17 15h4"
        case .stack: "M4 6h16v12H4zM8 3h12M6 21h12"
        case .settings: "M12 9a3 3 0 100 6 3 3 0 000-6zM12 5.5a6.5 6.5 0 100 13 6.5 6.5 0 000-13zM12 3v2.5M12 18.5v2.5M3 12h2.5M18.5 12h2.5M5.64 5.64l1.76 1.76M16.6 16.6l1.76 1.76M18.36 5.64L16.6 7.4M7.4 16.6l-1.76 1.76"
        case .volume: "M11 5L6 9H3v6h3l5 4V5zM15.5 9a4 4 0 010 6M18.5 6a8 8 0 010 12"
        case .volumeOff: "M11 5L6 9H3v6h3l5 4V5zM17 9l4 6M21 9l-4 6"
        case .fullscreen: "M3 9V3h6M21 9V3h-6M3 15v6h6M21 15v6h-6"
        case .record: "M12 7a5 5 0 100 10 5 5 0 000-10z"
        case .lock: "M5 11h14v10H5zM8 11V7a4 4 0 018 0v4"
        case .clock: "M12 3a9 9 0 100 18 9 9 0 000-18zM12 7v5l3 2"
        case .bell: "M18 8a6 6 0 10-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9M10 21h4"
        case .star: "M12 3l2.9 5.9 6.5.9-4.7 4.6 1.1 6.5-5.8-3-5.8 3 1.1-6.5L2.6 9.8l6.5-.9z"
        case .thumbUp: "M7 10v11H3V10zM7 10l5-7a2 2 0 013 2l-1 5h5a2 2 0 012 2.4l-1.6 7A2 2 0 0117 21H7"
        case .thumbDown: "M17 14V3h4v11zM17 14l-5 7a2 2 0 01-3-2l1-5H5a2 2 0 01-2-2.4l1.6-7A2 2 0 017 3h10"
        case .close: "M6 6l12 12M18 6L6 18"
        case .skip: "M5 4l10 8-10 8zM19 4v16"
        case .back10: "M4 11a8 8 0 11.6 4M4 5v6h6"
        case .forward10: "M20 11a8 8 0 10-.6 4M20 5v6h-6"
        case .subtitles: "M3 5h18v14H3zM7 11h4M13 11h4M7 15h10"
        case .audio: "M12 3v18M8 7v10M16 7v10M4 10v4M20 10v4"
        case .pip: "M3 5h18v14H3zM12 12h7v5h-7z"
        case .cast: "M3 16a4 4 0 014 4M3 12a8 8 0 018 8M3 8a12 12 0 0112 12M3 4h18v14h-6"
        case .layers: "M12 4.5l9 5-9 5-9-5zM3 14.5l9 5 9-5"
        case .sparkle: "M12 5l1.8 5.2L19 12l-5.2 1.8L12 19l-1.8-5.2L5 12l5.2-1.8z"
        case .heart: "M12 18.5s-7-4.4-7-9a4 4 0 017-2.6A4 4 0 0119 9.5c0 4.6-7 9-7 9z"
        case .person: "M12 11a4 4 0 100-8 4 4 0 000 8zM4 21a8 8 0 0116 0"
        case .keyboard: "M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10"
        case .alert: "M12 9v5M12 17h.01M10.3 3.9L1.8 18a2 2 0 001.7 3h17a2 2 0 001.7-3L13.7 3.9a2 2 0 00-3.4 0z"
        case .sun: "M12 8a4 4 0 100 8 4 4 0 000-8zM12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M19.1 4.9l-1.4 1.4M6.3 17.7l-1.4 1.4"
        case .moon: "M20 14.5A8 8 0 019.5 4 8 8 0 1020 14.5z"
        // Apple's own: the phone's More tab (Windows' window has room for every tab).
        case .more: "M5 10.6a1.4 1.4 0 100 2.8 1.4 1.4 0 000-2.8zM12 10.6a1.4 1.4 0 100 2.8 1.4 1.4 0 000-2.8zM19 10.6a1.4 1.4 0 100 2.8 1.4 1.4 0 000-2.8z"
            }
        }
    }

    let name: Name
    var size: CGFloat = 20
    var strokeWidth: CGFloat = 1.8
    var filled = false

    var body: some View {
        let shape = SVGPathShape(path: name.path)
        Group {
            if filled {
                shape.fill(style: FillStyle(eoFill: false))
            } else {
                // The stroke is in the view box's units, as SVG's is.
                shape.stroke(style: StrokeStyle(lineWidth: strokeWidth * size / 24, lineCap: .round, lineJoin: .round))
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

/// An SVG path's `d`, in a 24-point view box scaled to the frame. Covers what Windows'
/// icons use: moves, lines, cubic curves (with their shorthand) and arcs, absolute and
/// relative.
struct SVGPathShape: Shape {
    let path: String

    func path(in rect: CGRect) -> Path {
        let scale = min(rect.width, rect.height) / 24
        return Self.parse(path).applying(CGAffineTransform(scaleX: scale, y: scale)
            .concatenating(CGAffineTransform(translationX: rect.minX, y: rect.minY)))
    }

    static func parse(_ d: String) -> Path {
        var tokens = Tokens(d)
        var path = Path()
        var current = CGPoint.zero
        var start = CGPoint.zero
        var lastControl: CGPoint?
        var command: Character = "M"
        while let next = tokens.command() ?? (tokens.hasNumber ? command : nil) {
            // A repeated move is a line, as SVG has it.
            let isRepeat = !tokens.consumedCommand
            command = (isRepeat && next == "M") ? "L" : (isRepeat && next == "m") ? "l" : next
            let relative = command.isLowercase
            let origin = relative ? current : .zero
            func point() -> CGPoint {
                let x = tokens.number(), y = tokens.number()
                return CGPoint(x: origin.x + x, y: origin.y + y)
            }
            var control: CGPoint?
            switch command.uppercased() {
            case "M":
                current = point(); start = current
                path.move(to: current)
            case "L":
                current = point(); path.addLine(to: current)
            case "H":
                current = CGPoint(x: (relative ? current.x : 0) + tokens.number(), y: current.y)
                path.addLine(to: current)
            case "V":
                current = CGPoint(x: current.x, y: (relative ? current.y : 0) + tokens.number())
                path.addLine(to: current)
            case "C":
                let c1 = point(), c2 = point(), end = point()
                path.addCurve(to: end, control1: c1, control2: c2)
                control = c2; current = end
            case "S":
                let c1 = lastControl.map { CGPoint(x: 2 * current.x - $0.x, y: 2 * current.y - $0.y) } ?? current
                let c2 = point(), end = point()
                path.addCurve(to: end, control1: c1, control2: c2)
                control = c2; current = end
            case "A":
                let rx = tokens.number(), ry = tokens.number()
                _ = tokens.number() // rotation: Windows' arcs are circular
                let large = tokens.flag(), sweep = tokens.flag()
                let end = point()
                arc(&path, from: current, to: end, radius: max(rx, ry), large: large, sweep: sweep)
                current = end
            case "Z":
                path.closeSubpath(); current = start
            default:
                return path
            }
            lastControl = control
        }
        return path
    }

    /// SVG's endpoint arc on a circle, by way of its centre.
    private static func arc(_ path: inout Path, from p1: CGPoint, to p2: CGPoint, radius: CGFloat, large: Bool, sweep: Bool) {
        let half = CGPoint(x: (p1.x - p2.x) / 2, y: (p1.y - p2.y) / 2)
        let d2 = half.x * half.x + half.y * half.y
        guard d2 > 0 else { return }
        let r = max(radius, d2.squareRoot())
        let k = ((r * r - d2) / d2).squareRoot() * (large == sweep ? -1 : 1)
        let centre = CGPoint(x: k * half.y + (p1.x + p2.x) / 2, y: -k * half.x + (p1.y + p2.y) / 2)
        let a1 = atan2(p1.y - centre.y, p1.x - centre.x)
        let a2 = atan2(p2.y - centre.y, p2.x - centre.x)
        // A positive sweep runs the way the angle grows, which on a y-down canvas is
        // what Core Graphics calls counter-clockwise.
        path.addArc(center: centre, radius: r, startAngle: .radians(a1), endAngle: .radians(a2), clockwise: !sweep)
    }

    private struct Tokens {
        let chars: [Character]
        var i = 0
        var consumedCommand = false
        init(_ d: String) { chars = Array(d) }

        mutating func skip() { while i < chars.count, chars[i] == " " || chars[i] == "," || chars[i] == "\n" { i += 1 } }

        var hasNumber: Bool {
            var j = i
            while j < chars.count, chars[j] == " " || chars[j] == "," { j += 1 }
            return j < chars.count && (chars[j].isNumber || chars[j] == "-" || chars[j] == "." || chars[j] == "+")
        }

        mutating func command() -> Character? {
            skip()
            guard i < chars.count, chars[i].isLetter else { consumedCommand = false; return nil }
            consumedCommand = true
            defer { i += 1 }
            return chars[i]
        }

        /// One digit: arc flags may run into what follows (`a3 3 0 100 6`).
        mutating func flag() -> Bool {
            skip()
            guard i < chars.count else { return false }
            defer { i += 1 }
            return chars[i] == "1"
        }

        mutating func number() -> CGFloat {
            skip()
            var s = ""
            if i < chars.count, chars[i] == "-" || chars[i] == "+" { s.append(chars[i]); i += 1 }
            var dot = false
            while i < chars.count {
                let c = chars[i]
                if c.isNumber { s.append(c) }
                else if c == ".", !dot { dot = true; s.append(c) }
                else if (c == "e" || c == "E"), !s.isEmpty {
                    s.append(c); i += 1
                    if i < chars.count, chars[i] == "-" || chars[i] == "+" { s.append(chars[i]) } else { continue }
                }
                else { break }
                i += 1
            }
            return CGFloat(Double(s) ?? 0)
        }
    }
}
