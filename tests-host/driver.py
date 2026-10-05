"""
A WebDriver client for driving the *real* Aurora host.

Everything else that calls itself end-to-end in this repository runs the UI against
the mock transport (`src-ui/src/ipc/mock.ts`). That is deliberate — it is how the
screens are developable without Windows — and it is also the single biggest source of
bugs this project has shipped. F-09 (favourites the host never returned), F-12 (a CSP
that blocked every provider logo), `progress.save` called only by the mock, a progress
bar fed from the mock's own memory: each one looked perfect in a browser and was dead
on a real machine, because the thing under test answered its own questions.

This harness closes that. It runs the actual Tauri binary against a real SQLite
database and drives it over WebDriver. The transport is the real IPC, the commands are
the real Rust, the library is a real file on disk.

It runs on either platform, and which one matters more than it looks. On Linux it
drives WebKitGTK under Xvfb, and `aurora-player` is the `NullBackend` — so every
scenario up to the point where a picture would appear is covered, and the picture
itself is not. On Windows it drives WebView2, which is what ships, and the backend is
the real libmpv one. The Win32 child surface and its z-order under a transparent
WebView2 can only be seen there.

Requires `tauri-driver` (`cargo install tauri-driver`) on both, plus Xvfb and
WebKitWebDriver (`apt install webkit2gtk-driver`) on Linux, or Microsoft Edge WebDriver
on Windows. `run.py` checks for whichever set applies; docs/TESTING_THE_HOST.md and
docs/TESTING_ON_WINDOWS.md say how to get them.
"""
import base64, json, re, time, urllib.request, urllib.error

BASE = "http://127.0.0.1:4444"


def wire_command(name):
    """The host's name for a command the UI spells in its own way.

    `src-ui/src/ipc/index.ts` sends `library.browseFacets` across as
    `library_browse_facets`. A name already in the host's spelling has no dots and no
    capitals, so it passes through untouched.
    """
    return re.sub(r"[A-Z]", lambda m: "_" + m.group(0).lower(), name.replace(".", "_"))


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

    def js(self, script, *args):
        """Run JavaScript in the page and return its value.

        `return` is required, as WebDriver requires. Worth having beyond convenience:
        it reaches the real Tauri bridge, so a scenario can ask the host a question
        directly rather than inferring the answer from pixels — which is how the
        difference between "the host is playing" and "the OSD says nothing is
        playing" became visible at all.
        """
        return self.r("POST", "/execute/sync", {"script": script, "args": list(args)})["value"]

    def invoke(self, command, args=None):
        """Call a host command over the same bridge the UI uses.

        The name is translated the way the UI translates it, so a scenario can write
        `app.diagnostics` — what the UI calls it, and what is greppable — and reach
        `app_diagnostics`, which is what the host registered. Either spelling works.
        Arguments go under `args` because every command takes them as one struct.

        A refusal is raised carrying what the host actually said. Letting the promise
        reject instead gives "Could not parse script result" from the driver, which
        reads like a broken harness and is in fact a command that does not exist.
        """
        got = self.js(
            "const [c, a] = arguments;"
            " return window.__TAURI_INTERNALS__.invoke(c, a === null ? {} : { args: a })"
            "   .then((v) => ({ ok: true, value: v === undefined ? null : v }))"
            "   .catch((e) => ({ ok: false, error: String((e && e.message) || e) }));",
            wire_command(command), args,
        )
        if not got.get("ok"):
            raise AssertionError(f"the host refused {command!r}: {got.get('error')}")
        return got["value"]

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
