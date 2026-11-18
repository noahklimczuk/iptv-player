import SwiftUI

/// A channel's logo in a rounded square: Windows' `ChannelTile` (`Primitives.tsx`).
///
/// With a logo, the artwork sits on a sunk tile so a transparent PNG keeps an edge; with
/// none, the tile takes the channel's hue (`artHue`) and carries its number, or the
/// first letter of its name, which is what a viewer zaps by anyway.
struct ChannelTile: View {
    let name: String
    let number: UInt32?
    let logo: String?
    var size: CGFloat = 48

    var body: some View {
        AsyncImage(url: logo.flatMap(URL.init(string:))) { phase in
            if let image = phase.image {
                image.resizable().scaledToFit()
                    .frame(width: size * 0.78, height: size * 0.78)
                    .frame(width: size, height: size)
                    .background(.quaternary)
            } else {
                Text(number.map(String.init) ?? String(name.prefix(1)).uppercased())
                    .font(.system(size: size * 0.32, weight: .heavy))
                    .monospacedDigit()
                    .foregroundStyle(.white)
                    .frame(width: size, height: size)
                    .background(Self.hue(name))
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: (size * 0.3).rounded()))
        .accessibilityHidden(true)
    }

    /// Windows' `--art-1`…`--art-7`.
    private static let hues: [Color] = [
        Color(red: 0x2B / 255, green: 0x4B / 255, blue: 0xFF / 255),
        Color(red: 0xFF / 255, green: 0x5A / 255, blue: 0x36 / 255),
        Color(red: 0x14 / 255, green: 0xA3 / 255, blue: 0x7F / 255),
        Color(red: 0xFF / 255, green: 0xB0 / 255, blue: 0x20 / 255),
        Color(red: 0x7B / 255, green: 0x3F / 255, blue: 0xF2 / 255),
        Color(red: 0xE8 / 255, green: 0x41 / 255, blue: 0x7C / 255),
        Color(red: 0x0F / 255, green: 0x8B / 255, blue: 0xD6 / 255),
    ]

    /// `artHue`: a 32-bit `h * 31 + code unit` over the name's UTF-16, as JavaScript
    /// computes it, so a channel has the same colour on every screen and both apps.
    static func hue(_ name: String) -> Color {
        var h: Int32 = 0
        for unit in name.utf16 { h = h &* 31 &+ Int32(unit) }
        return hues[Int(h.magnitude % 7)]
    }
}
