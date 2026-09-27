"""
A stream that does not open, and whether the app ever says so.

This is the most ordinary thing that happens to an IPTV player: a URL the provider
handed out is dead. README §7.14 says a failure is named and the next source tried;
`PlaybackError::classify` exists to turn mpv's words into something a viewer can act on.
None of it had been run, because nothing off Windows has an mpv to fail.

What it is really testing is the event pump. libmpv2 does not deliver a failed file as
`Event::EndFile` — it turns an end-file carrying an error into `Err(...)` from
`wait_event`, which is a different arm of the same match. A drain loop written as
`while let Some(Ok(event))` therefore stops dead on the first failure and throws away
every event queued behind it, so the OSD keeps showing whatever the load set and the
error never arrives.

Skipped unless it is given a playlist whose **first** channel is a dead URL, because a
dead URL is not this repository's to carry either:

    AURORA_TEST_DEAD_M3U=http://127.0.0.1:8123/dead.m3u
"""
import os
import sys
import time

M3U = os.environ.get("AURORA_TEST_DEAD_M3U")

# Generous: mpv retries a connection before it gives up, and `network-timeout` is 12s.
GIVE_UP_AFTER = 45


class Skipped(Exception):
    """Raised when this scenario has nothing to run against."""


def run(d, ctx):
    if sys.platform != "win32":
        raise Skipped("mpv is Windows-only; NullBackend never fails to open anything")
    if not M3U:
        raise Skipped("no AURORA_TEST_DEAD_M3U in the environment")

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
        lambda b: "your library is ready" in b.lower(), timeout=120
    ), "the import never finished"
    d.click(d.by_text("button", "Start watching"))
    time.sleep(3)

    d.click(d.by_text("nav a", "Live TV", timeout=30))
    assert d.wait_body(lambda b: "Live TV" in b, timeout=60)
    time.sleep(2)
    d.click(d.find('[data-testid="channel-row"]', timeout=30))
    d.find('[aria-label="Back"]', timeout=30)

    end = time.time() + GIVE_UP_AFTER
    seen = []
    state = {}
    while time.time() < end:
        state = d.invoke("player_state") or {}
        status = state.get("status")
        if status and (not seen or seen[-1] != status):
            seen.append(status)
        if status == "error" or state.get("error"):
            break
        time.sleep(0.3)

    ctx.assert_no_panic()
    d.shot(ctx.shot("dead"))
    print(f"   statuses seen: {seen}, error={state.get('error')!r}")

    assert state.get("status") == "error", (
        "a channel whose URL refuses every connection left the player reporting "
        f"{state.get('status')!r} for {GIVE_UP_AFTER}s, with error={state.get('error')!r}. "
        "Nothing on screen would ever tell the viewer the stream is dead."
    )
    assert state.get("error"), (
        "the player is in an error state with no error to show, so the OSD has nothing "
        "to say beyond that something went wrong"
    )

    # And the pump must not have stopped: a failure arriving as `Err` must not take the
    # events behind it with it. Tuning a channel that *does* work is the proof.
    rows = d.find_all('[data-testid="channel-row"]')
    if len(rows) < 2:
        d.click(d.find('[aria-label="Back"]'))
        time.sleep(2)
        rows = d.find_all('[data-testid="channel-row"]')
    assert len(rows) >= 2, "this playlist needs a working channel after the dead one"
    d.click(rows[1])

    end = time.time() + 45
    after = {}
    while time.time() < end:
        after = d.invoke("player_state") or {}
        if after.get("status") == "playing":
            break
        time.sleep(0.3)
    print(f"   after a dead channel: {after.get('status')!r}, "
          f"{(after.get('stats') or {}).get('resolution')}")
    assert after.get("status") == "playing", (
        "after one channel failed to open, a working one never started: the event pump "
        f"stopped at the failure. The player is {after.get('status')!r}."
    )
    assert not after.get("error"), (
        f"the previous channel's error is still on screen: {after.get('error')!r}"
    )
