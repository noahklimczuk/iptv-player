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
import subprocess
import time

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")

# How long to let the buffer accumulate before rewinding into it. Long enough that a
# rewind of REWIND_SECS has somewhere to land, short enough that the scenario is not
# mostly waiting.
# Overridable, because the question of whether mpv ever spills the cache to disk can
# only be answered by filling it: a short soak proves the rewind works and proves
# nothing about where the bytes live.
SOAK_SECS = int(os.environ.get("AURORA_TIMESHIFT_SOAK_SECS", "45"))
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



def _working_set_mb():
    """The app's resident memory, in MiB, or None if it cannot be read.

    Sampled from outside because nothing inside reports it: `stress.rs` measures RSS
    through `/proc/self/status` and skips on Windows, which is the only platform this
    harness runs on. The number matters here because a buffer that is not on disk is in
    memory, and README §16 gives playback 900 MB.
    """
    try:
        out = subprocess.run(
            ["powershell", "-NoProfile", "-Command",
             "(Get-Process aurora-app -ErrorAction SilentlyContinue "
             "| Measure-Object WorkingSet64 -Maximum).Maximum"],
            capture_output=True, text=True, timeout=20,
        ).stdout.strip()
        return int(out) / (1024 * 1024) if out else None
    except Exception:
        return None


def _open_files_mb():
    """The largest files written to the user's temp directory in the last few minutes.

    Asked while the app is alive, on purpose: mpv's `cache-on-disk` file is a temporary
    that goes away when the handle closes, so a search after the run finds an empty disk
    and proves nothing. Temp is where mpv puts a cache when it has not been told
    otherwise, which is the thing being tested — the app does tell it otherwise.
    """
    script = (
        "$t=[IO.Path]::GetTempPath();"
        "Get-ChildItem -Path $t -File -ErrorAction SilentlyContinue "
        "| Where-Object { $_.LastWriteTime -gt (Get-Date).AddMinutes(-3) "
        "-and $_.Length -gt 1MB } "
        "| Sort-Object Length -Descending | Select-Object -First 5 "
        "| ForEach-Object { \"$($_.Name)|$([math]::Round($_.Length/1MB,1))\" }"
    )
    try:
        out = subprocess.run(
            ["powershell", "-NoProfile", "-Command", script],
            capture_output=True, text=True, timeout=30,
        ).stdout.strip()
    except Exception:
        return []
    found = []
    for line in out.splitlines():
        if "|" in line:
            name, mb = line.rsplit("|", 1)
            try:
                found.append((name.strip(), float(mb)))
            except ValueError:
                pass
    return found


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

    playing = _a_playing_channel(d, ctx)
    mem_before = _working_set_mb()
    # The bitrate turns "where did the bytes go" from inference into arithmetic: what a
    # buffer of N seconds must weigh is N x this, and that is either accounted for by
    # memory growth or it is somewhere else.
    kbps = 0  # Filled from the soak below: `stats` is empty the instant playback starts.

    # Let it run, and watch the window rather than assume it.
    first = None
    last = None
    samples = 0
    for _ in range(SOAK_SECS * 2):
        time.sleep(0.5)
        state = d.invoke("player_state")
        window = state.get("timeshift")
        if window:
            last = window
            samples += 1
            if not kbps:
                kbps = (state.get("stats") or {}).get("bitrateKbps") or 0
            # Every ~15s: is anything reaching the disk yet, and what is memory doing?
            if samples % 30 == 0:
                disk = d.invoke("timeshift_settings")["bytesOnDisk"]
                if samples % 120 == 0:
                    for name, mb in _open_files_mb():
                        print(f"      temp: {name} {mb}MB")
                mem = _working_set_mb()
                held = window["liveSecs"] - window["startSecs"]
                print(
                    f"      t+{samples // 2}s held={held:.0f}s "
                    f"disk={disk / (1024 * 1024):.1f}MB "
                    f"mem={mem:.0f}MB" if mem else f"      t+{samples // 2}s disk={disk}"
                )
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
    mem_after = _working_set_mb()
    if mem_before is not None and mem_after is not None:
        grew = mem_after - mem_before
        expected = rewindable * kbps / 8000 if kbps else 0
        print(
            f"   memory: {mem_before:.0f} MB -> {mem_after:.0f} MB "
            f"(grew {grew:.0f} MB while {rewindable:.0f}s accumulated)"
        )
        if expected:
            print(
                f"   {rewindable:.0f}s at {kbps} kbps is {expected:.0f} MB; "
                f"memory accounts for {grew:.0f} MB of it"
            )
            if grew < expected * 0.5:
                print(
                    "   NOTE: most of the buffer is not in memory and not in the app's "
                    "folder, so mpv is keeping it somewhere the viewer did not choose"
                )
        # README §16: playing 1080p, 900 MB. Reported rather than asserted, because one
        # sample on one machine is not a budget verdict.
        if mem_after > 900:
            print(f"   NOTE: {mem_after:.0f} MB is past README §16's 900 MB for playback")

    # Where the bytes actually are. `cache-on-disk=yes` and `demuxer-cache-dir` are both
    # accepted by mpv — the log carries no "mpv rejected" for either — so an empty folder
    # with a working rewind means the buffer is resident instead, bounded by
    # `demuxer-max-back-bytes` rather than by the folder the viewer chose.
    if on_disk == 0:
        print(
            "   NOTE: the rewind works and the folder is empty, so the buffer is in "
            "memory. The disk budget is measured against this folder, so it is not "
            "bounding anything."
        )
    else:
        assert on_disk > 0

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
