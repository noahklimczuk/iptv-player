"""
The Phase 0 spike, on a machine with a screen.

Everything else in this repository stops where a picture would appear. `aurora-player`
falls back to `NullBackend` off Windows, the browser journeys drive a mock, and the
Windows CI job compiles the mpv backend without ever running it — so
`AUDIT/release-checklist.md` item 1 has stood open as "run it and look at it", with
three questions under it that no test could answer:

  1. Does video render *behind* the UI, rather than in a window of its own?
  2. Does the UI still receive input while it plays underneath?
  3. Does resizing keep the two surfaces together?

This answers them without a person looking, because each one is a fact about the
machine rather than about a screenshot. A WebDriver capture is no use here — it
photographs the WebView, which is transparent, over nothing at all. So the proof comes
from the desktop and the window tree (`tests-host/winprobe.py`):

  * One visible top-level window for the process, with mpv's surface a `WS_CHILD` of
    it, last in z-order. That is (1) and the shape of (2) — a sibling child behind a
    transparent WebView2 leaves every click going to the UI.
  * Two desktop captures a second apart, with different pixels in the middle of the
    window. That is frames being decoded, composited and presented: the one claim
    `video surface attached` in a log does not make.
  * The child's rectangle after the window is resized. That is (3).

Skipped unless there is a playlist to play, because a real stream is not this
repository's to carry — every fixture host in this tree is `example.com` or a loopback
address, and a public test stream is neither:

    AURORA_TEST_STREAM_M3U=http://127.0.0.1:8123/streams.m3u

and skipped off Windows, where there is no mpv to ask.
"""
import os
import sys
import time

import winprobe as wp

M3U = os.environ.get("AURORA_TEST_STREAM_M3U")

# README §16: a channel change has 1.5 seconds before it feels broken.
ZAP_BUDGET_SECS = 1.5

# How much of a sampled grid has to move between two captures for it to be a moving
# picture rather than a still interface. Real video moves far more than this; the
# threshold is low because a stream is allowed to be showing a near-static shot.
MOTION_FLOOR = 0.01


class Skipped(Exception):
    """Raised when this scenario has nothing to run against."""


def state(d):
    return d.invoke("player_state")


def wake_osd(d):
    """Bring the on-screen display back.

    `PlayerOverlay` hides itself after a few idle seconds — deliberately, since this is
    a television — so its Back button genuinely is not in the DOM by the time a capture
    and two sleeps have gone by. Its root div bumps the timer on `onMouseMove`, and
    React delegates, so a bubbling mousemove on whatever is topmost at the centre of the
    window is the same thing a viewer twitching the mouse does.
    """
    return d.js(
        "const x = Math.floor(innerWidth / 2), y = Math.floor(innerHeight / 2);"
        " const el = document.elementFromPoint(x, y);"
        " if (el) el.dispatchEvent(new MouseEvent('mousemove',"
        " { bubbles: true, clientX: x, clientY: y }));"
        " return el ? el.className || el.tagName : null;"
    )


def wait_for_playing(d, timeout=45):
    """Playing *and* past its first frame — a status alone is not a picture."""
    end = time.time() + timeout
    last = {}
    while time.time() < end:
        last = state(d) or {}
        if last.get("status") == "playing" and last.get("positionSecs", 0) > 0.4:
            return last
        time.sleep(0.2)
    return last


def run(d, ctx):
    if sys.platform != "win32":
        raise Skipped("mpv is Windows-only; this platform has NullBackend")
    if not M3U:
        raise Skipped("no AURORA_TEST_STREAM_M3U in the environment")

    time.sleep(6)

    # Through the wizard with a playlist of real streams.
    d.send(d.find("textarea"), M3U)
    time.sleep(1)
    d.click(d.by_text("button", "Check connection"))
    assert d.wait_body(
        lambda b: "entries" in b.lower() or "could not" in b.lower(), timeout=60
    ), f"the connection check never finished; screen said {d.body()[:300]!r}"
    ctx.assert_no_panic()
    d.click(d.by_text("button", "Continue"))
    d.click(d.by_text("button", "Import library", timeout=30))
    assert d.wait_body(
        lambda b: "your library is ready" in b.lower(), timeout=180
    ), f"the import never finished; screen said {d.body()[:300]!r}"
    d.click(d.by_text("button", "Start watching"))
    time.sleep(3)

    # The log's own account, before anything is asked of the screen. `create_backend`
    # falls back to `NullBackend` when libmpv will not load, which would make every
    # assertion below fail for a reason that has nothing to do with compositing.
    log = ctx.log()
    assert "libmpv unavailable" not in log, (
        "libmpv did not load, so this is NullBackend and there is nothing to see. "
        "Is libmpv-2.dll beside the exe?"
    )
    assert "video surface attached" in log, (
        "the surface was never created; the log says "
        f"{[ln for ln in log.splitlines() if 'surface' in ln]}"
    )
    assert "no video surface" not in log, (
        f"attaching the surface failed: "
        f"{[ln for ln in log.splitlines() if 'no video surface' in ln][:1]}"
    )

    # Tune the first channel.
    d.click(d.by_text("nav a", "Live TV", timeout=30))
    assert d.wait_body(lambda b: "Live TV" in b, timeout=60)
    time.sleep(2)
    d.click(d.find('[data-testid="channel-row"]', timeout=30))
    d.find('[aria-label="Back"]', timeout=30)

    playing = wait_for_playing(d)
    ctx.assert_no_panic()
    assert playing.get("status") == "playing", (
        f"the host never reached playing; its last state was {playing}"
    )

    # What mpv itself says it is doing. A resolution and a codec come from the decoder,
    # not from the app's own bookkeeping, so they cannot be reported by a backend that
    # is not decoding anything.
    stats = playing.get("stats") or {}
    print(f"   mpv: {stats.get('resolution')} {stats.get('videoCodec')} "
          f"{stats.get('fps')}fps hw={stats.get('hwDecoder')} "
          f"buffer={stats.get('bufferSecs')}s")
    assert stats.get("resolution"), (
        f"nothing is being decoded: mpv reported no resolution. stats={stats}"
    )

    # Now the window tree, which is where questions 1 and 2 actually live.
    hwnd = wp.find("Aurora")
    assert hwnd, (
        "no window titled Aurora on the desktop, though the app is running — "
        f"visible windows: {[wp.title(h) for h in wp.top_levels() if wp.title(h)][:12]}"
    )
    pid = wp.pid_of(hwnd)
    # Tao keeps a 16x16 "Tao Thread Event Target" window per process to receive its own
    # messages on, and it is technically visible — so the question is not how many
    # windows there are but whether any *other* one is big enough to be showing a
    # picture. A video window that had escaped would be the size of the video.
    mine = [h for h in wp.top_levels(pid) if min(wp.window_rect(h)[2:]) >= 64]
    assert len(mine) == 1, (
        "video is in a window of its own — the process has "
        f"{len(mine)} visible top-level windows of any size: "
        f"{[wp.describe(h) for h in mine]}"
    )

    order = wp.children_front_to_back(hwnd)
    assert order, "the window has no children at all, so nothing was ever attached"
    surface = [h for h in order if wp.class_name(h).lower() == "static"]
    assert surface, (
        "no STATIC child window: mpv's surface is not there. Children: "
        f"{[wp.describe(h) for h in order]}"
    )
    surface = surface[0]
    assert wp.is_child_of(surface, hwnd), "the surface is not a child of the main window"
    assert order[-1] == surface, (
        "the video surface is not at the back of the z-order, so it paints over the UI. "
        f"Order front-to-back: {[wp.describe(h) for h in order]}"
    )
    print(f"   surface: {wp.describe(surface)}, last of {len(order)} children")

    # And the desktop: two captures of the middle of the window, a second apart.
    wp.bring_to_front(hwnd)
    time.sleep(1.5)
    x, y, width, height = wp.client_rect(hwnd)
    middle = (width // 4, height // 4, width // 2, height // 2)

    first = wp.grab(x, y, width, height)
    time.sleep(1.2)
    second = wp.grab(x, y, width, height)
    first.save_png(ctx.shot("playing-1"))
    second.save_png(ctx.shot("playing-2"))

    moved = first.changed_against(second, region=middle)
    colours = len(first.colours(region=middle))
    print(f"   desktop: {moved:.1%} of the middle changed in 1.2s, {colours} colours")
    assert colours > 16, (
        f"the middle of the window is {colours} colours — a flat fill, not a picture. "
        "If the surface attached and mpv is decoding, this is the compositing failing."
    )
    assert moved > MOTION_FLOOR, (
        f"only {moved:.2%} of the middle of the window changed in 1.2 seconds. The "
        "surface is attached and mpv reports a resolution, so frames are being decoded "
        "and not reaching the screen — which is the fallback in item 1 of the checklist."
    )

    # Question 2, from the other side: the UI is still live while video plays under it.
    # A click that lands and changes the screen is the whole of it — a video surface
    # painted over the WebView would swallow this.
    before = d.body()
    wake_osd(d)
    d.click(d.find('[aria-label="Back"]', timeout=20))
    time.sleep(2)
    assert d.body() != before, (
        "clicking Back while video played changed nothing on screen — input is not "
        "reaching the UI over the video surface"
    )
    ctx.assert_no_panic()

    # Question 3: the surface has to follow the window. `WindowEvent::Resized` is the
    # only thing that moves it, and nothing has ever exercised that path.
    _, _, was_wide, was_high = wp.client_rect(hwnd)
    left, top, outer_w, outer_h = wp.window_rect(hwnd)
    wp.resize(hwnd, outer_w - 220, outer_h - 160)
    time.sleep(2)
    _, _, now_wide, now_high = wp.client_rect(hwnd)
    assert (now_wide, now_high) != (was_wide, was_high), "the window did not resize"
    surface_rect = wp.window_rect(surface)
    assert abs(surface_rect[2] - now_wide) <= 4 and abs(surface_rect[3] - now_high) <= 4, (
        f"the client area is {now_wide}x{now_high} but the video surface is "
        f"{surface_rect[2]}x{surface_rect[3]} — they have torn apart on resize"
    )
    print(f"   resize: client {now_wide}x{now_high}, surface "
          f"{surface_rect[2]}x{surface_rect[3]}")
    wp.resize(hwnd, outer_w, outer_h)
    time.sleep(1)

    # Zap time, which is the last of item 1's four questions and the only one with a
    # number attached. Measured from the click to the host reporting a position on the
    # channel that was asked for — not to a status, which arrives before a frame does.
    rows = d.find_all('[data-testid="channel-row"]')
    if len(rows) < 2:
        d.click(d.by_text("nav a", "Live TV", timeout=30))
        time.sleep(2)
        rows = d.find_all('[data-testid="channel-row"]')
    assert len(rows) >= 2, "the playlist needs two channels to measure a channel change"

    was = state(d).get("channelId")
    started = time.time()
    d.click(rows[1])
    landed = None
    while time.time() - started < 20:
        now = state(d) or {}
        # Not `positionSecs`: a live HLS stream can sit at 0 while its buffer fills,
        # so the elapsed clock is not what says a frame has arrived. A filled buffer on
        # the channel that was asked for is.
        if (
            now.get("status") == "playing"
            and now.get("channelId") != was
            and (
                now.get("positionSecs", 0) > 0
                or (now.get("stats") or {}).get("bufferSecs", 0) > 0
            )
        ):
            landed = time.time() - started
            break
        time.sleep(0.05)
    ctx.assert_no_panic()
    assert landed is not None, (
        f"the second channel never started; the host says {state(d)}"
    )
    print(f"   zap: {landed:.2f}s (budget {ZAP_BUDGET_SECS}s)")
    third = wp.grab(x, y, width, height)
    third.save_png(ctx.shot("zapped"))

    # Reported rather than asserted. The budget is a claim about a provider's stream and
    # a viewer's connection as much as about this code, and failing a run on somebody
    # else's CDN would make this suite lie about the thing it is meant to prove.
    if landed > ZAP_BUDGET_SECS:
        print(f"   NOTE: over the {ZAP_BUDGET_SECS}s budget in README §16")

    # A second display, if there is one, and only now — the input test above clicks Back,
    # which stops playback, so this has to come after the zap has put a channel on again.
    # Moving between monitors is the case the resize handler does not obviously cover:
    # the window changes scale factor as well as position, and a surface repositioned
    # only on `Resized` could be left behind or left at the old size.
    screens = wp.monitors()
    others = [m for m in screens if not (m[0] == 0 and m[1] == 0)]
    if others:
        mx, my, mw, mh = others[0]
        wp.move_to(hwnd, mx + 40, my + 40)
        time.sleep(2.5)
        wp.bring_to_front(hwnd)
        time.sleep(1.5)
        mx2, my2, cw2, ch2 = wp.client_rect(hwnd)
        sr = wp.window_rect(surface)
        print(f"   second display {mw}x{mh} at ({mx},{my}): client {cw2}x{ch2}, "
              f"surface {sr[2]}x{sr[3]}")
        assert abs(sr[2] - cw2) <= 4 and abs(sr[3] - ch2) <= 4, (
            f"moved to the second display, the client area is {cw2}x{ch2} and the video "
            f"surface is {sr[2]}x{sr[3]} — they came apart across a DPI change"
        )
        a = wp.grab(mx2, my2, cw2, ch2)
        time.sleep(1.2)
        b = wp.grab(mx2, my2, cw2, ch2)
        b.save_png(ctx.shot("second-display"))
        mid = (cw2 // 4, ch2 // 4, cw2 // 2, ch2 // 2)
        moved2 = a.changed_against(b, region=mid)
        print(f"   second display: {moved2:.1%} of the middle changed in 1.2s")
        assert moved2 > MOTION_FLOOR, (
            f"video stopped reaching the screen on the second display ({moved2:.2%})"
        )
        wp.move_to(hwnd, left, top)
        time.sleep(1.5)
    else:
        print(f"   only one display ({screens}), so the monitor change is untested")
