# What Aurora TV sends, and where

Short version: Aurora talks to your provider, to Google, to TMDB, and to GitHub. It has no
analytics, no telemetry and no account. Nothing is sent anywhere to be counted, and
nobody — including whoever built this — can see what you watch.

The long version matters more than the short one, because "no telemetry" is often said by
software that still sends plenty. Here is each destination and exactly what goes to it.

## Your provider

Everything you would expect: the playlist, the guide, the artwork your provider hosts, and
the streams themselves. Your username and password go to your provider, because that is
what they are for.

**Your password is not kept as a password anywhere in the library.** It lives in Windows
Credential Manager, and the `providers` table holds only a reference to the entry — so
nothing in Aurora stores the thing you typed.

**But `library.db` is not safe to hand to somebody today, and this note used to say it
was.** Xtream addresses a stream as
`http://host/<username>/<password>/<id>` — the credentials are in the path, because that
is how the protocol works — and the library stores those URLs as they come, in
`channel_sources.url`, `movies.url` and `episodes.url`. On a subscription with a few
thousand channels and a large catalogue that is hundreds of thousands of copies of your
sign-in, in plain text, in one file.

So: **do not attach `library.db` to a bug report.** The work to store a stream id and
rebuild the URL when it is needed is in progress; when it is done this paragraph will say
so, and until then it says what is actually true.

Credentials no longer appear in a **log**. Every URL written to one goes through
`http::redact` first, which strips `username`, `password`, `token` and the rest from query
strings, and also handles the form that carries no markers at all —
`http://host/<user>/<pass>/<id>`, which is how most panels address a stream. That second
case has been a real leak twice: once when it was not handled at all (F-15), and once
because the fix required a file extension on the end, which the panel this app is measured
against does not put there — so none of its live channels was redacted while its films
were. And a third time, in the component that handles a stream URL on every single play:
the player logged mpv's own output through no redaction at all — `redact` was not
referenced once anywhere in `aurora-player`, because it lived in the crate that also
carries the HTTP client and the player cannot depend on that. It is in `aurora-core` now,
and the two call sites in `mpv.rs` go through it.

Routing them through it was not sufficient on its own, which is worth writing down here
because it is the kind of thing this note has been wrong about before. `redact` took a
URL and nothing else; mpv's log is never a bare URL but ffmpeg's sentence about one ---
`Failed to open http://host/<user>/<pass>/<id>: Server returned 403 Forbidden` — and
that went through the old function completely unchanged, because everything after the
last `/` was read as one path segment and so did not look like a stream id.
`redact_in_text` picks the URLs out of a line and redacts each on its own.

Settings → Diagnostics offers to copy the log "for a bug report", and that is now a
safe thing to accept.

## Google, for the assistant and the recommendation rail

**This is the one worth reading.**

Two features ask a Google model: the assistant, and the "Because of what you watch" rail
on the home screen.

What goes to Google:

- **The recommendation rail** sends the titles you have watched, with their years and
  genres, how much of each you watched, and whether you liked it. Up to the most recent
  few dozen.
- **The assistant** sends what you type, the conversation so far, and a one-line summary
  of your library — how many films, series and channels you have, and your commonest
  genres. When it looks something up, it sends the search terms and gets back titles from
  your own library.

What does not: your provider, your credentials, your IP beyond the fact of the request,
or anything naming you or your subscription.

**It is on by default in an official build.** Releases are built with an API key compiled
in, so the rail asks Google the first time you open the home screen with something in your
watch history, without you entering anything. That is a deliberate trade — the alternative
is a feature that does nothing until you go and find a Google key — but it means *your
viewing history leaves the machine by default*, and you should know that rather than
discover it.

To stop it: turn **Suggestions and the assistant** off, in
**Settings → Recommendations**. Nothing is then sent to Google, including by the key
compiled into the build, and both features say they need a key.

Clearing the key is *not* how to stop it, and this note used to say it was. In an official
build, removing the key you entered falls back to the one compiled in — so the rail carried
on and your history kept going out. A build you compile yourself has no key at all unless
you supply one, which is why the instruction looked right to whoever wrote it.

Google's terms govern what they do with it. Aurora does not send it anywhere else and
keeps no copy beyond the conversation stored in your own library.

## TMDB, for artwork and metadata

Titles and years are sent to look up posters, backdrops, cast and plots. Nothing about you
goes with them — TMDB is told "what is the film called *Arrival* from 2016", not who is
asking. Artwork is then cached on your disk, so the same poster is not fetched twice.

The key is compiled into official builds, as Google's is. TMDB keys are free and
revocable, which is why that one is a literal in CI and Google's is a secret.

## GitHub, for updates

Aurora asks GitHub whether there is a newer release, by default once a few seconds after
launch. The request carries nothing but the fact that it was made. Turn it off in
**Settings → Updates** and nothing is asked.

Downloads are verified against the SHA-256 GitHub publishes before anything is swapped.

## What there is none of

No analytics. No crash reporting. No usage counting. No account, no sign-in, no server
belonging to this project — there is no such server. The only machine that knows what you
watch is yours, with the exception above, which is written in full because it is the
exception.

## Where your data lives

Everything is in one folder: the library database, the artwork cache, the recordings and
the log. On an installed copy that is `%LOCALAPPDATA%\tv.aurora.player`; a portable copy
keeps it all beside the executable instead. Delete the folder and nothing of Aurora
remains but the credentials in Credential Manager, which Settings can clear.
