import SwiftUI

/// A view that is drawn again every `every` seconds, as `TimelineView(.periodic(...))`
/// was, with the time in `context.date`.
///
/// Not `TimelineView` itself: in Home's lazy scroll view SwiftUI's timeline asked for a
/// new render from inside each render, so Core Animation never finished laying the page
/// out and scrolling froze for up to fifteen seconds (Instruments on an iPhone,
/// 2026-11-22: every main-thread sample in `_UIHostingView.layoutSubviews`, the only
/// view updated `TimelineView.UpdateFilter`). Here a plain task sets the time, outside
/// any layout pass.
struct Ticking<Content: View>: View {
    let every: Double
    @ViewBuilder let content: (TickContext) -> Content
    @State private var date = Date()

    init(every: Double, @ViewBuilder content: @escaping (TickContext) -> Content) {
        self.every = every
        self.content = content
    }

    var body: some View {
        content(TickContext(date: date))
            .task(id: every) {
                date = Date()
                while !Task.isCancelled {
                    do { try await Task.sleep(for: .seconds(every)) } catch { return }
                    date = Date()
                }
            }
    }
}

/// What `Ticking` hands its content: the time, as a timeline's context has it. Outside
/// `Ticking`, so its type does not hang on the content's.
struct TickContext {
    let date: Date
}
