"""
Media keys and the Windows media overlay, from Windows' side (`aurora-app/src/media_keys`).

The browser journeys (`tests-e2e/media-keys.spec.ts`) hand the page a press the way the
host would. This asks the other half: whether Windows has heard of Aurora at all. It
reads the same session list the overlay reads -- `tests-host/smtc.ps1`, through
`GlobalSystemMediaTransportControlsSessionManager` -- and presses the overlay's buttons
through it, which is the path a keyboard's media keys and a headset take too.

  * Playing a channel puts Aurora in the overlay, titled with the channel, with next and
    previous on and no position bar (live TV has none).
  * Pause and play from the overlay pause and resume the player.
  * Next changes channel, and the overlay's title follows.
  * Closing the player takes Aurora out of the overlay.

Skipped unless there is a playlist of real streams, like `video_surface`:

    AURORA_TEST_STREAM_M3U=http://127.0.0.1:8123/streams.m3u

and skipped off Windows, where there is no overlay to ask.
"""
import json
import os
import subprocess
import sys
import time

M3U = os.environ.get("AURORA_TEST_STREAM_M3U")
SMTC = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "smtc.ps1")


class Skipped(Exception):
    pass


def smtc(*args):
    out = subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", SMTC, *args],
        capture_output=True, text=True, timeout=60, check=False,
    )
    if out.returncode not in (0, 2):
        raise AssertionError(f"smtc.ps1 {' '.join(args)} failed: {out.stderr.strip()[:400]}")
    return out.stdout.strip()


def sessions():
    return json.loads(smtc("list") or "[]")


def wait(predicate, what, timeout=30):
    end = time.time() + timeout
    last = None
    while time.time() < end:
        last = predicate()
        if last:
            return last
        time.sleep(0.5)
    raise AssertionError(f"{what}; last saw {last!r}")


def ours(title):
    """Aurora's session, found by what it says is playing."""
    return next((s for s in sessions() if s.get("title") == title), None)


def run(d, ctx):
    if sys.platform != "win32":
        raise Skipped("the media overlay is Windows'")
    if not M3U:
        raise Skipped("no AURORA_TEST_STREAM_M3U in the environment")

    time.sleep(6)
    d.send(d.find("textarea"), M3U)
    time.sleep(1)
    d.click(d.by_text("button", "Check connection"))
    assert d.wait_body(
        lambda b: "entries" in b.lower() or "could not" in b.lower(), timeout=60
    ), f"the connection check never finished; screen said {d.body()[:300]!r}"
    d.click(d.by_text("button", "Continue"))
    d.click(d.by_text("button", "Import library", timeout=30))
    assert d.wait_body(
        lambda b: "your library is ready" in b.lower(), timeout=180
    ), f"the import never finished; screen said {d.body()[:300]!r}"
    d.click(d.by_text("button", "Start watching"))
    time.sleep(3)

    d.click(d.by_text("nav a", "Live TV", timeout=30))
    time.sleep(2)
    d.click(d.find('[data-testid="channel-row"]', timeout=30))
    playing = wait(
        lambda: (lambda s: s if s.get("status") == "playing" else None)(d.invoke("player_state")),
        "the channel never started playing", timeout=90,
    )
    first = playing["title"]
    ctx.assert_no_panic()

    s = wait(lambda: (lambda s: s if s and s["status"] == "Playing" else None)(ours(first)),
             f"Windows' overlay never showed {first!r} playing")
    print(f"   overlay: {s}")
    assert s["next"] and s["previous"], f"next and previous should change channel: {s}"
    assert s["endSecs"] == 0, f"live TV has no position bar: {s}"
    app = s["app"]

    assert smtc("pause", app) == "True", "Windows did not take the pause"
    wait(lambda: d.invoke("player_state").get("status") == "paused",
         "pausing from the overlay did not pause the player")
    wait(lambda: (ours(first) or {}).get("status") == "Paused",
         "the overlay never showed it paused")

    assert smtc("play", app) == "True", "Windows did not take the play"
    wait(lambda: d.invoke("player_state").get("status") in ("playing", "buffering"),
         "playing from the overlay did not resume the player")

    was = d.invoke("player_state")
    before = was.get("channelId")
    print(f"   before next: channel={before} title={was.get('title')!r} live={was.get('isLive')}")
    assert smtc("next", app) == "True", "Windows did not take next"
    try:
        after = wait(lambda: (lambda s: s if s.get("title") != was.get("title") else None)(
            d.invoke("player_state")), "next from the overlay did not change channel")
    except AssertionError:
        now = d.invoke("player_state")
        print(f"   after next: channel={now.get('channelId')} title={now.get('title')!r} "
              f"status={now.get('status')} overlay={sessions()}")
        raise
    wait(lambda: ours(after["title"]), f"the overlay never followed the zap to {after['title']!r}")
    ctx.assert_no_panic()

    d.invoke("player_stop")
    wait(lambda: not any(x.get("app") == app for x in sessions()),
         "Aurora stayed in the overlay after the player closed", timeout=15)
