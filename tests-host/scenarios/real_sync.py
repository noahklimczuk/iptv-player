"""
Two real installs, one household, through the real Firebase project.

`aurora-sync`'s own tests run two engines against an in-memory server, and its `live`
test runs two engines against the real project, but both of those are a library the test
built by hand. This is the claim the Sync panel makes to a person: set it up on one copy
of Aurora, sign in on another, and a favourite and a film's position follow you.

So it is two portable copies, each with a library imported from the real panel by the
app itself, in turn:

  1. **A** imports the panel, sets up a new household, favourites a channel and stops a
     film part way, and syncs.
  2. **B** -- a separate library, so a separate device id and a separate provider UUID --
     imports the same panel, joins with the passphrase, and must have that favourite and
     that position.
  3. **B** stops the film later on and syncs; **A** comes back and opening the film must
     resume at B's position (`progress.get`, which asks the server for that one record).

Items are matched by the provider's own stream key, not by row id: row 4,812 on A is not
row 4,812 on B, which is the whole reason sync has keys.

The household is a throwaway account (`aurora-live-…@example.com`) and is deleted at the
end, records, vault and all, the way `aurora-sync/tests/live.rs` deletes its own. Every
read and write here counts against the project's free daily quota, which every build
using this project shares; one run is a few dozen of each.

Skipped unless the panel credentials are in the environment and the build has Firebase
compiled in:

    AURORA_PANEL_URL / _USER / _PASS
    AURORA_FIREBASE_API_KEY / AURORA_FIREBASE_PROJECT_ID   (for the cleanup)

Imports only. Nothing here plays a stream, so the line's one connection is not used.
"""
import json
import os
import secrets
import sys
import time
import urllib.error
import urllib.request

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")
API_KEY = os.environ.get("AURORA_FIREBASE_API_KEY")
PROJECT = os.environ.get("AURORA_FIREBASE_PROJECT_ID")

# Live and films only: series are a third of the import and nothing here needs them.
CONTENT = {"live": True, "vod": True, "series": False}
IMPORT_TIMEOUT = 900
PASSPHRASE = "two installs, one household"
A_STOPPED_AT = 1234
B_STOPPED_AT = 2345
DURATION = 5400


class Skipped(Exception):
    """Raised when this scenario has nothing to run against."""


def run(d, ctx):
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")
    if not (API_KEY and PROJECT):
        raise Skipped("no Firebase config in the environment, so the account could not be deleted")

    # The runner is `__main__`; its paths and cleanup are the ones to use, so this
    # scenario's second library is put away exactly the way the runner puts away its first.
    runner = sys.modules["__main__"]
    data = runner.DATA
    parked = data + "-install-a"
    runner.reclaim_credentials(parked)
    runner.wipe(parked)

    email = f"aurora-live-{secrets.token_hex(6)}@example.com"
    password = f"pw-{secrets.token_hex(12)}"
    try:
        # ── Install A ────────────────────────────────────────────────────────────────
        d = _wait_for_ui(d)
        if not d.invoke("sync.status")["available"]:
            raise Skipped("this build has no Firebase project compiled in")
        a_profile = _import_panel(d, ctx)
        channel_key, movie_key = ctx.rows(
            """SELECT (SELECT provider_key FROM channels ORDER BY id LIMIT 1 OFFSET 10),
                      (SELECT provider_key FROM movies   ORDER BY id LIMIT 1 OFFSET 10)"""
        )[0]
        assert channel_key and movie_key, "the panel imported no channels or no films"

        joined = d.invoke("sync.createAccount", {
            "email": email, "password": password,
            "passphrase": PASSPHRASE, "deviceName": "Install A",
        })
        assert joined["problem"] is None, f"setting up the household failed: {joined['problem']}"
        assert joined["recoveryCode"], "the household was set up without a recovery code"

        channel = _id(ctx, "channels", channel_key)
        movie = _id(ctx, "movies", movie_key)
        assert d.invoke("favorites.toggle", {"profileId": a_profile, "channelId": channel}) is True
        d.invoke("progress.save", {
            "profileId": a_profile, "kind": "movie", "id": movie,
            "positionSecs": A_STOPPED_AT, "durationSecs": DURATION,
        })
        _sync(d)
        a_device = d.invoke("sync.status")["deviceId"]
        a_profile_sync = ctx.rows("SELECT sync_id FROM profiles WHERE id = ?", (a_profile,))[0][0]
        print(f"   A: favourited channel {channel_key}, stopped film {movie_key} at {A_STOPPED_AT}s")

        # ── Install B: its own library, so its own device ────────────────────────────
        d = _switch(ctx, d, runner, park=parked, take=None)
        _import_panel(d, ctx)
        joined = d.invoke("sync.signIn", {
            "email": email, "password": password,
            "passphrase": PASSPHRASE, "deviceName": "Install B",
        })
        assert joined["problem"] is None, f"joining the household failed: {joined['problem']}"
        _sync(d)
        assert d.invoke("sync.status")["deviceId"] not in (None, a_device), "the two installs share a device id"

        b_profile = ctx.rows("SELECT id FROM profiles WHERE sync_id = ?", (a_profile_sync,))
        assert b_profile, "B's library has no profile carrying the household's profile id"
        b_profile = b_profile[0][0]
        b_channel = _id(ctx, "channels", channel_key)
        b_movie = _id(ctx, "movies", movie_key)
        favourite = ctx.rows(
            """SELECT count(*) FROM favorites
                WHERE profile_id = ? AND item_kind = 'channel' AND item_id = ?""",
            (b_profile, b_channel),
        )[0][0]
        assert favourite == 1, "A's favourite channel did not reach B"
        got = d.invoke("progress.get", {"profileId": b_profile, "kind": "movie", "id": b_movie})
        assert got and got["positionSecs"] == A_STOPPED_AT, (
            f"B should resume A's film at {A_STOPPED_AT}s and has {got!r}")
        print("   B: has the favourite and resumes the film where A stopped")

        d.invoke("progress.save", {
            "profileId": b_profile, "kind": "movie", "id": b_movie,
            "positionSecs": B_STOPPED_AT, "durationSecs": DURATION,
        })
        _sync(d)

        # ── Back to A: opening the film must pick up where B stopped ─────────────────
        holding_b = data + "-install-b"
        runner.reclaim_credentials(holding_b)
        runner.wipe(holding_b)
        d = _switch(ctx, d, runner, park=holding_b, take=parked)
        started = time.time()
        got = d.invoke("progress.get", {"profileId": a_profile, "kind": "movie", "id": movie})
        took = time.time() - started
        assert got and got["positionSecs"] == B_STOPPED_AT, (
            f"A should resume at B's {B_STOPPED_AT}s on opening the film and has {got!r}")
        print(f"   A: opening the film resumes at B's position ({took:.2f}s to answer)")

        usage = d.invoke("sync.status")["usage"]
        print(f"   A spent {usage['reads']} reads and {usage['writes']} writes today")
        ctx.assert_no_panic()
        parked = holding_b
    finally:
        _delete_household(email, password)
        runner.reclaim_credentials(parked)
        runner.wipe(parked)


def _wait_for_ui(d):
    """The bridge is there once the page has loaded; the wizard is the first screen."""
    end = time.time() + 30
    while time.time() < end:
        try:
            if d.js("return !!(window.__TAURI_INTERNALS__ && document.body.innerText.length)"):
                return d
        except Exception:
            pass
        time.sleep(0.5)
    raise RuntimeError("the window never came up")


def _import_panel(d, ctx):
    """Add the panel and import it, the way the wizard does, and return the profile."""
    draft = {"name": "Real Panel", "kind": "xtream", "url": URL, "username": USER, "password": PASS}
    provider = d.invoke("providers.save", {"draft": draft})["id"]
    d.script_timeout(IMPORT_TIMEOUT)
    try:
        d.invoke("providers.refresh", {"providerId": provider, "content": CONTENT})
    finally:
        d.script_timeout(30)
    counts = {t: ctx.count(t) for t in ("channels", "movies")}
    print(f"   imported {counts}")
    return ctx.rows("SELECT id FROM profiles ORDER BY id LIMIT 1")[0][0]


def _id(ctx, table, key):
    rows = ctx.rows(f"SELECT id FROM {table} WHERE provider_key = ?", (key,))
    assert rows, f"{table} has nothing with the provider's key {key!r}"
    return rows[0][0]


def _sync(d):
    """One round now. A round that fails is refused, which `invoke` raises."""
    d.script_timeout(120)
    try:
        return d.invoke("sync.now")
    finally:
        d.script_timeout(30)


def _switch(ctx, d, runner, park, take):
    """Close this install, put its library at `park`, and open `take` (or a new one)."""
    try:
        d.quit()
    except Exception:
        pass
    time.sleep(3)
    # Kills the app by path and brings `tauri-driver` back, as `ctx.relaunch` does.
    ctx.restart_driver()
    _move(runner.DATA, park)
    if take:
        _move(take, runner.DATA)
    ctx.driver = runner.WD(runner.EXE)
    return _wait_for_ui(ctx.driver)


def _move(src, dst):
    """A rename, retried: the web view lets go of its folder a moment after the app."""
    for _ in range(120):
        try:
            os.replace(src, dst)
            return
        except OSError:
            time.sleep(0.25)
    raise RuntimeError(f"could not move {src} to {dst}: something still holds it")


def _delete_household(email, password):
    """Delete the throwaway account and everything it put on the server."""
    def call(url, body, token=None):
        req = urllib.request.Request(url, data=json.dumps(body).encode(), method="POST")
        req.add_header("Content-Type", "application/json")
        if token:
            req.add_header("Authorization", f"Bearer {token}")
        with urllib.request.urlopen(req, timeout=30) as r:
            return json.loads(r.read() or b"{}")

    try:
        session = call(
            f"https://identitytoolkit.googleapis.com/v1/accounts:signInWithPassword?key={API_KEY}",
            {"email": email, "password": password, "returnSecureToken": True},
        )
    except urllib.error.HTTPError as e:
        # Never created -- the run failed before the household was set up.
        print(f"   no account to delete ({e.code})")
        return
    token, uid = session["idToken"], session["localId"]
    docs = f"projects/{PROJECT}/databases/(default)/documents"
    base = f"https://firestore.googleapis.com/v1/{docs}"
    deleted = 0
    try:
        while True:
            found = call(f"{base}/users/{uid}:runQuery", {"structuredQuery": {
                "from": [{"collectionId": "records"}],
                "select": {"fields": [{"fieldPath": "__name__"}]},
                "limit": 400,
            }}, token)
            names = [r["document"]["name"] for r in found if "document" in r]
            if not names:
                break
            call(f"{base}:commit", {"writes": [{"delete": n} for n in names]}, token)
            deleted += len(names)
        call(f"{base}:commit", {"writes": [{"delete": f"{docs}/users/{uid}/meta/vault"}]}, token)
    finally:
        call(f"https://identitytoolkit.googleapis.com/v1/accounts:delete?key={API_KEY}",
             {"idToken": token})
    print(f"   deleted the household: {deleted} records, the vault and the account")
