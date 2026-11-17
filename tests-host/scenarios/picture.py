"""
The picture, on the real line: a film plays and its frames reach the screen.

`video_surface` asks the same question but only knows how to load an M3U, so with the
panel as the only source it has been skipping, and nothing has looked at the screen
since the player was rebuilt. This logs in to the panel the way `resume` does, plays a
film, and then asks the desktop rather than the page: a WebDriver capture photographs
the transparent WebView over nothing, so it cannot tell a picture from a hole.

It also reports what the page has painted over the middle of the window, so when the
picture is missing it says whether the page covered it or the surface never drew.
"""
import os
import sys
import time

import winprobe as wp

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")

MOTION_FLOOR = 0.01

PAINTED = r"""
const x = Math.floor(innerWidth / 2), y = Math.floor(innerHeight / 2);
const out = [];
for (let el = document.elementFromPoint(x, y); el; el = el.parentElement) {
  const s = getComputedStyle(el);
  out.push(`${el.tagName.toLowerCase()}${el.className && typeof el.className === 'string'
    ? '.' + el.className.split(' ').join('.') : ''} bg=${s.backgroundColor} img=${s.backgroundImage.slice(0, 40)} op=${s.opacity}`);
}
out.push(`html bg=${getComputedStyle(document.documentElement).backgroundColor}`);
return out;
"""


class Skipped(Exception):
    """Raised when this scenario has nothing to run against."""


def run(d, ctx):
    if sys.platform != "win32":
        raise Skipped("mpv is Windows-only")
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")

    time.sleep(6)
    d.send(d.find("textarea"), URL)
    time.sleep(1)
    d.click(d.by_text("button", "Xtream / panel login"))
    time.sleep(0.5)
    d.send(d.by_label("Username"), USER)
    d.send(d.by_label("Password"), PASS)
    d.click(d.by_text("button", "Check connection"))
    assert d.wait_body(
        lambda b: "days left" in b.lower() or "connections" in b.lower()
        or "could not" in b.lower(),
        timeout=120,
    ), "the connection check never finished"
    d.click(d.by_text("button", "Continue"))
    d.click(d.by_text("button", "Import library", timeout=30))
    assert d.wait_body(lambda b: "your library is ready" in b.lower(), timeout=900), (
        "the import never finished"
    )
    d.click(d.by_text("button", "Start watching"))
    time.sleep(4)

    d.click(d.by_text("nav a", "Movies", timeout=40))
    assert d.wait_body(lambda b: "Movies" in b, timeout=90), "Movies never drew"
    time.sleep(8)
    d.click(d.find('[data-testid="catalog-card"]', timeout=60))
    time.sleep(2)
    for label in ("Play", "Resume"):
        try:
            d.click(d.by_text("button", label, timeout=6))
            break
        except Exception:
            pass
    d.find('[aria-label="Back"]', timeout=45)

    end = time.time() + 45
    state = {}
    while time.time() < end:
        state = d.invoke("player_state") or {}
        if state.get("status") == "playing" and state.get("positionSecs", 0) > 2:
            break
        time.sleep(0.5)
    ctx.assert_no_panic()
    stats = state.get("stats") or {}
    print(f"   host: status={state.get('status')} hasPicture={state.get('hasPicture')} "
          f"position={round(state.get('positionSecs', 0), 1)} decoder={stats.get('resolution')}")
    assert state.get("status") == "playing", f"nothing played: {state}"

    hwnd = wp.find("Aurora")
    assert hwnd, "no Aurora window on the desktop"
    order = wp.children_front_to_back(hwnd)
    print(f"   children front to back: {[wp.describe(h) for h in order]}")
    surface = [h for h in order if wp.is_video_surface(h)]
    assert surface, "no video surface child window"
    print(f"   surface rect: {wp.window_rect(surface[0])}")

    # Twitch-free: let the controls hide so the middle is whatever the page leaves.
    time.sleep(5)
    print("   painted over the middle, innermost first:")
    for line in d.js(PAINTED) or []:
        print(f"      {line}")

    wp.bring_to_front(hwnd)
    time.sleep(1.5)
    x, y, width, height = wp.client_rect(hwnd)
    middle = (width // 4, height // 4, width // 2, height // 2)
    first = wp.grab(x, y, width, height)
    time.sleep(1.2)
    second = wp.grab(x, y, width, height)
    first.save_png(ctx.shot("picture-1"))
    second.save_png(ctx.shot("picture-2"))
    moved = first.changed_against(second, region=middle)
    colours = len(first.colours(region=middle))
    print(f"   desktop: {moved:.1%} of the middle changed in 1.2s, {colours} colours")
    assert colours > 16 and moved > MOTION_FLOOR, (
        f"no moving picture on screen ({colours} colours, {moved:.2%} changed) although "
        "the host is decoding: frames are not reaching the screen"
    )
    ctx.assert_no_panic()
