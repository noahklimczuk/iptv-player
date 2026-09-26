#!/usr/bin/env python3
"""
Run the host scenarios against a real, running Aurora.

    python3 tests-host/run.py                # all scenarios
    python3 tests-host/run.py starts_up      # one of them

Each scenario gets a fresh library, a fresh app process and a fresh WebDriver session,
because the interesting state — an empty database, a first import — only happens once
per install and a shared one would hide it.

Deliberately the *release* binary, for two reasons. It is the build people run, with
`panic = "abort"` set, so a panic that unwinds harmlessly in a debug build takes the
whole application down here — which is the behaviour worth testing. And only a release
build writes `aurora.log`: `init_logging` gives a debug build the console it already
has, and `tauri-driver` does not carry the app's console anywhere this can read it.
A `portable.txt` beside the executable then puts both the log and the library in
`target/release/data`, where a scenario can look at them.
"""
import importlib.util
import os
import pathlib
import shutil
import sqlite3
import subprocess
import sys
import time
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "tests-host"))
from driver import WD  # noqa: E402
from fakegithub import FakeGitHub  # noqa: E402

# `AURORA_TEST_EXE=…/target/debug/aurora-app` runs the same scenarios against a debug
# build. Worth having for one reason: a debug build unwinds where a release build
# aborts, so a panic that kills the shipped app can be inspected in a live window here.
WINDOWS = os.name == "nt"

# The only place the two platforms disagree about what to launch. Everything else
# below asks `WINDOWS` rather than branching on a path.
APP_EXE = "aurora-app.exe" if WINDOWS else "aurora-app"

EXE = os.environ.get(
    "AURORA_TEST_EXE",
    os.path.join(ROOT, "src-native", "target", "release", APP_EXE),
)
EXE_DIR = os.path.dirname(EXE)
# What to kill when a process outlives its session — see `kill()`.
APP_NAME = os.path.splitext(os.path.basename(EXE))[0]
# Where `portable_dir()` puts everything: `<exe dir>/data`, holding library.db and
# aurora.log. Wiped between scenarios.
DATA = os.path.join(EXE_DIR, "data")
SHOTS = os.path.join(ROOT, "screenshots", "host")
FIXTURES = os.path.join(ROOT, "tests-host", "fixtures")
# Linux only: Windows runs the app on the real desktop, and there has to be one —
# see docs/TESTING_ON_WINDOWS.md on why a service session will not do.
DISPLAY = ":99"
FIXTURE_PORT = 8099
DRIVER_PORT = 4444

# How many times a scenario may be restarted after the WebDriver connection dies.
#
# Two, because this happens: WebKit under Xvfb holding a 140,000-row library and a
# page of remote posters is not robust. Narrow, though — only for transport errors,
# and only when the app's own log shows no panic. A host that died is what this suite
# exists to catch and must never be retried away.
DRIVER_RETRIES = 2


def read_only(path):
    """A read-only SQLite URI for a path, on either platform.

    `file:{path}?mode=ro` is fine until the path is `C:\\Users\\…`: a URI cannot carry
    backslashes, and SQLite reads what survives as a relative path that does not
    exist — so every `count()` in every scenario would raise "unable to open database
    file" and the harness would look broken rather than the app. `as_uri()` gives
    `file:///C:/Users/…`, which both platforms accept.
    """
    return pathlib.Path(path).absolute().as_uri() + "?mode=ro"


def kill(*names):
    """Stop these by executable name, however this platform spells that.

    Best-effort by design: it is called to clear the way, and a name that was not
    running is the outcome it wanted anyway.
    """
    quiet = dict(check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for name in names:
        if WINDOWS:
            subprocess.run(["taskkill", "/F", "/T", "/IM", f"{name}.exe"], **quiet)
        else:
            # `-x`, not `-f`: matching the whole command line makes the pattern match
            # this script's own, and killing your own shell is a confusing way to
            # discover that.
            subprocess.run(["pkill", "-x", name], **quiet)


def wipe_data(tries=12):
    """Delete this scenario's library, and be sure it is actually gone.

    `rmtree(..., ignore_errors=True)` is a silent no-op against a file another process
    still holds open — which on Windows is every file the app has, right until it
    exits. That would not fail; it would hand the next scenario the last one's
    library, and "an empty database is a state that only happens once" would quietly
    stop being true. Exactly the class of bug this suite exists to catch, so it is
    worth failing loudly over.

    Linux unlinks an open file happily, so the first pass almost always returns and
    none of the rest of this runs there.
    """
    for attempt in range(tries):
        shutil.rmtree(DATA, ignore_errors=True)
        if not os.path.exists(DATA):
            return
        # Halfway through, stop waiting politely for the last app to exit.
        if attempt == tries // 2:
            kill(APP_NAME)
        time.sleep(0.5)

    left = [
        os.path.join(root, f)
        for root, _dirs, files in os.walk(DATA)
        for f in files
    ]
    raise RuntimeError(
        "could not clear the scenario's library — something still holds it open:\n  "
        + "\n  ".join(left[:10])
    )


class Ctx:
    """What a scenario can ask about the machine outside the window."""

    def __init__(self, name, stderr_path, since):
        self.name = name
        # `tauri-driver`'s own output, which carries anything the app wrote to stderr
        # before its file logger existed. One long file held open for the whole run, so
        # it is read from an offset rather than truncated between scenarios.
        self.stderr_path = stderr_path
        self.since = since
        os.makedirs(SHOTS, exist_ok=True)

    def shot(self, tag):
        return os.path.join(SHOTS, f"{self.name}-{tag}.png")

    def log(self):
        """Everything this scenario's process said, from both places it can say it."""
        parts = []
        app_log = os.path.join(DATA, "aurora.log")
        if os.path.exists(app_log):
            with open(app_log, encoding="utf-8", errors="replace") as f:
                parts.append(f.read())
        try:
            with open(self.stderr_path, encoding="utf-8", errors="replace") as f:
                f.seek(self.since)
                parts.append(f.read())
        except (FileNotFoundError, OSError):
            pass
        return "\n".join(parts)

    def log_count(self, needle):
        return self.log().count(needle)

    def assert_no_panic(self):
        """A panicked host is a failure even when the screen looks fine.

        Two spellings, because a panic is recorded twice and either one may be all
        there is. `log_panics()` writes "panic at <location>: <message>" through
        `tracing`, which is the copy that reaches `aurora.log`; the default hook writes
        the familiar "thread '…' panicked at" to stderr, which is the only copy of a
        panic that happens before the logger is built.

        With `panic = "abort"` set for release, this is rarely a survivable event — the
        process is gone and the window with it — so catching it here is the difference
        between finding it and a viewer finding it.
        """
        log = self.log()
        for needle in ("panicked at", "panic at "):
            if needle in log:
                first = log[log.index(needle):][:500]
                raise AssertionError(f"the host panicked:\n{first}")

    def db_path(self):
        return os.path.join(DATA, "library.db")

    def db_exists(self):
        return os.path.exists(self.db_path())

    def _query(self, sql):
        con = sqlite3.connect(read_only(self.db_path()), uri=True)
        try:
            return con.execute(sql).fetchone()[0]
        finally:
            con.close()

    def rows(self, sql, args=()):
        """Read from the library directly.

        For the things the IPC deliberately does not expose — a film's provider
        category, say, which the host uses for recommendations and the UI has no
        business knowing about. Asserting through the UI where the UI is the thing
        under test is how the mock/host gap opened in the first place; this is the
        other direction, and it is the one that checks the work.
        """
        con = sqlite3.connect(read_only(self.db_path()), uri=True)
        try:
            return con.execute(sql, args).fetchall()
        finally:
            con.close()

    def schema_version(self):
        """How far the migrations got.

        `PRAGMA user_version` rather than counting log lines: it is what `migrate.rs`
        actually writes, it is written inside the same transaction as the migration it
        names, and it survives the log being rotated. A half-applied schema reads as
        the last version that committed, which is exactly the question worth asking.
        """
        return self._query("PRAGMA user_version")

    def channel_count(self):
        return self._query("SELECT count(*) FROM channels")

    def updates_dir(self):
        """Where a downloaded release lands, beside the library."""
        return os.path.join(DATA, "updates")

    def downloaded(self):
        """What is in the updates folder, so a scenario can see what arrived."""
        try:
            return sorted(os.listdir(self.updates_dir()))
        except FileNotFoundError:
            return []

    def count(self, table):
        return self._query(f"SELECT count(*) FROM {table}")


def _is_transport_error(e):
    """Whether this is the WebDriver connection dying rather than an assertion."""
    text = f"{type(e).__name__}: {e}"
    return any(
        marker in text
        for marker in (
            "Remote end closed connection",
            "Connection reset by peer",
            "RemoteDisconnected",
            "Connection refused",
            "URLError",
        )
    )


# What has to be out of the way before tauri-driver can bind the port again. It shells
# out to the platform's own WebDriver — WebKit's on Linux, Microsoft Edge's for WebView2
# on Windows — and a stuck one holds the port whichever it is.
NATIVE_DRIVER = "msedgedriver" if WINDOWS else "WebKitWebDriver"


def start_driver(stderr_path, github, append):
    """tauri-driver, wired to the fake releases API — the only one the app accepts."""
    env = dict(os.environ, AURORA_UPDATE_API=github.url)
    if not WINDOWS:
        env["DISPLAY"] = DISPLAY

    cmd = ["tauri-driver", "--port", str(DRIVER_PORT)]
    # Microsoft Edge WebDriver has to match the installed WebView2 runtime build, so
    # the right one is often not the one on PATH. `AURORA_NATIVE_DRIVER` names it.
    native = os.environ.get("AURORA_NATIVE_DRIVER")
    if native:
        cmd += ["--native-driver", native]

    log = open(stderr_path, "a" if append else "w")
    proc = subprocess.Popen(cmd, env=env, stdout=log, stderr=subprocess.STDOUT)
    time.sleep(3)
    return proc


def restart_driver(procs, stderr_path, github):
    """Bring `tauri-driver` back up, leaving the display and the fixtures alone."""
    for p in procs:
        if "tauri-driver" in " ".join(p.args):
            try:
                p.terminate()
                p.wait(timeout=10)
            except Exception:
                pass
    procs = [p for p in procs if "tauri-driver" not in " ".join(p.args)]
    # The app too: on Windows it holds the library open, and `wipe_data` cannot clear
    # a scenario that is about to be retried while the last attempt is still running.
    kill("tauri-driver", NATIVE_DRIVER, APP_NAME)
    time.sleep(2)

    procs.append(start_driver(stderr_path, github, append=True))
    return procs, stderr_path, github


def wait_for_port(port, timeout=20):
    end = time.time() + timeout
    while time.time() < end:
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=1)
            return True
        except Exception as e:
            # A refusal to serve `/` still proves something is listening.
            if "Connection refused" not in str(e):
                return True
        time.sleep(0.3)
    return False


def start_background():
    """A display, the fixture provider, and tauri-driver. Returns them for teardown."""
    procs = []
    devnull = subprocess.DEVNULL
    if not WINDOWS:
        # Xvfb is how a headless Linux container gets a desktop. Windows has a real one
        # already — and needs it: see docs/TESTING_ON_WINDOWS.md.
        procs.append(subprocess.Popen(
            ["Xvfb", DISPLAY, "-screen", "0", "1440x900x24"],
            stdout=devnull, stderr=devnull))
        time.sleep(2)

    procs.append(subprocess.Popen(
        [sys.executable, "-m", "http.server", str(FIXTURE_PORT)],
        cwd=FIXTURES, stdout=devnull, stderr=devnull))
    wait_for_port(FIXTURE_PORT)

    # A stand-in for GitHub's releases API. In-process rather than a subprocess,
    # because a scenario needs to ask it what the app requested.
    github = FakeGitHub().start()

    # The app only accepts a loopback update API (see `test_api_base`), which is what
    # `FakeGitHub` binds — `start_driver` puts it in the environment the app inherits.
    stderr_path = os.path.join(SHOTS, "host-stderr.log")
    procs.append(start_driver(stderr_path, github, append=False))
    return procs, stderr_path, github


def main():
    if not os.path.exists(EXE):
        if WINDOWS:
            # Building here needs an MSVC toolchain and libmpv's import library, which
            # is why the usual answer on Windows is to run a release build rather than
            # make one. `bootstrap.ps1` fetches and unpacks it.
            sys.exit(
                f"nothing at {EXE}\n"
                f"get a build first:  powershell -ExecutionPolicy Bypass "
                f"-File tests-host\\bootstrap.ps1\n"
                f"then point AURORA_TEST_EXE at it — see docs/TESTING_ON_WINDOWS.md"
            )
        sys.exit(
            f"build it first:  cargo build --release -p aurora-app\n"
            f"(nothing at {EXE})"
        )
    print(f"exe: {EXE}")
    doc = "docs/TESTING_ON_WINDOWS.md" if WINDOWS else "docs/TESTING_THE_HOST.md"
    needed = ["tauri-driver"]
    if WINDOWS:
        # Unless one is named outright, in which case it need not be on PATH.
        if not os.environ.get("AURORA_NATIVE_DRIVER"):
            needed.append(NATIVE_DRIVER)
    else:
        needed += ["Xvfb", NATIVE_DRIVER]
    for tool in needed:
        if not shutil.which(tool):
            sys.exit(f"{tool} is not installed — see {doc}")

    # Portable mode, so the library and the log land somewhere a scenario can read
    # them instead of under this container's XDG data directory.
    marker = os.path.join(EXE_DIR, "portable.txt")
    if not os.path.exists(marker):
        with open(marker, "w") as f:
            f.write("written by tests-host/run.py so the harness can read the library\n")

    os.makedirs(SHOTS, exist_ok=True)
    wanted = sys.argv[1:]
    names = [
        f[:-3] for f in sorted(os.listdir(os.path.join(ROOT, "tests-host", "scenarios")))
        if f.endswith(".py") and not f.startswith("_")
    ]
    if wanted:
        unknown = [w for w in wanted if w not in names]
        if unknown:
            sys.exit(f"no such scenario: {', '.join(unknown)}  (have: {', '.join(names)})")
        names = [n for n in names if n in wanted]

    procs, stderr_path, github = start_background()
    failures = []
    skipped = []
    try:
        for name in names:
            path = os.path.join(ROOT, "tests-host", "scenarios", f"{name}.py")
            spec = importlib.util.spec_from_file_location(name, path)
            mod = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(mod)

            # A fresh library per scenario: an empty database is a state that only
            # happens once, and it is the one that has never been tested.
            wipe_data()
            since = os.path.getsize(stderr_path) if os.path.exists(stderr_path) else 0

            print(f"── {name}")
            d = None
            ctx = Ctx(name, stderr_path, since)
            ctx.github = github
            try:
                d = WD(EXE)
                mod.run(d, ctx)
                print("   ok")
            except Exception as e:
                # A WebDriver transport error is the driver or the WebView process
                # going away, not the app failing an assertion — this run imports a
                # 140,000-row library into a WebKit that is also holding a grid of
                # posters, and it happens. Retried once, out loud, and only when the
                # app's own log shows no panic: a host that died is this suite's whole
                # reason for existing and must never be retried away.
                attempt = 0
                while (
                    attempt < DRIVER_RETRIES
                    and _is_transport_error(e)
                    and "panicked at" not in ctx.log()
                ):
                    attempt += 1
                    print(
                        f"   driver went away ({type(e).__name__}); "
                        f"restarting it ({attempt} of {DRIVER_RETRIES})"
                    )
                    if d:
                        d.quit()
                    d = None
                    procs, stderr_path, github = restart_driver(procs, stderr_path, github)
                    wipe_data()
                    since = os.path.getsize(stderr_path) if os.path.exists(stderr_path) else 0
                    ctx = Ctx(name, stderr_path, since)
                    ctx.github = github
                    try:
                        d = WD(EXE)
                        mod.run(d, ctx)
                        print(f"   ok (after restarting the driver {attempt}x)")
                        e = None
                        break
                    except Exception as again:
                        e = again
                if e is None:
                    continue
                # A scenario with nothing to run against is not a failure. The real
                # panel needs credentials that belong to a person, not to this
                # repository, so it sits out a run that does not have them — it says
                # so by raising its own `Skipped`, which is matched by name so that
                # scenarios need import nothing from here.
                if type(e).__name__ == "Skipped":
                    skipped.append((name, e))
                    print(f"   skipped: {e}")
                    continue
                failures.append((name, e))
                print(f"   FAILED: {e}")
                if d:
                    try:
                        d.shot(os.path.join(SHOTS, f"{name}-FAILED.png"))
                    except Exception:
                        # An aborted host has no window left to photograph, which is
                        # itself worth saying rather than swallowing.
                        print("   (no screenshot: the window was already gone)")
            finally:
                if d:
                    d.quit()
                # Keep the log this scenario produced; the next one wipes `data`.
                if os.path.exists(os.path.join(DATA, "aurora.log")):
                    shutil.copy(
                        os.path.join(DATA, "aurora.log"),
                        os.path.join(SHOTS, f"{name}-aurora.log"),
                    )
                time.sleep(1)
    finally:
        github.stop()
        for p in procs:
            try:
                p.terminate()
            except Exception:
                pass

    print()
    ran = len(names) - len(skipped)
    print(f"{ran - len(failures)}/{ran} passed" + (f", {len(skipped)} skipped" if skipped else ""))
    for name, err in failures:
        print(f"  {name}: {err}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
