"""
Parental controls in the real app, over a library imported from the real panel.

`aurora-db`'s tests prove the gate on a dozen rows and `gate_cost` times it on a made-up
117,510; this is the claim made to a parent, in the shipped host over a real
subscription's library (`docs/DECISIONS.md` D29, `AUDIT/findings.md` F-36):

  1. adult categories are hidden on a fresh install, from the lists and from playback;
  2. a kids profile, once chosen, sees only films rated within its limit -- unrated ones
     included in that, since a kids profile blocks them -- and the list is timed against
     the owner's, on this library;
  3. once a parental PIN exists, a locked channel is refused until the PIN is entered,
     and an age limit cannot be changed without it;
  4. a restart forgets who was watching and anything the PIN had opened.

Skipped unless the panel credentials are in the environment:

    AURORA_PANEL_URL / _USER / _PASS

Imports only. Every playback here is one the gate refuses before a stream is planned,
so the line's one connection is never used.
"""
import os
import re
import time

from driver import HostRefused

URL = os.environ.get("AURORA_PANEL_URL")
USER = os.environ.get("AURORA_PANEL_USER")
PASS = os.environ.get("AURORA_PANEL_PASS")

CONTENT = {"live": True, "vod": True, "series": False}
IMPORT_TIMEOUT = 900
PIN = "2468"
KIDS_LIMIT = 12

NOT_ALLOWED = "This profile can't watch that."
LOCKED = "This is locked."
PIN_REQUIRED = "Enter the parental PIN to change this."

# `aurora_core::parental::looks_adult`, as far as SQL `LIKE` can say it. Used only to
# find something to try; the host's own answer is what is asserted.
ADULT_GROUP = """lower(COALESCE(custom_group, group_title)) LIKE '%adult%'
              OR lower(COALESCE(custom_group, group_title)) LIKE '%xxx%'
              OR lower(COALESCE(custom_group, group_title)) LIKE '%porn%'
              OR COALESCE(custom_group, group_title) LIKE '%18+%'"""

# `aurora_core::parental::min_age` for the forms a panel's films carry; anything else is
# reported rather than guessed at.
NAMED = {
    "U": 0, "G": 0, "TV-Y": 0, "TV-G": 0, "PG": 6, "TV-PG": 6, "TV-Y7": 6,
    "PG-13": 12, "12A": 12, "TV-14": 12, "M": 12, "TEEN": 13, "MA15": 15, "FSK16": 16,
    "R": 17, "TV-MA": 17, "MA": 17, "NC-17": 17,
}


class Skipped(Exception):
    """Raised when this scenario has nothing to run against."""


def run(d, ctx):
    if not (URL and USER and PASS):
        raise Skipped("no panel credentials in the environment")

    d = _wait_for_ui(d)
    owner = _import_panel(d, ctx)
    d.invoke("profiles.select", {"profileId": owner})

    # 1. Adult categories, hidden by default ------------------------------------------
    adult = ctx.rows(f"SELECT id, COALESCE(custom_group, group_title) FROM channels WHERE {ADULT_GROUP} LIMIT 1")
    if adult:
        channel, group = adult[0]
        _refused(d, "player.play", {"kind": "live", "id": channel}, NOT_ALLOWED)
        groups = [g["name"] for g in d.invoke("channels.groups")]
        assert group not in groups, f"the adult category {group!r} is listed"
        print(f"   adult: {group!r} is not listed and channel {channel} is refused")
    else:
        print("   adult: this line files nothing under an adult category; step skipped")

    owner_films, owner_ms = _films(d)
    print(f"   owner: first page of {len(owner_films)} films in {owner_ms:.0f} ms")

    # 2. A kids profile ----------------------------------------------------------------
    kid = d.invoke("profiles.create", {"name": "Sam", "isKids": True, "maxAge": KIDS_LIMIT})
    d.invoke("profiles.select", {"profileId": kid})
    kid_films, kid_ms = _films(d)
    assert kid_films, "a kids profile with a limit of 12 sees no films at all on this line"
    ids = ",".join(str(f["id"]) for f in kid_films)
    certs = ctx.rows(f"SELECT id, certification FROM movies WHERE id IN ({ids})")
    unknown = []
    for film, cert in certs:
        assert cert and cert.strip(), f"film {film} has no rating and a kids profile was shown it"
        age = _age(cert)
        if age is None:
            unknown.append(cert)
            continue
        assert age <= KIDS_LIMIT, f"film {film} is rated {cert!r}, above the kids limit"
    rated_total = ctx.rows("SELECT count(*) FROM movies WHERE hidden = 0 AND certification IS NOT NULL AND certification != ''")[0][0]
    print(f"   kids: first page of {len(kid_films)} films in {kid_ms:.0f} ms, all rated "
          f"{KIDS_LIMIT} or under ({rated_total} of {ctx.count('movies')} films carry a rating at all)")
    if unknown:
        print(f"   kids: ratings this check cannot read, left to the host: {sorted(set(unknown))[:8]}")
    over = ctx.rows("SELECT id FROM movies WHERE certification IN ('R','18','TV-MA','NC-17','15') LIMIT 1")
    if over:
        _refused(d, "player.play", {"kind": "movie", "id": over[0][0]}, NOT_ALLOWED)
        print(f"   kids: film {over[0][0]}, rated above the limit, is refused")

    # 3. The parental PIN and a lock ---------------------------------------------------
    d.invoke("profiles.select", {"profileId": owner})
    d.invoke("profiles.setParental", {"hideAdult": True, "lockSettings": False, "masterPin": PIN})
    plain = ctx.rows(f"SELECT id FROM channels WHERE hidden = 0 AND NOT ({ADULT_GROUP}) ORDER BY id LIMIT 1 OFFSET 5")[0][0]
    d.invoke("parental.setLock", {"kind": "channel", "value": str(plain), "locked": True})
    d.invoke("parental.relock")
    _refused(d, "player.play", {"kind": "live", "id": plain}, LOCKED)
    _refused(d, "profiles.setLimits",
             {"profileId": kid, "maxAge": None, "allowUnrated": True, "dailyLimitMin": None},
             PIN_REQUIRED)
    wrong = d.invoke("parental.unlock", {"pin": "0000"})
    assert isinstance(wrong, dict) and "wrong" in wrong, f"a wrong PIN was answered {wrong!r}"
    assert d.invoke("parental.unlock", {"pin": PIN}) == "ok"
    d.invoke("profiles.setLimits", {"profileId": kid, "maxAge": KIDS_LIMIT, "allowUnrated": False, "dailyLimitMin": None})
    print(f"   lock: channel {plain} is refused until the PIN, and so is lifting a limit")

    # 4. A restart forgets who was watching -------------------------------------------
    d = ctx.relaunch(d)
    d = _wait_for_ui(d)
    state = d.invoke("parental.state")
    assert state["profileId"] is None, f"a restart still has profile {state['profileId']} watching"
    assert state["unlockedUntil"] is None, "a restart is still unlocked"
    _refused(d, "player.play", {"kind": "live", "id": plain}, LOCKED)
    # Nobody chosen and a kids profile exists: the strictest limits apply.
    fallback, _ = _films(d)
    assert [f["id"] for f in fallback] == [f["id"] for f in kid_films], (
        "with nobody chosen, the list is not the kids profile's")
    print("   restart: nobody watching, nothing unlocked, the strictest profile's list")
    ctx.assert_no_panic()


def _films(d):
    start = time.perf_counter()
    films = d.invoke("library.movies", {"sort": "title", "limit": 60, "offset": 0})
    return films, (time.perf_counter() - start) * 1000


def _age(cert):
    c = cert.strip().upper()
    c = re.sub(r"^(RATED|RATING) ", "", c)
    m = re.fullmatch(r"(?:AGE )?(\d{1,2})\+?", c)
    if m:
        return int(m.group(1))
    return NAMED.get(c)


def _refused(d, command, args, words):
    try:
        d.invoke(command, args)
    except HostRefused as e:
        assert words in str(e), f"{command} was refused, but with {e}"
        return
    raise AssertionError(f"{command} {args} was allowed; expected {words!r}")


def _wait_for_ui(d):
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
    draft = {"name": "Real Panel", "kind": "xtream", "url": URL, "username": USER, "password": PASS}
    provider = d.invoke("providers.save", {"draft": draft})["id"]
    d.script_timeout(IMPORT_TIMEOUT)
    try:
        d.invoke("providers.refresh", {"providerId": provider, "content": CONTENT})
    finally:
        d.script_timeout(30)
    print(f"   imported {ctx.count('channels')} channels and {ctx.count('movies')} films")
    return ctx.rows("SELECT id FROM profiles ORDER BY id LIMIT 1")[0][0]
