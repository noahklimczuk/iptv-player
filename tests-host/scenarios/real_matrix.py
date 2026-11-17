"""
The player on the real line, one case after another (docs/PLAYER.md section 4, step 4).

Every other player check runs against the mock or a local file. This one runs the cases a
viewer meets against the panel itself, on a fresh library the harness owns:

    live          a channel plays and its frames move on the desktop
    timeshift     pausing live falls behind it, and Back to live catches up
    film (MP4)    plays, seeks to the middle, and the frames move
    film (MKV)    the same, on the other container the panel serves
    episode       played from the show's page; at the credits Up Next counts down,
                  and Play now starts the next episode
    catch-up      a programme from earlier today on a catch-up channel plays
    PiP           the picture goes to a corner and keeps playing; it comes back
    multi-view    one tile opens in front of the main surface (the line allows one)

Every case starts playback the way a viewer does -- a channel number and Enter, a film
from the search palette, an episode from its show's page -- because the page is what
opens the player. A `player.play` sent straight to the host plays behind whatever page
is up, and the desktop then shows that page rather than the picture.

Each case is tried and reported on its own, and the scenario fails at the end if any
did, so one stream the provider will not serve today does not hide the rest. A film or
channel that will not start is retried with the next one: the claim is that the app
plays what the line serves, not that every entry in a catalogue of thousands works.

The line allows one connection, so nothing here overlaps: each case stops what the last
one started before it begins.

Skipped unless the panel credentials are in the environment (AURORA_PANEL_URL,
AURORA_PANEL_USER, AURORA_PANEL_PASS). They are typed into the window and never logged.
"""
import os
import sys
import time

import winprobe as wp

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")

# The picture has to be moving, not merely present: a frozen first frame is the bug
# this most often catches. Same floor as `picture`.
MOTION_FLOOR = 0.01
START_SECS = 45


class Skipped(Exception):
    """Raised when this scenario has nothing to run against."""


class CaseFailed(AssertionError):
    pass


def run(d, ctx):
    if sys.platform != "win32":
        raise Skipped("mpv is Windows-only")
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")

    _import_the_library(d)
    hwnd = wp.find("Aurora")
    assert hwnd, "no Aurora window on the desktop"

    results = []

    def case(name, fn):
        print(f"   -- {name}")
        started = time.time()
        try:
            note = fn() or ""
            results.append((name, "pass", note))
        except Exception as e:  # noqa: BLE001 -- every case is reported, then judged
            results.append((name, "FAIL", str(e).splitlines()[0][:160]))
            print(f"      failed: {e}")
            try:
                d.shot(ctx.shot(f"fail-{name.split()[0]}"))
            except Exception:
                pass
        finally:
            _stop(d)
            print(f"      ({time.time() - started:.0f}s)")
        ctx.assert_no_panic()

    case("live", lambda: _live(d, ctx, hwnd))
    case("timeshift", lambda: _timeshift(d, ctx))
    case("film mp4", lambda: _film(d, ctx, hwnd, "mp4"))
    case("film mkv", lambda: _film(d, ctx, hwnd, "mkv"))
    case("episode + Up Next", lambda: _episode_up_next(d, ctx))
    case("catch-up", lambda: _catchup(d, ctx, hwnd))
    case("PiP", lambda: _pip(d, ctx, hwnd))
    case("multi-view", lambda: _multiview(d, ctx, hwnd))

    print("   matrix:")
    for name, verdict, note in results:
        print(f"      {verdict:4}  {name:18} {note}")
    failed = [r for r in results if r[1] != "pass"]
    assert not failed, f"{len(failed)} of {len(results)} cases failed: " + "; ".join(
        f"{n}: {why}" for n, _, why in failed
    )


# ---- the cases -------------------------------------------------------------------


def _live(d, ctx, hwnd):
    channel, state = _a_playing_channel(d, ctx)
    moved = _picture_moves(ctx, hwnd, "live")
    return f"{channel['name']!r}, {moved:.0%} of the middle moved"


def _timeshift(d, ctx):
    settings = d.invoke("timeshift_settings")
    if not settings["enabled"]:
        d.invoke(
            "timeshift_set_settings",
            {"enabled": True, "bytes": settings["bytes"], "secs": settings["secs"]},
        )
    # The buffer is set when a stream is opened, so tune after turning it on.
    channel, _ = _a_playing_channel(d, ctx)
    window = _wait(d, "a timeshift window", 30, lambda s: s.get("timeshift"))["timeshift"]
    d.invoke("player_pause")
    time.sleep(10)
    d.invoke("player_resume")
    state = _wait(d, "playing again after the pause", 20, _is_playing)
    window = state.get("timeshift") or window
    behind = window["liveSecs"] - window["positionSecs"]
    if behind < 5:
        raise CaseFailed(f"paused 10s and only {behind:.1f}s behind live")
    d.invoke("player_back_to_live")

    def caught_up(s):
        w = s.get("timeshift")
        return _is_playing(s) and w and (w["liveSecs"] - w["positionSecs"]) < 4

    state = _wait(d, "back at live", 20, caught_up)
    after = state["timeshift"]["liveSecs"] - state["timeshift"]["positionSecs"]
    return f"{channel['name']!r}: {behind:.0f}s behind after a 10s pause, {after:.1f}s after Back to live"


def _film(d, ctx, hwnd, container):
    rows = ctx.rows(
        "SELECT id, title FROM movies WHERE container = ? AND hidden = 0 "
        "AND custom_title IS NULL ORDER BY added_at DESC LIMIT 8",
        (container,),
    )
    if not rows:
        raise CaseFailed(f"the library holds no {container} films")
    for movie_id, stored in rows:
        # The title as the screen shows it, which is what a person types: the stored one
        # carries the panel's prefix ("EN Street Fighter"), and the search does not.
        item = d.invoke("library_item", {"kind": "movie", "id": movie_id}) or {}
        title = item.get("title") or stored
        if not _open_from_palette(d, "movies", title):
            print(f"      the palette did not find {title!r}, trying the next")
            continue
        state = _wait(d, "playback", START_SECS, _is_playing, quiet=True)
        if not state or state.get("itemId") != movie_id:
            print(f"      {title!r} did not start ({(state or {}).get('title')!r}), trying the next")
            _stop(d)
            continue
        duration = state.get("durationSecs") or 0
        if duration < 120:
            raise CaseFailed(f"{title!r} plays but reports a length of {duration:.0f}s")
        half = duration / 2
        d.invoke("player_seek", {"positionSecs": half})
        _wait(
            d,
            "the seek to the middle",
            30,
            lambda s: _is_playing(s) and abs(s.get("positionSecs", 0) - half) < 15,
        )
        moved = _picture_moves(ctx, hwnd, f"film-{container}")
        tracks = len(state.get("audioTracks") or [])
        return f"{title!r}, {duration / 60:.0f} min, seeked to the middle, {tracks} audio track(s), {moved:.0%} moved"
    raise CaseFailed(f"none of {len(rows)} {container} films started")


def _episode_up_next(d, ctx):
    """Through the page, because Up Next is the page's: the host only plays."""
    d.click(d.by_text("nav a", "Series", timeout=30))
    d.wait_body(lambda b: "Series" in b, timeout=60)
    time.sleep(4)
    # The first show on the page with two episodes or more: Up Next needs a next one.
    cards = len(d.find_all('[data-testid="catalog-card"]'))
    opened = False
    for i in range(min(cards, 8)):
        # A real click, as `series_episodes` does.
        d.click(d.find_all('[data-testid="catalog-card"]')[i])
        listed = d.wait_body(lambda b: "S01E" in b or "S02E" in b or "No episodes" in b, timeout=90)
        d.wait_body(lambda b: "Loading episodes" not in b, timeout=30)
        time.sleep(1)
        thumbs = len(d.find_all('[data-testid="episode-thumb"]'))
        print(f"      show {i + 1}: listed={listed}, {thumbs} episode(s) on its page")
        if thumbs >= 2:
            opened = True
            break
        d.js(
            "const c = document.querySelector('[role=dialog] [aria-label=\"Close\"]');"
            " if (c) c.click(); return true;"
        )
        time.sleep(1)
    if not opened:
        raise CaseFailed("none of the first shows on the Series page has two episodes")
    d.click(d.find('[data-testid="episode-thumb"]'))
    state = _wait(d, "the episode playing", START_SECS, _is_playing, fail_on_error=True)
    first_id, first_title = state.get("itemId"), state.get("title")
    duration = state.get("durationSecs") or 0
    if duration < 120:
        raise CaseFailed(f"the episode reports a length of {duration:.0f}s")
    d.invoke("player_seek", {"positionSecs": duration - 45})
    if not d.wait_body(lambda b: "up next" in b.lower(), timeout=40):
        raise CaseFailed(f"no Up Next card at the credits of {first_title!r}")
    d.shot(ctx.shot("up-next"))
    d.click(d.by_text("button", "Play now", timeout=10))
    state = _wait(
        d,
        "the next episode playing",
        START_SECS,
        lambda s: _is_playing(s) and s.get("itemId") not in (None, first_id),
        fail_on_error=True,
    )
    d.js("location.hash = '#/'; return true;")
    return f"{first_title!r}: Up Next at the credits; Play now started {state.get('title')!r}"


def _catchup(d, ctx, hwnd):
    now = int(time.time())
    candidates = ctx.rows(
        "SELECT c.id, c.name, p.start, p.stop, p.title FROM channels c "
        "JOIN epg_programmes p ON p.channel_id = c.epg_channel_id "
        "WHERE c.catchup_mode IS NOT NULL AND c.hidden = 0 "
        "AND p.stop < ? AND p.start > ? ORDER BY p.stop DESC LIMIT 6",
        (now - 1800, now - 86400),
    )
    if not candidates:
        # The guide arrives after the import; without it, an hour that has passed on a
        # catch-up channel is still a programme the archive holds.
        chans = ctx.rows(
            "SELECT id, name FROM channels WHERE catchup_mode IS NOT NULL AND hidden = 0 LIMIT 6"
        )
        if not chans:
            raise CaseFailed("the line has no catch-up channels")
        hour = now - now % 3600
        candidates = [(cid, name, hour - 7200, hour - 3600, "(no guide)") for cid, name in chans]
    for cid, name, start, stop, title in candidates:
        # The player is opened on the channel first, and the programme is then asked of
        # the host, which is what choosing it in the player's own guide does.
        if not _tune_by_number(d, ctx, cid):
            print(f"      {name!r} has no number to tune it by, trying the next")
            continue
        d.invoke("player_play_catchup", {"channelId": cid, "start": start, "stop": stop})
        state = _wait(d, "catch-up playing", START_SECS, _is_playing, quiet=True)
        if not state:
            print(f"      {name!r} {title!r} did not start, trying the next")
            _stop(d)
            continue
        if state.get("isLive"):
            raise CaseFailed(f"{name!r} catch-up plays as live")
        moved = _picture_moves(ctx, hwnd, "catchup")
        return f"{name!r} {title!r}, {moved:.0%} moved"
    raise CaseFailed(f"none of {len(candidates)} catch-up programmes started")


def _pip(d, ctx, hwnd):
    _a_playing_channel(d, ctx)
    _key(d, "p")
    time.sleep(1)
    if not d.invoke("pip_state")["enabled"]:
        raise CaseFailed("P did not put the picture in a corner")
    time.sleep(2)
    _, _, cw, ch = wp.client_rect(hwnd)
    surfaces = [
        c for c in wp.children_front_to_back(hwnd) if wp.is_video_surface(c) and wp.is_visible(c)
    ]
    rects = [wp.window_rect(s) for s in surfaces]
    small = [r for r in rects if r[2] < cw * 0.5 and r[3] < ch * 0.5]
    if not small:
        raise CaseFailed(f"no surface in a corner; visible surfaces are {rects} in a {cw}x{ch} window")
    t0 = d.invoke("player_state").get("positionSecs", 0)
    time.sleep(4)
    state = d.invoke("player_state")
    if not _is_playing(state) or state.get("positionSecs", 0) <= t0:
        raise CaseFailed(f"the picture went to the corner and stopped: {state.get('status')}")
    _key(d, "p")
    time.sleep(1.5)
    if d.invoke("pip_state")["enabled"]:
        raise CaseFailed("P again did not bring the picture back")
    full = [
        wp.window_rect(c)
        for c in wp.children_front_to_back(hwnd)
        if wp.is_video_surface(c) and wp.is_visible(c)
    ]
    if not any(r[2] >= cw * 0.9 for r in full):
        raise CaseFailed(f"the picture did not come back from the corner: {full}")
    return f"tile {small[0][2]}x{small[0][3]} kept playing, and came back full size"


def _multiview(d, ctx, hwnd):
    channel, _ = _a_playing_channel(d, ctx)
    _stop(d)
    d.js(
        "const [ids] = arguments;"
        " window.__mosaicOpened = undefined; window.__mosaicFailed = undefined;"
        " window.__TAURI_INTERNALS__"
        "   .invoke('mosaic_open', { args: { layout: 'grid2x2', channelIds: ids } })"
        "   .then((v) => { window.__mosaicOpened = v; })"
        "   .catch((e) => { window.__mosaicFailed = String(e); });"
        " return true;",
        [channel["id"], None, None, None],
    )
    try:
        deadline = time.time() + 40
        while time.time() < deadline:
            time.sleep(0.5)
            failed = d.js("return window.__mosaicFailed ?? null;")
            if failed:
                raise CaseFailed(f"the host would not open a mosaic: {failed}")
            tiles = d.invoke("mosaic_state")["tiles"]
            playing = [t for t in tiles if t and t.get("status") == "playing"]
            errors = [t["error"] for t in tiles if t and t.get("error")]
            if playing:
                _, _, cw, ch = wp.client_rect(hwnd)
                quarter = [
                    wp.window_rect(c)
                    for c in wp.children_front_to_back(hwnd)
                    if wp.is_video_surface(c)
                    and wp.is_visible(c)
                    and wp.window_rect(c)[2] <= cw // 2 + 2
                ]
                if not quarter:
                    raise CaseFailed("a tile plays but no quarter-size surface is visible")
                return f"{channel['name']!r} plays in a tile {quarter[0][2]}x{quarter[0][3]}"
            if errors:
                raise CaseFailed(f"the tile reported {errors[0]}")
        raise CaseFailed("the tile never started")
    finally:
        d.js(
            "window.__TAURI_INTERNALS__.invoke('mosaic_close', { args: {} }).catch(() => {});"
            " return true;"
        )
        time.sleep(3)


# ---- helpers ---------------------------------------------------------------------


def _is_playing(s):
    return bool(s) and s.get("status") == "playing" and s.get("positionSecs", 0) > 1


def _wait(d, what, secs, pred, fail_on_error=False, quiet=False):
    deadline = time.time() + secs
    state = {}
    while time.time() < deadline:
        state = d.invoke("player_state") or {}
        if pred(state):
            return state
        if state.get("status") == "error":
            if quiet:
                return None
            if fail_on_error:
                raise CaseFailed(f"{what}: the player reported {state.get('error')}")
        time.sleep(0.5)
    if quiet:
        return None
    raise CaseFailed(f"timed out waiting for {what}; last status {state.get('status')!r}")


def _stop(d):
    """Leave the player the way a viewer does, then make sure the host let go."""
    try:
        if d.invoke("pip_state")["enabled"]:
            _key(d, "p")
        # Escape, as a person leaves (C16): the Back button is only there while the
        # controls are up, and they hide a few seconds into playback. Twice at most --
        # the first leaves fullscreen when the player is in it.
        for _ in range(2):
            if not d.js("return !!document.querySelector('[data-player-overlay]');"):
                break
            _key(d, "Escape")
            time.sleep(1)
        d.invoke("player_stop")
    except Exception:
        pass
    # Whatever a failed case left open -- the search palette, a title's page -- so the next
    # case starts on a clear page rather than clicking into the last one's dialog.
    _close_dialogs(d)
    time.sleep(2)


def _close_dialogs(d):
    """Close every open dialog, as a person would: its Close button, or Escape in it."""
    for _ in range(4):
        try:
            if d.js(
                "const c = document.querySelector('[role=dialog] [aria-label=\"Close\"]');"
                " if (c) { c.click(); return true; } return false;"
            ):
                time.sleep(0.5)
                continue
            boxes = d.find_all('[role="dialog"] input')
            if boxes:
                # A real keystroke into the field: the palette listens on its input, not
                # on the window, so a synthetic Escape there is never heard.
                d.send(boxes[0], "\ue00c")
                time.sleep(0.5)
                continue
            if not d.js("return !!document.querySelector('[role=dialog]');"):
                return
            d.js("document.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true})); return true;")
            time.sleep(0.5)
        except Exception:
            return


def _key(d, key, **mods):
    d.js(
        "const [k, m] = arguments;"
        " window.dispatchEvent(new KeyboardEvent('keydown', {key: k, bubbles: true, ...m}));"
        " return true;",
        key,
        mods,
    )


def _tune_by_number(d, ctx, channel_id):
    """Type the channel's number and press Enter (C20). False if it has no number."""
    row = ctx.rows(
        "SELECT COALESCE(custom_number, number) FROM channels WHERE id = ?", (channel_id,)
    )
    if not row or row[0][0] is None:
        return False
    for digit in str(row[0][0]):
        _key(d, digit)
        time.sleep(0.15)
    _key(d, "Enter")
    return True


def _open_from_palette(d, group, title):
    """Search for `title`, pick it from `group`, and press Play on its page."""
    _key(d, "k", ctrlKey=True)
    box = d.find('[role="dialog"] input', timeout=10)
    d.clear(box)
    d.send(box, title)
    time.sleep(2)
    picked = d.js(
        "const [group, title] = arguments;"
        " const opts = [...document.querySelectorAll(`[data-testid=\"palette-group-${group}\"] [role=option]`)];"
        " const hit = opts.find((o) => o.innerText.split('\\n')[0].trim() === title) || null;"
        " if (!hit) return false; hit.click(); return true;",
        group,
        title,
    )
    if not picked:
        _close_dialogs(d)
        return False
    time.sleep(2)
    return bool(d.js(
        "const b = [...document.querySelectorAll('[role=dialog] button')]"
        "   .find((el) => /^(Play|Resume)/.test((el.getAttribute('aria-label') || el.innerText || '').trim()));"
        " if (!b) return false; b.click(); return true;"
    ))


def _a_playing_channel(d, ctx):
    """A channel that plays, tuned by its number, from an ordinary group."""
    rows = ctx.rows(
        "SELECT id, name FROM channels WHERE hidden = 0 AND is_radio = 0 "
        "AND COALESCE(custom_number, number) IS NOT NULL "
        "AND lower(coalesce(group_title, '')) NOT LIKE '%ppv%' "
        "AND lower(coalesce(group_title, '')) NOT LIKE '%event%' "
        "AND lower(coalesce(group_title, '')) NOT LIKE '%24/7%' "
        "ORDER BY COALESCE(custom_number, number), id LIMIT 12"
    )
    if not rows:
        raise CaseFailed("no channel on this line has a number to type")
    for cid, name in rows:
        _tune_by_number(d, ctx, cid)
        state = _wait(d, "playback", START_SECS, _is_playing, quiet=True)
        if state and state.get("channelId") == cid:
            return {"id": cid, "name": name}, state
        print(f"      {name!r} did not start, trying the next")
        _stop(d)
    raise CaseFailed(f"none of {len(rows)} channels started")


def _picture_moves(ctx, hwnd, tag):
    """The desktop, not the page: a WebDriver capture cannot see the video surface."""
    wp.bring_to_front(hwnd)
    time.sleep(1.5)
    x, y, width, height = wp.client_rect(hwnd)
    middle = (width // 4, height // 4, width // 2, height // 2)
    first = wp.grab(x, y, width, height)
    time.sleep(1.2)
    second = wp.grab(x, y, width, height)
    first.save_png(ctx.shot(tag))
    moved = first.changed_against(second, region=middle)
    colours = len(first.colours(region=middle))
    if colours <= 16 or moved <= MOTION_FLOOR:
        raise CaseFailed(
            f"no moving picture on screen ({colours} colours, {moved:.2%} changed)"
        )
    return moved


def _import_the_library(d):
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
