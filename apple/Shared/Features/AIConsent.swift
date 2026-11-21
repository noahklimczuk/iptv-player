import SwiftUI

/// Asking before anything goes to Google's Gemini: an Apple-only departure.
///
/// Windows turns the "Because of what you watch" rail and the assistant on out of the
/// box, with the key a release compiles in, and says so in Settings. The App Store
/// requires an app to ask before it shares personal data with a third-party AI
/// (guideline 5.1.2(i)), so on Apple the switch starts off (`gemini.rs`
/// `DEFAULT_ENABLED`) and this card asks, once, on Home; the assistant asks the same
/// way. Settings → Recommendations keeps Windows' switch to change the answer.
struct AIConsentCard: View {
    @Environment(AppModel.self) private var model
    @Environment(\.auroraTheme) private var theme
    /// Called once it is answered either way, so the card goes.
    let answered: () -> Void

    /// Set once the card has been answered, so it is never shown again on this device.
    static let answeredKey = "AuroraAIConsentAnswered"

    static var wasAnswered: Bool {
        UserDefaults.standard.bool(forKey: answeredKey)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Label("Picks from what you watch", systemImage: "sparkles")
                .font(.display(.title3))
            AIConsentText()
            HStack(spacing: 12) {
                Button("Turn on") {
                    Task {
                        await model.setGeminiEnabled(true)
                        finish()
                    }
                }
                .buttonStyle(DaylightButtonStyle(variant: .primary))
                Button("Not now") { finish() }
                    .buttonStyle(DaylightButtonStyle(variant: .secondary))
            }
        }
        .padding(20)
        .frame(maxWidth: 720, alignment: .leading)
        .daylightRaised(theme, radius: 20)
    }

    private func finish() {
        UserDefaults.standard.set(true, forKey: Self.answeredKey)
        answered()
    }
}

/// What is sent, said the same way wherever Aurora asks.
struct AIConsentText: View {
    var body: some View {
        Text("Aurora can ask Google's Gemini for films and shows like the ones you watch, and answer questions about your library in Ask Aurora. To do that it sends Google the titles you have watched, with their years and genres, how much of each you watched and whether you liked them, and anything you type to Ask Aurora. Nothing about your provider or your sign-ins is sent. You can turn this off in Settings → Recommendations at any time.")
            .font(.ui(.callout))
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }
}
