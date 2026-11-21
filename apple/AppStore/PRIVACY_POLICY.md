# Aurora TV privacy policy

*For Aurora TV on iPhone, iPad and Apple TV. Effective 20 November 2026.*

Aurora TV is a player for TV and video subscriptions you already have. It has no
analytics, no advertising, no tracking and no crash reporting. Nobody, including the
people who make Aurora, can see what you watch. This page says exactly what leaves
your device, where it goes, and how to stop it.

## What stays on your device

Your library (channels, guide, films and series), your profiles, favourites, lists,
watch history, recordings and settings are kept in Aurora's own storage on the device.
Provider passwords and API keys are kept in the Apple Keychain. Deleting the app
deletes all of it.

## What leaves your device

**Your TV provider.** Aurora talks to the provider you add (an Xtream Codes panel or an
M3U playlist) to fetch the playlist, the guide, artwork and the streams themselves.
Your username and password go to that provider, because that is what they are for.
Aurora does not choose or supply providers, and the provider's own privacy policy
covers what it does.

**TMDB (The Movie Database).** To find posters, cast and plots, Aurora sends TMDB the
names and years of titles in your library. Nothing about you is sent with them.

**Google Gemini, only if you turn it on.** "Because of what you watch" and Ask Aurora
are off until you agree to them. When on, Aurora sends Google the titles you have
watched (with their years and genres, how much of each you watched and whether you
liked them), and what you type to Ask Aurora along with a short summary of your
library, such as how many films it holds and its commonest genres. Your provider, your
sign-ins and your identity are not sent. Google's terms cover what Google does with it.
Turn it off at any time in Settings → Recommendations, and nothing more is sent.

**Sync, only if you create an account.** Sync keeps your devices in step. Creating a
sync account stores its email address and a password with Google Firebase
Authentication. What syncs (favourites, lists, watch progress, profiles, parental
settings, recording rules and settings, and optionally playlist passwords and keys) is
**encrypted on your device** with a key unlocked by your sync passphrase, which never
leaves your devices. Firebase stores only that ciphertext and cannot read it.

**Artwork hosts.** Posters and channel logos load from the addresses your provider or
TMDB gives for them.

**Your home network.** Setting up an Apple TV from an iPhone finds it on your local
network (Bonjour) and sends the sign-in to it encrypted. Nothing goes through the
internet for this.

## Your choices

- **Watch history:** clear it, or choose how long it is kept, in Settings → Privacy.
- **Gemini:** off until you turn it on; Settings → Recommendations turns it off again.
- **Sync:** off until you create an account or sign in. Settings → Account and sync → Sign out stops
  this device syncing. **Delete account** in the same place deletes the account
  and everything it keeps on the server, immediately and for good.
- **Everything else:** delete the app.

## Children

Aurora has kids profiles, age limits and a parental PIN, set by the household. It
collects nothing from children beyond what is described above for everyone, and only
when the household turns those features on.

## Contact

Questions about this policy: open an issue at
<https://github.com/noahklimczuk/iptv-player/issues>. Changes to this policy are
published at this address with a new effective date.
