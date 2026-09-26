# Running the harness on Windows

`tests-host/` runs on Linux today and covers everything up to the point where a picture
would appear. It stops there for one reason: `aurora-player` is `NullBackend` off
Windows, so nothing in this repository has ever executed a line of `mpv.rs`. The Win32
child surface, its `HWND_BOTTOM` ordering under a transparent WebView2, hardware
decoding, and the viewer's original report — *audio plays, no video* — are all on the
other side of that line.

This is how to get the same nine scenarios running where those questions can be
answered.

> **What here has been run, and what has not.** `bootstrap.ps1` was executed
> end to end under PowerShell 7 on Linux, which covers everything in it that is not
> Windows-specific: it fetched the v0.11.0 portable zip, checked it against the digest
> GitHub published, unpacked it — `libmpv-2.dll` is there, 120 MB of it — read the
> WebView2 version (correctly finding none, and falling back), and fetched a matching
> `msedgedriver.exe`, which `file` confirms is a PE32+ Windows binary. That run found
> two real bugs, both fixed: the Edge WebDriver version endpoint answers in UTF-16
> bytes rather than text, which produced a URL the CDN answered with `BlobNotFound`.
>
> What has *not* been run is any of it on Windows: the registry lookup, `tauri-driver`
> against WebView2, and the scenarios themselves. Expect something here to be wrong,
> and correct the file when it is.

## What is different

| | Linux | Windows |
| --- | --- | --- |
| Display | Xvfb on `:99` | the real desktop — see below |
| WebView | WebKitGTK | WebView2, which is what ships |
| WebDriver | `WebKitWebDriver` | Microsoft Edge WebDriver (`msedgedriver.exe`) |
| Playback | `NullBackend` | **libmpv, for real** |
| Stopping a stray process | `pkill -x` | `taskkill /F /T /IM` |
| Deleting the library between scenarios | always works | only once the app has exited |

The last row is the one that would have gone wrong quietly. `shutil.rmtree(...,
ignore_errors=True)` is a silent no-op against a file another process still holds open,
and on Windows that is every file the app has until it exits. The wipe would have been
skipped, the next scenario would have inherited the last one's library, and
"an empty database is a state that only happens once" would have stopped being tested
without a single failure to show for it. `wipe_data()` now retries, kills the app
halfway through, and raises if the folder is still there.

The other one: `file:C:\Users\…?mode=ro` is not a URI. A URI cannot carry backslashes,
SQLite reads what survives as a relative path, and every `count()` in every scenario
would fail with "unable to open database file" — making the harness look broken rather
than the app. `read_only()` goes through `pathlib.Path.as_uri()` instead.

## The trap that costs an afternoon

**The harness has to run in an interactive desktop session.** A GUI application needs a
window station and a desktop to create a window in, and WebView2 needs one to start at
all. A command run over SSH, from a service, or from a scheduled task under
"run whether user is logged on or not" does not have one — the app will fail to start,
or start invisibly, and the errors point everywhere except the actual cause.

So: connect over RDP, or sit at the console, and run the harness from that session. If
the machine is headless and only reachable over SSH, arrange an autologon console
session and run it there.

## Getting the pieces

`tests-host/bootstrap.ps1` does all of this. Run it from an ordinary PowerShell prompt
in the repository root:

```powershell
powershell -ExecutionPolicy Bypass -File tests-host\bootstrap.ps1
```

What it does, and what to do by hand if it fails:

1. **A build to test.** It downloads the portable zip from the latest release,
   checks it against the SHA-256 GitHub published for it, and unpacks it to
   `.windows\app\`. That is deliberately the *shipped artifact* rather than a local
   build: it carries `libmpv-2.dll` beside the exe, which is the thing under test, and
   it means nothing here needs an MSVC toolchain or the libmpv import library CI
   generates with `lib /def:`. Building locally is [documented in
   `BUILDING.md`](BUILDING.md) and is the slower path.

   The zip already carries its own `portable.txt`, so that copy keeps its library and
   its log in `.windows\app\data\` whether the harness runs it or you do.

2. **Microsoft Edge WebDriver**, matching the installed WebView2 runtime. The version
   has to match — a mismatched driver refuses the session with a message that names
   both versions, which is at least an honest failure. The script reads the runtime's
   version out of the registry and fetches that exact build.

3. **`tauri-driver`**, via `cargo install tauri-driver --locked`. This is the only
   piece that needs Rust. If `rustup` is not installed the script says so rather than
   installing a toolchain behind your back; `stable-x86_64-pc-windows-gnu` is enough
   for this and does not need Visual Studio Build Tools.

4. **Python 3.** The harness imports nothing outside the standard library, so any
   3.8-or-later interpreter will do.

## Running it

```powershell
$env:AURORA_TEST_EXE = "$PWD\.windows\app\aurora-app.exe"

# Only if msedgedriver.exe is not on PATH:
$env:AURORA_NATIVE_DRIVER = "$PWD\.windows\msedgedriver.exe"

# The real panel, for the scenarios that need one. Typed straight into the window;
# never written to a file, an argument or a log.
$env:AURORA_PANEL_URL  = "https://…"
$env:AURORA_PANEL_USER = "…"
$env:AURORA_PANEL_PASS = "…"

python tests-host\run.py                 # all nine
python tests-host\run.py player_controls # one of them
```

Screenshots and logs land in `screenshots\host\`, which is gitignored — the shots are
of a real subscription.

`run.py` writes `portable.txt` beside whatever `AURORA_TEST_EXE` names, which puts
`library.db` and `aurora.log` in a `data\` folder next to it where a scenario can read
them. Delete that marker if you later want to run the same copy normally.

## What to look at that Linux cannot show

The scenarios assert what they can, but the compositing question is a visual one, and
the screenshots are the evidence. In descending order of what it would mean to get a
wrong answer:

0. **Did libmpv load at all?** `starts_up` now asserts this outright and fails on
   Windows if the engine is the null one, so a whole suite of green playback scenarios
   can no longer be hiding the fact that nothing was ever going to render. Settings →
   Diagnostics shows the same fact to a person, and the log says it on every launch.

1. **Is there a picture behind the interface?** `screenshots\host\player_controls-*.png`.
   Three outcomes and three different meanings — `docs/TESTING_A_BUILD.md` sets them
   out. With (0) answered, a black rectangle means the compositing and nothing else.
2. **Does the UI float over it, and still take clicks?** The WebView2 is configured
   transparent so the React UI paints above the video window. If video covers the UI,
   the z-order is inverted; if clicks land on video instead of buttons, hit-testing is.
3. **Does the video window follow the frame?** Resize and snap the window. `resize()`
   keeps the child on the client rect from the host's `WM_SIZE`; the two tearing apart
   during a drag is the visible symptom.
4. **Does `player.stop` actually stop it?** This is the half of the viewer's report
   that the dead-event fix explains — but it was verified against `NullBackend`, where
   "stopped" is a boolean and not a sound. Close the player and listen.
5. **What `hwdec-current` says.** The stats overlay reports it. `d3d11va-copy` means
   hardware decoding took; `no` means everything is on the CPU and a 4K stream will
   say so.

## When something goes wrong

- **`session not created: This version of Microsoft Edge WebDriver only supports…`** —
  the driver does not match the runtime. Re-run the bootstrap, or fetch the build it
  names.
- **The app never appears and the driver times out** — almost always the session
  problem above. Check you are in an interactive desktop session, not SSH.
- **`could not clear the scenario's library`** — an `aurora-app.exe` outlived its
  WebDriver session and still holds `library.db`. The message lists what is locked;
  `taskkill /F /IM aurora-app.exe` clears it. Worth reporting if it happens routinely,
  because it means the app is not exiting when the session ends.
- **Everything passes and there is still no picture.** That is the most useful
  possible outcome and exactly why this exists. Keep `screenshots\host\` and
  `aurora.log`.
