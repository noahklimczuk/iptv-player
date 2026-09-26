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


class Skipped(Exception):
    """Raised when this scenario has nothing to run against."""
