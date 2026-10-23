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
import sys
import time

import winprobe as wp

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

    # What the panel sends and the import used to parse and discard — see
    # docs/DECISIONS.md D26. Read straight out of the library rather than off the
    # screen, because these are the columns the recommender ranks on and only two of
    # them are ever drawn.
    #
    # The thresholds are deliberately far below the measured coverage (90% of films
    # rated, 96% of shows with a genre): this is here to catch the field being dropped
    # again, not to pin the assertion to one subscription's exact numbers.
    rated, dates, genred, plotted = ctx.rows(
        """SELECT (SELECT count(*) FROM movies WHERE rating IS NOT NULL),
                  (SELECT count(DISTINCT added_at) FROM movies),
                  (SELECT count(*) FROM series WHERE genres IS NOT NULL),
                  (SELECT count(*) FROM series WHERE overview IS NOT NULL)"""
    )[0]
    print(
        f"   kept: {rated} films rated, {dates} distinct added-dates, "
        f"{genred} shows with genres, {plotted} with a plot"
    )
    assert rated > counts["movies"] // 2, (
        f"the panel scores most of its films and only {rated} of {counts['movies']} "
        f"carry one — the import is dropping them again"
    )
    assert dates > 100, (
        f"{counts['movies']} films imported with only {dates} distinct added-dates; "
        f"'Recently added' is ordering by the time of the import"
    )
    assert genred > counts["series"] // 2, (
        f"only {genred} of {counts['series']} shows have genres. Genres are the "
        f"recommender's heaviest signal and this panel sends one for almost all of them"
    )

    _the_line_limit_is_stored_and_enforced(d, ctx)

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

    _the_bulk_hide_asks_first(d, ctx)
    _records_a_real_stream(d, ctx)
    # Last. It leaves its overlay up -- the screen learns a mosaic has closed from its own
    # click or the next `mosaic.state`, and neither has happened by the time the next step
    # would run -- so anything after it reads a page with a mosaic drawn over it. The two
    # also cannot overlap on a line that allows one connection.
    _a_mosaic_gets_the_main_surface_out_of_the_way(d, ctx)


def _the_line_limit_is_stored_and_enforced(d, ctx):
    """
    How many streams at once does the app think this line allows?

    The one check that could only ever have been made here. `max_connections` is on the
    authentication response, which the wizard displayed and then dropped: no production
    path wrote the column, so it was NULL on every real install and every budget built on
    it had nothing to enforce. The DVR fell back to two simultaneous recordings on a line
    that allows one, the warning strip written for exactly this subscription could not
    appear, and multi-view offered every layout. Three places asserted the refusal worked
    and none of them could observe it -- the Playwright journeys because `fixtures.ts`
    declares `maxConnections: 2`, and this file because it opens one tile. R-108.

    Read out of the library and asked of the host, rather than off a screen: the number is
    what the DVR and the mosaic budget consult, and neither draws it.
    """
    stored = ctx.rows("SELECT max_connections FROM providers WHERE enabled = 1")[0][0]
    print(f"   line limit: providers.max_connections = {stored!r}")
    assert stored is not None, (
        "the panel declared a connection limit on the authentication response and the "
        "column is still NULL -- nothing wrote it, which is R-108 exactly: the DVR, the "
        "warning strip and multi-view all have no limit to enforce"
    )
    assert stored >= 1, f"a stored limit of {stored} is not a limit"

    # The DVR's cap is the same number, asked per tick rather than once at startup --
    # which on a first run is before any provider exists, so the session that set the
    # subscription up ran on the default of two for its whole length.
    storage = d.invoke("dvr.storage")
    print(f"   dvr: maxConcurrent = {storage['maxConcurrent']}")
    assert storage["maxConcurrent"] == stored, (
        f"the line allows {stored} stream(s) and the DVR will run "
        f"{storage['maxConcurrent']} recordings at once"
    )

    # And the budget now has an opinion. `unknown` is the answer that let every layout
    # through; on a declared limit of one, four tiles must come back refused.
    check = d.invoke("mosaic.check", {"layout": "grid2x2"})
    print(
        f"   mosaic grid2x2: verdict={check['verdict']} needed={check.get('needed')} "
        f"limit={check.get('limit')} largestFitting={check.get('largestFitting')!r}"
    )
    assert check["verdict"] != "unknown", (
        "the provider declared a limit and the budget still says it cannot know -- "
        "`Budget::check` is reading a NULL column"
    )
    if stored < check["needed"]:
        assert check["verdict"] == "exceeds", (
            f"four tiles need {check['needed']} connections on a line that allows "
            f"{stored}, and the budget says {check['verdict']!r}"
        )
        assert check["over"] == check["needed"] - stored
    ctx.assert_no_panic()


def _a_mosaic_gets_the_main_surface_out_of_the_way(d, ctx):
    """
    Does a mosaic tile have anywhere to draw?

    The fourth placement in `surfaces`, and the one that needs a real subscription: a
    mosaic will not open without a channel, and the loopback fixture's URLs are stubs
    that mpv never finishes deciding about.

    What went wrong, and reached the viewer as "multi-view plays in the background but
    doesn't play video": every `place` sends its surface to `HWND_BOTTOM`, so each tile
    lands *behind* the main player's full-window surface. Stopping playback is not enough
    -- the window is still there, still full-window, still in front, and an mpv surface
    with nothing loaded is black.

    One filled tile, deliberately: this line allows a single connection, so one tile is
    the most that can ever open here.

    That the software *refuses* the other layouts is not this step's claim to make, and
    it used to be made here -- until `providers.max_connections` was written, `Budget`
    answered `Unknown` on this very subscription and a four-stream mosaic was opened
    rather than refused, which this scenario could not see because it opens one tile
    (R-108). `_the_line_limit_is_stored_and_enforced` asks that question directly.
    """
    if sys.platform != "win32":
        print("   mosaic: skipped, no mpv surface off Windows")
        return

    hwnd = wp.find("Aurora")
    assert hwnd, "no window titled Aurora on the desktop"
    # Visible STATIC children only, and there should be exactly one before a mosaic
    # exists. Taking the frontmost without filtering picked a hidden sibling and reported
    # "already hidden before any mosaic", which was the lookup being wrong rather than the
    # app. `video_surface` is the scenario that pins the z-order itself.
    visible = [
        c
        for c in wp.children_front_to_back(hwnd)
        if wp.class_name(c).lower() == "static" and wp.is_visible(c)
    ]
    assert len(visible) == 1, (
        "expected one visible video surface before any mosaic; found "
        f"{[wp.describe(c) for c in visible]}"
    )
    main = visible[0]

    # One channel, from the smallest group.
    #
    # Not `channels.list` with no argument: it has no limit, and this library holds
    # thousands -- marshalling all of them through WebDriver is minutes of nothing. Not
    # `channels.byNumber` either, which was the first attempt: this panel numbers nothing
    # in the first ten, and a channel number is a thing a provider may simply not send.
    # A group is bounded and its count comes back with it, so the smallest one is both
    # cheap and certain to contain something.
    groups = d.invoke("channels_groups", {})
    assert groups, "the panel imported channels into no groups at all"

    # The smallest group, but not a pay-per-view one.
    #
    # Smallest alone picked "PPV NETFLIX 01 [EVENT ONLY]", and a channel that only exists
    # during an event never streams -- which is a fine tile to *place* and a terrible one
    # to stop, because letting go of a stream that never started does not return. A
    # group whose name says it carries ordinary channels is both small enough to list and
    # likely to answer.
    def ordinary(name):
        lowered = name.lower()
        return not any(w in lowered for w in ("ppv", "event", "24/7", "vod", "adult"))

    candidates = sorted(
        (g for g in groups if ordinary(g["name"])), key=lambda g: g["count"]
    ) or sorted(groups, key=lambda g: g["count"])
    group = candidates[0]
    rows = d.invoke("channels_list", {"group": group["name"]})
    assert rows, f"the group {group['name']!r} says {group['count']} and returned none"
    channel = rows[0]
    print(f"   mosaic: using {channel['name']!r} from {group['name']!r}")

    # Fired and watched rather than waited for: `mosaic.open` does not return until mpv
    # has decided about the stream, and everything asserted here has happened by then --
    # the main surface is hidden before the first tile is built, and a tile's surface is
    # created and placed before its stream is loaded.
    d.js(
        "const [ids] = arguments;"
        " window.__TAURI_INTERNALS__"
        "   .invoke('mosaic_open', { args: { layout: 'grid2x2', channelIds: ids } })"
        "   .then((v) => { window.__mosaicOpened = v; })"
        "   .catch((e) => { window.__mosaicFailed = String(e); });"
        " return true;",
        [channel["id"], None, None, None],
    )

    # A tile surface appears, and it has to be *in front of* the main one.
    #
    # In front rather than instead of: the main surface is left exactly where it was and
    # the tile is ordered above it. Hiding it was the first fix and could not be undone
    # (F-36). `children_front_to_back` is front-first, so the tile must come before the
    # main surface in that list.
    ordered = False
    for _ in range(40):
        time.sleep(0.5)
        failed = d.js("return window.__mosaicFailed ?? null;")
        assert not failed, f"the host would not open a mosaic: {failed}"
        order = [
            c
            for c in wp.children_front_to_back(hwnd)
            if wp.class_name(c).lower() == "static" and wp.is_visible(c)
        ]
        if len(order) > 1 and order.index(main) > 0:
            ordered = True
            break
    assert ordered, (
        "no tile surface is in front of the main player's full-window surface, so it "
        "covers them -- which is what 'plays in the background but no video' was"
    )
    assert wp.is_visible(main), "the main surface was hidden; see F-36"
    print("   mosaic: a tile surface is in front of the main one")

    # A tile surface, in one of the quarters of a 2x2.
    cx, cy, cw, ch = wp.client_rect(hwnd)
    # Everything except the main surface, which is legitimately full-window: it is still
    # there, underneath, and that is the point of this arrangement.
    tiles = [
        c
        for c in wp.children_front_to_back(hwnd)
        if wp.class_name(c).lower() == "static" and wp.is_visible(c) and c != main
    ]
    rects = [wp.window_rect(t) for t in tiles]
    print(f"   mosaic: {len(tiles)} visible surface(s) at {[(r[0] - cx, r[1] - cy, r[2], r[3]) for r in rects]}")
    assert tiles, "a mosaic is opening and there is no visible surface anywhere"
    for left, top, width, height in rects:
        x, y = left - cx, top - cy
        assert width <= cw // 2 + 2 and height <= ch // 2 + 2, (
            f"a tile surface is {width}x{height}, which is not a quarter of {cw}x{ch}"
        )
        assert -2 <= x <= cw and -2 <= y <= ch, f"a tile surface is off the window at {x},{y}"

    # Deliberately no desktop capture here. `Image.save_png` encodes a 1440x900 frame a
    # pixel at a time in Python, and this step has already said everything a picture
    # would: the rectangles are printed above, and `video_surface` is where a capture is
    # the evidence rather than a decoration.

    # And given back, or the window is black with nothing over it.
    d.js(
        "window.__TAURI_INTERNALS__.invoke('mosaic_close', { args: {} })"
        "  .then(() => { window.__mosaicClosed = true; })"
        "  .catch(() => { window.__mosaicClosed = true; });"
        " return true;"
    )
    # Whether the close itself finishes is reported rather than asserted. It does, now
    # that nothing is hidden (F-36) -- but it depends on a provider letting go of a
    # stream, and failing here would hide the things above that do not.
    finished = False
    for _ in range(20):
        if d.js("return window.__mosaicClosed ?? false;"):
            finished = True
            break
        time.sleep(1)

    # What matters either way: the main surface is still there to go back to.
    assert wp.is_visible(main), "the main surface vanished while the mosaic was closing"

    # And the screen is told. The overlay is `position: fixed; inset: 0`, so one left
    # behind is not a cosmetic leftover -- it is a sheet over every page. It used to stay
    # for ever when a mosaic was closed by anything other than its own button, because the
    # player's heartbeat had nothing to say once the session was gone.
    gone = False
    for _ in range(40):
        time.sleep(0.5)
        if not d.find_all('[data-testid="mosaic-overlay"]'):
            gone = True
            break
    assert gone, (
        "the mosaic overlay is still on screen after the mosaic closed, so every page "
        "behind it is covered"
    )
    print(
        "   mosaic: the close "
        + ("finished" if finished else "has not returned")
        + ", the overlay is gone, and the main surface is intact"
    )


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
        # Through the route rather than the nav pill. The pill is hidden whenever the
        # shell thinks there is video behind it, and WebDriver reports no text at all for
        # a hidden element -- so clicking it by name is a test that depends on what the
        # player happens to be doing. The hash is what the link would set anyway, and
        # `HashRouter` does not reload the document.
        d.js("window.location.hash = '#/recordings'; return true;")
        assert d.wait_body(lambda b: "recordings" in b.lower(), timeout=30), (
            f"the recordings page never drew; screen said {d.body()[:200]!r}"
        )
        # The recording that was just made is on it, which is the thing a viewer came to
        # this page for.
        assert d.wait_body(lambda b: "harness recording" in b.lower(), timeout=30), (
            f"the recording is not listed; screen said {d.body()[:300]!r}"
        )
        d.shot(ctx.shot("recorded"))
        ctx.assert_no_panic()
        return

    raise AssertionError(
        "no channel could be recorded. This is the recorder meeting a real provider "
        "for the first time, so the reasons matter: " + "; ".join(failures)
    )


class Skipped(Exception):
    """Raised when this scenario has nothing to run against."""


def _the_bulk_hide_asks_first(d, ctx):
    """
    "Hide all N matching" is a confirmation, not a button.

    Only reachable here. The control appears when nothing is selected and the filter
    matches more rows than are on screen -- which is true the moment the Movies tab
    opens on a real library, and false on every fixture in this repository, because the
    mock carries 160 films and the first page is 300. So the one-click offer to hide a
    hundred thousand films could not be exercised anywhere but a real panel (R-125).

    Recoverable in principle -- `show: 'hidden'` lists them and `playlist.reset` takes
    ids -- but in pages of 300, which on this library is four hundred rounds of
    select-all-and-reset. Hence an ask.

    One press only. The second press is the behaviour that always worked; this asserts
    the first one no longer acts, so the step leaves the library as it found it.
    """
    d.click(d.by_text("nav a", "Playlist", timeout=30))
    assert d.wait_body(lambda b: "Playlist" in b, timeout=60), "the playlist never drew"
    d.click(d.by_text("[role=tab]", "Movies", timeout=30))
    time.sleep(3)

    hidden_before = ctx.rows("SELECT count(*) FROM movies WHERE hidden = 1")[0][0]

    offer = d.by_text("button", "Hide all", timeout=30)
    label = d.text(offer)
    print(f"   the offer reads {label!r}")
    d.click(offer)
    time.sleep(1)
    d.shot(ctx.shot("bulk-hide-asked"))
    ctx.assert_no_panic()

    body = d.body()
    assert "select-and-reset" in body, (
        "one press on the bulk hide produced no confirmation; the screen says "
        f"{body[:400]!r}"
    )
    assert d.find_all("[data-testid=playlist-hide-all-confirm]"), (
        "the confirmation has no button to confirm with"
    )

    hidden_after = ctx.rows("SELECT count(*) FROM movies WHERE hidden = 1")[0][0]
    assert hidden_after == hidden_before, (
        f"one press hid {hidden_after - hidden_before} films before anybody confirmed"
    )

    # And it can be backed out of, which is the other half of asking.
    d.click(d.by_text("button", "Cancel", timeout=15))
    time.sleep(1)
    assert d.by_text("button", "Hide all", timeout=15), (
        "cancelling the confirmation did not put the offer back"
    )
    print(f"   asked first, hid nothing, {hidden_before} films hidden before and after")
