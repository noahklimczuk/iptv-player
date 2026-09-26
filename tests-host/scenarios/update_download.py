"""
Check for an update, download it, verify it, and stage it — through the real host.

The report this is the regression test for: "it doesn't download anything, it pulls
the update but won't download or install", and then again after a fix, "I click
download and it does nothing".

Nothing here could be caught anywhere else. The Rust tests drive `download_asset` and
`stage_zip` directly against a local server, which proves the pieces and not the flow;
the browser journeys drive a mock that reports progress out of its own memory. What
joins them is a Tauri command, a background thread, and an `update.download` event —
and that event was one of the ones that never arrived, because every event name in
this app contains a `.` and Tauri 2 rejects those outright. So the button really did
do nothing visible: the download ran, the progress bar sat at zero, and "Install and
restart" never appeared.

The app cannot reach GitHub from a container without direct egress — it bundles
Mozilla's roots rather than the system store, so a TLS-inspecting proxy is fatal — so
`run.py` serves a stand-in on loopback and points the app at it with
`AURORA_UPDATE_API`. The archive it serves is a real zip of the shape `stage_zip`
requires, and its digest is computed from the bytes actually served.
"""
import os
import time


def run(d, ctx):
    time.sleep(6)

    # Past the wizard, with the five-line fixture rather than a real subscription:
    # this scenario is about Settings, and a 78 MB import would only be waiting.
    #
    # Note "Skip for now" cannot be used here. `needsSetup` in `App.tsx` is
    # `!setupDone || providers?.length === 0`, so on a fresh library the button is on
    # screen and does nothing — a provider has to exist before the wizard will let go.
    d.send(d.find("textarea"), "http://127.0.0.1:8099/playlist.m3u")
    time.sleep(1)
    d.click(d.by_text("button", "Check connection"))
    assert d.wait_body(lambda b: "entries" in b.lower(), timeout=45), (
        "the connection check never finished"
    )
    d.click(d.by_text("button", "Continue"))
    d.click(d.by_text("button", "Import library", timeout=30))
    assert d.wait_body(lambda b: "your library is ready" in b.lower(), timeout=120), (
        "the import never finished"
    )
    d.click(d.by_text("button", "Start watching"))
    time.sleep(3)

    d.click(d.by_text("nav a", "Settings", timeout=30))
    # The check runs on its own eight seconds after launch, but forcing it makes the
    # scenario about the button rather than about the timer.
    # Scroll by text rather than by handle: React re-renders this panel while the
    # first check runs, and an element handle taken before that is stale by the time
    # it is clicked.
    d.by_text("button", "Check now", timeout=45)
    d.js(
        "const b = [...document.querySelectorAll('button')]"
        "  .find((e) => e.textContent.trim() === 'Check now');"
        " if (b) b.scrollIntoView({block: 'center'});"
    )
    time.sleep(1)
    d.click(d.by_text("button", "Check now", timeout=15))
    found = d.wait_body(lambda b: "99.0.0" in b, timeout=60)
    ctx.assert_no_panic()
    d.shot(ctx.shot("checked"))
    assert found, (
        "the check never reported the published release. The app asked for "
        f"{ctx.github.hits}; the screen says {d.body()[:400]!r}"
    )
    assert any("releases/latest" in h for h in ctx.github.hits), (
        f"the app never asked for the release list; it asked for {ctx.github.hits}"
    )

    # The whole report, in one click.
    d.js(
        "const b = [...document.querySelectorAll('button')]"
        "  .find((e) => e.textContent.trim() === 'Download update');"
        " if (b) b.scrollIntoView({block: 'center'});"
    )
    time.sleep(1)
    d.click(d.by_text("button", "Download update", timeout=30))

    # "Install and restart" is the only thing that means downloaded *and verified*:
    # the digest is checked before the state becomes Ready, and for a portable copy
    # the archive is unpacked before it too.
    ready = d.wait_body(lambda b: "Install and restart" in b, timeout=180)
    ctx.assert_no_panic()
    d.shot(ctx.shot("ready"))
    assert ready, (
        "the download never finished. This is 'I click download and it does nothing'. "
        f"The app asked for {ctx.github.hits}; the updates folder holds "
        f"{ctx.downloaded()}; the screen says {d.body()[:400]!r}"
    )

    # And it is really on disk, not just on screen.
    assert any("/download/" in h for h in ctx.github.hits), (
        f"the asset itself was never fetched; the app asked for {ctx.github.hits}"
    )
    files = ctx.downloaded()
    assert not any(f.endswith(".part") for f in files), (
        f"a half-finished download was left behind: {files}"
    )

    # A portable copy unpacks at download time rather than at install time, so that a
    # bad archive is found while someone is watching a progress bar.
    staged = os.path.join(ctx.updates_dir(), "staged")
    assert os.path.isdir(staged), (
        f"the archive was downloaded but never staged; updates holds {files}"
    )
    inside = sorted(os.listdir(staged))
    app_exe = "aurora-app.exe" if os.name == "nt" else "aurora-app"
    assert app_exe in inside, (
        f"the staged folder does not hold the new binary; it holds {inside}"
    )
    # The marker `staged()` looks for, written last and only once everything else
    # worked — so its presence is the difference between "unpacked" and "usable".
    assert ".ready" in inside, (
        f"the staged update was never marked ready; the folder holds {inside}"
    )
    print(f"   updates folder {files}, staged {inside}")
