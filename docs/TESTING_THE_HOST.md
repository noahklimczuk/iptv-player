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
npm run build                                      # the UI is embedded at compile time
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

### Why the release build

Two reasons, and both matter.

`panic = "abort"` is set for release. A panic that unwinds harmlessly in a debug build
takes the whole application down in the build people actually run, so a debug run can
pass while the shipped binary dies.

And only a release build writes a log this harness can read. `init_logging` gives a
debug build the console it already has — but `tauri-driver` launches the app itself and
that console goes nowhere. A release build logs to `aurora.log` beside its data, so
`portable.txt` beside the executable puts both the log and `library.db` in
`src-native/target/release/data`, where a scenario can open them. `run.py` writes that
marker itself.

## What it cannot show

libmpv. `aurora-player` falls back to `NullBackend` off Windows, so video compositing,
the Win32 child surface and the `HWND_BOTTOM` ordering stay Windows-only questions.
The harness covers everything up to the point where a picture would appear — which, as
it turns out, is where most of the bugs were.

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
