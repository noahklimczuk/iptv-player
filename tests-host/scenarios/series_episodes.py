"""
Opening a series on a real panel fetches its episodes.

"Series are loading but there are no episodes" was the report. An import writes the
series row and not its listing — one request per show would be 28,715 of them before
the library was usable — so the episodes are fetched the first time somebody opens the
show. That was the fix; this is the first thing to check it actually happens, because
the mock has always answered `library.episodes` out of its own memory and the Rust
tests call the Xtream client directly.

Deliberately checks the database as well as the screen. "3 seasons" rendered from a
list the host returned but never stored would look right and still leave the show
empty the next time it was opened.
"""
import os
import time

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")


class Skipped(Exception):
    pass


def run(d, ctx):
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
        lambda b: "days left" in b.lower() or "connections" in b.lower(), timeout=120
    ), "the connection check never finished"
    d.click(d.by_text("button", "Continue"))
    d.click(d.by_text("button", "Import library", timeout=30))
    assert d.wait_body(
        lambda b: "your library is ready" in b.lower(), timeout=900
    ), "the import never finished"
    d.click(d.by_text("button", "Start watching"))
    time.sleep(4)

    shows = ctx.count("series")
    assert shows > 0, "the panel serves series and the library holds none"
    # Lazily, by design — so this is the state the fix is about, not a failure.
    assert ctx.count("episodes") == 0, (
        "episodes arrived at import time; this scenario is about the lazy fetch and "
        "needs rewriting if that changed"
    )

    d.click(d.by_text("nav a", "Series", timeout=30))
    assert d.wait_body(lambda b: "Series" in b, timeout=60)
    time.sleep(4)

    d.click(d.find('[data-testid="catalog-card"]', timeout=30))
    # The listing is a round trip to the panel, so give it room. "S01E" is the episode
    # rows themselves rather than any summary above them.
    arrived = d.wait_body(lambda b: "S01E" in b or "S02E" in b, timeout=90)
    # Let the header catch up before the screenshot: it says "Loading episodes…" until
    # the listing is in hand, and a picture of that is not what this proves.
    d.wait_body(lambda b: "Loading episodes" not in b, timeout=30)
    time.sleep(1)
    ctx.assert_no_panic()
    d.shot(ctx.shot("series-detail"))
    assert arrived, (
        "opening the show never produced an episode. This is 'series load but there "
        f"are no episodes'. The screen says {d.body()[:400]!r}"
    )

    # And the count above them has to agree with them. It used to read `item.seasons`,
    # which is written at import time before any episode exists — so a show sat there
    # saying "0 seasons" with its episodes listed directly underneath.
    body = d.body()
    assert "0 seasons" not in body, (
        "the header says '0 seasons' over a list of episodes"
    )

    stored = ctx.count("episodes")
    assert stored > 0, (
        f"the show drew its seasons but the library stored no episodes ({stored}) — "
        "they would be fetched again every time, and nothing offline would work"
    )
    print(f"   {shows} shows, {stored} episodes after opening one")
