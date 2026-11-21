# App Store listing

One listing, **universal purchase**: the iPhone/iPad app and the Apple TV app share the
bundle identifier `tv.aurora.app` (`AURORA_BUNDLE_ID` in `apple/project.yml`), so they
are one product in App Store Connect with an iOS and a tvOS platform.

Character limits are Apple's. Counts are given where they are close.

## Identity

| Field | Value |
|---|---|
| Name (30) | **Aurora TV: IPTV Player** (22). "Aurora" alone is almost certainly taken; check availability when making the app record. |
| Subtitle (30) | **Live TV, guide, DVR and sync** (28) |
| Bundle ID | `tv.aurora.app` (+ `tv.aurora.app.topshelf`) |
| SKU | `aurora-tv-2` |
| Primary category | Entertainment |
| Secondary category | Photo & Video |
| Price | Free (decide before submitting) |
| Copyright | 2026 Noah Klimczuk |
| Support URL | https://github.com/noahklimczuk/iptv-player/issues |
| Marketing URL | https://github.com/noahklimczuk/iptv-player |
| Privacy policy URL | https://github.com/noahklimczuk/iptv-player/blob/main/apple/AppStore/PRIVACY_POLICY.md |

## Age rating

Recommend **18+**. Aurora plays whatever the viewer's provider carries, which can include
adult channels the app cannot rate in advance (it has "Hide adult categories" and
parental controls, which reviewers will see). In the questionnaire: no content made by
the developer, so answer "None" to each content type, but say yes to **Unrestricted
access to content not reviewed by the developer** where the questionnaire asks it, which
sets the rating. A lower rating invites rejection under guideline 1.

## Promotional text (170)

> New in 2.0: Aurora comes to iPhone, iPad and Apple TV, with your guide, recordings and
> favourites kept in step across every screen, Windows included.

## Description (4000)

> Aurora TV is a fast, beautiful player for the TV subscription you already have. Add an
> M3U playlist or an Xtream Codes login, and Aurora builds your library: live channels
> with a full programme guide, films and series with artwork and cast, all searchable in
> one place.
>
> Aurora does not include any channels or content. You need a playlist or subscription
> from a provider of your own.
>
> LIVE TV AND GUIDE
> • A programme guide a week wide, fetched only for the rows on screen, so even very large
> lineups stay quick
> • Pause and rewind live TV, and catch-up where your provider keeps an archive
> • Channel up and down, number entry and last channel, right from the player
> • Favourite channels and your own named lists
>
> RECORDINGS
> • Record a programme or a whole series from the guide
> • Recordings are kept on your device and play like any film
>
> FILMS AND SERIES
> • Continue Watching, My List and personal rails on Home
> • Skip intros, recaps and credits, and Up Next at the end of an episode
> • Subtitles, audio tracks, speed, aspect and a sleep timer
>
> MULTI-VIEW (iPad and Apple TV)
> • Watch up to nine channels at once, and save your favourite walls
>
> FOR THE WHOLE HOUSEHOLD
> • Profiles with PINs, age limits, daily viewing limits and locked channels
> • Optional sync keeps favourites, progress, profiles and recordings rules the same on
> every device, end-to-end encrypted with a passphrase only your devices know
> • Set up your Apple TV from your iPhone, no typing with the remote
>
> APPLE TV
> • Continue Watching on the Top Shelf
> • Built for the remote, with focus that goes where you expect
>
> PRIVATE BY DESIGN
> No ads, no analytics, no tracking. Optional recommendations from Google Gemini are off
> until you turn them on, and Aurora tells you exactly what they send.

## Keywords (100)

`iptv,m3u,xtream,epg,tv guide,live tv,player,dvr,catch up,playlist,m3u8,channels,stream`
(88)

Do not use other apps' or providers' names: guideline 2.3.7.

## What's New in 2.0 (4000)

> Aurora 2.0 is here, and it's on iPhone, iPad and Apple TV for the first time.
>
> • Everything from Aurora for Windows: live TV and guide, films and series, search,
> recordings, multi-view, profiles and parental controls
> • Sync across your devices, end-to-end encrypted, with in-app account creation and
> deletion
> • Set up an Apple TV from your iPhone
> • Continue Watching on the Apple TV Top Shelf
> • AirPlay on iPhone and iPad

## Screenshots

Take every screenshot with the demo playlist (`review-playlist.m3u`) or other content you
have the rights to show. **No real channel logos, posters or broadcaster names**: they
are someone else's trademarks and copyright, and a frequent reason IPTV apps are
rejected (guideline 5.2).

| Device | Size (pixels) | Required |
|---|---|---|
| iPhone 6.9" | 1320 × 2868 (portrait) | Yes, 3 to 10 |
| iPad 13" | 2064 × 2752 (portrait) | Yes, 3 to 10 (the app runs on iPad) |
| Apple TV | 3840 × 2160 or 1920 × 1080 | Yes, 1 to 10 |

Suggested set, in order: Home, Live TV with the guide, the player with its controls,
Film page, Recordings, Settings → Account and sync, Multi-view (iPad, Apple TV).

On an Intel Mac the simulator cannot draw mpv's picture (the player shows "only the sound
is playing"), so the player screenshot must come from a real device or an Apple-silicon
Mac.

## App preview video (optional)

Up to 30 seconds per device size, recorded with the demo playlist.
