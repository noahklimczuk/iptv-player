#if DEBUG
import Foundation

/// Launch arguments for trying the app without tapping through it: in a simulator, from
/// a script, or in CI. Debug builds only; a release build has none of this.
///
///     xcrun simctl launch <device> tv.aurora.mobile \
///       -AuroraDemoPlaylist http://127.0.0.1:8000/demo.m3u -AuroraAutoplay YES
///
/// `-AuroraDemoPlaylist <url>` adds that M3U and imports it (live TV only) when the
/// library has no providers; `-AuroraDemoXtreamURL`, `-AuroraDemoXtreamUser` and
/// `-AuroraDemoXtreamPass` do the same for an Xtream panel (`apple/scripts/sim-demo.sh`
/// reads them from a local file). `-AuroraAutoplay YES` then plays the first channel.
/// Arguments of the form `-Key value` land in UserDefaults' argument domain, so nothing
/// here parses the command line.
enum DemoLaunch {
    @MainActor
    static func prepare(_ model: AppModel) async -> PlayerItem? {
        let defaults = UserDefaults.standard
        let liveOnly = ImportContent(live: true, vod: false, series: false)
        let playlist = defaults.string(forKey: "AuroraDemoPlaylist")
        let panel = defaults.string(forKey: "AuroraDemoXtreamURL")
        guard playlist != nil || panel != nil else { return nil }
        await model.reload()
        if model.providers.isEmpty {
            if let panel, let user = defaults.string(forKey: "AuroraDemoXtreamUser"),
               let pass = defaults.string(forKey: "AuroraDemoXtreamPass") {
                await model.addXtream(name: "Test panel", url: panel, username: user, password: pass,
                                      content: liveOnly)
            } else if let playlist {
                await model.addM3u(name: "Demo", url: playlist, content: liveOnly)
            }
        }
        guard defaults.bool(forKey: "AuroraAutoplay"),
              let first = await model.channels(group: nil, limit: 1).first
        else { return nil }
        return .channel(id: first.id, name: first.name)
    }
}
#endif
