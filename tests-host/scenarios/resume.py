"""
Whether a film reports a position and a duration — which is what three features need.

Continue Watching is empty on a real subscription. `saveProgress` refuses to write
unless `durationSecs > 0` **and** `positionSecs > 0`, so a player that reports neither
saves nothing, ever, and the rail can only be empty. Up Next hangs off the same numbers:
it appears when the position is near the end, which needs an end to be near.

Nothing in this repository could have caught that. The browser journeys drive
`src-ui/src/ipc/mock.ts`, which answers with a tidy duration and a position that
advances — so the rail fills, the card appears, and both look perfect. Only a real
stream from a real panel can disagree.

Skipped unless the credentials are in the environment.
"""
import os
import sys
import time

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")

IMPORT_TIMEOUT = 900
# Long enough for the ten-second save timer to have fired at least once.
WATCH_FOR = 25


class Skipped(Exception):
    """Raised when this scenario has nothing to run against."""


def run(d, ctx):
    if sys.platform != "win32":
        raise Skipped("NullBackend reports no position, so this would prove nothing")
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
    assert d.wait_body(
        lambda b: "your library is ready" in b.lower(), timeout=IMPORT_TIMEOUT
    ), "the import never finished"
    d.click(d.by_text("button", "Start watching"))
    time.sleep(4)
    ctx.assert_no_panic()

    # A film, not a channel: live TV has no position worth resuming and is not what
    # Continue Watching is about.
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

    print("   what the host reports:")
    seen = []
    deadline = time.time() + WATCH_FOR
    state = {}
    while time.time() < deadline:
        time.sleep(1)
        state = d.invoke("player_state") or {}
        row = (
            state.get("status"),
            round(state.get("positionSecs", 0), 1),
            round(state.get("durationSecs", 0), 1),
        )
        if not seen or seen[-1] != row:
            seen.append(row)
            print(f"      status={row[0]:<10} position={row[1]:<8} duration={row[2]}")
    ctx.assert_no_panic()
    d.shot(ctx.shot("playing"))

    kind = state.get("itemKind")
    print(f"   itemKind={kind} itemId={state.get('itemId')} isLive={state.get('isLive')}")
    stats = state.get("stats") or {}
    print(f"   decoder: {stats.get('resolution')} {stats.get('videoCodec')}")

    # The exact predicate `saveProgress` applies, reported rather than inferred.
    savable = (
        not state.get("isLive")
        and kind in ("movie", "episode")
        and state.get("durationSecs", 0) > 0
        and state.get("positionSecs", 0) > 0
    )
    print(f"   -> saveProgress would {'write' if savable else 'REFUSE'}")

    assert state.get("status") == "playing", f"nothing played: {state}"
    assert state.get("durationSecs", 0) > 0, (
        "the host reports no duration for a film, so Continue Watching can never save a "
        "position and Up Next can never know where the end is. "
        f"state={ {k: state.get(k) for k in ('status', 'positionSecs', 'durationSecs', 'itemKind')} }"
    )
    assert state.get("positionSecs", 0) > 0, (
        "the host reports a duration but no position, so the progress save is refused "
        f"and the OSD clock cannot move. state={seen}"
    )

    # And the row it should have written by now.
    time.sleep(12)
    rows = ctx.count("watch_progress")
    print(f"   watch_progress rows: {rows}")
    assert rows > 0, (
        "the host reported a position and a duration and still nothing was saved, so "
        "the fault is in the save path rather than in what mpv says"
    )
