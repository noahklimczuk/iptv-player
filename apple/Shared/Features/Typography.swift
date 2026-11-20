import CoreText
import SwiftUI
#if canImport(UIKit)
import UIKit
#endif

/// Windows' typefaces (src-ui/src/styles/tokens.css): `--font-ui`, Manrope, for every
/// word of the interface, and `--font-display`, Bricolage Grotesque at weight 800 drawn
/// a little tight (`--tracking-display`), for headings (`dl-display`, `dl-page-title`).
/// The files are Windows' own, rebuilt as TrueType by scripts/make-fonts.swift and
/// listed in each app's `UIAppFonts`.
///
/// Fonts are named by an instance's PostScript name: the rebuilt files carry the family
/// names fontsource gave them ("Manrope ExtraLight"), which a family lookup would not
/// find. From that instance `.weight`, `.bold()` and `.fontWeight` reach every other
/// weight of the variable font.
enum AuroraFont {
    /// Manrope at 500, Windows' body weight (`body { font-weight: 500 }`).
    static let ui = "Manrope-Medium"
    /// Bricolage Grotesque at 800, the only weight Windows draws it in.
    static let display = "BricolageGrotesque96ptExtraBold-Regular"
    /// `--tracking-display`, as a share of the size.
    static let displayTracking: CGFloat = -0.04

    /// A text style's size at the default text size, which `relativeTo:` then scales
    /// with Dynamic Type as the system font would.
    static func baseSize(_ style: Font.TextStyle) -> CGFloat {
        #if canImport(UIKit)
        let trait = UITraitCollection(preferredContentSizeCategory: .large)
        return UIFont.preferredFont(forTextStyle: style.uiKit, compatibleWith: trait).pointSize
        #else
        return 17
        #endif
    }

    #if canImport(UIKit)
    /// The bars UIKit draws for SwiftUI: navigation titles and the search field.
    @MainActor static func installAppearance() {
        let title = UIFont(name: display, size: baseSize(.largeTitle)) ?? .preferredFont(forTextStyle: .title1)
        let inline = UIFont(name: ui, size: baseSize(.headline))?.withWeight(.heavy)
            ?? .preferredFont(forTextStyle: .headline)
        let bar = UINavigationBar.appearance()
        bar.titleTextAttributes = [.font: inline]
        #if os(iOS)
        bar.largeTitleTextAttributes = [.font: title, .kern: title.pointSize * displayTracking]
        #endif
        if let field = UIFont(name: ui, size: baseSize(.body)) {
            UITextField.appearance(whenContainedInInstancesOf: [UISearchBar.self]).font = field
        }
    }
    #endif
}

extension Font {
    /// Manrope at a fixed size, for what was `.system(size:weight:design:)`. A monospaced
    /// design stays the system's: Windows has no monospace of its own.
    static func ui(size: CGFloat, weight: Font.Weight? = nil, design: Font.Design? = nil) -> Font {
        if design == .monospaced { return .system(size: size, weight: weight ?? .regular, design: .monospaced) }
        let font = Font.custom(AuroraFont.ui, fixedSize: size)
        return weight.map { font.weight($0) } ?? font
    }

    /// Manrope at a weight between the named ones: CSS weights such as the tab bar's 650,
    /// set on the variable font's axis.
    static func ui(size: CGFloat, wght: CGFloat) -> Font {
        let descriptor = CTFontDescriptorCreateWithAttributes([
            kCTFontNameAttribute: AuroraFont.ui,
            kCTFontVariationAttribute: [0x7767_6874 as UInt32: wght], // 'wght'
        ] as CFDictionary)
        return Font(CTFontCreateWithFontDescriptor(descriptor, size, nil))
    }

    /// Manrope at a text style's size, following Dynamic Type; the headline keeps its
    /// semibold, as the system's does.
    static func ui(_ style: Font.TextStyle) -> Font {
        let font = Font.custom(AuroraFont.ui, size: AuroraFont.baseSize(style), relativeTo: style)
        return style == .headline ? font.weight(.semibold) : font
    }

    /// Bricolage Grotesque at 800 at a fixed size. Pair it with `.displayTracking(size)`
    /// for Windows' spacing, or use `Text.display`.
    static func display(size: CGFloat) -> Font { .custom(AuroraFont.display, fixedSize: size) }

    /// Bricolage Grotesque at 800 at a text style's size.
    static func display(_ style: Font.TextStyle) -> Font {
        .custom(AuroraFont.display, size: AuroraFont.baseSize(style), relativeTo: style)
    }
}

extension View {
    /// Windows' `dl-display`: Bricolage Grotesque at 800, tracked in by
    /// `--tracking-display`.
    func displayFont(size: CGFloat) -> some View {
        font(.display(size: size)).tracking(size * AuroraFont.displayTracking)
    }

    /// `displayFont`, at a text style's size.
    func displayFont(_ style: Font.TextStyle) -> some View {
        font(.display(style)).tracking(AuroraFont.baseSize(style) * AuroraFont.displayTracking)
    }
}

#if canImport(UIKit)
private extension Font.TextStyle {
    var uiKit: UIFont.TextStyle {
        switch self {
        #if os(tvOS)
        case .largeTitle: .title1
        #else
        case .largeTitle: .largeTitle
        #endif
        case .title: .title1
        case .title2: .title2
        case .title3: .title3
        case .headline: .headline
        case .subheadline: .subheadline
        case .callout: .callout
        case .footnote: .footnote
        case .caption: .caption1
        case .caption2: .caption2
        default: .body
        }
    }
}

private extension UIFont {
    /// Another weight of the same variable font.
    func withWeight(_ weight: UIFont.Weight) -> UIFont {
        let traits = [UIFontDescriptor.TraitKey.weight: weight]
        return UIFont(descriptor: fontDescriptor.addingAttributes([.traits: traits]), size: pointSize)
    }
}
#endif
