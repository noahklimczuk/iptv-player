"""
The zap budget, against a provider instead of a CDN, and broken into its parts.

README §16 gives a channel change 1.5 seconds before it feels broken.
`video_surface.py` measured 1.91s, and `docs/ROADMAP.md` records the honest caveat with
it: *"measured against a public CDN rather than a provider. Worth re-measuring against a
real panel before optimising anything, since the number may be the CDN."* This is that
re-measurement.

It also takes the number apart, because a total says nothing about what to fix:

  * **call** — how long `player.play` itself takes to return. Time here is the host
    blocking the caller: resolving the url, taking the player mutex, telling mpv to load.
  * **playing** — from the call returning to the status saying `playing`. mpv opening the
    connection and the provider answering.
  * **frame** — from `playing` to the first evidence of data, which is `positionSecs`
    moving or `bufferSecs` filling. The stream actually arriving.

The same definition of "landed" as `video_surface.py`, deliberately, so the numbers are
comparable with the one in the roadmap: playing, on the channel asked for, with either a
position or a buffer. Not `positionSecs` alone — a live stream can sit at 0 while its
buffer fills.

One sample is an anecdote, so this zaps around a ring of channels and reports the
distribution. Nothing is asserted: the budget is a claim about a provider and a
connection as much as about this code, and a suite that fails on somebody else's bad
minute stops being evidence. `video_surface.py` makes the same choice for the same
reason.
"""
import os
import statistics
import time

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")

ZAP_BUDGET_SECS = 1.5
# Enough channels that no two consecutive zaps reuse a connection, and enough zaps that
# one slow handshake does not decide the median.
RING = 6
ZAPS = 10
# A dead channel should not cost the measurement window to discover.
PROBE_SECS = 8


class Skipped(Exception):
    pass


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


def _landed(state, was_channel):
    """`video_surface.py`'s definition, so the numbers can be compared."""
    if state.get("status") != "playing":
        return False
    if state.get("channelId") == was_channel:
        return False
    return (
        state.get("positionSecs", 0) > 0
        or (state.get("stats") or {}).get("bufferSecs", 0) > 0
    )


def _time_one_zap(d, channel, was_channel, timeout=25):
    """(call, playing, frame, total) for one change, or None if it never started.

    `timeout` is short while building the ring and generous while measuring: a
    provider's list carries plenty of dead channels, and waiting the full measurement
    window on each one turns picking six playable channels into several minutes of
    nothing.
    """
    started = time.time()
    d.invoke("player_play", {"kind": "live", "id": channel["id"]})
    call = time.time() - started

    playing_at = None
    while time.time() - started < timeout:
        state = d.invoke("player_state")
        if playing_at is None and state.get("status") == "playing":
            playing_at = time.time() - started
        if _landed(state, was_channel):
            total = time.time() - started
            # `playing` can be observed in the same poll as the first data, in which
            # case the split is below the sampling interval rather than zero.
            playing = playing_at if playing_at is not None else total
            return call, playing, total - playing, total
        if state.get("status") == "error":
            return None
        time.sleep(0.05)
    return None


def run(d, ctx):
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")

    _import_the_library(d)

    # A ring of channels that actually play. A provider's list has plenty that do not,
    # and a dead channel is not a zap measurement.
    channels = d.invoke("channels_list", {})
    assert channels, "the real library returned no channels"
    ring = []
    was = None
    for candidate in channels:
        if len(ring) >= RING:
            break
        if _time_one_zap(d, candidate, was, timeout=PROBE_SECS) is not None:
            ring.append(candidate)
            was = candidate["id"]
        ctx.assert_no_panic()
    assert len(ring) >= 2, f"only {len(ring)} channels would play; cannot zap between them"
    print(f"   ring of {len(ring)}: {', '.join(c['name'][:22] for c in ring)}")

    calls, playings, frames, totals = [], [], [], []
    was = ring[-1]["id"]
    for i in range(ZAPS):
        target = ring[i % len(ring)]
        if target["id"] == was:
            target = ring[(i + 1) % len(ring)]
        timing = _time_one_zap(d, target, was)
        ctx.assert_no_panic()
        if timing is None:
            print(f"   zap {i + 1}: {target['name'][:22]!r} never started, skipped")
            continue
        call, playing, frame, total = timing
        calls.append(call)
        playings.append(playing)
        frames.append(frame)
        totals.append(total)
        was = target["id"]
        print(
            f"   zap {i + 1}: {total:.2f}s "
            f"(call {call:.2f} / playing {playing:.2f} / frame {frame:.2f})"
        )

    assert totals, "not one zap completed"
    print(
        f"   zaps: n={len(totals)} "
        f"min={min(totals):.2f}s median={statistics.median(totals):.2f}s "
        f"max={max(totals):.2f}s  budget={ZAP_BUDGET_SECS}s"
    )
    print(
        f"   where it goes (medians): call={statistics.median(calls):.2f}s "
        f"playing={statistics.median(playings):.2f}s "
        f"frame={statistics.median(frames):.2f}s"
    )
    over = [t for t in totals if t > ZAP_BUDGET_SECS]
    print(f"   over budget: {len(over)} of {len(totals)}")

    # Reported, not asserted — see the module docstring.
    if over:
        print(
            f"   NOTE: {len(over)} of {len(totals)} zaps exceeded README §16's "
            f"{ZAP_BUDGET_SECS}s, median {statistics.median(totals):.2f}s"
        )

    d.invoke("player_stop")
    ctx.assert_no_panic()
