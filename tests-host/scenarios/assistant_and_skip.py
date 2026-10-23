"""
The two bugs a viewer reported, on the real thing.

Both were fixed against the mock and the unit tests, and the mock is exactly what could
not see either of them in the first place — which is the same hole that let an earlier
"Skip Intro" fix ship without helping a real library. So both are checked here, against
the panel, the real database and a real catalogue.

  * **The assistant deadlocked on its own first command.** `assistant_status` took the
    database lock and then asked `gemini::key_available`, which takes it again;
    `parking_lot::Mutex` is not reentrant, so the second lock blocked forever with the
    first guard still held. `AssistantPage` issues that command on mount, so opening the
    assistant hung — and the database stayed locked for everything after it. A unit test
    cannot assert this: it would hang rather than fail. Here it has a timeout, and the
    step afterwards proves the lock was actually released rather than merely not held
    long enough to notice.

  * **Skip Intro had no way to come into being.** Intro markers are evidence-only, the
    only seed was a chapter list a provider's VOD stream does not carry, and the only way
    a viewer could supply evidence was the button that evidence puts on screen. The
    journeys prove the UI path against fixtures that *have* chapters; what they cannot
    prove is the half that matters here — that a real episode of a real series, with no
    chapters anywhere, gains an intro from a drag and passes it to its siblings through
    the real learning query.

One import serves both, because a real one is a quarter of an hour.

Skipped unless the panel credentials are in the environment. The Gemini half is skipped
on its own if no key is available, so the Skip Intro half still runs without one.
"""
import os
import time

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")


class Skipped(Exception):
    pass


def _import_the_library(d):
    """The wizard, as `recommendations.py` drives it."""
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


def _an_episode_without_chapters(d):
    """
    A real series episode, with the episode list its siblings come from.

    Walks series until one answers with at least two episodes: a show with one episode
    cannot demonstrate the learning half, and a real catalogue has plenty of both.
    """
    rows = d.invoke(
        "library_series", {"sort": "title", "limit": 60, "offset": 0}
    )
    assert rows, "the real library returned no series at all"
    for row in rows:
        eps = d.invoke("library_episodes", {"seriesId": row["id"]})
        if len(eps) >= 2:
            return row, eps
    raise AssertionError("no series on this line has two episodes to learn across")


def run(d, ctx):
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")

    _import_the_library(d)
    profile_id = d.invoke("profiles_list")[0]["id"]

    # ---- The assistant -------------------------------------------------------------
    #
    # This single call is the whole deadlock. Before the fix it never returned, so the
    # failure here is a timeout rather than a wrong answer.
    started = time.time()
    status = d.invoke("assistant_status", {"profileId": profile_id})
    took = time.time() - started
    ctx.assert_no_panic()
    assert took < 20, f"assistant_status took {took:.1f}s, which is the deadlock"
    print(f"   assistant_status answered in {took:.2f}s, model={status['model']}")

    # And the lock really was released, rather than held for less time than the call
    # above would notice. Anything db-backed will do; this one is cheap and unrelated.
    d.invoke("profiles_list")
    aids_after_status = d.invoke(
        "library_playback_aids",
        {"profileId": profile_id, "episodeId": 0, "durationSecs": 0},
    )
    ctx.assert_no_panic()
    assert "markers" in aids_after_status, "the database was still locked afterwards"
    print("   the database still answers after it, so no guard was left held")

    if not status["hasKey"]:
        print("   no Gemini key available, so the conversation itself is not exercised")
    else:
        reply = d.invoke(
            "assistant_send",
            {"profileId": profile_id, "text": "Name one film in my library and why."},
        )
        ctx.assert_no_panic()
        text = (reply.get("message") or {}).get("text") or ""
        assert text.strip(), f"the model replied with nothing: {reply!r}"
        print(f"   the assistant answered {len(text)} characters")
        # And it was remembered, which is the other half of the screen working.
        history = d.invoke("assistant_history", {"profileId": profile_id})
        assert len(history) >= 2, f"the exchange was not kept: {len(history)} messages"
        print(f"   {len(history)} messages kept")

    # ---- Skip Intro ----------------------------------------------------------------
    series, episodes = _an_episode_without_chapters(d)
    first, sibling = episodes[0]["id"], episodes[1]["id"]
    print(f"   learning across {series['title']!r} ({len(episodes)} episodes)")

    def intro_of(episode_id):
        aids = d.invoke(
            "library_playback_aids",
            {"profileId": profile_id, "episodeId": episode_id, "durationSecs": 0},
        )
        return next((m for m in aids["markers"] if m["kind"] == "intro"), None)

    # Nothing has said where this show's intro is. A provider stream carries no
    # chapters, and the clock refuses to guess an intro, so there is nothing.
    assert intro_of(first) is None, "this episode already had an intro marker"
    print("   no intro marker to begin with, as expected on a chapterless stream")

    # A nudge teaches nothing: the overlay's own buttons move ten seconds.
    assert d.invoke(
        "library_observe_seek",
        {"episodeId": first, "fromSecs": 30.0, "toSecs": 40.0, "durationSecs": 0.0},
    ) is False, "a ten-second nudge was taken as evidence"

    # A drag past the titles does.
    took_it = d.invoke(
        "library_observe_seek",
        {"episodeId": first, "fromSecs": 12.0, "toSecs": 102.0, "durationSecs": 0.0},
    )
    ctx.assert_no_panic()
    assert took_it is True, "the drag past the titles was not taken as evidence"

    marker = intro_of(first)
    assert marker is not None, "the drag was accepted and produced no marker"
    assert marker["startSecs"] == 12.0 and marker["endSecs"] == 102.0, marker
    print(f"   the drag became an intro marker: {marker['startSecs']}-{marker['endSecs']}s")

    # The point of tier 3, on real rows: the next episode gets it without being asked.
    inherited = intro_of(sibling)
    assert inherited is not None, "a sibling episode learned nothing from it"
    print(
        f"   its sibling inherited {inherited['startSecs']}-{inherited['endSecs']}s "
        f"from source {inherited['source']!r}"
    )
    ctx.assert_no_panic()
