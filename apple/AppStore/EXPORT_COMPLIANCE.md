# Export compliance

Both apps' Info.plist set `ITSAppUsesNonExemptEncryption` to `NO`
(`apple/project.yml`), so App Store Connect does not ask on every upload. This file
records why. **The account holder makes this declaration to Apple, so confirm it before
the first submission.** If in doubt, answer App Store Connect's questions by hand once
and keep the key in step with the answer.

What Aurora encrypts, and with what:

| Use | Algorithm | Whose data |
|---|---|---|
| Talking to providers, TMDB, Google and Firebase | TLS (rustls, the system's URLSession) | the viewer's |
| Sync records and the vault key | XChaCha20-Poly1305, Argon2id key wrapping | the viewer's own library |
| Setting up an Apple TV from a phone | X25519 key agreement, ChaCha20-Poly1305 (CryptoKit) | the viewer's own sign-in |

All are standard, published algorithms, used only to protect the viewer's own data and
authenticate to services. Aurora offers no encryption as a feature to others, and
nothing proprietary. That is the case Apple describes as exempt (EAR Category 5, Part
2, Note 4, and the mass-market provisions), which needs no documentation uploaded and,
since 2021, no annual self-classification report.

App Store Connect's questions, answered for this build:

1. Does your app use encryption? **Yes.**
2. Does it qualify for any of the exemptions in Category 5, Part 2? **Yes.**

France: Apple may ask about distribution in France separately. Standard algorithms
used for authentication and data protection need no declaration there either.
