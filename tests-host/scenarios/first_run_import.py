"""
The first-run wizard, against a real playlist, all the way into the library.

This is the path every complaint in this project has been about: paste a URL, check it,
import, and see whether anything arrives. It had never been run end to end — the Rust
tests drive `sync::run` directly, and the browser journeys drive a mock that answers
instantly and always succeeds.

The assertions at the end are deliberately about SQLite rather than about the screen.
"Your library is ready — 3 channels" is a claim about React; `SELECT count(*) FROM
channels` is a claim about the thing that kept being broken.
"""
import time

PLAYLIST = "http://127.0.0.1:8099/playlist.m3u"

# What `tests-host/fixtures/playlist.m3u` carries: three live channels, one film, and
# one series with one episode. The wizard sorts them by URL path and group, so these
# also check that a `/movie/` entry does not land in `channels`.
EXPECT = {"channels": 3, "movies": 1, "series": 1, "episodes": 1}


def run(d, ctx):
    time.sleep(6)

    d.send(d.find("textarea"), PLAYLIST)
    time.sleep(1)
    # The wizard fills the name in from the URL's host, so this appends to it. What the
    # provider ends up called is not what this scenario is about.
    d.send(d.find("input"), "Test Provider")

    # The wizard gates Continue on a successful check, so this is not optional.
    #
    # Waiting for "Checking…" to *disappear* would pass before the click had even
    # rendered — the word is not on screen to begin with. So wait for the outcome the
    # check writes instead, which is the one state that means it finished.
    d.click(d.by_text("button", "Check connection"))
    ok = d.wait_body(
        lambda b: "entries" in b.lower() or "could not" in b.lower(), timeout=45
    )
    ctx.assert_no_panic()
    assert ok, f"the connection check never finished; screen said {d.body()[:300]!r}"
    d.shot(ctx.shot("checked"))
    body = d.body()
    assert "Found 5 entries" in body, f"the playlist has 5 entries; the check said {body[:300]!r}"

    d.click(d.by_text("button", "Continue"))
    d.by_text("button", "Import library")
    d.shot(ctx.shot("content"))

    d.click(d.by_text("button", "Import library"))
    done = d.wait_body(lambda b: "your library is ready" in b.lower(), timeout=120)
    ctx.assert_no_panic()
    assert done, f"the import never finished; screen said {d.body()[:300]!r}"
    d.shot(ctx.shot("imported"))

    # The question the whole project has been asking: did anything arrive, and did it
    # arrive in the right table?
    got = {table: ctx.count(table) for table in EXPECT}
    assert got == EXPECT, f"the library holds {got}, and the playlist listed {EXPECT}"
