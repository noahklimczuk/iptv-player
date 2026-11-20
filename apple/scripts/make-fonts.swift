#!/usr/bin/env swift
// Aurora's typefaces for Apple, the same files Windows draws with: Manrope for the
// interface and Bricolage Grotesque for display headings (src-ui/src/styles/tokens.css,
// `--font-ui` and `--font-display`). Windows bundles them through fontsource as WOFF2,
// which iOS and tvOS can't register, so this rebuilds each as a plain TrueType file:
// CoreText reads the WOFF2, and every table it holds (the variation tables included,
// so every weight stays available) is copied into a new sfnt.
//
//   swift apple/scripts/make-fonts.swift     (after `pnpm install` at the repo root)
//
// Writes apple/Shared/Fonts/*.ttf, which are committed; run it again only when Windows
// moves to a new version of either font. Both are SIL Open Font License 1.1.
import CoreText
import Foundation

let appleDir = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent().deletingLastPathComponent()
let modules = appleDir.deletingLastPathComponent().appendingPathComponent("node_modules/@fontsource-variable")
let out = appleDir.appendingPathComponent("Shared/Fonts")

// The Latin subset, as Windows' `wght.css` loads first: everything the interface writes.
let fonts: [(source: String, file: String)] = [
    ("manrope/files/manrope-latin-wght-normal.woff2", "Manrope.ttf"),
    ("bricolage-grotesque/files/bricolage-grotesque-latin-wght-normal.woff2", "BricolageGrotesque.ttf"),
]

func be32(_ v: UInt32) -> Data { withUnsafeBytes(of: v.bigEndian) { Data($0) } }
func be16(_ v: UInt16) -> Data { withUnsafeBytes(of: v.bigEndian) { Data($0) } }

/// The sfnt checksum: the sum of the big-endian 32-bit words, the last one zero-padded.
func checksum(_ data: Data) -> UInt32 {
    var padded = data
    while padded.count % 4 != 0 { padded.append(0) }
    var sum: UInt32 = 0
    padded.withUnsafeBytes { raw in
        for i in stride(from: 0, to: raw.count, by: 4) {
            sum = sum &+ UInt32(raw[i]) << 24 &+ UInt32(raw[i + 1]) << 16 &+ UInt32(raw[i + 2]) << 8 &+ UInt32(raw[i + 3])
        }
    }
    return sum
}

func tagString(_ tag: UInt32) -> String {
    String(bytes: [24, 16, 8, 0].map { UInt8((tag >> $0) & 0xFF) }, encoding: .ascii) ?? "?"
}

try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

for (source, file) in fonts {
    let data = try Data(contentsOf: modules.appendingPathComponent(source))
    let descriptors = CTFontManagerCreateFontDescriptorsFromData(data as CFData)
    guard CFArrayGetCount(descriptors) > 0 else { fatalError("CoreText could not read \(source)") }
    let descriptor = unsafeBitCast(CFArrayGetValueAtIndex(descriptors, 0), to: CTFontDescriptor.self)
    let font = CTFontCreateWithFontDescriptor(descriptor, 12, nil)
    guard let tagArray = CTFontCopyAvailableTables(font, []) else { fatalError("no tables in \(source)") }
    let tags = (0..<CFArrayGetCount(tagArray))
        .map { UInt32(UInt(bitPattern: CFArrayGetValueAtIndex(tagArray, $0))) }
        .sorted()
    var tables: [(tag: UInt32, data: Data)] = tags.compactMap { tag in
        guard let table = CTFontCopyTable(font, CTFontTableTag(tag), []) else { return nil }
        return (tag, table as Data)
    }
    let headTag: UInt32 = 0x6865_6164 // 'head'
    // checkSumAdjustment (bytes 8–11 of 'head') is worked out over the finished file.
    if let i = tables.firstIndex(where: { $0.tag == headTag }) {
        tables[i].data.replaceSubrange(8..<12, with: Data(count: 4))
    }

    let count = UInt16(tables.count)
    var power: UInt16 = 1, log: UInt16 = 0
    while power * 2 <= count { power *= 2; log += 1 }
    let glyf = tags.contains(0x676C_7966) // 'glyf': TrueType outlines, else CFF
    var sfnt = Data()
    sfnt.append(be32(glyf ? 0x0001_0000 : 0x4F54_544F))
    for field in [count, power * 16, log, count * 16 - power * 16] { sfnt.append(be16(field)) }
    var offset = UInt32(12 + 16 * tables.count)
    var body = Data()
    var headOffset: Int?
    for table in tables {
        if table.tag == headTag { headOffset = Int(offset) }
        for field in [table.tag, checksum(table.data), offset, UInt32(table.data.count)] { sfnt.append(be32(field)) }
        var padded = table.data
        while padded.count % 4 != 0 { padded.append(0) }
        body.append(padded)
        offset += UInt32(padded.count)
    }
    sfnt.append(body)
    if let at = headOffset {
        let adjustment: UInt32 = 0xB1B0_AFBA &- checksum(sfnt)
        sfnt.replaceSubrange((at + 8)..<(at + 12), with: be32(adjustment))
    }

    // Read back, so a file iOS would refuse is never written.
    let check = CTFontManagerCreateFontDescriptorsFromData(sfnt as CFData)
    guard CFArrayGetCount(check) > 0 else { fatalError("rebuilt \(file) does not load") }
    let back = CTFontCreateWithFontDescriptor(unsafeBitCast(CFArrayGetValueAtIndex(check, 0), to: CTFontDescriptor.self), 12, nil)
    let axes = (CTFontCopyVariationAxes(back) as? [Any])?.count ?? 0
    try sfnt.write(to: out.appendingPathComponent(file))
    print("wrote \(file): \(CTFontCopyFamilyName(back)), \(tables.count) tables (\(tags.map(tagString).joined(separator: " "))), \(axes) axis, \(sfnt.count) bytes")
}

// The licence travels with the fonts.
for (package, name) in [("manrope", "Manrope"), ("bricolage-grotesque", "BricolageGrotesque")] {
    let licence = modules.appendingPathComponent("\(package)/LICENSE")
    let target = out.appendingPathComponent("\(name)-OFL.txt")
    try? FileManager.default.removeItem(at: target)
    try FileManager.default.copyItem(at: licence, to: target)
}
