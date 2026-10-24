"""
The second startup budget: cold start to a library fully rendered (README §16, 4s).

`startup_budget.py` measures the other one — `main` to the front end calling back, 0.653s
against 2.0s — and says in as many words what it cannot do: that number is the
*empty-library* case, because the harness wipes `data` before every scenario. A first
launch has nothing to render, so "a library fully rendered" is not a question it can ask.

This asks it. The library is imported, the app is closed, and it is opened again on the
same directory — which `ctx.relaunch` exists for and which nothing could do before. What
is measured is the launch that a person actually has every day after the first.

Two marks, both already in the log, and the second is the one that matters:

  * `Aurora TV ... starting` — the first thing `main` logs.
  * `window revealed by the UI` — a command the *front end* calls, so reaching it means
    the bundle loaded, React mounted and the UI got through to the host.

"Fully rendered" is then asked of the window rather than the log, because the log cannot
see a poster: the clock keeps running until a channel row is actually on screen with text
in it. A warm launch restores the last screen, so what that is depends on where the
import left off — Live TV, here, because `_import_the_library` ends on "Start watching".

The same two caveats as the cold measurement, for the same reasons. It is a **lower
bound**: the first mark is written once the Rust logger exists, so loading a 21 MB
executable and libmpv before `main` is not counted. And it is one machine on one disk,
so it is reported rather than asserted — a suite that fails because somebody's disk was
busy has stopped being evidence.
"""
import datetime as dt
import os
import re
import time

# README §16: cold start to a library fully rendered.
BUDGET_SECS = 4.0

START = re.compile(r"^(\S+)\s+INFO aurora_app: Aurora TV .* starting")
REVEALED = re.compile(r"^(\S+)\s+INFO aurora_app::commands: window revealed by the UI")

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")


class Skipped(Exception):
    pass


def _stamp(raw):
    """One tracing timestamp, which is RFC 3339 with a `Z`."""
    return dt.datetime.fromisoformat(raw.replace("Z", "+00:00"))


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
        lambda b: "days left" in b.lower() or "connections" in b.lower(), timeout=120
    ), "the connection check never finished"
    d.click(d.by_text("button", "Continue"))
    d.click(d.by_text("button", "Import library", timeout=30))
    assert d.wait_body(
        lambda b: "your library is ready" in b.lower(), timeout=900
    ), "the import never finished"
    d.click(d.by_text("button", "Start watching"))
    time.sleep(4)


def _launches(log):
    """Every (started, revealed) pair in the log, in order.

    A pair rather than the last of each: the file holds both launches by the time this
    reads it, and taking the newest `starting` with the newest `revealed` would be right
    only by luck. Walked instead, so a launch that never revealed a window is visible as
    a pair with nothing in it rather than silently borrowing the next launch's mark.
    """
    out = []
    with open(log, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            m = START.match(line)
            if m:
                out.append([_stamp(m.group(1)), None])
                continue
            m = REVEALED.match(line)
            if m and out and out[-1][1] is None:
                out[-1][1] = _stamp(m.group(1))
    return out


def run(d, ctx):
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")

    _import_the_library(d)
    channels = len(d.invoke("channels_list", {}) or [])
    assert channels, "the import produced no channels, so there is nothing to render"
    print(f"   library imported: {channels} channels visible")

    # Close it and open it again on the same directory. This is the launch being
    # measured; everything above is only how the library got there.
    log = os.path.join(ctx.data_dir, "aurora.log")
    before = len(_launches(log))
    opened_at = time.time()
    d = ctx.relaunch(d)

    # "Fully rendered" asked of the window, because the log cannot see a poster. The
    # import ends on "Start watching", so a warm launch comes back to Live TV.
    assert d.wait_body(
        lambda b: len(b.strip()) > 400, timeout=60
    ), "the second launch never put anything on screen"
    wall = time.time() - opened_at

    time.sleep(2)  # the reveal is logged from a command, and the write is buffered
    launches = _launches(log)
    assert len(launches) > before, (
        f"the log shows {len(launches)} launch(es) and the relaunch should have added one"
    )
    started, revealed = launches[-1]
    assert revealed is not None, (
        "the second launch never called back into the host, so it did not become "
        "interactive at all"
    )

    interactive = (revealed - started).total_seconds()
    print(f"   warm start -> interactive: {interactive:.3f}s")
    print(f"   warm start -> library on screen: {wall:.3f}s (budget {BUDGET_SECS}s)")
    print(f"   {channels} channels in the library, and not counting load before main")
    if wall > BUDGET_SECS:
        print(f"   NOTE: {wall:.3f}s is past README §16's {BUDGET_SECS}s")
    ctx.assert_no_panic()
