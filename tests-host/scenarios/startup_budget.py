"""
Cold start, against README §16's two-second budget to an interactive UI.

`docs/ROADMAP.md` has said "startup is still unmeasured" since the budgets were written.
It needs no panel and no import, which is why it is worth having as its own scenario
rather than a line inside one: this runs on a fresh profile in seconds.

The two marks both already exist in the log, so nothing had to be instrumented:

  * `Aurora TV ... starting` — the first thing `main` logs.
  * `window revealed by the UI` — emitted by a *command the front end calls*. Reaching
    it means the bundle loaded, React mounted, and the UI invoked the host. That is a
    closer proxy for "interactive" than anything the harness can see from outside, which
    only knows when an element it polls for happens to appear.

Two things it is not, said here rather than left for somebody to assume:

  * **It is a lower bound on true cold start.** The first mark is written once the Rust
    logger exists, so loading a 21 MB executable and libmpv before `main` is not counted.
  * **It is the empty-library case.** The harness wipes the data directory per scenario,
    so this is a new install's first launch. §16's other budget — cold start to a library
    fully rendered, 4 seconds — needs a second launch with a populated library, which
    `run.py` does not do; that one stays unmeasured and should not be reported from here.

Reported rather than asserted, like the other budget scenarios: a startup number on one
machine is evidence, not a verdict, and a suite that fails because somebody's disk was
busy stops being useful.
"""
import datetime as dt
import os
import re
import time

# README §16: cold start to an interactive UI.
BUDGET_SECS = 2.0

START = re.compile(r"^(\S+)\s+INFO aurora_app: Aurora TV .* starting")
REVEALED = re.compile(r"^(\S+)\s+INFO aurora_app::commands: window revealed by the UI")


def _stamp(raw):
    """One tracing timestamp, which is RFC 3339 with a `Z`."""
    return dt.datetime.fromisoformat(raw.replace("Z", "+00:00"))


def run(d, ctx):
    # The app is already up — the session exists — so the marks are in the log by now.
    # Given a moment, because the reveal is logged from a command and the write is
    # buffered.
    time.sleep(3)

    log = os.path.join(ctx.data_dir, "aurora.log")
    assert os.path.exists(log), f"no aurora.log to read (looked at {log!r})"

    started = revealed = None
    with open(log, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            if started is None:
                m = START.match(line)
                if m:
                    started = _stamp(m.group(1))
                    continue
            if revealed is None:
                m = REVEALED.match(line)
                if m:
                    revealed = _stamp(m.group(1))
                    break

    assert started is not None, "the log has no 'starting' line"
    assert revealed is not None, (
        "the log has no 'window revealed by the UI' line, so the front end never "
        "called the host and this launch did not become interactive at all"
    )

    elapsed = (revealed - started).total_seconds()
    print(f"   cold start -> interactive: {elapsed:.3f}s (budget {BUDGET_SECS}s)")
    print("   empty library, and not counting process load before main")
    if elapsed > BUDGET_SECS:
        print(f"   NOTE: {elapsed:.3f}s is past README §16's {BUDGET_SECS}s")
    ctx.assert_no_panic()
