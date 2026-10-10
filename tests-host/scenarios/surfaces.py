"""
Where the video surfaces actually are, for every feature that moves one.

The same bug has now reached the viewer three times, in three features, with the same
shape each time: the *frame* was right and the picture was not. The guide's preview was
never placed at all and showed a blurred logo; picture-in-picture had the shell painted
over its hole; multi-view put every tile behind the main player's full-window surface, so
it played sound and showed nothing.

None of it was catchable from a browser. The picture is a native child window of the app's
own window -- a sibling of the WebView2, deliberately behind it -- so "is there a picture
here" is a question about the window tree, and a WebDriver screenshot photographs a
transparent page over nothing at all.

So this asks the window tree, for each of the four places a surface can be:

  * **full window**, where it starts with nothing asked of it;
  * **a corner**, which is `pip.setEnabled`;
  * **an inlay**, which is the guide's preview panel;
None of it needs a stream, a playlist or an account: the surface is attached at startup,
before anything is playing, so it can be moved and measured on a bare profile. That is
what makes this cheap enough to run before every release.

The fourth place a surface can be -- *hidden*, while a mosaic is open -- is in
`real_panel`, because it needs channels and a mosaic will not open without one. It is the
same question asked the same way.

`video_surface` remains the scenario that proves frames reach a surface; this one is about
where the surface is.
"""
import sys
import time

import winprobe as wp



#: A rectangle for the inlay: off-origin and not 16:9, so a host that ignored the
#: arguments and used a geometry of its own could not pass by coincidence.
INLAY = {"x": 420, "y": 64, "width": 600, "height": 280}

#: The client area is measured through a different API than the one the host resizes
#: against, so a pixel either way on the full-window comparison is not a failure. The
#: placements themselves are compared exactly.
SLACK = 2


def _statics(hwnd, visible_only=True):
    """The STATIC children, which is what mpv attaches, front of the z-order first."""
    out = []
    for child in wp.children_front_to_back(hwnd):
        if wp.class_name(child).lower() != "static":
            continue
        if visible_only and not wp.is_visible(child):
            continue
        out.append(child)
    return out


def _rect(hwnd, surface):
    """A surface's rectangle in the coordinates the host places surfaces in."""
    cx, cy, _, _ = wp.client_rect(hwnd)
    sx, sy, sw, sh = wp.window_rect(surface)
    return {"x": sx - cx, "y": sy - cy, "width": sw, "height": sh}


def _close(got, want, slack=0):
    return all(abs(got[k] - want[k]) <= slack for k in ("x", "y", "width", "height"))


def _settle():
    """Windows moves child windows asynchronously often enough to be worth not racing."""
    time.sleep(0.4)


def run(d, ctx):
    if sys.platform != "win32":
        print("   skipped: no mpv surfaces to find off Windows")
        return

    time.sleep(6)

    hwnd = wp.find("Aurora")
    assert hwnd, "no window titled Aurora on the desktop, though the app is running"
    _, _, client_w, client_h = wp.client_rect(hwnd)
    full = {"x": 0, "y": 0, "width": client_w, "height": client_h}

    surfaces = _statics(hwnd)
    assert len(surfaces) == 1, (
        "expected exactly one video surface before anything is asked of it; found "
        f"{[wp.describe(s) for s in surfaces]}"
    )
    main = surfaces[0]
    assert _close(_rect(hwnd, main), full, SLACK), (
        f"the surface does not start over the client area: {_rect(hwnd, main)} "
        f"against {full}"
    )
    print(f"   full window: {_rect(hwnd, main)}")

    # -- A corner ---------------------------------------------------------------
    view = d.invoke("pip.setEnabled", {"enabled": True})
    assert view["enabled"] is True, view
    _settle()
    got = _rect(hwnd, main)
    assert _close(got, view["rect"]), (
        f"picture-in-picture reports {view['rect']} and the surface is at {got}"
    )
    assert not _close(got, full, SLACK), "the corner is the whole window"
    print(f"   corner ({view['corner']}): {got}")

    d.invoke("pip.setEnabled", {"enabled": False})
    _settle()
    assert _close(_rect(hwnd, main), full, SLACK), (
        f"the surface did not go back to the whole window: {_rect(hwnd, main)}"
    )

    # -- An inlay, which is the guide's preview ----------------------------------
    view = d.invoke("preview.place", INLAY)
    assert view["inlay"] == INLAY, view
    _settle()
    got = _rect(hwnd, main)
    assert _close(got, INLAY), f"the inlay reports {INLAY} and the surface is at {got}"
    print(f"   inlay: {got}")
    d.invoke("preview.clear")
    _settle()
    assert _close(_rect(hwnd, main), full, SLACK), _rect(hwnd, main)

    ctx.assert_no_panic()
