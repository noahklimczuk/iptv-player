# App Store Connect: App Privacy answers

What to enter under **App Privacy** for Aurora TV. It matches the app's privacy manifest
(`apple/Shared/Privacy/PrivacyInfo.xcprivacy`) and the public policy
(`PRIVACY_POLICY.md`). Update all three together.

**Privacy policy URL:**
`https://github.com/noahklimczuk/iptv-player/blob/main/apple/AppStore/PRIVACY_POLICY.md`

**Do you or your third-party partners collect data from this app?** Yes.

| Data type | Collected for | Linked to the user? | Used for tracking? | Why |
|---|---|---|---|---|
| Contact Info → **Email Address** | App Functionality | Yes | No | The sync account's sign-in (Firebase Authentication). Only when the household creates a sync account. |
| Usage Data → **Product Interaction** | App Functionality | No | No | Watched titles sent to Google Gemini for "Because of what you watch". Off until the viewer agrees. |
| User Content → **Other User Content** | App Functionality | No | No | Questions typed to Ask Aurora, sent to Google Gemini. Off until the viewer agrees. |

Not collected, and why, in case review asks:

- **Synced records** (favourites, progress, profiles…): end-to-end encrypted on the device
  with a key Firebase never receives, so neither the developer nor Google can read them.
- **Provider credentials and streams:** go to the viewer's own provider, never to the
  developer or a partner of the developer.
- **TMDB lookups:** title names and years, with nothing identifying the viewer.
- **Diagnostics:** the exportable log stays on the device unless the viewer shares it.

**Tracking:** No. No IDFA, no App Tracking Transparency prompt, no tracking domains.
