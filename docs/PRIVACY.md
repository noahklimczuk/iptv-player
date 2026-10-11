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

They are **not** stored in the library database. They live in Windows Credential Manager,
and the database keeps only a reference to the entry. That means a copy of `library.db` —
a backup, an export, a file handed to somebody for a bug report — contains no credentials.

Nor do they appear in a log. Every URL written to the log goes through `http::redact`
first, which strips `username`, `password`, `token` and the rest from query strings, and
also handles the form that has no markers at all: `http://host/<user>/<pass>/12345.ts`,
which is how most panels address a stream. That second case was a real leak once (F-15).

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

To stop it: clear the key in **Settings → Assistant**. With no key, both features say they
need one and nothing is sent. A build you compile yourself has no key unless you supply
one.

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
