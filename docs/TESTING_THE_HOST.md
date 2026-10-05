# Testing the host

Everything in this repository that calls itself end-to-end runs the UI against the
mock transport in `src-ui/src/ipc/mock.ts`. That is deliberate — it is what makes the
screens developable without Windows — and it is also the single largest source of bugs
this project has shipped:

| Bug | What the browser showed | What a real machine did |
| --- | --- | --- |
| F-09 | Favourites worked | The host never returned any |
| F-12 | Provider logos loaded | The CSP blocked every one of them |
| `progress.save` | Progress was saved | Only the mock implemented it |
| Continue Watching | A rail with resume positions | The host had no rail to build |

The pattern is the same every time: **the thing under test answered its own
questions**. A mock that is written from the same understanding as the UI agrees with
the UI, and a real host disagrees in exactly the places where the understanding was
wrong.

`tests-host/` closes that gap. It runs the actual Tauri binary under Xvfb against a
real SQLite file and drives it over WebDriver. The transport is the real IPC, the
commands are the real Rust, and the assertions are about what is on disk afterwards.

## Running it

```sh
pnpm run build                                     # the UI is embedded at compile time
cargo build --release -p aurora-app --manifest-path src-native/Cargo.toml
python3 tests-host/run.py                          # all scenarios
python3 tests-host/run.py starts_up                # one of them
```

Screenshots and logs from each run land in `screenshots/host/`.

### What it needs

```sh
apt-get install -y xvfb webkit2gtk-driver          # Xvfb + WebKitWebDriver
cargo install tauri-driver --locked                # the Tauri WebDriver shim
```

### On Windows

The same harness runs on Windows, which is where it can finally see the half described
under *What it cannot show* below. Three things differ and `run.py` handles all of them:
there is already a desktop so no Xvfb is started, `tauri-driver` is pointed at
msedgedriver rather than WebKitWebDriver, and a file that is still open cannot be
deleted — so the per-scenario wipe retries and then says so, instead of silently handing
the next scenario the previous one's library.

```powershell
cargo install tauri-driver --locked
# msedgedriver must match the WebView2 *runtime*, which is not the same build as the
# installed Edge browser — on 2026-10-03 Edge was 154.0.4258.48 while the runtime was
# 154.0.4258.53, and only the latter matters. The runtime registers its version under
# EdgeUpdate with a fixed GUID, which is the authoritative answer:
$guid = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
$pv = (Get-ItemProperty "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$guid").pv
#   https://msedgedriver.microsoft.com/$pv/edgedriver_win64.zip
$env:AURORA_MSEDGEDRIVER = "C:\tools\msedgedriver.exe"   # unless it is on PATH
python tests-host/run.py
```

#### Where the build tree goes, which is not optional on a synced checkout

A portable copy keeps its library beside the executable, so the harness's data directory
is `<target>/release/data` and the per-scenario wipe has to be able to delete it. **Under
OneDrive it cannot.** Cloud placeholders deny `rmdir` on a directory written moments
earlier, so the wipe exhausts its retries and the run stops. Measured on this project's
own checkout:

```
OneDrive tree : FAILED after 30.5s
external tree : wiped ok in 0.00s
```

So build outside the synced tree. `run.py` honours `CARGO_TARGET_DIR`, which means saying
that once rather than also pointing `AURORA_TEST_EXE` at the result:

```powershell
$env:CARGO_TARGET_DIR = "C:\aurora-target"
cargo build --release -p aurora-app --manifest-path src-native/Cargo.toml
python tests-host/run.py
```

It is worth doing for its own sake too: every build artefact under `target/` is otherwise
being uploaded.

`libmpv-2.dll` has to be beside `aurora-app.exe`, and **not in the way that
`create_backend` suggests**: libmpv is a load-time import, so without the DLL the Windows
loader ends the process with `0xC0000135` before `main` runs. No window, no log, no
message — the `NullBackend` fallback never gets the chance to happen (F-34). If a run
produces nothing at all, check that the DLL is there before looking anywhere else.

### Why the release build

Two reasons, and both matter.

`panic = "abort"` is set for release. A panic that unwinds harmlessly in a debug build
takes the whole application down in the build people actually run, so a debug run can
pass while the shipped binary dies.

And only a release build writes a log this harness can read. `init_logging` gives a
debug build the console it already has — but `tauri-driver` launches the app itself and
that console goes nowhere. A release build logs to `aurora.log` beside its data, so
`portable.txt` beside the executable puts both the log and `library.db` in
`<target>/release/data` — `src-native/target/release/data` unless `CARGO_TARGET_DIR`
says otherwise — where a scenario can open them. `run.py` writes that marker itself.

## What it cannot show, off Windows

libmpv — on Linux. `aurora-player` falls back to `NullBackend` here, so video
compositing, the Win32 child surface and the `HWND_BOTTOM` ordering stay Windows-only
questions. This covers everything up to the point where a picture would appear —
which, as it turns out, is where most of the bugs were.

The same nine scenarios run on Windows, where the backend is the real one and a
picture either appears or does not. `docs/TESTING_ON_WINDOWS.md` is how to get there;
`tests-host/bootstrap.ps1` does the setting up.

**On Windows it shows them too.** `tests-host/scenarios/video_surface.py` answers the
three questions in `AUDIT/release-checklist.md` item 1 without a person looking at the
screen, because each is a fact about the machine rather than about a screenshot — and a
WebDriver capture is no use for any of them, since it photographs a transparent WebView
over nothing at all. `tests-host/winprobe.py` asks the desktop instead:

| Question | How it is answered |
| --- | --- |
| Video renders *behind* the UI | One top-level window of any size for the process, with mpv's `STATIC` surface a `WS_CHILD` of it and last in z-order |
| Frames actually reach the screen | Two desktop captures 1.2s apart, compared over the middle of the window — measured at 95% of a sampled grid moving |
| The UI still takes input over it | A click on the OSD that changes what is on screen |
| The surfaces stay together | The child's rectangle after the window is resized |
| Anything is being decoded at all | mpv's own `resolution`, `videoCodec`, `fps` and `hwDecoder`, read back through `player_state` |

It needs a playlist to play, which this repository does not carry — every fixture host
in the tree is `example.com` or a loopback address, and a public test stream is neither.
So it skips unless it is given one:

```powershell
$env:AURORA_TEST_STREAM_M3U = "http://127.0.0.1:8123/streams.m3u"
python tests-host/run.py video_surface
```

The captures land in `screenshots/host/` as PNGs, which is worth looking at even when it
passes: a frame of the stream, with no OSD over it, is the whole of Phase 0 in one file.

### And it is how multi-view's commands are checked at all

`tests-host/scenarios/multiview.py` asks the real host for every `mosaic.*` command with
no provider configured. That is not a formality: eleven commands were added to
`shared/ipc.ts` at once, the mock answers all of them, and **that is the exact shape of
F-04** — eight commands were once declared and never registered, so the browser preview
looked complete and the shipped app answered "command not found". `contract.rs` pins the
names now; this checks that the things behind them answer, including that an unknown
layout is refused by name and that a command needing an open mosaic says so rather than
panicking a release build.

It asserts the budget comes back **`unknown`** for every layout, because no provider has
declared a limit — the host claiming `fits` there would be claiming to know something it
cannot (docs/DECISIONS.md D27).

Note that `d.invoke` takes the *host* spelling: `mosaic_check`, not `mosaic.check`. The
driver calls `__TAURI_INTERNALS__.invoke` directly, so the translation
`src-ui/src/ipc/index.ts` does is not in the way.

## What driving it for real found immediately

The first time the first-run wizard was driven against a real host rather than a mock,
a **debug** build panicked on "Check connection" and left the button on "Checking…"
forever:

```
thread 'tokio-rt-worker' panicked at tokio-1.53.1/src/runtime/blocking/shutdown.rs:51:
Cannot drop a runtime in a context where blocking is not allowed.
   7: reqwest::blocking::wait::enter          reqwest-0.12.28/src/blocking/wait.rs:80
   8: reqwest::blocking::wait::timeout
  12: aurora_ingest::http::HttpClient::send_with_retry   http.rs:174
  14: aurora_ingest::playlist::fetch                     playlist.rs:19
  15: aurora_app::providers::providers_validate          providers.rs:150
```

Frame 7 is the whole story. `reqwest::blocking::wait::enter` is this, and nothing else:

```rust
fn enter() {
    // Check we aren't already in a runtime
    #[cfg(debug_assertions)]
    {
        let _enter = tokio::runtime::Builder::new_current_thread()
            .build().expect("build shell runtime").enter();
    }
}
```

It is reqwest's own detector for exactly one mistake — calling the blocking client from
inside an async runtime — and it exists only in a debug build. **So a release build does
not panic**: the same scenario against `target/release` imports the fixture and reaches
"Your library is ready". The guard is compiled out; the misuse it was detecting is not.

What is left in release is a Tauri async worker parked for the whole length of the
request. Every command in this app is `#[tauri::command(async)]`, which runs it on the
shared Tokio runtime, and four of them do blocking HTTP on that thread:

| Command | How long it parks a worker |
| --- | --- |
| `providers_validate` | one fetch, up to 12s connect + 120s read, ×3 attempts |
| `providers_refresh` | a whole sync — 23.7s measured on a real subscription |
| `updates_download` | the length of a release download |
| `artwork_prefetch` | one TMDB fetch per poster |

The runtime has one worker per core, so this is contention rather than a freeze — the
WebView is on the main thread and keeps drawing. But it is the wrong pool, it is
`artwork_prefetch` away from exhausting itself, and in development it is simply a
crash.

### The harness turns that detector on

Because the check lives behind `debug_assertions`, running the scenarios against a
debug build makes reqwest fail the run for any blocking call that reaches an async
worker:

```sh
cargo build -p aurora-app --manifest-path src-native/Cargo.toml
AURORA_TEST_EXE=$PWD/src-native/target/debug/aurora-app python3 tests-host/run.py
```

That is worth doing before a release even though the shipped build is the release one.
A green release run says the app works; a green debug run says it works for the right
reason.

## Adding a scenario

Drop a `run(d, ctx)` into `tests-host/scenarios/`. `d` is the WebDriver client from
`driver.py` (`find`, `by_text`, `click`, `send`, `body`, `wait_body`, `shot`); `ctx` is
the machine outside the window (`schema_version`, `channel_count`, `count`,
`assert_no_panic`, `log`, `shot`).

Assert on `ctx` wherever you can. "The screen says 3 channels" is a claim about React;
"`SELECT count(*) FROM channels` is 3" is a claim about the thing that was broken.
