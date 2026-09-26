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
import shutil
import signal
import sqlite3
import subprocess
import sys
import time
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "tests-host"))
from driver import WD  # noqa: E402

# `AURORA_TEST_EXE=…/target/debug/aurora-app` runs the same scenarios against a debug
# build. Worth having for one reason: a debug build unwinds where a release build
# aborts, so a panic that kills the shipped app can be inspected in a live window here.
EXE = os.environ.get(
    "AURORA_TEST_EXE",
    os.path.join(ROOT, "src-native", "target", "release", "aurora-app"),
)
EXE_DIR = os.path.dirname(EXE)
# Where `portable_dir()` puts everything: `<exe dir>/data`, holding library.db and
# aurora.log. Wiped between scenarios.
DATA = os.path.join(EXE_DIR, "data")
SHOTS = os.path.join(ROOT, "screenshots", "host")
FIXTURES = os.path.join(ROOT, "tests-host", "fixtures")
DISPLAY = ":99"
FIXTURE_PORT = 8099
DRIVER_PORT = 4444


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
        con = sqlite3.connect(f"file:{self.db_path()}?mode=ro", uri=True)
        try:
            return con.execute(sql).fetchone()[0]
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

    def count(self, table):
        return self._query(f"SELECT count(*) FROM {table}")


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
    """Xvfb, the fixture provider, and tauri-driver. Returns them for teardown."""
    procs = []
    devnull = subprocess.DEVNULL
    procs.append(subprocess.Popen(
        ["Xvfb", DISPLAY, "-screen", "0", "1440x900x24"], stdout=devnull, stderr=devnull))
    time.sleep(2)

    procs.append(subprocess.Popen(
        [sys.executable, "-m", "http.server", str(FIXTURE_PORT)],
        cwd=FIXTURES, stdout=devnull, stderr=devnull))
    wait_for_port(FIXTURE_PORT)

    env = dict(os.environ, DISPLAY=DISPLAY)
    stderr_path = os.path.join(SHOTS, "host-stderr.log")
    log = open(stderr_path, "w")
    procs.append(subprocess.Popen(
        ["tauri-driver", "--port", str(DRIVER_PORT)],
        env=env, stdout=log, stderr=subprocess.STDOUT))
    time.sleep(3)
    return procs, stderr_path


def main():
    if not os.path.exists(EXE):
        sys.exit(
            f"build it first:  cargo build --release -p aurora-app\n"
            f"(nothing at {EXE})"
        )
    print(f"exe: {EXE}")
    for tool in ("Xvfb", "WebKitWebDriver", "tauri-driver"):
        if not shutil.which(tool):
            sys.exit(f"{tool} is not installed — see docs/TESTING_THE_HOST.md")

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

    procs, stderr_path = start_background()
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
            shutil.rmtree(DATA, ignore_errors=True)
            since = os.path.getsize(stderr_path) if os.path.exists(stderr_path) else 0

            print(f"── {name}")
            d = None
            ctx = Ctx(name, stderr_path, since)
            try:
                d = WD(EXE)
                mod.run(d, ctx)
                print("   ok")
            except Exception as e:
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
        for p in procs:
            try:
                p.send_signal(signal.SIGTERM)
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
