import SwiftUI

/// The Apple TV app. Everything but this entry point is in `Shared/`, used by both
/// targets (docs/APPLE_PORT_AND_SYNC.md §2.1).
@main
struct AuroraTVApp: App {
    @State private var model = AppModel()

    init() {
        // Windows' typefaces in the bars UIKit draws (Typography.swift).
        AuroraFont.installAppearance()
    }

    var body: some Scene {
        WindowGroup {
            RootView().environment(model)
        }
    }
}
