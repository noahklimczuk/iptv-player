#if DEBUG
import Foundation

/// Launch arguments for trying the app without tapping through it: in a simulator, from
/// a script, or in CI. Debug builds only; a release build has none of this.
///
///     xcrun simctl launch <device> tv.aurora.mobile \
///       -AuroraDemoPlaylist http://127.0.0.1:8000/demo.m3u -AuroraAutoplay YES
///
/// `-AuroraDemoPlaylist <url>` adds that M3U and imports it (live TV only) when the
/// library has no M3U provider; `-AuroraDemoXtreamURL`, `-AuroraDemoXtreamUser` and
/// `-AuroraDemoXtreamPass` do the same for an Xtream panel (`apple/scripts/sim-demo.sh`
/// reads them from a local file). `-AuroraDemoContent all` imports films and series too,
/// and adds them to a library that so far has only live TV. `-AuroraAutoplay YES` then
/// plays the first channel. `-AuroraPlayEpisode <id>` opens that episode, at
/// `-AuroraStartAt <seconds>` when given: for trying skip markers and Up Next without
/// sitting through the episode.
/// Arguments of the form `-Key value` land in UserDefaults' argument domain, so nothing
/// here parses the command line.
enum DemoLaunch {
    @MainActor
    static func prepare(_ model: AppModel) async -> PlayerItem? {
        let defaults = UserDefaults.standard
        if let episode = defaults.object(forKey: "AuroraPlayEpisode") as? String, let id = Int64(episode) {
            let at = (defaults.object(forKey: "AuroraStartAt") as? String).flatMap(Double.init)
            return .episode(id: id, title: "Episode \(id)", startAt: at)
        }
        let everything = defaults.string(forKey: "AuroraDemoContent") == "all"
        let content = ImportContent(live: true, vod: everything, series: everything)
        let playlist = defaults.string(forKey: "AuroraDemoPlaylist")
        let panel = defaults.string(forKey: "AuroraDemoXtreamURL")
        guard playlist != nil || panel != nil else { return nil }
        await model.reload()
        let login = panel.flatMap { url in
            defaults.string(forKey: "AuroraDemoXtreamUser").flatMap { user in
                defaults.string(forKey: "AuroraDemoXtreamPass").map { (url, user, $0) }
            }
        }
        // The provider of the kind asked for, not just the first: a simulator used for
        // both keeps an old local playlist next to the panel, and refreshing that one
        // fails once its server is gone.
        let kind = login != nil ? "xtream" : "m3u"
        if let provider = model.providers.first(where: { $0.kind == kind }) {
            if everything, provider.movieCount == 0 && provider.seriesCount == 0 {
                await model.refresh(providerId: provider.id, content: content)
            }
        } else if let login {
            await model.addXtream(name: "Test panel", url: login.0, username: login.1, password: login.2,
                                  content: content)
        } else if let playlist {
            await model.addM3u(name: "Demo", url: playlist, content: content)
        }
        guard defaults.bool(forKey: "AuroraAutoplay"),
              let first = await model.channels(group: nil, limit: 1).first
        else { return nil }
        return .channel(id: first.id, name: first.name)
    }
}
#endif
