"""
A WebDriver client for driving the *real* Aurora host.

Everything else that calls itself end-to-end in this repository runs the UI against
the mock transport (`src-ui/src/ipc/mock.ts`). That is deliberate — it is how the
screens are developable without Windows — and it is also the single biggest source of
bugs this project has shipped. F-09 (favourites the host never returned), F-12 (a CSP
that blocked every provider logo), `progress.save` called only by the mock, a progress
bar fed from the mock's own memory: each one looked perfect in a browser and was dead
on a real machine, because the thing under test answered its own questions.

This harness closes that. It builds the actual Tauri binary, runs it under Xvfb against
a real SQLite database, and drives it over WebDriver. The transport is the real IPC, the
commands are the real Rust, the library is a real file on disk.

What it still cannot show: libmpv. `aurora-player` falls back to `NullBackend` off
Windows, so video compositing and the Win32 surface remain Windows-only questions. It
covers everything up to the point where a picture would appear.

Requires: Xvfb, WebKitWebDriver (`apt install webkit2gtk-driver`) and `tauri-driver`
(`cargo install tauri-driver`). `run.sh` checks for all three.
"""
import base64, json, time, urllib.request, urllib.error

BASE = "http://127.0.0.1:4444"

class WD:
    def __init__(self, exe, data_dir=None):
        caps = {"tauri:options": {"application": exe}}
        r = self._rq("POST", "/session",
                     {"capabilities": {"alwaysMatch": caps}, "desiredCapabilities": caps})
        v = r.get("value", r)
        self.sid = v.get("sessionId") or r.get("sessionId")

    def _rq(self, method, path, body=None):
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(BASE + path, data=data, method=method,
                                     headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                return json.loads(resp.read() or b"{}")
        except urllib.error.HTTPError as e:
            raise RuntimeError(f"{method} {path} -> {e.code}: {e.read().decode()[:500]}")

    def r(self, method, path, body=None):
        return self._rq(method, f"/session/{self.sid}{path}", body)

    def find_all(self, css):
        out = self.r("POST", "/elements", {"using": "css selector", "value": css})["value"]
        return [list(v.values())[0] for v in out]

    def find(self, css, timeout=15):
        end = time.time() + timeout
        while time.time() < end:
            got = self.find_all(css)
            if got:
                return got[0]
            time.sleep(0.25)
        raise AssertionError(f"no element matched {css!r} within {timeout}s")

    def text(self, eid):
        return self.r("GET", f"/element/{eid}/text")["value"]

    def click(self, eid):
        self.r("POST", f"/element/{eid}/click", {})

    def send(self, eid, s):
        self.r("POST", f"/element/{eid}/value", {"text": s, "value": list(s)})

    def by_label(self, label, timeout=15):
        """An input by its `aria-label`. The wizard's fields have no ids."""
        return self.find(f'[aria-label="{label}"]', timeout=timeout)

    def clear(self, eid):
        self.r("POST", f"/element/{eid}/clear", {})

    def by_text(self, css, needle, timeout=15):
        """The first element matching `css` whose text contains `needle`."""
        end = time.time() + timeout
        while time.time() < end:
            for e in self.find_all(css):
                try:
                    if needle.lower() in (self.text(e) or "").lower():
                        return e
                except Exception:
                    pass
            time.sleep(0.25)
        raise AssertionError(f"no {css} containing {needle!r} within {timeout}s")

    def wait_body(self, predicate, timeout=30):
        """Poll the page text until `predicate` holds. False on timeout.

        Returns rather than raises so a scenario can assert with its own words — "the
        connection check never finished" says more than a locator timeout.
        """
        end = time.time() + timeout
        while time.time() < end:
            try:
                if predicate(self.body()):
                    return True
            except Exception:
                pass
            time.sleep(0.4)
        return False

    def body(self):
        return self.text(self.find("body"))

    def shot(self, path):
        png = self.r("GET", "/screenshot")["value"]
        open(path, "wb").write(base64.b64decode(png))

    def quit(self):
        try:
            self._rq("DELETE", f"/session/{self.sid}")
        except Exception:
            pass
