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
from fakegithub import FakeGitHub  # noqa: E402

# Windows is what this project ships, and the only host where `aurora-player` compiles
# libmpv in at all, so the harness runs there too. It differs in three places: there is
# already a desktop, so no Xvfb; the WebView is WebView2, so `tauri-driver` drives it
# through msedgedriver rather than WebKitWebDriver; and a file that is still open cannot
# be deleted, which is what `wipe` is for.
WINDOWS = sys.platform == "win32"

if WINDOWS:
    # A Windows console encodes in the system code page — cp1252 here — which has no
    # box-drawing characters and no `★`. Reconfiguring is the fix rather than
    # removing them: scenarios print what a provider actually called a channel, and a
    # real panel names them "US ★ QVC HD".
    for _stream in (sys.stdout, sys.stderr):
        try:
            _stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, OSError):
            pass

# Where the build tree is, which on this project's own machine cannot be the default.
#
# A portable copy keeps its library beside the executable, so the harness's data
# directory is `<target>/release/data` — and `wipe` between scenarios needs to actually
# delete it. Under OneDrive it cannot: cloud placeholders deny `rmdir` on a directory
# written moments earlier, so every run after the first is handed the previous run's
# library (`AUDIT/test-report.md` §12). The fix is to build outside the synced tree,
# and honouring `CARGO_TARGET_DIR` here means saying that once rather than also having
# to point `AURORA_TEST_EXE` at the result:
#
#     CARGO_TARGET_DIR=C:/aurora-target cargo build --release -p aurora-app #         --manifest-path src-native/Cargo.toml
#     CARGO_TARGET_DIR=C:/aurora-target python tests-host/run.py
#
# A relative value is resolved against the current directory, which is what cargo does
# with it too, and both commands above are documented as run from the repository root.
TARGET_DIR = os.path.abspath(
    os.environ.get("CARGO_TARGET_DIR") or os.path.join(ROOT, "src-native", "target")
)

# `AURORA_TEST_EXE=…/target/debug/aurora-app` runs the same scenarios against a debug
# build. Worth having for one reason: a debug build unwinds where a release build
# aborts, so a panic that kills the shipped app can be inspected in a live window here.
EXE = os.environ.get(
    "AURORA_TEST_EXE",
    os.path.join(
        TARGET_DIR, "release",
        "aurora-app.exe" if WINDOWS else "aurora-app",
    ),
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

# What `tauri-driver` shells out to. On Linux that is WebKitWebDriver, which apt puts on
# PATH and the shim finds by itself. On Windows it is msedgedriver, whose build has to
# match the WebView2 runtime the app loads — so it is named rather than guessed, and
# `AURORA_MSEDGEDRIVER` points at a copy that is not on PATH.
NATIVE_DRIVER = os.environ.get("AURORA_MSEDGEDRIVER", "msedgedriver")

# How many times a scenario may be restarted after the WebDriver connection dies.
#
# Two, because this happens: WebKit under Xvfb holding a 140,000-row library and a
# page of remote posters is not robust. Narrow, though — only for transport errors,
# and only when the app's own log shows no panic. A host that died is what this suite
# exists to catch and must never be retried away.
DRIVER_RETRIES = 2


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

    def rows(self, sql, args=()):
        """Read from the library directly.

        For the things the IPC deliberately does not expose — a film's provider
        category, say, which the host uses for recommendations and the UI has no
        business knowing about. Asserting through the UI where the UI is the thing
        under test is how the mock/host gap opened in the first place; this is the
        other direction, and it is the one that checks the work.
        """
        con = sqlite3.connect(f"file:{self.db_path()}?mode=ro", uri=True)
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


# What `keyring` files a secret under on Windows: the target name is `{user}.{service}`,
# and the service is fixed by `aurora-ingest::credentials`.
CREDENTIAL_SERVICE = "AuroraTV"


def reclaim_credentials(path):
    """Delete the credential entries belonging to the library at `path`.

    The OS credential store is the one piece of state a scenario touches that `wipe` does
    not reach: it is per-user and, on Windows, shared by every application the account
    runs. Deleting the data directory therefore leaves each run's secret behind in
    Credential Manager with nothing left that references it.

    It must not be tidied up by pattern. That mistake has already been made once here —
    keys used to be `aurora-provider-{row id}`, every library's first provider was
    `aurora-provider-1`, and this harness quietly destroyed the password of the copy of
    Aurora the machine's owner actually uses. So the keys are **read out of the library
    that is about to be destroyed**, which is the only way to be certain every one of
    them was created by this suite.

    Best effort throughout: there is no state here worth failing a run over, and a
    library that was never created has nothing to hand back.
    """
    db = os.path.join(path, "library.db")
    if not os.path.exists(db):
        return
    try:
        con = sqlite3.connect(f"file:{db.replace(os.sep, '/')}?mode=ro", uri=True)
    except sqlite3.Error:
        return
    try:
        keys = [
            row[0]
            for row in con.execute(
                "SELECT credential_ref FROM providers WHERE credential_ref IS NOT NULL")
        ]
    except sqlite3.Error:
        keys = []
    finally:
        con.close()

    for key in keys:
        # A row can only name an un-namespaced key if this library predates namespacing,
        # which a library this harness just made cannot. Refuse it rather than risk it.
        if not key.startswith("aurora-") or key.count("-") < 3:
            print(f"   leaving {key!r} alone: not a key this suite would have made")
            continue
        target = f"{key}.{CREDENTIAL_SERVICE}"
        if WINDOWS:
            subprocess.run(
                ["cmdkey", f"/delete:{target}"],
                check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        else:
            subprocess.run(
                ["secret-tool", "clear", "service", CREDENTIAL_SERVICE, "username", key],
                check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def wipe(path):
    """Empty a directory, and be sure it is actually gone.

    `rmtree(..., ignore_errors=True)` is a trap on Windows: a file another process still
    holds open cannot be deleted, and ignoring that hands the next scenario the
    *previous* scenario's library instead of the empty one it means to test — which is
    the one state that only happens once. So retry briefly, because an app that has just
    been told to quit is on its way out, and say so if it never lets go.
    """
    for _ in range(120):
        shutil.rmtree(path, ignore_errors=True)
        if not os.path.exists(path):
            return
        time.sleep(0.25)
    raise RuntimeError(
        f"{path} could not be emptied after 30s. Either an earlier aurora-app is still "
        "running, or something else holds a handle on it — a tree under OneDrive or a "
        "real-time virus scanner will both do this to a directory that was written a "
        "moment ago, and neither lets go on demand. A build tree does not belong in a "
        "synced folder."
    )


def kill_strays():
    """Kill a driver, or the app, left behind by a session that died mid-scenario.

    The app matters more on Windows than on Linux: an exe that is still running holds
    `data\\library.db` open, so the next scenario cannot be given the empty library it
    is testing (see `wipe`). It is killed **by path**, not by name — the person running
    this may well have their own installed copy of Aurora open, and a harness that
    closes somebody's television is not an acceptable way to free a port.
    """
    for name in ("tauri-driver", "WebKitWebDriver", "msedgedriver"):
        if WINDOWS:
            subprocess.run(
                ["taskkill", "/F", "/IM", f"{name}.exe"],
                check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        else:
            subprocess.run(["pkill", "-x", name], check=False)
    if WINDOWS:
        subprocess.run(
            ["powershell", "-NoProfile", "-Command",
             "Get-Process -ErrorAction SilentlyContinue"
             " | Where-Object { $_.Path -eq $env:AURORA_KILL_PATH }"
             " | Stop-Process -Force"],
            check=False, env=dict(os.environ, AURORA_KILL_PATH=os.path.abspath(EXE)),
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    else:
        subprocess.run(["pkill", "-f", EXE], check=False)


def driver_cmd():
    cmd = ["tauri-driver", "--port", str(DRIVER_PORT)]
    if WINDOWS:
        cmd += ["--native-driver", NATIVE_DRIVER]
    return cmd


def driver_env(github):
    env = dict(os.environ)
    if not WINDOWS:
        env["DISPLAY"] = DISPLAY
    # Loopback only, which is the only thing the app will accept (see `test_api_base`).
    env["AURORA_UPDATE_API"] = github.url
    return env


def restart_driver(procs, stderr_path, github):
    """Bring `tauri-driver` back up, leaving Xvfb and the fixtures alone."""
    for p in procs:
        if "tauri-driver" in " ".join(p.args):
            try:
                p.send_signal(signal.SIGTERM)
                p.wait(timeout=10)
            except Exception:
                pass
    procs = [p for p in procs if "tauri-driver" not in " ".join(p.args)]
    kill_strays()
    time.sleep(2)

    log = open(stderr_path, "a")
    procs.append(subprocess.Popen(
        driver_cmd(), env=driver_env(github), stdout=log, stderr=subprocess.STDOUT))
    time.sleep(3)
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
    """Xvfb, the fixture provider, and tauri-driver. Returns them for teardown."""
    procs = []
    devnull = subprocess.DEVNULL
    # Windows has a desktop already; Xvfb is how Linux gets one.
    if not WINDOWS:
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

    stderr_path = os.path.join(SHOTS, "host-stderr.log")
    log = open(stderr_path, "w")
    procs.append(subprocess.Popen(
        driver_cmd(), env=driver_env(github), stdout=log, stderr=subprocess.STDOUT))
    time.sleep(3)
    return procs, stderr_path, github


def main():
    if not os.path.exists(EXE):
        sys.exit(
            f"build it first:  cargo build --release -p aurora-app\n"
            f"(nothing at {EXE})"
        )
    print(f"exe: {EXE}")
    needed = ("tauri-driver", NATIVE_DRIVER) if WINDOWS else (
        "Xvfb", "WebKitWebDriver", "tauri-driver")
    for tool in needed:
        if not (shutil.which(tool) or os.path.isfile(tool)):
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

    kill_strays()
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
            # happens once, and it is the one that has never been tested. Its secrets go
            # back before it does — they outlive the directory otherwise.
            reclaim_credentials(DATA)
            wipe(DATA)
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
                    reclaim_credentials(DATA)
                    wipe(DATA)
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
                p.send_signal(signal.SIGTERM)
            except Exception:
                pass
        # `tauri-driver` spawns the native driver, which spawns the app. Killing the
        # shim on Windows does not take its children with it, and a stray driver holds
        # port 4444 against the next run.
        if WINDOWS:
            kill_strays()

    print()
    ran = len(names) - len(skipped)
    print(f"{ran - len(failures)}/{ran} passed" + (f", {len(skipped)} skipped" if skipped else ""))
    for name, err in failures:
        print(f"  {name}: {err}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
