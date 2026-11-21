# Notes for App Review

Paste the block below into App Store Connect → the version → **App Review Information →
Notes**. No demo account is needed: sync is optional, and the reviewer can create one.

---

Aurora TV is a player for TV and video subscriptions the viewer already has, in the two
standard formats providers use (M3U playlists and Xtream Codes panels). It includes no
channels, films or series of its own, does not sell or recommend any provider, and links
to none. Everything it plays comes from a source the viewer adds.

To try it, add our demo playlist of public test streams and open-licence films:

1. Open the app. The setup screen asks for a playlist or provider address.
2. Paste: https://raw.githubusercontent.com/noahklimczuk/iptv-player/main/apple/AppStore/review-playlist.m3u
3. Continue, choose to import live TV with films and series, and wait a few seconds.
4. Live TV lists three channels and Movies four open films (Big Buck Bunny, Tears of
   Steel, Elephants Dream and the Sintel trailer). Choose one to play. Settings →
   Providers can add more.

Optional features:

- **Sync** (Settings → Account and sync): creates an account with an email and password so a
  household's devices share favourites and progress. The data is end-to-end encrypted
  on the device. The account can be deleted in Settings → Account and sync → Delete account.
- **Recommendations / Ask Aurora**: uses Google Gemini. Off until the viewer agrees on
  the Home screen card or in Settings → Recommendations, which says what is sent.
- **Apple TV set-up from iPhone**: uses the local network (Bonjour) to find the TV, so
  the app asks for local network access only when that screen is opened.

App Transport Security: providers serve their playlists, guides and artwork from
addresses of their own, very often plain http://, which the viewer types in. Arbitrary
loads are allowed for that reason only; Aurora's own services (sync, Gemini, TMDB) are
all HTTPS.

Recording (Apple TV, iPhone, iPad): programmes are recorded on the device from the
viewer's own provider, for the viewer's own later viewing, as a personal video recorder
does.

---

## Before pasting

- Check every stream in `review-playlist.m3u` still plays, and that the raw URL above
  opens the file (the repository must stay public, or host the file elsewhere and
  change both URLs).
- If Apple asks for a video of the app working with a real subscription, record one on
  a device with your own provider, showing no channel logos you do not have the right to
  show.
