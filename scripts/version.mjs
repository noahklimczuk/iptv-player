/**
 * Work out what the next version is, and write it into every manifest that carries one.
 *
 * README §23 wants real versions: the third number for a fix, the middle one for a
 * feature. Rather than ask anyone to remember to bump a file, the release build derives
 * it from the commit subjects since the last version tag — the repository already writes
 * conventional commits, so the rule costs nobody anything.
 *
 * The same decision lives in `aurora_core::version` for the updater, which has to
 * compare rather than compute. Two small implementations of "parse a triple" beats one
 * shared one that either language has to shell out to mid-build.
 *
 *   node scripts/version.mjs next     prints the next version and why
 *   node scripts/version.mjs apply    writes it into the manifests, prints it
 *   node scripts/version.mjs current  prints what the manifests say today
 */
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');

/** Every file that states the version, and how to find it in that file. */
export const MANIFESTS = [
  // The workspace version is the one that matters: `env!("CARGO_PKG_VERSION")` is how
  // the running binary knows what it is, and the updater compares against that.
  { path: 'src-native/Cargo.toml', pattern: /^version = "(\d+\.\d+\.\d+)"$/m },
  { path: 'src-native/crates/aurora-app/tauri.conf.json', pattern: /"version": "(\d+\.\d+\.\d+)"/ },
  { path: 'package.json', pattern: /"version": "(\d+\.\d+\.\d+)"/ },
];

export function parse(raw) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)$/.exec(String(raw).trim());
  if (!m) return null;
  return { major: +m[1], minor: +m[2], patch: +m[3] };
}

export const format = (v) => `${v.major}.${v.minor}.${v.patch}`;

/** Numeric, not lexicographic: 0.10.0 is above 0.9.0, which a string compare denies. */
export function compare(a, b) {
  return a.major - b.major || a.minor - b.minor || a.patch - b.patch;
}

/**
 * The conventional-commit type of a subject: `feat(dvr)!: …` is `{type:'feat', breaking:true}`.
 * Anything that is not a conventional commit is null, and counts as a patch.
 */
export function commitType(subject) {
  const colon = subject.indexOf(':');
  if (colon < 0) return null;
  let head = subject.slice(0, colon).trim();

  let breaking = false;
  if (head.endsWith('!')) {
    breaking = true;
    head = head.slice(0, -1);
  }

  const open = head.indexOf('(');
  if (open >= 0) {
    // An unclosed bracket is prose, not a scope.
    if (!head.endsWith(')')) return null;
    head = head.slice(0, open);
  }

  if (!/^[A-Za-z]+$/.test(head)) return null;
  return { type: head.toLowerCase(), breaking };
}

/**
 * What a batch of subjects adds up to.
 *
 * One feature makes the whole release a feature release. Everything else is a patch,
 * including a batch with nothing conventional in it: a build that ships still needs a
 * number of its own. A breaking marker is a minor, not a major — below 1.0 that is what
 * semver says, and above it the call is not a script's to make.
 */
export function bumpFor(subjects) {
  for (const subject of subjects) {
    const parsed = commitType(subject);
    if (!parsed) continue;
    if (parsed.breaking || parsed.type === 'feat') return 'minor';
  }
  return 'patch';
}

export function bumped(v, bump) {
  return bump === 'minor'
    ? { major: v.major, minor: v.minor + 1, patch: 0 }
    : { major: v.major, minor: v.minor, patch: v.patch + 1 };
}

const git = (...args) => execFileSync('git', args, { cwd: ROOT, encoding: 'utf8' }).trim();

/** The highest `v*` tag, or null when nothing has been released yet. */
export function lastReleasedTag() {
  let tags;
  try {
    tags = git('tag', '--list', 'v*').split('\n').filter(Boolean);
  } catch {
    return null;
  }
  const versions = tags
    .map((tag) => ({ tag, version: parse(tag) }))
    .filter((t) => t.version !== null)
    .sort((a, b) => compare(a.version, b.version));
  return versions.at(-1) ?? null;
}

/** What the manifests say right now. */
export function current() {
  const { path, pattern } = MANIFESTS[0];
  const found = pattern.exec(readFileSync(join(ROOT, path), 'utf8'));
  if (!found) throw new Error(`no version in ${path}`);
  return parse(found[1]);
}

/**
 * The commit subjects since `tag`, or the whole history when there is no tag yet.
 *
 * Deliberately not forgiving: a shallow clone cannot answer `git log v0.1.0..HEAD`, and
 * swallowing that would report "no commits", which reads as a patch. Every release would
 * then be a patch and no feature would ever move the middle number — a wrong answer that
 * looks exactly like a right one. Failing here is how that gets noticed.
 */
export function subjectsSince(tag) {
  const range = tag ? `${tag}..HEAD` : 'HEAD';
  try {
    return git('log', '--no-merges', '--pretty=%s', range).split('\n').filter(Boolean);
  } catch (cause) {
    throw new Error(
      `could not read the commits in ${range}. A shallow clone cannot answer this; ` +
        'the release build needs fetch-depth: 0.',
      { cause },
    );
  }
}

/**
 * The version this build should carry.
 *
 * Counts from the last released tag when there is one, and from whatever the manifests
 * say otherwise — so the very first release after adding this lands one step above the
 * committed 0.1.0 rather than resetting to it.
 */
export function next() {
  const last = lastReleasedTag();
  const from = last?.version ?? current();
  const subjects = subjectsSince(last?.tag ?? null);
  const bump = bumpFor(subjects);
  return { from, bump, version: bumped(from, bump), since: last?.tag ?? null, subjects };
}

/**
 * Put `version` into `text` wherever `pattern` finds one.
 *
 * Only the matched version substring is replaced, so the rest of the line — the key
 * name, the quoting, a trailing comma — survives untouched. Separated from the file
 * writing so the substitution itself can be tested without a repository.
 */
export function stamp(text, pattern, version) {
  if (!pattern.test(text)) return null;
  return text.replace(pattern, (match, found) => match.replace(found, format(version)));
}

export function apply(version) {
  const written = [];
  for (const { path, pattern } of MANIFESTS) {
    const full = join(ROOT, path);
    const after = stamp(readFileSync(full, 'utf8'), pattern, version);
    if (after === null) throw new Error(`no version to replace in ${path}`);
    writeFileSync(full, after);
    written.push(path);
  }
  return written;
}

/* ── CLI ────────────────────────────────────────────────────────────────────── */

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].replace(/\\/g, '/'))) {
  const command = process.argv[2] ?? 'next';
  if (command === 'current') {
    console.log(format(current()));
  } else if (command === 'next') {
    const n = next();
    console.error(
      `${format(n.from)} -> ${format(n.version)} (${n.bump}, ` +
        `${n.subjects.length} commits since ${n.since ?? 'the beginning'})`,
    );
    console.log(format(n.version));
  } else if (command === 'apply') {
    const n = next();
    const written = apply(n.version);
    console.error(`wrote ${format(n.version)} to ${written.join(', ')}`);
    console.log(format(n.version));
  } else {
    console.error(`unknown command ${command}; expected current, next or apply`);
    process.exit(2);
  }
}
