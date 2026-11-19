"""
Settings → Privacy in the real app, over a library imported from the real panel.

`repo::progress` proves the deletes on a handful of rows; this is the claim the panel
makes, in the shipped host over a real subscription's films (`docs/DECISIONS.md` D33,
`AUDIT/findings.md` F-40):

  1. watch history is kept for good until a window is chosen, and choosing one applies
     it at once and survives a restart;
  2. a window outside 1 to 3650 days is refused;
  3. once Settings are locked behind the parental PIN, clearing history is refused until
     the PIN is entered;
  4. clearing forgets the profile's history, which the library itself confirms.

Skipped unless the panel credentials are in the environment:

    AURORA_PANEL_URL / _USER / _PASS

Imports only. Progress is recorded through `progress.save`, the command the player
calls, so nothing is ever played and the line's one connection is never used.
"""
import os
import time

from driver import HostRefused

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")

CONTENT = {"live": True, "vod": True, "series": False}
IMPORT_TIMEOUT = 900
PIN = "2468"
FILMS = 3

PIN_REQUIRED = "Enter the parental PIN to change this."
OUT_OF_RANGE = "1 to 3650 days"


class Skipped(Exception):
    """Raised when this scenario has nothing to run against."""


def run(d, ctx):
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")

    d = _wait_for_ui(d)
    owner = _import_panel(d, ctx)
    d.invoke("profiles.select", {"profileId": owner})

    films = [r[0] for r in ctx.rows(f"SELECT id FROM movies WHERE hidden = 0 ORDER BY id LIMIT {FILMS}")]
    assert len(films) == FILMS, f"the line has {len(films)} films; need {FILMS}"
    for film in films:
        d.invoke("progress.save", {
            "profileId": owner, "kind": "movie", "id": film,
            "positionSecs": 600, "durationSecs": 6000,
        })

    # 1. Kept for good, until a window is chosen ---------------------------------------
    history = d.invoke("privacy.history", {"profileId": owner})
    assert history == {"days": None, "items": FILMS}, f"a fresh library reports {history}"
    removed = d.invoke("privacy.setHistoryDays", {"days": 30})
    assert removed == 0, f"a 30-day window forgot {removed} things watched just now"
    print(f"   window: forever by default, 30 days chosen, {FILMS} films kept")

    # 2. Nonsense is refused, and the window it would have replaced stays -------------
    for days in (0, 3651):
        _refused(d, "privacy.setHistoryDays", {"days": days}, OUT_OF_RANGE)
    assert d.invoke("privacy.history", {"profileId": owner})["days"] == 30

    # 3. Locked Settings refuse to clear it -------------------------------------------
    d.invoke("profiles.setParental", {"hideAdult": True, "lockSettings": True, "masterPin": PIN})
    d = ctx.relaunch(d)
    d = _wait_for_ui(d)
    d.invoke("profiles.select", {"profileId": owner})
    history = d.invoke("privacy.history", {"profileId": owner})
    assert history == {"days": 30, "items": FILMS}, f"after a restart: {history}"
    _refused(d, "privacy.clearHistory", {"profileId": owner}, PIN_REQUIRED)
    _refused(d, "privacy.setHistoryDays", {"days": None}, PIN_REQUIRED)
    kept = ctx.rows("SELECT COUNT(*) FROM watch_progress WHERE profile_id = ?", (owner,))[0][0]
    assert kept == FILMS, f"a refused clear still removed {FILMS - kept} entries"
    print("   locked: the window survived a restart; clearing is refused without the PIN")

    # 4. With the PIN, clearing forgets it all ----------------------------------------
    assert d.invoke("parental.unlock", {"pin": PIN}) == "ok"
    started = time.perf_counter()
    removed = d.invoke("privacy.clearHistory", {"profileId": owner})
    took = time.perf_counter() - started
    assert removed == FILMS, f"clearing removed {removed}, expected {FILMS}"
    left = ctx.rows("SELECT COUNT(*) FROM watch_progress WHERE profile_id = ?", (owner,))[0][0]
    assert left == 0, f"{left} entries survived clearing"
    assert d.invoke("privacy.history", {"profileId": owner})["items"] == 0
    assert d.invoke("privacy.setHistoryDays", {"days": None}) == 0
    assert d.invoke("privacy.history", {"profileId": owner})["days"] is None
    print(f"   cleared: {removed} entries in {took * 1000:.0f} ms; the library holds none")
    ctx.assert_no_panic()


def _refused(d, command, args, words):
    try:
        d.invoke(command, args)
    except HostRefused as e:
        assert words in str(e), f"{command} was refused, but with {e}"
        return
    raise AssertionError(f"{command} {args} was allowed; expected {words!r}")


def _wait_for_ui(d):
    end = time.time() + 30
    while time.time() < end:
        try:
            if d.js("return !!(window.__TAURI_INTERNALS__ && document.body.innerText.length)"):
                return d
        except Exception:
            pass
        time.sleep(0.5)
    raise RuntimeError("the window never came up")


def _import_panel(d, ctx):
    draft = {"name": "Real Panel", "kind": "xtream", "url": URL, "username": USER, "password": PASS}
    provider = d.invoke("providers.save", {"draft": draft})["id"]
    d.script_timeout(IMPORT_TIMEOUT)
    try:
        d.invoke("providers.refresh", {"providerId": provider, "content": CONTENT})
    finally:
        d.script_timeout(30)
    print(f"   imported {ctx.count('channels')} channels and {ctx.count('movies')} films")
    return ctx.rows("SELECT id FROM profiles ORDER BY id LIMIT 1")[0][0]
