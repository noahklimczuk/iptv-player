"""
Every screen, photographed, against a real library.

Not an assertion suite — the other scenarios do that. This exists so there is a set
of pictures of the actual app on an actual subscription, taken the same way every
time, that somebody can look through after a change. A hundred and fourteen green
browser journeys did not stop the genre filter rendering as an empty box on every
library without a TMDB key, because nothing in them ever looked.

It still fails on a panic, and it still fails if a screen does not draw at all: a
tour that photographs a blank page and calls it done is worse than no tour.
"""
import os
import time

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")

# Each screen, and a word that has to be on it before the shutter opens.
SCREENS = [
    ("Home", "Continue Watching"),
    ("Live TV", "Live TV"),
    ("Guide", "Guide"),
    ("Movies", "Movies"),
    ("Series", "Series"),
    ("Recordings", "Recordings"),
    ("Playlist", "Playlist"),
    ("Settings", "Settings"),
]


class Skipped(Exception):
    pass


def run(d, ctx):
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")

    time.sleep(6)
    d.shot(ctx.shot("00-wizard"))

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
    d.shot(ctx.shot("01-imported"))
    d.click(d.by_text("button", "Start watching"))
    time.sleep(4)

    # Something in Continue Watching, so Home has all its rails.
    profile_id = d.invoke("profiles_list")[0]["id"]
    for (film_id,) in ctx.rows(
        "SELECT id FROM movies WHERE hidden = 0 AND group_title IS NOT NULL"
        " ORDER BY id LIMIT 3"
    ):
        d.invoke("progress_save", {
            "profileId": profile_id, "kind": "movie", "id": film_id,
            "positionSecs": 5400, "durationSecs": 6000,
        })

    # Somewhere else first, so that arriving at Home remounts it. Home fetches its
    # rails once and keeps them, and the progress above was written after it did —
    # without this the tour photographs the rails of an empty library.
    d.click(d.by_text("nav a", "Settings", timeout=30))
    time.sleep(3)

    for index, (screen, expect) in enumerate(SCREENS, start=2):
        d.click(d.by_text("nav a", screen, timeout=30))
        drew = d.wait_body(lambda b, e=expect: e in b, timeout=60)
        # A grid of a hundred posters is a lot of network; let it settle.
        time.sleep(5)
        ctx.assert_no_panic()
        d.shot(ctx.shot(f"{index:02d}-{screen.lower().replace(' ', '-')}"))
        assert drew, f"{screen} never drew; the screen says {d.body()[:200]!r}"
        print(f"   {screen}")
