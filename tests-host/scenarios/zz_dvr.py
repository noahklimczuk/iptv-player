"""Temporary: does the DVR scheduler pick up a recording that is due right now?"""
import time

PLAYLIST = "http://127.0.0.1:8099/playlist.m3u"


def run(d, ctx):
    time.sleep(6)
    d.send(d.find("textarea"), PLAYLIST)
    time.sleep(1)
    d.click(d.by_text("button", "Check connection"))
    assert d.wait_body(lambda b: "entries" in b.lower() or "could not" in b.lower(), timeout=45)
    d.click(d.by_text("button", "Continue"))
    d.by_text("button", "Import library")
    d.click(d.by_text("button", "Import library"))
    assert d.wait_body(lambda b: "your library is ready" in b.lower(), timeout=120)

    chans = ctx.rows("SELECT id, name FROM channels WHERE hidden = 0 ORDER BY id LIMIT 1")
    print("channel:", chans)
    cid = chans[0][0]

    start = int(time.time())
    rid = d.invoke(
        "dvr_schedule",
        {
            "channelId": cid,
            "title": "Diag recording",
            "airStart": start,
            "airStop": start + 30,
            "prePaddingSecs": 0,
            "postPaddingSecs": 0,
        },
    )
    print("scheduled id:", rid, "python now:", start)

    row = ctx.rows(
        "SELECT id, state, start, stop, air_start, air_stop, priority FROM recordings"
    )
    print("row (id,state,start,stop,air_start,air_stop,priority):", row)

    for i in range(16):
        time.sleep(5)
        listed = [r for r in d.invoke("dvr_list", {}) if r["id"] == rid]
        state = listed[0]["state"] if listed else "<gone>"
        reason = listed[0].get("reason") if listed else None
        print(f"  t+{(i + 1) * 5:>3}s state={state} reason={reason}")
        if state in ("completed", "failed", "skipped"):
            break

    print("--- log lines mentioning the DVR ---")
    for line in (ctx.log() or "").splitlines():
        low = line.lower()
        if any(k in low for k in ("dvr", "record", "tick", "panic")):
            print("LOG:", line[:260])
