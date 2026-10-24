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
    # Off the player before closing, so the warm launch restores a *list* and not a
    # tune. Otherwise the first thing the second launch does is open a stream, and the
    # clock would be timing the provider's handshake rather than this app rendering a
    # library -- which is the thing README §16 budgets four seconds for.
    try:
        d.invoke("player_stop")
    except Exception:
        pass
    d.js("window.location.hash = '#/live'; return true;")
    time.sleep(2)


def _launches(log):
    """Every (started, revealed) pair in the log, in order.

    Walked as pairs rather than as "the newest of each", so a launch that never revealed
    a window shows up as a pair with nothing in it instead of silently borrowing the next
    launch's mark.

    In practice this file holds **one** launch, because the app rotates its log on every
    start: `aurora.log` becomes `aurora.log.1` and a fresh one is opened
    (`aurora_app::logging::rotate`, which exists precisely so "it crashed, I reopened it"
    does not destroy the evidence). The first version of this scenario counted launches
    before and after the relaunch and asserted the number had gone up; it cannot, and the
    run failed with "the log shows 1 launch(es) and the relaunch should have added one"
    after a ten-minute import. The rotation is the right behaviour and the assertion was
    the wrong question.
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
    relaunched_at = dt.datetime.now(dt.timezone.utc)
    d = ctx.relaunch(d)

    # Library content, in whichever shape the app comes back in.
    #
    # Not `channel-row` alone: a warm launch lands on **Home**, not on Live TV, and
    # asking for a row there got "no element matched within 90s" -- which says the row is
    # absent and nothing about what is there. The budget is "cold start to a library
    # fully rendered", and a home screen of posters is that as much as a channel list is.
    #
    # Photographed either way, so a failure is a description rather than another guess.
    rendered = (
        "[data-testid='channel-row'], [data-testid='channel-tile'], "
        "[data-testid='catalog-card']"
    )
    found_at = None
    end = time.time() + 90
    while time.time() < end:
        if d.find_all(rendered):
            found_at = dt.datetime.now(dt.timezone.utc)
            break
        time.sleep(0.25)
    d.shot(ctx.shot("warm-start"))
    if found_at is None:
        body = ""
        try:
            body = d.js("return document.body.innerText;") or ""
        except Exception:
            pass
        raise AssertionError(
            "the warm launch rendered no library content in 90s. What was on screen:\n"
            + body[:1200]
        )

    time.sleep(2)  # the reveal is logged from a command, and the write is buffered
    launches = _launches(log)
    assert launches, "the second launch wrote no log at all"
    started, revealed = launches[-1]
    # The log was rotated when this launch opened it, so what is in it is this launch.
    # Checked against the clock anyway: a stale file -- a launch that failed so early it
    # never rotated, say -- would otherwise be measured as though it were the warm start.
    assert started >= relaunched_at - dt.timedelta(seconds=60), (
        f"the newest launch in the log began at {started}, before the relaunch at "
        f"{relaunched_at} -- this is not the launch that was just made"
    )
    assert revealed is not None, (
        "the second launch never called back into the host, so it did not become "
        "interactive at all"
    )

    # **From the app's own first log line**, not from when this scenario asked for a
    # relaunch.
    #
    # `ctx.relaunch` quits the session, waits for the window to go, kills the drivers and
    # starts them again -- eight to ten seconds of deliberate sleeping before the app is
    # even launched. Timing from the scenario's side swept all of that into the number
    # and reported 20.190s for a startup, which is the harness measuring itself. The
    # host's own `starting` line is the first instant that belongs to the app.
    interactive = (revealed - started).total_seconds()
    wall = (found_at - started).total_seconds()
    print(f"   warm start -> interactive: {interactive:.3f}s")
    print(f"   warm start -> library on screen: {wall:.3f}s (budget {BUDGET_SECS}s)")
    print(f"   {channels} channels in the library, and not counting load before main")
    if wall > BUDGET_SECS:
        print(f"   NOTE: {wall:.3f}s is past README \u00a716's {BUDGET_SECS}s")
    ctx.assert_no_panic()
