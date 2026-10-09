"""
The inlaid picture: does the surface actually go where the page asked?

This is the half of the guide's preview that a browser cannot show. `tests-e2e/
guide-preview.spec.ts` proves the page asks for the right rectangle — it measures its box,
converts to physical pixels and calls `preview.place` — but there is no video surface in a
browser, so nothing there can say whether mpv ended up in it. A preview that asks
correctly and is ignored looks exactly like one that works, right up until someone runs
the release.

And it is measured rather than photographed. The surface is a child window of the app's
own window, so where it is is a fact about the window tree: `preview.place` should move
that child to exactly the rectangle it was handed, and `preview.clear` should put it back
over the whole client area.

**No stream needed, deliberately.** The surface is attached at startup, before anything is
playing, so placement is answerable without a playlist or an account — which is what makes
this runnable on any Windows machine rather than only one with a subscription.
`video_surface` is still the scenario that proves frames reach it.
"""
import sys
import time

import winprobe as wp

#: Where to ask for the picture. Arbitrary, off-origin and not 16:9, so a host that
#: ignored the arguments and used its own geometry could not pass by coincidence.
WANT = {"x": 480, "y": 72, "width": 640, "height": 300}

#: How far out the surface may be. Zero would be right, and is what this asserts for the
#: placement itself; the full-window comparison allows a pixel for the client area being
#: measured through a different API than the one the host resized against.
SLACK = 2


def _surface(hwnd):
    """mpv's child window, which is the one STATIC child at the back of the z-order."""
    order = wp.children_front_to_back(hwnd)
    statics = [h for h in order if wp.class_name(h).lower() == "static"]
    assert statics, (
        "no STATIC child window, so mpv's surface was never attached. Children: "
        f"{[wp.describe(h) for h in order]}"
    )
    return statics[0]


def _client_relative(hwnd, surface):
    """The surface's rectangle in the coordinates `preview.place` speaks."""
    cx, cy, _, _ = wp.client_rect(hwnd)
    sx, sy, sw, sh = wp.window_rect(surface)
    return {"x": sx - cx, "y": sy - cy, "width": sw, "height": sh}


def _close(got, want, slack=SLACK):
    return all(abs(got[k] - want[k]) <= slack for k in ("x", "y", "width", "height"))


def run(d, ctx):
    if sys.platform != "win32":
        print("   skipped: no mpv surface to place off Windows")
        return

    time.sleep(6)

    hwnd = wp.find("Aurora")
    assert hwnd, "no window titled Aurora on the desktop, though the app is running"
    surface = _surface(hwnd)
    _, _, client_w, client_h = wp.client_rect(hwnd)

    # Where it starts: over the whole client area, because nothing has asked otherwise.
    full = _client_relative(hwnd, surface)
    assert _close(full, {"x": 0, "y": 0, "width": client_w, "height": client_h}), (
        f"the surface does not start over the client area: {full} against "
        f"{client_w}x{client_h}"
    )

    # The thing being tested.
    view = d.invoke("preview.place", WANT)
    assert view["inlay"] == WANT, (
        f"the host reported a different rectangle than it was given: {view['inlay']}"
    )
    # A short settle: the host places the surface on the calling thread, but Windows
    # moves child windows asynchronously often enough to be worth not racing.
    time.sleep(0.4)

    placed = _client_relative(hwnd, surface)
    assert _close(placed, WANT, 0), (
        f"the surface is at {placed}, not the {WANT} the page asked for — "
        "`preview.place` is being accepted and ignored"
    )
    print(f"   placed: {placed}")
    wp.bring_to_front(hwnd)
    time.sleep(0.3)
    cx, cy, cw, ch = wp.client_rect(hwnd)
    wp.grab(cx, cy, cw, ch).save_png(ctx.shot("guide-preview-placed"))

    # And it must be given back, or the picture stays pinned to a box on a page that is
    # no longer on screen.
    cleared = d.invoke("preview.clear")
    assert cleared["inlay"] is None, cleared
    time.sleep(0.4)
    back = _client_relative(hwnd, surface)
    assert _close(back, {"x": 0, "y": 0, "width": client_w, "height": client_h}), (
        f"the surface did not go back to the whole window: {back}"
    )

    # The clamp, against the real window rather than a unit test's idea of one: a box
    # measured mid-animation or mid-scroll can hang off the edge, and a child window
    # placed outside its parent is simply not drawn.
    over = {"x": client_w - 100, "y": 40, "width": 600, "height": 200}
    view = d.invoke("preview.place", over)
    assert view["inlay"] == {"x": client_w - 100, "y": 40, "width": 100, "height": 200}, (
        f"a rectangle hanging off the right edge was not trimmed: {view['inlay']}"
    )
    time.sleep(0.4)
    trimmed = _client_relative(hwnd, surface)
    assert _close(trimmed, view["inlay"], 0), (
        f"the surface is at {trimmed}, not the trimmed {view['inlay']}"
    )

    # Nothing of it on screen at all: the picture goes back to the window rather than
    # being put somewhere nobody can see it.
    view = d.invoke("preview.place", {"x": client_w + 50, "y": 0, "width": 200, "height": 100})
    assert view["inlay"] is None, f"a rectangle entirely off the window was accepted: {view}"
    time.sleep(0.4)
    fallback = _client_relative(hwnd, surface)
    assert _close(fallback, {"x": 0, "y": 0, "width": client_w, "height": client_h}), (
        f"an impossible rectangle left the surface at {fallback}"
    )

    d.invoke("preview.clear")
    ctx.assert_no_panic()
