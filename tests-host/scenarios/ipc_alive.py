"""
The host still answers after the window is up.

This is the scenario that was missing when 1.0.1 shipped unusable: the app drew, the UI
mounted, and then every command the first screen makes pended forever, so it sat on
"Opening your library..." with no error anywhere. Nothing in the suite was asking the one
question that would have caught it -- *does an invoke come back* -- because every other
scenario assumes it does and reads the answer.

It separates the two causes, which need different fixes:

  * a static fetch pending too means the host's main thread is parked, and wry's
    custom-protocol handler answers nothing at all;
  * assets serving while `invoke` hangs means the IPC route alone is dead -- a command
    deadlock, or a CSP without `connect-src`.

Both probes are fired together and read back synchronously, so a hang shows up as a hang
rather than as this scenario itself timing out.
"""
import json
import time

PROBE = r"""
window.__ipcAlive = { asset: 'pending', invoke: 'pending' };
fetch(window.location.href, { cache: 'no-store' })
  .then((r) => { window.__ipcAlive.asset = 'ok ' + r.status; })
  .catch((e) => { window.__ipcAlive.asset = 'err ' + e; });
window.__TAURI_INTERNALS__.invoke('player_state', {})
  .then(() => { window.__ipcAlive.invoke = 'ok'; })
  .catch((e) => { window.__ipcAlive.invoke = 'err ' + e; });
return true;
"""

#: Generous. The point is to tell "slow" from "never", and the failure mode being caught
#: never returns at all.
BUDGET_SECS = 15


def run(d, ctx):
    time.sleep(5)
    assert d.js(PROBE) is True, "the probe did not run"

    got = {}
    for _ in range(BUDGET_SECS):
        time.sleep(1)
        got = json.loads(d.js("return JSON.stringify(window.__ipcAlive);"))
        if "pending" not in got.values():
            break

    assert not got["asset"].startswith("pending"), (
        f"a static asset never came back, so the host's main thread is parked: {got}"
    )
    assert got["asset"].startswith("ok"), f"the host would not serve its own page: {got}"
    assert got["invoke"] != "pending", (
        "assets serve but `invoke` never returns: the IPC route is dead while the window "
        f"looks fine -- which is what 1.0.1 shipped. {got}"
    )
    assert got["invoke"] == "ok", f"the host refused a plain command: {got}"

    # And the screen the UI actually puts up, rather than its loading state.
    body = d.body()
    assert "Opening your library" not in body, (
        "still on the loading screen after the host answered -- the UI is waiting on "
        "something else"
    )

    ctx.assert_no_panic()
