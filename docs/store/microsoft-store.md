# Microsoft Store listing — Aurora TV 2.0

Everything Partner Center asks for, ready to paste (`docs/RELEASE_2.0.md` §5). Fields
are in the order the submission pages show them. Limits are Partner Center's.

---

## Product setup

| Field | Value |
|---|---|
| Name to reserve | **Aurora TV** (fallback: *Aurora TV Player*) |
| Category | Entertainment |
| Subcategory | — |
| Pricing | Free |
| Markets | All, except where the owner chooses otherwise |
| Package | the `aurora-tv-msix` artifact of the release run being submitted (x64) |

After reserving the name, copy *Product identity* → **Package/Identity/Name**,
**Package/Identity/Publisher** and **Package/Properties/PublisherDisplayName** into the
repository variables `STORE_IDENTITY_NAME`, `STORE_PUBLISHER` and
`STORE_PUBLISHER_DISPLAY_NAME`. The next merge builds an MSIX Partner Center accepts.

## Restricted capability justification

Asked for because the manifest declares `runFullTrust`:

> Aurora TV is a desktop media player built on WebView2 and libmpv. Video is decoded by
> libmpv into a native child window composited behind the web UI, provider passwords are
> stored in Windows Credential Manager, and recordings are written to a folder the user
> chooses. These need a full-trust Win32 process; the app has no UWP equivalent.

## Store listing (English)

**Short title:** Aurora TV

**Short description** (≤ 100 characters, shown in search results)

> Live TV, a full guide, films and series from your own IPTV subscription — in one fast app.

**Description** (≤ 10,000 characters)

> Aurora TV turns the IPTV subscription you already have into a living-room TV
> experience on Windows: channel surfing that feels like cable, a full programme guide,
> and films and series laid out like a streaming service.
>
> **Aurora does not provide any channels, films or series.** You add your own provider —
> an Xtream Codes login or an M3U playlist, with an XMLTV guide — and Aurora plays what
> that subscription includes.
>
> LIVE TV
> • Zap with the arrow keys or type a channel number, with a banner showing what is on now
>   and next
> • A full-screen guide with search and reminders
> • Pause and rewind live TV
> • Multi-view: watch up to nine channels at once, with audio on the one you pick
> • Picture-in-picture while you browse
>
> FILMS AND SERIES
> • Artwork, descriptions and ratings, with Continue Watching and Up Next
> • Skip intro and credits
> • Recommendations based on what you watch, worked out on your PC
>
> RECORDINGS
> • Record a programme or a whole series from the guide, with rules and a check that you
>   stay within your provider's connection limit
>
> FOR THE WHOLE HOUSEHOLD
> • Profiles with PINs, kids profiles and parental controls by rating, channel and
>   category
> • Optional free Aurora account to keep favourites, lists, watch progress and profiles
>   the same on every device — end-to-end encrypted with a passphrase only your devices
>   know
>
> MADE FOR A TV OR A DESK
> • A ten-foot TV mode, keyboard shortcuts for everything, light and dark themes
> • Hardware-accelerated playback of H.264, HEVC, AV1 and more, with HDR tone-mapped for
>   SDR screens
>
> PRIVATE BY DESIGN
> No ads, no analytics, no tracking. Your provider password stays in Windows Credential
> Manager. Aurora is open source under the GPL.

**What's new in this version**

> Aurora TV 2.0 is the first release on the Microsoft Store. New: an optional Aurora
> account to sync favourites, lists, progress and profiles between your devices,
> end-to-end encrypted.

**Product features** (≤ 20, each ≤ 200 characters)

1. Plays your own IPTV subscription: Xtream Codes logins and M3U playlists with XMLTV guides
2. Cable-style channel surfing with number entry and a now-and-next banner
3. Full-screen programme guide with search and reminders
4. Pause and rewind live TV
5. Multi-view: up to nine channels at once
6. Picture-in-picture while you browse
7. Films and series with artwork, Continue Watching and Up Next
8. Skip intro and skip credits
9. Record programmes and series from the guide
10. Profiles, kids profiles and parental controls
11. Optional end-to-end encrypted sync between your devices
12. TV mode for a ten-foot screen, keyboard shortcuts throughout
13. Hardware-accelerated playback of H.264, HEVC and AV1, with HDR tone-mapping
14. No ads, no analytics, open source

**Search terms** (≤ 7)

IPTV · IPTV player · M3U · Xtream · EPG · live TV · TV guide

**Screenshots** (1920×1080 PNG, at least one, up to ten). From `screenshots/`, which the
end-to-end journeys write against the mock library — no real broadcaster's name or logo
appears in it. Use only the files **without** a name suffix (the `-Noah` files come from a
real profile):

1. `01-home.png` — home
2. `04-guide.png` — guide
3. `06-live.png` — live TV
4. `08-player-banner.png` — the zap banner
5. `11-movies.png` — films
6. `14-tv-mode.png` — TV mode
7. `20-recordings.png` — recordings
8. `22-profile-picker.png` — profiles
9. `60-sync-signed-in.png` — sync

Check each at full size before uploading; re-run `pnpm e2e` to refresh them.

**Store logos:** Partner Center takes the package's own (`aurora-app/msix/Assets`). A
1:1 "box art" of 1080×1080 or 2160×2160 is optional; `tools/make_icons.py` can draw one.

**Additional information**

| Field | Value |
|---|---|
| Privacy policy URL | https://github.com/noahklimczuk/iptv-player/blob/main/docs/PRIVACY.md |
| Website | https://github.com/noahklimczuk/iptv-player |
| Support contact | https://github.com/noahklimczuk/iptv-player/issues |
| Copyright | © 2026 the Aurora TV contributors. GPL-3.0-or-later. |

## Age rating (IARC questionnaire)

Answer as a **media player / streaming app with user-supplied content**: Aurora ships no
content, so the questionnaire's content questions are "no", and the "unrestricted
internet / user-generated content" question is "yes" — the user's provider decides what
plays. Mention the parental controls where the form allows it.

## Notes for certification

Paste into *Submission options → Notes for certification*:

> Aurora TV is a player for the user's own IPTV subscription and contains no content.
> To test it, paste this address into the first-run wizard's **Paste your playlist URL or
> Xtream address** field:
>
> https://raw.githubusercontent.com/noahklimczuk/iptv-player/main/docs/store/reviewer-demo.m3u
>
> It lists public test streams from Apple, Mux and Akamai, which are free to play. No
> account or sign-in is needed for anything except the optional sync feature (Settings →
> Account and sync), which can be tested by creating an account with any email address.
>
> Updates come from the Store: the app detects that it is packaged and does not update
> itself.

Before each submission, open the demo playlist in the app and check every stream still
plays — public test streams do move.
