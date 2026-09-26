"""
The app starts, builds its library, and draws its first screen.

Deliberately the least ambitious scenario in the suite, and still worth having: it is
the only thing in this repository that proves `aurora-app` — the real Tauri host, the
real migrations, the real SQLite file — gets as far as rendering anything. Every other
check either compiles the host without running it, or runs the UI against a mock.

Asserts on a *fresh* library, because that is the state a new install is in and the one
no test had ever exercised end to end.
"""
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

    # Nothing may have panicked on the way up. The host logs panics through
    # `log_panics()`, and a panicked background thread is otherwise invisible.
    ctx.assert_no_panic()
