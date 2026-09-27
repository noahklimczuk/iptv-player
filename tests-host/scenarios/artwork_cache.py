"""
Posters served from disk instead of from TMDB on every paint (checklist item 7).

`aurora-ingest::artwork` has downloaded, evicted and reported since it was written, and
nothing read from it: the UI rendered remote URLs, so a hundred-thousand-row library
fetched every poster from the network on each paint. Two things were missing — the
`assetProtocol` scope, and image components that prefer the local copy — and neither
could be verified without running the app.

There is a third thing neither of those fixes, which is why this scenario asserts on
`naturalWidth` rather than on the `src` attribute alone. `artwork::asset_url` builds
`http://asset.localhost/...` from Tauri's documentation, and its own comment said
"nothing has run the app to confirm the WebView serves it". An `<img>` whose `src` is an
asset URL and whose `naturalWidth` is 0 is a broken image that a screenshot would show
as a grey rectangle and a `src` check would call a pass.

Needs a real panel, because it needs a library with artwork:

    AURORA_PANEL_URL=https://…  AURORA_PANEL_USER=…  AURORA_PANEL_PASS=…

and a TMDB key compiled into the binary (`AURORA_TMDB_KEY` at build time), or enrichment
has nothing to fetch.
"""
import os
import sys
import time

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")

IMPORT_TIMEOUT = 900
# Enough posters to be sure, few enough to be quick. The point is not throughput.
ENRICH_BATCH = 40
# How long to let a screen warm itself before calling it broken.
WARM_TIMEOUT = 90


class Skipped(Exception):
    """Raised when this scenario has nothing to run against."""


def images(d):
    """Every image on the page, with whether it has loaded and whether it decoded.

    `complete` matters as much as `naturalWidth` here. `Poster` sets `loading="lazy"`, so
    a grid of a hundred cards only ever fetches the handful in the viewport — the rest
    have `naturalWidth === 0` because the browser has not asked for them yet, not because
    anything is wrong with them. Reading that as "broken" is how the first version of
    this scenario accused the asset protocol of failing on 76 images it had never
    requested.
    """
    return d.js(
        "return [...document.querySelectorAll('img')].map(i => ({"
        " src: i.currentSrc || i.src, w: i.naturalWidth, h: i.naturalHeight,"
        " done: i.complete }))"
    )


def run(d, ctx):
    if sys.platform != "win32":
        raise Skipped("the asset protocol is only interesting where the app ships")
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")

    time.sleep(6)

    d.send(d.find("textarea"), URL)
    time.sleep(1)
    d.click(d.by_text("button", "Xtream / panel login"))
    time.sleep(0.5)
    d.send(d.by_label("Username"), USER)
    d.send(d.by_label("Password"), PASS)
    d.click(d.by_text("button", "Check connection"))
    assert d.wait_body(
        lambda b: "days left" in b.lower() or "connections" in b.lower()
        or "could not" in b.lower(),
        timeout=120,
    ), "the connection check never finished"
    d.click(d.by_text("button", "Continue"))
    d.click(d.by_text("button", "Import library", timeout=30))
    assert d.wait_body(
        lambda b: "your library is ready" in b.lower(), timeout=IMPORT_TIMEOUT
    ), "the import never finished"
    d.click(d.by_text("button", "Start watching"))
    time.sleep(4)
    ctx.assert_no_panic()

    # The key has to be there, or there is nothing to cache.
    status = d.invoke("metadata_status") or {}
    print(f"   metadata: hasKey={status.get('hasKey')} "
          f"builtIn={status.get('keyIsBuiltIn')} movies={status.get('movies')}")
    assert status.get("hasKey"), (
        "no TMDB key, so enrichment fetches nothing and there is no artwork to cache. "
        "Build with AURORA_TMDB_KEY set, or set one in Settings."
    )

    # Enrichment, which is a separate question and worth answering while we are here:
    # TMDB had never been called with a real key from this application.
    report = d.invoke(
        "metadata_run", {"batch": ENRICH_BATCH, "movies": True, "series": True}
    ) or {}
    print(f"   enriched: {report}")
    ctx.assert_no_panic()

    # While the enrichment is fresh: the canonical title. A provider files a film under
    # a string with the language, the group and the quality folded in; TMDB knows what it
    # is called. Enrichment fetched that and threw it away until now.
    import sqlite3
    con = sqlite3.connect(f"file:///{ctx.db_path().replace(chr(92), '/')}?mode=ro", uri=True)
    stored = 0
    for table in ("movies", "series"):
        # Both, because which of the two a batch reaches is up to the planner: asking
        # only about films is how the first version of this check reported "no canonical
        # titles" while 81 shows had one.
        n = con.execute(
            f"SELECT count(*) FROM {table} WHERE tmdb_title IS NOT NULL").fetchone()[0]
        taglines = con.execute(
            f"SELECT count(*) FROM {table} WHERE tagline IS NOT NULL").fetchone()[0]
        stored += n
        print(f"   {table:7}: {n} carry a canonical title, {taglines} a tagline")
        for was, now in con.execute(
            f"SELECT title, tmdb_title FROM {table}"
            " WHERE tmdb_title IS NOT NULL AND tmdb_title <> title LIMIT 2"
        ):
            print(f"      {was[:52]!r} -> {now!r}")
        # Whatever it is called, a list has to paint the canonical one.
        mismatched = con.execute(
            f"SELECT count(*) FROM {table} WHERE tmdb_title IS NOT NULL"
            " AND custom_title IS NULL"
            " AND COALESCE(custom_title, tmdb_title, title) <> tmdb_title"
        ).fetchone()[0]
        assert mismatched == 0, (
            f"{mismatched} {table} rows have a canonical title that the lists would not show"
        )
    con.close()
    assert stored > 0, (
        f"enrichment reported {report} but stored no canonical name for anything"
    )

    # Now the cache itself. Deliberately *not* `artwork_prefetch`: it takes an unordered
    # LIMIT from a table of 117,587 rows, so it caches an arbitrary few dozen posters
    # that the screens a viewer opens are almost never among — measured at no overlap at
    # all. What the cache has to do is warm what is actually looked at.
    before = d.invoke("artwork_status") or {}
    print(f"   cache before: {before.get('files')} files in {before.get('folder')}")

    d.click(d.by_text("nav a", "Movies", timeout=30))
    assert d.wait_body(lambda b: "Movies" in b, timeout=60)
    time.sleep(5)
    first = images(d)
    local_first = [i for i in first if "asset.localhost" in (i["src"] or "")]
    print(f"   first look : {len(first)} images, {len(local_first)} from the cache "
          f"(a cold cache should be near zero)")
    d.shot(ctx.shot("first-visit"))

    # The screen warms itself in the background and the images swap as they land. Wait
    # for the *warm* to settle, not for the first cached image: a cache that already
    # holds something from an earlier screen would otherwise end this immediately, which
    # is exactly what the first run of this scenario did.
    deadline = time.time() + WARM_TIMEOUT
    settled_at = None
    while time.time() < deadline:
        time.sleep(4)
        files = (d.invoke("artwork_status") or {}).get("files", 0)
        if files == settled_at:
            break
        settled_at = files
    # One more beat for the re-ask the host's progress events trigger.
    time.sleep(3)
    shown = images(d)

    after = d.invoke("artwork_status") or {}
    ctx.assert_no_panic()
    d.shot(ctx.shot("warmed"))

    local = [i for i in shown if "asset.localhost" in (i["src"] or "")]
    remote = [i for i in shown if i["src"] and "asset.localhost" not in i["src"]]
    # Only images the browser actually went and fetched can be called broken.
    loaded = [i for i in local if i["done"]]
    broken = [i for i in loaded if not i["w"]]
    print(f"   cache after: {before.get('files')} -> {after.get('files')} files, "
          f"{after.get('usedBytes')} bytes")
    print(f"   images     : {len(shown)} total, {len(local)} from the cache "
          f"({len(loaded)} of them fetched so far — the rest are lazy and below the "
          f"fold), {len(remote)} from the network, {len(broken)} broken")
    if local:
        print(f"   example    : {local[0]['src'][:96]} ({local[0]['w']}x{local[0]['h']})")

    assert after.get("files", 0) > before.get("files", 0), (
        "opening a screen full of posters cached nothing, so the warm-on-view path is "
        f"not running: {before.get('files')} -> {after.get('files')}"
    )
    assert os.path.isdir(after["folder"]) and os.listdir(after["folder"]), (
        "the cache reports files but the folder is empty"
    )
    assert len(local) >= 10, (
        f"{after.get('files')} posters are cached but only {len(local)} of {len(shown)} "
        "images on the page are being served from disk — the screen is not picking up "
        f"what it warmed. Sample: {[i['src'][:70] for i in shown[:3]]}"
    )
    assert loaded, (
        "no cached image was ever fetched by the browser, so nothing here proves the "
        "asset protocol serves anything"
    )
    assert not broken, (
        f"{len(broken)} of {len(loaded)} fetched images have an asset URL and decoded to "
        "nothing — the WebView is "
        "not serving the asset protocol, so these are grey rectangles on screen. Is "
        "`assetProtocol.enable` set, and was the folder granted at startup? "
        f"Sample: {broken[0]['src'][:100]}"
    )
    # Some of the provider's own poster URLs are simply dead, which is not this
    # application's bug and must not be reported as one. What matters is that the
    # fallback path still renders: if *every* remote image failed, it would be broken.
    network = [i for i in remote if (i["src"] or "").startswith("http") and i["done"]]
    dead = [i for i in network if not i["w"]]
    if dead:
        print(f"   note       : {len(dead)} of {len(network)} provider images do not "
              f"load, e.g. {dead[0]['src'][:80]}")
    assert not network or len(dead) < len(network), (
        "not one image served from the network decoded; the remote fallback is broken"
    )
    print(f"   verdict    : {len(local)} posters served from disk, none broken")
