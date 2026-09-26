"""
Browse, on a library of 146,000 rows.

Everything here was found by opening the page against a real subscription rather than
a fixture:

  * the genre filter was an empty dropdown, because genres came from TMDB enrichment
    and a viewer may never set a key — while the panel had been filing everything
    under 202 named shelves the whole time. Since docs/DECISIONS.md D26 the panel's
    own genres are kept, which splits this in two: `get_series` sends a genre for
    27,488 of the 28,529 shows here, and `get_vod_streams` sends none at all, so
    Series has the filter and Movies still does not;
  * the count read "120+", which is the page size wearing a library's clothes, and
    turned into a number that climbed as you scrolled;
  * there was no way to narrow a hundred thousand rows at all.

So the assertions are about the things a real library makes visible: a count that
matches SQLite, shelves that exist, and filters that actually change what is fetched.
"""
import os
import time

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")


class Skipped(Exception):
    pass


def digits(text):
    return int("".join(c for c in text if c.isdigit()) or 0)


def wait_for_count(d, predicate, timeout=30):
    """The browse heading's number, once it satisfies `predicate`."""
    end = time.time() + timeout
    last = None
    while time.time() < end:
        last = digits(d.text(d.find('[data-testid="browse-count"]')))
        if predicate(last):
            return last
        time.sleep(0.4)
    return last


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
    time.sleep(3)

    d.click(d.by_text("nav a", "Movies", timeout=30))
    assert d.wait_body(lambda b: "Movies" in b, timeout=60)
    time.sleep(5)
    d.shot(ctx.shot("movies"))
    ctx.assert_no_panic()

    # The count is the library, not the page. Checked against SQLite, because the
    # screen agreeing with itself is what "120+" was.
    stored = ctx.count("movies")
    shown = digits(d.text(d.find('[data-testid="browse-count"]')))
    assert shown == stored, (
        f"the heading says {shown:,} and the library holds {stored:,}"
    )
    assert shown > 120, "this library is bigger than one page; the count must say so"

    # The shelves the panel actually publishes.
    chips = d.find_all('[data-testid="browse-category"]')
    assert len(chips) > 1, "no categories offered on a library with 202 of them"
    labels = d.js(
        'return [...document.querySelectorAll("[data-testid=browse-category]")]'
        '  .map((e) => e.textContent.trim());'
    )
    assert labels[0] == "All", f"the first chip should clear the filter: {labels[:3]}"
    print(f"   {stored:,} films, shelves offered: {labels[1:4]}")

    # And they narrow it. The second chip is the biggest shelf.
    biggest = ctx.rows(
        "SELECT group_title, count(*) c FROM movies"
        " WHERE hidden = 0 AND group_title IS NOT NULL AND group_title != ''"
        " GROUP BY group_title ORDER BY c DESC, group_title LIMIT 1"
    )[0]
    d.click(chips[1])
    # Poll the count itself rather than the page text: the heading is one number among
    # a hundred thousand posters, and `body()` is not a reliable place to find it.
    narrowed = wait_for_count(d, lambda n: n != stored, timeout=30)
    ctx.assert_no_panic()
    assert narrowed == biggest[1], (
        f"filtering by {biggest[0]!r} should leave {biggest[1]:,}; the heading says "
        f"{narrowed:,}"
    )
    d.shot(ctx.shot("filtered"))

    # Searching narrows within that, and only fetches a page of it.
    title = ctx.rows(
        "SELECT title FROM movies WHERE hidden = 0 AND group_title = ? LIMIT 1",
        (biggest[0],),
    )[0][0]
    needle = title.split(" ")[-1] if len(title.split(" ")[-1]) > 3 else title[:6]
    d.send(d.find('[data-testid="browse-search"]'), needle)
    searched = wait_for_count(d, lambda n: n != narrowed, timeout=30)
    ctx.assert_no_panic()
    assert 0 < searched <= narrowed, (
        f"searching {needle!r} within {biggest[0]!r} gave {searched:,} of {narrowed:,}"
    )
    d.shot(ctx.shot("searched"))
    print(f"   {biggest[0]!r} narrowed to {narrowed:,}, {needle!r} to {searched:,}")

    # No film on this panel has a genre — `get_vod_streams` does not send one and
    # nothing here has been enriched — so the control must stay off this screen.
    assert not d.find_all('[data-testid="select-genre"]'), (
        "Movies offers a genre filter, but no film in this library has a genre"
    )

    # The category picker holds the rest of the 202 shelves and is searchable, because
    # two hundred options is not a list anybody scrolls.
    d.click(d.find('[data-testid="select-category"]'))
    rows = d.find_all('[data-testid="select-row"]')
    assert len(rows) > 20, f"the picker offers only {len(rows)} of 202 shelves"
    d.send(d.by_label("Filter category"), "ALBANIA")
    time.sleep(1)
    narrowed_rows = d.js(
        'return [...document.querySelectorAll("[data-testid=select-row]")]'
        '  .map((e) => e.textContent.trim());'
    )
    assert len(narrowed_rows) < len(rows), (
        f"typing did not narrow the picker: {len(narrowed_rows)} of {len(rows)}"
    )
    assert any("ALBANIA" in r for r in narrowed_rows), narrowed_rows
    d.shot(ctx.shot("picker"))
    print(f"   picker: {len(rows)} shelves, 'ALBANIA' narrows to {len(narrowed_rows)}")

    # Series, which does have genres — and must offer its own.
    #
    # This is the bug that escaped into the run before this one. `library::genres`
    # unioned both tables while `categories` was scoped by kind, which did not matter
    # while nothing had genres at all; the moment shows had them, Movies grew a
    # dropdown of 28,529 shows' genres and filtering a film by one matched nothing.
    # So: the filter is here, its contents belong to this half of the library, and
    # choosing one actually narrows the count.
    d.click(d.by_text("nav a", "Series", timeout=30))
    assert d.wait_body(lambda b: "Series" in b, timeout=60)
    time.sleep(5)
    picker = d.find_all('[data-testid="select-genre"]')
    assert picker, (
        "Series has a genre for almost every show on this panel and offers no filter"
    )

    shows = ctx.count("series")
    d.click(picker[0])
    time.sleep(1)
    # The first row is the placeholder that clears the filter; the genres follow it.
    genres = d.js(
        'return [...document.querySelectorAll("[data-testid=select-row]")]'
        '  .map((e) => e.textContent.trim());'
    )
    assert len(genres) > 6, f"only {len(genres) - 1} genres across {shows:,} shows: {genres}"
    d.click(d.find_all('[data-testid="select-row"]')[1])
    filtered = wait_for_count(d, lambda n: 0 < n < shows, timeout=30)
    assert 0 < filtered < shows, (
        f"filtering by {genres[1]!r} left {filtered:,} of {shows:,} shows — a genre "
        f"that matches nothing is what a list taken from the other table looks like"
    )
    d.shot(ctx.shot("series-genre"))
    print(
        f"   series: {len(genres) - 1} genres, {genres[1]!r} narrows "
        f"{shows:,} to {filtered:,}"
    )
