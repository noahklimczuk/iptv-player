#if os(iOS)
import SwiftUI
import WebKit

/// A trailer, playing: Windows' `TrailerFrame`, YouTube's embed in a web view.
///
/// As on Windows, the embed and not mpv: TMDB gives a YouTube id and nothing else, and
/// mpv cannot open a YouTube page without yt-dlp beside it. iPhone and iPad only: tvOS
/// has no web view, so an Apple TV shows the backdrop as before (apple/README.md).
///
/// The page is served as the app's own origin, `https://<bundle id>`: YouTube refuses to
/// play an embed that arrives with no referrer (error 153), and that origin is what it
/// asks apps to send. Nothing is kept between trailers: the web view's store is not
/// persistent, and the `-nocookie` host sets no tracking cookie.
struct TrailerFrame: UIViewRepresentable {
    /// The YouTube id. The caller has already decided there is one.
    let key: String
    /// Muted autoplay is the only autoplay a page gets by itself, so a preview starts
    /// silent. A change after it opens is sent as a command, so unmuting does not start
    /// the trailer again.
    var muted: Bool
    /// YouTube's own controls: off for a preview, on when the trailer is the subject.
    var controls = false
    /// Round again at the end: a preview does; one somebody asked for does not.
    var loop = true

    static let embedOrigin = "https://www.youtube-nocookie.com"
    static let pageOrigin = "https://\(Bundle.main.bundleIdentifier ?? "io.github.noahklimczuk.aurora")"

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.allowsInlineMediaPlayback = true
        config.mediaTypesRequiringUserActionForPlayback = []
        config.allowsPictureInPictureMediaPlayback = false
        config.websiteDataStore = .nonPersistent()
        let web = WKWebView(frame: .zero, configuration: config)
        web.isOpaque = false
        web.backgroundColor = .black
        web.scrollView.backgroundColor = .black
        web.scrollView.isScrollEnabled = false
        web.scrollView.contentInsetAdjustmentBehavior = .never
        // A preview is scenery: a tap belongs to the billboard under it.
        web.isUserInteractionEnabled = controls
        web.navigationDelegate = context.coordinator
        web.uiDelegate = context.coordinator
        web.accessibilityLabel = "Trailer"
        context.coordinator.muted = muted
        web.loadHTMLString(page, baseURL: URL(string: Self.pageOrigin))
        return web
    }

    func updateUIView(_ web: WKWebView, context: Context) {
        web.isUserInteractionEnabled = controls
        guard context.coordinator.muted != muted else { return }
        context.coordinator.muted = muted
        web.evaluateJavaScript("send('\(muted ? "mute" : "unMute")')")
    }

    /// Gone means silent at once, not when the web view is next freed.
    static func dismantleUIView(_ web: WKWebView, coordinator: Coordinator) {
        web.stopLoading()
        web.loadHTMLString("", baseURL: nil)
    }

    /// Windows' parameters, each for the same reason: autoplay, the starting mute, no
    /// controls on a preview, `loop` with the id in `playlist` (YouTube needs both), no
    /// logo or related videos, inline rather than full screen, no keyboard, no
    /// annotations, and the iframe API for mute and unmute.
    private var embedURL: String {
        let id = key.addingPercentEncoding(withAllowedCharacters: .alphanumerics.union(.init(charactersIn: "-_"))) ?? key
        var url = "\(Self.embedOrigin)/embed/\(id)?autoplay=1&mute=\(muted ? 1 : 0)&controls=\(controls ? 1 : 0)"
        if loop { url += "&loop=1&playlist=\(id)" }
        url += "&modestbranding=1&rel=0&playsinline=1&disablekb=1&iv_load_policy=3&enablejsapi=1"
        url += "&origin=\(Self.pageOrigin)"
        return url
    }

    /// The embed in a page of its own, filling it, and `send` for the iframe API: a
    /// command before the frame has loaded waits for it (Windows' `ready`).
    private var page: String {
        """
        <!doctype html><html><head>
        <meta name="viewport" content="width=device-width,initial-scale=1,user-scalable=no">
        <style>html,body{margin:0;height:100%;background:#000;overflow:hidden}
        iframe{position:absolute;top:0;left:0;width:100%;height:100%;border:0}</style>
        </head><body>
        <iframe id="t" src="\(embedURL)" allow="autoplay; encrypted-media"
                referrerpolicy="strict-origin-when-cross-origin"></iframe>
        <script>
        var frame = document.getElementById('t'), ready = false, wanted = null;
        frame.addEventListener('load', function () { ready = true; if (wanted) send(wanted); });
        function send(command) {
          wanted = command;
          if (!ready) return;
          try {
            frame.contentWindow.postMessage(JSON.stringify({event: 'command', func: command, args: []}),
                                            '\(Self.embedOrigin)');
          } catch (e) {}
        }
        </script>
        </body></html>
        """
    }

    final class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate {
        var muted = true

        /// The page and the embed load here; anything a tap opens (YouTube's logo, "Watch
        /// on YouTube") opens outside the app rather than replacing the trailer.
        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                     decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void) {
            if action.navigationType == .linkActivated, let url = action.request.url {
                UIApplication.shared.open(url)
                decisionHandler(.cancel)
            } else {
                decisionHandler(.allow)
            }
        }

        func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                     for action: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
            if let url = action.request.url { UIApplication.shared.open(url) }
            return nil
        }
    }
}
#endif
