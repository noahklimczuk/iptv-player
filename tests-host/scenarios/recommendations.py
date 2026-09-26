"""
Recommendations, on a real library, from real viewing.

The algorithm has twenty unit tests and never touches a database; this is the other
half — that the queries feeding it return what it expects on a library of 146,000 rows,
that ranking one is fast enough to sit on the home screen, and that what comes back is
recognisably about what was watched.

Watch history is seeded directly through the host's own `progress.save`, because the
alternative is sitting through part of a film. Everything after that is the real
thing: the real taste profile, the real pool, the real rail.
"""
import json
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

    profile_id = d.invoke("profiles_list")[0]["id"]

    # Before any history: rating-led, and honest about it.
    cold = d.invoke("library_recommended", {"profileId": profile_id, "limit": 20})
    ctx.assert_no_panic()
    assert cold["items"], "a fresh library should still have something worth a look"
    assert cold["personalised"] is False, (
        f"nothing has been watched; this must not claim to be personal: {cold['title']!r}"
    )
    assert cold["title"] == "Worth a look", cold["title"]

    # Watch three films off the same shelf, so there is a taste to find.
    #
    # Read straight from the library rather than through the IPC, because the shelf —
    # the provider's own category — is exactly what the UI does not expose and the
    # recommender leans on. Without a TMDB key nothing here has a genre at all, which
    # is the state a real first evening is in.
    shelf, shelf_size = ctx.rows(
        "SELECT group_title, count(*) c FROM movies"
        " WHERE hidden = 0 AND group_title IS NOT NULL AND group_title != ''"
        " GROUP BY group_title ORDER BY c DESC LIMIT 1"
    )[0]
    assert shelf_size >= 6, f"need a shelf with a few films; biggest was {shelf} ({shelf_size})"
    watched = ctx.rows(
        "SELECT id, title FROM movies WHERE hidden = 0 AND group_title = ?"
        " ORDER BY COALESCE(rating, 0) DESC, id LIMIT 3",
        (shelf,),
    )
    print(f"   shelf {shelf!r} holds {shelf_size}; watching {len(watched)} from it")

    for film_id, _ in watched:
        d.invoke("progress_save", {
            "profileId": profile_id, "kind": "movie", "id": film_id,
            "positionSecs": 5400, "durationSecs": 6000,
        })
    ctx.assert_no_panic()

    started = time.time()
    warm = d.invoke("library_recommended", {"profileId": profile_id, "limit": 20})
    elapsed = time.time() - started
    ctx.assert_no_panic()

    assert warm["personalised"] is True, (
        f"three films watched and it still says it knows nothing: {warm}"
    )
    assert warm["items"], "a viewer with history should get recommendations"
    assert warm["title"] != "Worth a look", (
        f"the heading should name their taste now: {warm['title']!r}"
    )

    # Nothing they have already watched.
    watched_ids = {f"movie:{i}" for i, _ in watched}
    returned = {f"{i['kind']}:{i['id']}" for i in warm["items"]}
    assert not (watched_ids & returned), (
        f"recommended something already watched: {watched_ids & returned}"
    )

    # Every item says why it is there, and at least some of them name the films that
    # were watched — which is the claim the whole rail rests on.
    assert len(warm["reasons"]) == len(warm["items"]), (
        f"{len(warm['items'])} items but {len(warm['reasons'])} reasons"
    )
    because = [r for r in warm["reasons"].values() if r.startswith("Because you watched")]
    assert because, f"no recommendation could name its source: {warm['reasons']}"
    named = {r[len("Because you watched "):] for r in because}
    titles = {t for _, t in watched}
    assert named <= titles, (
        f"named a film that was never watched: {named - titles}"
    )

    # The taste that was fed in should be visible in what comes back. Checked against
    # the library rather than the payload, for the same reason as above.
    ids = [i["id"] for i in warm["items"] if i["kind"] == "movie"]
    placeholders = ",".join("?" * len(ids))
    shelves = [
        row[0]
        for row in ctx.rows(f"SELECT group_title FROM movies WHERE id IN ({placeholders})", ids)
    ]
    matching = sum(1 for g in shelves if g == shelf)
    assert matching >= 3, (
        f"watched three films from {shelf!r} and only {matching} of {len(shelves)} "
        "came back from it"
    )
    # ...but not all of it, or the diversity penalty is not working.
    assert matching < len(shelves), (
        f"all {matching} recommendations are from {shelf!r}; the rail is one note"
    )

    # Fast enough to sit on the home screen of a 146,000-row library.
    assert elapsed < 5.0, f"ranking took {elapsed:.1f}s"

    # And the home screen really shows it.
    #
    # Away and back, because Home fetches its rails once: it was already open when the
    # history was seeded, and a screen that never remounts never re-asks. (Closing the
    # player bumps a catalog version for the same reason — see `closePlayer` — but
    # nothing here went through the player.)
    # Via Settings rather than Movies: both remount Home on the way back, and one of
    # them does not first draw a grid of a hundred and twenty remote posters. WebKit
    # under Xvfb does not always survive that.
    d.click(d.by_text("nav a", "Settings", timeout=30))
    time.sleep(3)
    d.click(d.by_text("nav a", "Home", timeout=30))
    time.sleep(8)
    body = d.body()
    assert warm["title"] in body, (
        f"the rail heading {warm['title']!r} is not on the home screen; it says "
        f"{body[:400]!r}"
    )

    # Bring the rail into view before photographing it — it sits below Continue
    # Watching, so the top of the page is not evidence about it either way.
    d.js(
        "const h = [...document.querySelectorAll('h2, h3, section')]"
        "  .find((e) => e.textContent.trim().startsWith(arguments[0]));"
        " if (h) h.scrollIntoView({block: 'center'});",
        warm["title"],
    )
    time.sleep(2)
    d.shot(ctx.shot("home"))
    # The reasons have to reach the posters, not just the payload.
    #
    # Read off the elements rather than out of `body()`: a rail scrolls sideways, and
    # WebDriver's text of an element leaves out what is scrolled out of view — so the
    # page text is not evidence either way about a row of twenty-four cards.
    # `textContent` rather than WebDriver's element text: a rail scrolls sideways and
    # sits below the fold, and element text leaves out whatever is not on screen — so
    # it says nothing about the twenty-four cards in the row.
    labels = d.js(
        "return [...document.querySelectorAll('[data-testid=\"card-reason\"]')]"
        "  .map((e) => e.textContent.trim());"
    )
    assert labels, "no card on the home screen says why it is there"
    assert any(r.startswith("Because you watched") for r in labels), (
        f"nothing named the film it came from; the cards say {sorted(set(labels))}"
    )
    print(f"   {len(labels)} cards carry a reason, e.g. {labels[0]!r}")

    print(f"   {matching}/{len(shelves)} from the watched shelf, {len(because)} named "
          f"a source, ranked in {elapsed:.2f}s, heading {warm['title']!r}")
