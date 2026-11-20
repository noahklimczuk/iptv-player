import TVServices

/// The Top Shelf for Aurora: Continue Watching, from the snapshot the app leaves in the
/// app group (`TopShelf.swift`). Choosing a title opens its page in the app; Play does
/// too, since resuming asks where to start, as it does inside the app.
final class ContentProvider: TVTopShelfContentProvider {
    override func loadTopShelfContent() async -> (any TVTopShelfContent)? {
        guard let snapshot = TopShelfSnapshot.read(), !snapshot.items.isEmpty else { return nil }
        // One shape for the row: wide art when every title has some, else the posters,
        // which a 16:9 frame would crop to a strip of their middle.
        let wide = snapshot.items.allSatisfy { $0.backdrop != nil }
        let items = snapshot.items.map { entry -> TVTopShelfSectionedItem in
            let item = TVTopShelfSectionedItem(identifier: "\(entry.kind.rawValue)-\(entry.id)")
            item.title = entry.title
            item.imageShape = wide ? .hdtv : .poster
            let art = wide ? entry.backdrop : (entry.poster ?? entry.backdrop)
            if let image = art.flatMap(URL.init(string:)) {
                item.setImageURL(image, for: [.screenScale1x, .screenScale2x])
            }
            if let progress = entry.progress { item.playbackProgress = progress }
            if let url = TopShelfSnapshot.url(for: entry) {
                item.displayAction = TVTopShelfAction(url: url)
                item.playAction = TVTopShelfAction(url: url)
            }
            return item
        }
        let section = TVTopShelfItemCollection(items: items)
        section.title = "Continue Watching"
        return TVTopShelfSectionedContent(sections: [section])
    }
}
