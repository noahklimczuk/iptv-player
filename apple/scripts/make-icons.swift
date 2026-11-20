#!/usr/bin/env swift
// Draws Aurora's icons for Apple from the Windows icon's design
// (src-native/crates/aurora-app/icons/icon.png): the white "A" on the violet-to-teal
// diagonal. Redrawn as vectors rather than scaled up, because the Windows file is 512
// pixels and the Apple TV's shelf image is 4640 wide.
//
//   swift apple/scripts/make-icons.swift
//
// Writes the PNGs into the two targets' asset catalogs. Run it again only if the design
// changes; the PNGs are committed.
import AppKit
import CoreGraphics

let appleDir = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent().deletingLastPathComponent()

// The gradient's ends, fitted to the Windows icon along its diagonal.
let violet = CGColor(srgbRed: 0x91 / 255, green: 0x78 / 255, blue: 0xFE / 255, alpha: 1)
let teal = CGColor(srgbRed: 0x32 / 255, green: 0xD9 / 255, blue: 0xCA / 255, alpha: 1)

// The "A", measured off the Windows icon in its 512-pixel square: the outline, then the
// counter, filled even-odd.
let outline: [CGPoint] = [(256, 102), (409, 409), (330, 409), (312, 366), (200, 366), (182, 409), (103, 409)]
    .map { CGPoint(x: $0.0, y: $0.1) }
let counter: [CGPoint] = [(256, 236), (290, 314), (222, 314)].map { CGPoint(x: $0.0, y: $0.1) }
let glyphBox = CGRect(x: 103, y: 102, width: 306, height: 307)

enum Layer { case all, back, front }

/// One PNG: `glyph` is the A's height as a share of the canvas's.
func render(_ width: Int, _ height: Int, layer: Layer, glyph: CGFloat, to path: String) {
    let opaque = layer != .front
    let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!,
                        bitmapInfo: opaque ? CGImageAlphaInfo.noneSkipLast.rawValue : CGImageAlphaInfo.premultipliedLast.rawValue)!
    // Top-left origin, as the measurements are.
    ctx.translateBy(x: 0, y: CGFloat(height))
    ctx.scaleBy(x: 1, y: -1)
    if layer != .front {
        let gradient = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: [violet, teal] as CFArray,
                                  locations: [0, 1])!
        ctx.drawLinearGradient(gradient, start: .zero, end: CGPoint(x: width, y: height), options: [])
    }
    if layer != .back {
        let scale = CGFloat(height) * glyph / glyphBox.height
        let origin = CGPoint(x: (CGFloat(width) - glyphBox.width * scale) / 2 - glyphBox.minX * scale,
                             y: (CGFloat(height) - glyphBox.height * scale) / 2 - glyphBox.minY * scale)
        let place = { (p: CGPoint) in CGPoint(x: origin.x + p.x * scale, y: origin.y + p.y * scale) }
        let shape = CGMutablePath()
        shape.addLines(between: outline.map(place))
        shape.closeSubpath()
        shape.addLines(between: counter.map(place))
        shape.closeSubpath()
        ctx.addPath(shape)
        ctx.setFillColor(CGColor(gray: 1, alpha: 1))
        ctx.fillPath(using: .evenOdd)
    }
    let url = appleDir.appendingPathComponent(path)
    try! FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    let rep = NSBitmapImageRep(cgImage: ctx.makeImage()!)
    try! rep.representation(using: .png, properties: [:])!.write(to: url)
    print("wrote", path)
}

func json(_ object: Any, to path: String) {
    let url = appleDir.appendingPathComponent(path)
    try! FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    let data = try! JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
    try! data.write(to: url)
}

let info: [String: Any] = ["author": "xcode", "version": 1]

// iPhone and iPad: one 1024-point square, full bleed and opaque; iOS rounds the corners.
let mobile = "AuroraMobile/Assets.xcassets"
json(["info": info], to: "\(mobile)/Contents.json")
render(1024, 1024, layer: .all, glyph: 307.0 / 512, to: "\(mobile)/AppIcon.appiconset/AppIcon.png")
json(["images": [["filename": "AppIcon.png", "idiom": "universal", "platform": "ios", "size": "1024x1024"]], "info": info],
     to: "\(mobile)/AppIcon.appiconset/Contents.json")

// Apple TV: the layered icon (the A floats over the gradient as it is focused), its App
// Store size, and the static shelf images shown when the app has no shelf content.
let tv = "AuroraTV/Assets.xcassets"
let brand = "\(tv)/App Icon & Top Shelf Image.brandassets"
json(["info": info], to: "\(tv)/Contents.json")
json(["assets": [
    ["filename": "App Icon.imagestack", "idiom": "tv", "role": "primary-app-icon", "size": "400x240"],
    ["filename": "App Icon - App Store.imagestack", "idiom": "tv", "role": "primary-app-icon", "size": "1280x768"],
    ["filename": "Top Shelf Image Wide.imageset", "idiom": "tv", "role": "top-shelf-image-wide", "size": "2320x720"],
    ["filename": "Top Shelf Image.imageset", "idiom": "tv", "role": "top-shelf-image", "size": "1920x720"],
], "info": info], to: "\(brand)/Contents.json")

func stack(_ name: String, width: Int, height: Int, scales: [Int]) {
    let dir = "\(brand)/\(name).imagestack"
    json(["info": info, "layers": [["filename": "Front.imagestacklayer"], ["filename": "Back.imagestacklayer"]]],
         to: "\(dir)/Contents.json")
    for (layer, label) in [(Layer.front, "Front"), (Layer.back, "Back")] {
        json(["info": info], to: "\(dir)/\(label).imagestacklayer/Contents.json")
        var images: [[String: String]] = []
        for scale in scales {
            let file = scale == 1 ? "\(label.lowercased()).png" : "\(label.lowercased())@\(scale)x.png"
            render(width * scale, height * scale, layer: layer, glyph: 0.6,
                   to: "\(dir)/\(label).imagestacklayer/Content.imageset/\(file)")
            images.append(["filename": file, "idiom": "tv", "scale": "\(scale)x"])
        }
        json(["images": images, "info": info], to: "\(dir)/\(label).imagestacklayer/Content.imageset/Contents.json")
    }
}
stack("App Icon", width: 400, height: 240, scales: [1, 2])
stack("App Icon - App Store", width: 1280, height: 768, scales: [1])

func shelf(_ name: String, width: Int, height: Int) {
    let dir = "\(brand)/\(name).imageset"
    render(width, height, layer: .all, glyph: 0.45, to: "\(dir)/shelf.png")
    render(width * 2, height * 2, layer: .all, glyph: 0.45, to: "\(dir)/shelf@2x.png")
    json(["images": [["filename": "shelf.png", "idiom": "tv", "scale": "1x"],
                     ["filename": "shelf@2x.png", "idiom": "tv", "scale": "2x"]], "info": info],
         to: "\(dir)/Contents.json")
}
shelf("Top Shelf Image", width: 1920, height: 720)
shelf("Top Shelf Image Wide", width: 2320, height: 720)
