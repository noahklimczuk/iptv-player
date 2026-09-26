"""
The playback controls, and what happens when the player is closed.

Two reports this is the regression test for:

  * "none of the player controls work"
  * "the content still plays once its closed"

Neither could be caught anywhere else in this repository. The browser journeys drive
`src-ui/src/ipc/mock.ts`, which answers `player.pause` out of its own memory and always
agrees with the UI that asked. Only a real host can disagree.

libmpv is not on this platform, so `aurora-player` is `NullBackend` — no decoding, no
picture. That is fine for these two questions, because neither is about video: they are
about whether a click reaches the host at all, and whether closing the overlay stops
what is playing. `NullBackend` models status, position and volume exactly as the real
one reports them, and every control writes a `player control control=… status=…` line
to `aurora.log` on its way through, so the host's own account of what it was asked to
do is readable from outside the window.
"""
import os
import time

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")


class Skipped(Exception):
    pass


def controls(ctx):
    """What the host says it was asked to do, in order."""
    out = []
    for line in ctx.log().splitlines():
        if "player control" in line and "refused" not in line:
            # `control="pause" status=Paused` — tracing quotes the string field.
            name = line.split("control=", 1)[1].split()[0].strip('"') if "control=" in line else "?"
            status = line.split("status=", 1)[1].split()[0].strip('"') if "status=" in line else "?"
            out.append((name, status.lower()))
    return out


def run(d, ctx):
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")

    time.sleep(6)

    # Straight through the wizard; this scenario is about what comes after it.
    d.send(d.find("textarea"), URL)
    time.sleep(1)
    d.click(d.by_text("button", "Xtream / panel login"))
    time.sleep(0.5)
    d.send(d.by_label("Username"), USER)
    d.send(d.by_label("Password"), PASS)
    d.click(d.by_text("button", "Check connection"))
    assert d.wait_body(
        lambda b: "days left" in b.lower() or "connections" in b.lower(), timeout=120
    ), "the connection check never finished"
    d.click(d.by_text("button", "Continue"))
    d.click(d.by_text("button", "Import library", timeout=30))
    assert d.wait_body(
        lambda b: "your library is ready" in b.lower(), timeout=900
    ), "the import never finished"
    d.click(d.by_text("button", "Start watching"))
    time.sleep(4)

    # Play the first channel in the list.
    d.click(d.by_text("nav a", "Live TV", timeout=30))
    assert d.wait_body(lambda b: "Live TV" in b, timeout=60)
    time.sleep(4)
    # The row itself is the play control — `aria-label="Watch <name>"`.
    d.click(d.find('[data-testid="channel-row"]', timeout=30))

    # The overlay is up once its own Back button is, which no other screen has.
    d.find('[aria-label="Back"]', timeout=30)
    time.sleep(2)
    d.shot(ctx.shot("playing"))
    ctx.assert_no_panic()

    played = controls(ctx)
    assert played, "nothing reached the host: no 'player control' line in the log"
    assert played[-1][1] == "playing", (
        f"the host should be playing after Play; its last word was {played[-1]}"
    )

    # Pause. The button is labelled from the host's status, so it flipping to "Play" is
    # the whole round trip: click, command, state event, re-render.
    d.click(d.find('[aria-label="Pause"]'))
    time.sleep(2)
    d.shot(ctx.shot("paused"))
    names = [c for c, _ in controls(ctx)]
    assert "pause" in names, (
        f"clicking Pause sent nothing to the host; it has only been asked for {names}"
    )
    assert d.find_all('[aria-label="Play"]'), (
        "the host was asked to pause but the button still says Pause — the OSD is not "
        "following the host's state"
    )

    # And back.
    d.click(d.find('[aria-label="Play"]'))
    time.sleep(2)
    names = [c for c, _ in controls(ctx)]
    assert "resume" in names, f"clicking Play sent nothing; the host has seen {names}"
    assert d.find_all('[aria-label="Pause"]'), "the OSD did not follow the resume"

    # Closing the player has to stop it. This is the report in full: the overlay goes
    # away, the window looks idle, and the stream is still running behind it.
    d.click(d.find('[aria-label="Back"]'))
    time.sleep(3)
    d.shot(ctx.shot("closed"))
    ctx.assert_no_panic()

    after = controls(ctx)
    assert "stop" in [c for c, _ in after], (
        "closing the player never asked the host to stop — this is 'the content still "
        f"plays once its closed'. The host was asked for {[c for c, _ in after]}"
    )
    assert after[-1][1] in ("idle", "stopped"), (
        f"the player is still {after[-1][1]} after being closed"
    )
