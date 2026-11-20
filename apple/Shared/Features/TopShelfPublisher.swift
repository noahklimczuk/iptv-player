#if os(tvOS)
import TVServices

/// Hands Home's Continue Watching to the Top Shelf (`AuroraTopShelf`): written to the app
/// group when Home reads its rails, and the shelf told to reload only when it changed.
enum TopShelf {
    /// The most the shelf is given: one row's worth on screen, and a few to scroll to.
    private static let limit = 12

    static func publish(_ rails: [Rail]) {
        let watching = rails.first { $0.kind == "continueWatching" }?.items ?? []
        let items = watching.prefix(limit).map { entry -> TopShelfSnapshot.Item in
            let progress = entry.progress.flatMap { p -> Double? in
                p.durationSecs > 0 ? min(Double(p.positionSecs) / Double(p.durationSecs), 1) : nil
            }
            switch entry.item {
            case .movie(let m):
                return .init(kind: .movie, id: m.id, title: m.title, backdrop: m.backdrop, poster: m.poster, progress: progress)
            case .series(let s):
                return .init(kind: .series, id: s.id, title: s.title, backdrop: s.backdrop, poster: s.poster, progress: progress)
            }
        }
        if TopShelfSnapshot(items: Array(items)).write() {
            TVTopShelfContentProvider.topShelfContentDidChange()
        }
    }
}
#endif
