"""
The one thing in the roadmap that says "No stream has yet been buffered and seeked".

`docs/ROADMAP.md` has carried that line since the buffer was built. The options stopped
being taken from documentation — libmpv v0.41 has no `cache-dir`, so the buffer had been
going to mpv's own folder until `demuxer-cache-dir` replaced it — but nobody had watched
a live stream accumulate on disk and then rewound into it. Every part of this needs a
real provider: a fixture has no live edge to fall behind, and the test server does not
hold a stream open long enough to have a past.

What it establishes, in order:

  * the buffer is configured on and pointed somewhere the app chose;
  * a live stream playing for a while reports a `timeshift` window at all, which is mpv
    answering rather than the app hoping;
  * that window grows — `liveSecs` moving is the proof bytes are being kept rather than
    a field being populated once;
  * seeking back inside it moves the playhead and playback survives it;
  * `player.backToLive` returns to the edge;
  * and the bytes are on disk in the folder the settings name, which is the half that
    `demuxer-cache-dir` was about.

One connection, so this runs alone — the line allows exactly one and the harness holds
it for the length of the scenario.
"""
import os
import time

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")

# How long to let the buffer accumulate before rewinding into it. Long enough that a
# rewind of REWIND_SECS has somewhere to land, short enough that the scenario is not
# mostly waiting.
SOAK_SECS = 45
REWIND_SECS = 20


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


def _a_playing_channel(d, ctx):
    """Tune channels until one actually plays, and return its state."""
    # `channels.list` takes a filter, not a page — the whole visible list comes back
    # and the first dozen is all this needs.
    channels = d.invoke("channels_list", {})
    assert channels, "the real library returned no channels"
    for channel in channels[:12]:
        d.invoke("player_play", {"kind": "live", "id": channel["id"]})
        for _ in range(40):
            time.sleep(0.5)
            state = d.invoke("player_state")
            if state["status"] == "playing" and state["positionSecs"] > 0:
                print(f"   playing {channel['name']!r}")
                return state
            if state["status"] == "error":
                break
        ctx.assert_no_panic()
    raise AssertionError("no channel out of twelve started playing")


def run(d, ctx):
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")

    _import_the_library(d)

    settings = d.invoke("timeshift_settings")
    print(
        f"   buffer: enabled={settings['enabled']} "
        f"secs={settings['secs']} bytes={settings['bytes']}"
    )
    if not settings["enabled"]:
        d.invoke(
            "timeshift_set_settings",
            {"enabled": True, "bytes": settings["bytes"], "secs": settings["secs"]},
        )
        settings = d.invoke("timeshift_settings")
        assert settings["enabled"], "the buffer would not turn on"
    folder = settings["folder"]
    assert folder, "the buffer has no folder, so mpv would choose its own"
    print(f"   folder: {folder}")

    _a_playing_channel(d, ctx)

    # Let it run, and watch the window rather than assume it.
    first = None
    last = None
    for _ in range(SOAK_SECS * 2):
        time.sleep(0.5)
        state = d.invoke("player_state")
        window = state.get("timeshift")
        if window:
            last = window
            if first is None:
                first = window
                print(
                    f"   first window: start={window['startSecs']:.1f} "
                    f"pos={window['positionSecs']:.1f} live={window['liveSecs']:.1f}"
                )
    ctx.assert_no_panic()

    assert first is not None, (
        "the stream played and never reported a timeshift window, so nothing was being "
        "kept -- which is what this scenario exists to find out"
    )
    assert last is not None
    grew = last["liveSecs"] - first["liveSecs"]
    print(f"   the live edge moved {grew:.1f}s over {SOAK_SECS}s of playing")
    assert grew > 5, f"the live edge barely moved ({grew:.1f}s): nothing is accumulating"

    rewindable = last["liveSecs"] - last["startSecs"]
    print(f"   rewindable: {rewindable:.1f}s")
    assert rewindable > REWIND_SECS, (
        f"only {rewindable:.1f}s is held, so there is nothing to seek back into"
    )

    # The whole point: go back into the past and keep playing.
    target = last["liveSecs"] - REWIND_SECS
    d.invoke("player_seek", {"positionSecs": target})
    time.sleep(3)
    after = d.invoke("player_state")
    ctx.assert_no_panic()
    assert after["status"] == "playing", f"playback stopped on a rewind: {after['status']}"
    behind = after["timeshift"]["liveSecs"] - after["timeshift"]["positionSecs"]
    print(f"   rewound and still playing, {behind:.1f}s behind live")
    assert behind > REWIND_SECS / 2, (
        f"the seek did not take: {behind:.1f}s behind live after asking for {REWIND_SECS}s"
    )

    # And the bytes are where the settings said they would be.
    on_disk = d.invoke("timeshift_settings")["bytesOnDisk"]
    print(f"   {on_disk} bytes on disk under the app's own folder")
    assert on_disk > 0, "nothing is on disk, so the buffer is somewhere else"

    # Rejoining live gives the delay up.
    d.invoke("player_back_to_live")
    time.sleep(3)
    live = d.invoke("player_state")
    ctx.assert_no_panic()
    assert live["status"] == "playing", f"rejoining live stopped playback: {live['status']}"
    if live.get("timeshift"):
        back = live["timeshift"]["liveSecs"] - live["timeshift"]["positionSecs"]
        print(f"   back at the edge, {back:.1f}s behind")
        assert back < behind, "backToLive did not reduce the delay"

    d.invoke("player_stop")
    ctx.assert_no_panic()
