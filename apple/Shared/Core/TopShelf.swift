import Foundation

/// What the Apple TV's Top Shelf shows for Aurora: the Continue Watching rail, as the
/// port's plan has it (docs/APPLE_PORT_AND_SYNC.md §2.4). The app writes a small
/// snapshot to the app group's container whenever Home reads its rails; the extension
/// (`AuroraTopShelf`) reads it, and never opens the library itself.
///
/// Shared with the extension target, so it must not reach for anything else in the app.
struct TopShelfSnapshot: Codable, Equatable {
    struct Item: Codable, Equatable {
        enum Kind: String, Codable { case movie, series }
        let kind: Kind
        let id: Int64
        let title: String
        /// Landscape art first (what the shelf shows), the poster when there is none.
        let image: String?
        /// 0…1, the bar under the picture.
        let progress: Double?
    }

    let items: [Item]

    static let appGroup = "group.tv.aurora.shared"
    private static let fileName = "top-shelf.json"

    /// The shelf's deep link into the app: opens the title's page (`aurora://title/movie/12`).
    static func url(for item: Item) -> URL? {
        URL(string: "aurora://title/\(item.kind.rawValue)/\(item.id)")
    }

    /// The title a deep link names, if it is one of ours.
    static func title(from url: URL) -> (kind: Item.Kind, id: Int64)? {
        guard url.scheme == "aurora", url.host() == "title" else { return nil }
        let parts = url.pathComponents.filter { $0 != "/" }
        guard parts.count == 2, let kind = Item.Kind(rawValue: parts[0]), let id = Int64(parts[1]) else {
            return nil
        }
        return (kind, id)
    }

    private static var fileURL: URL? {
        FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: appGroup)?
            .appendingPathComponent(fileName)
    }

    static func read() -> TopShelfSnapshot? {
        guard let url = fileURL, let data = try? Data(contentsOf: url) else { return nil }
        return try? JSONDecoder().decode(TopShelfSnapshot.self, from: data)
    }

    /// Returns whether it changed, so the shelf is only told to reload when it did.
    @discardableResult
    func write() -> Bool {
        guard let url = Self.fileURL else { return false }
        if Self.read() == self { return false }
        guard let data = try? JSONEncoder().encode(self) else { return false }
        return (try? data.write(to: url, options: .atomic)) != nil
    }
}
