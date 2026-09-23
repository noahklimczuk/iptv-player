# Trying a Windows build

The quickest route is **Releases → `latest-windows`**: a single `.exe` installer,
rebuilt on every merge to main, so the link never changes.

That run also attaches the raw builds under **Actions → the merge's run →
Artifacts**:

| Artifact | What it is |
|---|---|
| `aurora-tv-portable` | A zip. Unzip anywhere, run `aurora-app.exe`. Keeps all its state in a `data\` folder beside itself and touches nothing else. |
| `aurora-tv-installers` | The NSIS `.exe` and the MSI. Installs to Program Files and writes to `%LOCALAPPDATA%`. |

Use the portable one unless you are specifically testing the installer. It is a single
folder you can delete, and its log is right there next to the exe. It cannot be reduced
to a single file — libmpv ships as a DLL that has to sit beside the exe, which is what
the installer exists to do for you.

## The question this build exists to answer

Aurora renders video into a child window *behind* the WebView2 that draws the UI
(`docs/DECISIONS.md` D1). That has never been run on real hardware. Everything about
playback depends on it, so the first launch is really one question:

**Tune a channel. Is there video behind the interface?**

Three outcomes, and they mean different things:

- **Video plays, UI floats over it.** The architecture holds. Everything else is
  ordinary bug-fixing from here.
- **Black rectangle, UI fine.** Either libmpv did not load or the compositing does not
  work. The log tells you which — see below.
- **Video plays but covers the UI, or the UI is opaque where video should be.** The
  layering is wrong. That is a fixable bug, not a dead end.

## Where the log is

Release builds have no console, so the log is a file:

- Portable: `data\aurora.log` beside the exe.
- Installed: `%LOCALAPPDATA%\<app folder>\aurora.log`.

It is truncated on every launch, so it describes the run that just happened.

For more detail, set `AURORA_LOG` before starting:

```
set AURORA_LOG=debug
aurora-app.exe
```

The line that matters most for a black rectangle:

```
libmpv unavailable, falling back to null backend: ...
```

If that appears, the video layer never started, and the cause is on the same line —
usually `libmpv-2.dll` missing from the folder. If it does *not* appear, libmpv loaded
and the problem is the compositing.

## Pointing it at a real provider

The first-run wizard takes an Xtream panel (host, username, password) or an M3U URL.
Nothing is sent anywhere except your provider: the password goes to Windows Credential
Manager, never to the database, never to a log, never to an export.

If you would rather look before importing, there is a read-only probe that reports what
a provider actually serves without writing anything:

```
set AURORA_BASE=http://your-panel.example
set AURORA_USER=yourname
set AURORA_PASS=yourpassword
cargo run -p aurora-ingest --example probe
```

It prints account status, catalogue sizes, EPG coverage, and what the title and language
heuristics make of real names. That last part is the interesting one — the language
detector and the catch-up URL builder are both written from documented conventions and
have never met a real panel.

## What is worth reporting back

In rough order of value:

1. Whether video appeared, and the log if it did not.
2. Whether the import finished, how long it took, and how many channels, films and shows
   landed.
3. Anything in the library that looks wrong: titles that should have collapsed and did
   not, content hidden that should not have been, a guide that is empty where it should
   not be.
4. Zap time — pressing a channel to the first frame.
