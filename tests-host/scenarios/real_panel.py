"""
A real subscription, through the wizard, into the library.

Everything else in this suite runs against a five-line fixture. This one runs against
the panel a person actually pays for, which is the only place several of this project's
open questions live:

  * 22,305 live channels, 122,418 films and 28,715 series is 78 MB of JSON. Nothing
    smaller has ever exercised the deserializers, the batch inserts or the memory the
    import holds while it works.
  * "Live TV says No channels on a real panel" has been open since the first bug
    report and has never been reproducible from a fixture.
  * A real panel answers in shapes a fixture does not: numbers as strings, nulls where
    an integer is documented, categories that no stream references.

Skipped unless the credentials are in the environment, because they belong to a person
and not to this repository:

    AURORA_PANEL_URL=https://…  AURORA_PANEL_USER=…  AURORA_PANEL_PASS=…

They are typed into the window and never written down: no file, no argument, no log
line. What ends up in `target/release/data` is the app's own storage, which is
gitignored, and the assertions below read counts from it rather than anything
identifying.
"""
import os
import time

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")

# A real import of this size is minutes, not seconds.
IMPORT_TIMEOUT = 900


def run(d, ctx):
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")

    time.sleep(6)

    d.send(d.find("textarea"), URL)
    time.sleep(1)
    # Xtream, not M3U: a panel login is the case the fixture cannot cover.
    d.click(d.by_text("button", "Xtream / panel login"))
    time.sleep(0.5)

    name = d.by_label("Provider name")
    d.clear(name)
    d.send(name, "Real Panel")
    d.send(d.by_label("Username"), USER)
    d.send(d.by_label("Password"), PASS)
    # Before the credentials are on screen in any readable form. The password field is
    # `type="password"`, but the username is not, so this is the last safe frame.
    d.shot(ctx.shot("filled"))

    d.click(d.by_text("button", "Check connection"))
    ok = d.wait_body(
        lambda b: "days left" in b.lower()
        or "connections" in b.lower()
        or "could not" in b.lower()
        or "unreadable" in b.lower(),
        timeout=120,
    )
    ctx.assert_no_panic()
    assert ok, f"the connection check never finished; screen said {d.body()[:400]!r}"
    d.shot(ctx.shot("checked"))

    d.click(d.by_text("button", "Continue"))
    d.click(d.by_text("button", "Import library", timeout=30))

    done = d.wait_body(
        lambda b: "your library is ready" in b.lower(), timeout=IMPORT_TIMEOUT
    )
    ctx.assert_no_panic()
    assert done, f"the import never finished; screen said {d.body()[:400]!r}"
    d.shot(ctx.shot("imported"))

    counts = {t: ctx.count(t) for t in ("channels", "movies", "series", "episodes")}
    print(f"   imported: {counts}")
    assert counts["channels"] > 0, (
        "the panel serves live streams and the library holds none — this is the "
        f"'Live TV says No channels' report, reproduced. Got {counts}"
    )

    # Now the screen, which is the half that has been wrong before: a library full of
    # channels that the UI does not show is exactly bug #13.
    d.click(d.by_text("button", "Start watching"))
    time.sleep(4)
    d.shot(ctx.shot("home"))
    ctx.assert_no_panic()

    # Every page that reads the library, because "it imported" and "it is on screen"
    # have come apart before and the report was always about a particular page.
    for page, label in (("Live TV", "live"), ("Movies", "movies"), ("Series", "series")):
        d.click(d.by_text("nav a", page, timeout=30))
        # A page of twenty thousand rows is not instant, and an empty screen caught
        # mid-render reads exactly like an empty library — so wait for the page's own
        # heading before reading anything off it.
        settled = d.wait_body(lambda b, p=page: p in b, timeout=60)
        time.sleep(4)
        d.shot(ctx.shot(label))
        ctx.assert_no_panic()
        body = d.body()
        empty = [
            phrase
            for phrase in ("No channels", "Nothing here", "No movies", "No series")
            if phrase in body
        ]
        assert not empty, (
            f"{page} says {empty[0]!r} while the library holds "
            f"{counts} — this is bug #13's shape, on {page}"
        )
        print(f"   {page}: drew, settled={settled}")

        if page == "Live TV":
            _logos_render(d, ctx)

    _records_a_real_stream(d, ctx)


def _logos_render(d, ctx):
    """
    Do the channel logos actually appear?

    This cannot be answered anywhere but here. A browser serves no Content-Security-Policy,
    so every logo loads during development no matter what the policy says; F-12 widened
    `img-src` to allow plain `http:` precisely because panels serve their artwork over it,
    and whether that worked has been unconfirmed since. A blocked image is not a broken
    one either — it is an `<img>` that simply never decodes, so the only honest test is to
    ask the images themselves how wide they came out.

    `naturalWidth` is 0 for an image that failed, was blocked, or has not finished
    loading; the wait above has already let the page settle. Reported rather than
    asserted when a panel publishes no artwork at all, because that is the provider's
    choice and not a defect in this app.
    """
    stats = d.js(
        """
        const imgs = Array.from(document.images)
          .filter((i) => /^https?:/i.test(i.currentSrc || i.src));
        return {
          total: imgs.length,
          decoded: imgs.filter((i) => i.naturalWidth > 0).length,
          http: imgs.filter((i) => /^http:/i.test(i.currentSrc || i.src)).length,
          httpDecoded: imgs.filter(
            (i) => /^http:/i.test(i.currentSrc || i.src) && i.naturalWidth > 0,
          ).length,
          sample: imgs.slice(0, 3).map((i) => (i.currentSrc || i.src).slice(0, 60)),
        };
        """
    )
    ctx.assert_no_panic()
    print(
        f"   logos: {stats['decoded']}/{stats['total']} decoded"
        f" ({stats['httpDecoded']}/{stats['http']} of them over plain http)"
    )
    if not stats["total"]:
        print("   logos: this panel publishes none, so there is nothing to check")
        return
    assert stats["decoded"] > 0, (
        f"every channel logo failed to load — {stats['total']} images, none decoded. "
        f"If these are http: URLs this is F-12's img-src rule rejecting them, which is "
        f"invisible from a browser. Sample: {stats['sample']}"
    )
    if stats["http"]:
        assert stats["httpDecoded"] > 0, (
            f"{stats['http']} logos are served over plain http and none of them "
            f"decoded, while https ones did — that is the CSP, not the network. "
            f"Sample: {stats['sample']}"
        )


def _records_a_real_stream(d, ctx):
    """
    Record a real channel, briefly, and look at what landed on disk.

    The recorder has only ever met the test server, which serves a handful of canned
    bytes over localhost and answers instantly. A provider is none of those things: it
    redirects, it hands back a playlist of segments, it can refuse a second connection,
    and it can accept the request and then send nothing at all. A recording that writes
    an empty file is indistinguishable from a working one everywhere except here.

    Scheduled through `dvr_schedule` rather than the guide, because the guide needs EPG
    for the channel and the point is the recorder, not the listing. The scheduler ticks
    every ten seconds, so the window is wide enough to survive landing between two of
    them at each end.

    Up to three channels, spread across the list, because a dead channel is the
    provider's problem and not this app's — but all three failing is worth knowing, and
    the reasons are printed rather than swallowed.
    """
    picks = ctx.rows(
        "SELECT id, name FROM channels WHERE hidden = 0"
        " ORDER BY id LIMIT 3 OFFSET 20"
    )
    assert picks, "no channels in the library to record from"

    window = 40
    failures = []
    for cid, cname in picks:
        start = int(time.time())
        rid = d.invoke(
            "dvr_schedule",
            {
                "channelId": cid,
                "title": "Harness recording",
                "airStart": start,
                "airStop": start + window,
                "prePaddingSecs": 0,
                "postPaddingSecs": 0,
            },
        )
        if not rid:
            failures.append(f"{cname}: the scheduler refused it")
            continue

        # Two ticks past the end, so a reap that lands just after the window still
        # counts. Polling the state rather than sleeping the whole time: a failure
        # settles early and there is no reason to wait for it.
        rec = None
        deadline = time.time() + window + 45
        while time.time() < deadline:
            listed = [r for r in d.invoke("dvr_list", {}) if r["id"] == rid]
            if listed:
                rec = listed[0]
                if rec["state"] in ("completed", "failed", "skipped"):
                    break
            time.sleep(3)
        ctx.assert_no_panic()

        if not rec:
            failures.append(f"{cname}: never appeared in dvr_list")
            continue
        if rec["state"] != "completed":
            failures.append(f"{cname}: {rec['state']} — {rec.get('reason')}")
            continue

        path, size = rec.get("filePath"), rec.get("bytes") or 0
        assert path, f"{cname} completed but the recording has no file path"
        assert os.path.exists(path), f"{cname} recorded to {path}, which is not there"
        on_disk = os.path.getsize(path)
        # A file that exists and is empty is the failure this whole check is for: the
        # recorder opened the stream, wrote nothing, and reported success.
        assert on_disk > 64 * 1024, (
            f"{cname} recorded {on_disk} bytes in {window}s — the stream was opened and "
            f"produced nothing worth keeping"
        )
        print(
            f"   recording: {cname} -> {on_disk / 1024 / 1024:.1f} MB in {window}s"
            f" (host reported {size} bytes), {os.path.basename(path)}"
        )
        d.click(d.by_text("nav a", "Recordings", timeout=30))
        time.sleep(3)
        d.shot(ctx.shot("recorded"))
        ctx.assert_no_panic()
        return

    raise AssertionError(
        "no channel could be recorded. This is the recorder meeting a real provider "
        "for the first time, so the reasons matter: " + "; ".join(failures)
    )


class Skipped(Exception):
    """Raised when this scenario has nothing to run against."""
