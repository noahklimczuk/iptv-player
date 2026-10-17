# Release page copy

What goes on a GitHub release, and the two things that have to change if it ever goes
anywhere else. Written down because the awkward parts are easy to forget between releases
and expensive to get wrong once.

---

## The template

> ## Aurora TV `<version>`
>
> A television for an IPTV subscription: live channels with a proper guide, films and
> series from the same provider, recordings, and multi-view. Windows 10/11, x64.
>
> ### Install
>
> **`Aurora-TV-<version>-x64-setup.exe`** — installs for the current user, no
> administrator needed, and updates itself from here afterwards.
>
> **`aurora-tv-<version>-portable.zip`** — unzip and run. Keeps the library, settings and
> recordings beside the executable, so it leaves nothing on the machine.
>
> Your library and settings survive an upgrade. Both kinds of copy update in place.
>
> ### What's new
>
> `<the changes, as sentences about what is different for somebody using it — not commit
> subjects>`
>
> ### Known
>
> - **Windows SmartScreen will warn you.** Nothing here is code-signed. Certificates cost
>   money and this is one person's app; the installer is built by GitHub Actions from the
>   tagged commit, and the updater checks the SHA-256 published here before it swaps
>   anything.
> - **Catch-up does not work** on any subscription tested. Two panels advertise it and
>   neither serves it; the timezone handling cannot be fixed against evidence that does
>   not exist.
> - **x64 only.** There is no ARM64 build of libmpv to link against.
>
> ### Privacy
>
> No analytics, no telemetry, no account, no server belonging to this project. Aurora
> talks to your provider, to Google for the assistant and the recommendation rail, to
> TMDB for artwork, and to GitHub to check for updates. **Your watch history is sent to
> Google by default** — see [PRIVACY.md](docs/PRIVACY.md), which says exactly what goes
> where and how to turn it off.
>
> Your provider's password is kept in Windows Credential Manager, and never appears in a
> log. It is not yet safe to send anybody your `library.db`, though — Xtream puts the
> credentials inside every stream URL and the library stores those as they come. See
> [PRIVACY.md](docs/PRIVACY.md).

---

## Before tagging

Checklist item 0: run `ipc_alive` and `starts_up` against the binary being shipped, not a
local build. Two releases have gone out that drew a window and then answered nothing.

## If it ever goes to the Microsoft Store

Two things change, and the first one is not optional:

1. **The auto-updater must be off in a Store build.** The Store owns updating, and two
   mechanisms fighting over the same executable is worse than either — one of them
   swapping files under the other is how an install ends up half of each. `can_install()`
   already refuses for portable copies; a Store build needs the same refusal, keyed on
   however that build identifies itself. The update *check* can stay if it only points at
   the Store listing.
2. **The privacy declaration is a form, not a paragraph.** The Store asks what is
   collected and where it goes, and the answer must match `docs/PRIVACY.md` — including
   the Google one, which is the only answer on that form that is not "nothing".

Neither is written yet, because nothing is going to the Store yet.

## Tone

The sentences people actually read are "what's new" and "known". Both are about what is
different for somebody using the app. A release note that lists commit subjects is a
changelog, and a changelog is for whoever is reading the code.
