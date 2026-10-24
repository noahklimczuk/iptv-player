"""
Multi-view answers on the real host (README §7.4).

This is the check the browser suite cannot make. Eleven `mosaic.*` commands were added
to `shared/ipc.ts`, and the mock answers all of them — which is exactly the shape of
F-04: eight commands were declared and never registered, so the browser preview looked
complete and the shipped app answered "command not found". `contract.rs` now pins the
names, but a name being registered is not the same as the command working: `mosaic.check`
reads the providers table and the DVR, and `mosaic.state` serialises a view the UI has to
be able to parse.

So this asks the real host, through the real IPC, with no provider configured — which is
also the state that matters most, because it is what a fresh install is in.

What it deliberately does *not* do is open a mosaic. That needs channels with playable
URLs, and nine real streams from a provider; `real_panel` is where that belongs.
"""
import time

# Commands cross the bridge as `module_action`: `mosaic.check` in `shared/ipc.ts` is
# `mosaic_check` here, because the driver calls `__TAURI_INTERNALS__.invoke` directly and
# the translation the UI does in `src-ui/src/ipc/index.ts` is not in the way.


def run(d, ctx):
    time.sleep(6)

    # `mosaic.state` on a fresh host: closed, and shaped the way the UI expects rather
    # than `null` or a bare object.
    state = d.invoke("mosaic_state")
    assert state["open"] is False, f"a fresh host has a mosaic open: {state}"
    assert state["tiles"] == [], f"a closed mosaic has tiles: {state}"
    assert state["layout"] is None, state
    assert state["focused"] == 0, state

    # `mosaic.check` for every layout. With no provider there is no declared limit, so
    # every one of them must come back `unknown` — not `fits`, which would be the host
    # claiming to know something it cannot (docs/DECISIONS.md D27).
    expected_tiles = {"grid2x2": 4, "onePlusThree": 4, "onePlusFive": 6, "grid3x3": 9}
    for layout, tiles in expected_tiles.items():
        check = d.invoke("mosaic_check", {"layout": layout})
        assert check["layout"] == layout, check
        assert check["tiles"] == tiles, f"{layout} should have {tiles} tiles: {check}"
        assert check["verdict"] == "unknown", (
            f"{layout} with no provider should be 'unknown' and said {check['verdict']!r}: "
            f"{check}"
        )
        assert check["needed"] == tiles, check
        assert check["recordings"] == 0, check
        # Nothing declared means every layout is worth offering.
        assert check["largestFitting"] == "grid3x3", check

    # A host refusal arrives as an `AssertionError` from `driver.invoke`, so it cannot be
    # caught by type apart from a scenario's own failed assertion. Hence the sentinel:
    # raising inside the `try` would be swallowed by the `except` meant for the host.
    def refusal_of(command, args=None):
        """The host's own words for refusing `command`, or `None` if it answered."""
        try:
            d.invoke(command, args)
            return None
        except AssertionError as e:
            return str(e)

    # An unknown layout name is refused rather than guessed at.
    unknown = refusal_of("mosaic_check", {"layout": "grid4x4"})
    assert unknown is not None, "an unknown layout was accepted"
    assert "grid4x4" in unknown, f"the refusal should name the layout: {unknown}"

    # Commands that need an open mosaic say so, in words, rather than panicking the
    # host — which on a release build is an abort and takes the process with it.
    for command, args in (
        ("mosaic_focus", {"index": 0}),
        ("mosaic_set_tile", {"index": 0, "channelId": None}),
        ("mosaic_promote", {"index": 0}),
        ("mosaic_save", {"name": "x"}),
    ):
        said = refusal_of(command, args)
        assert said is not None, f"{command} answered with no mosaic open"
        assert "not open" in said, f"{command} said {said!r}"

    # The saved-layout table exists and is empty, which is migration 11 having run.
    assert d.invoke("mosaic_layouts") == [], "a fresh library has saved layouts"

    # And the screen draws on the real host.
    #
    # Reaching it on a fresh install means dismissing the wizard, which is only possible
    # since "Skip for now" was fixed — `needsSetup` is also true while no provider
    # exists, so dismissing it used to put it straight back.
    d.click(d.by_text("button", "Skip for now"))
    assert d.wait_body(lambda b: "Welcome to Aurora TV" not in b, timeout=20), (
        "Skip for now left the wizard where it was"
    )

    d.click(d.by_text("a", "Multi-view"))
    # With no channels the screen is its empty state rather than four empty pickers —
    # which is also why this waits for *that* and not for the intro paragraph: the page
    # returns before rendering it.
    assert d.wait_body(lambda b: "No channels to show" in b, timeout=20), (
        f"the Multi-view screen did not draw; body was {d.body()[:300]!r}"
    )
    assert "needs live channels" in d.body()
    d.shot(ctx.shot("multiview"))

    # Opening a real mosaic belongs in `real_panel`: it needs channels with playable
    # URLs, and as many provider connections as the layout has tiles.
    # Seen once on the way through here and not reproduced since: a toast reading
    # "Could not seek - mpv command failed: seek: Raw(-12)" on a fresh install with
    # nothing playing. Nothing in this scenario asks for a seek, and `assert_no_panic`
    # cannot catch it because it is a refused command rather than a panic. Recorded
    # rather than chased: it is on `main`, and intermittent.
    ctx.assert_no_panic()
