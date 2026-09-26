"""
The app starts, builds its library, and draws its first screen.

Deliberately the least ambitious scenario in the suite, and still worth having: it is
the only thing in this repository that proves `aurora-app` — the real Tauri host, the
real migrations, the real SQLite file — gets as far as rendering anything. Every other
check either compiles the host without running it, or runs the UI against a mock.

Asserts on a *fresh* library, because that is the state a new install is in and the one
no test had ever exercised end to end.
"""
import os
import time

# Must match `LATEST_VERSION` in `aurora-db/src/schema.rs`. A migration added without
# this moving is a migration that a real launch has never run.
EXPECT_SCHEMA = 8


def run(d, ctx):
    time.sleep(6)
    d.shot(ctx.shot("startup"))

    # A fresh database means the first-run wizard, not the library.
    body = d.body()
    assert "Welcome to Aurora TV" in body, f"the wizard did not draw; body was {body[:200]!r}"
    assert d.find("textarea"), "nowhere to paste a playlist"

    # The library file is real, and the schema is all the way forward. A migration that
    # fails leaves the app running against a half-built database, which is worse than
    # not starting — and `user_version` is written in the same transaction as the
    # migration it names, so it says where that stopped.
    assert ctx.db_exists(), "no library.db was created"
    schema = ctx.schema_version()
    assert schema == EXPECT_SCHEMA, (
        f"the schema should be at version {EXPECT_SCHEMA} and the database says {schema}"
    )

    # Which video engine actually started.
    #
    # This is the fact that separates "no picture because the compositing is wrong"
    # from "no picture because libmpv never loaded", and until it was reported here
    # the only way to tell was to find aurora.log and read it. It also guards the
    # suite against its own worst failure mode on Windows: every playback scenario
    # passing against a silently null backend, proving nothing at all.
    engine = d.invoke("app.diagnostics")["videoEngine"]
    if os.name == "nt":
        assert engine["name"] == "mpv" and engine["rendersVideo"], (
            f"this is a Windows build and it has no video engine: {engine}. "
            f"aurora.log says why — usually libmpv-2.dll missing beside the exe."
        )
    else:
        # Off Windows `create_backend` has no mpv to choose. A run claiming otherwise
        # would mean the harness is not testing what it thinks it is.
        assert engine == {"name": "null", "version": None, "rendersVideo": False}, engine

    # And "Skip for now" has to actually skip.
    #
    # It did not. `needsSetup` in App.tsx is also true while no provider exists, so
    # dismissing the wizard put it straight back on screen and the button did nothing
    # at all. The browser suite has a *passing* test for this: the mock ships with a
    # provider already in it, so the one case where skipping matters is the one case
    # it could never cover. Only a fresh host can tell the difference — which is the
    # whole reason this file exists.
    d.click(d.by_text("button", "Skip for now"))
    left = d.wait_body(lambda b: "Welcome to Aurora TV" not in b, timeout=20)
    d.shot(ctx.shot("skipped"))
    assert left, "Skip for now left the wizard exactly where it was"

    # What it lands on has to work, too. An empty library is what a skipped setup
    # leaves behind and nothing had ever drawn a screen from one, so either the
    # profile picker or the app shell is a pass and a blank page is not.
    body = d.body()
    landed = "Home" in body or "watching" in body.lower()
    assert landed, f"skipping landed nowhere usable; body was {body[:300]!r}"

    # Nothing may have panicked on the way up. The host logs panics through
    # `log_panics()`, and a panicked background thread is otherwise invisible.
    ctx.assert_no_panic()
