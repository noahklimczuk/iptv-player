/**
 * Tests for the release versioning (README §23).
 *
 * `node --test scripts/` — no framework, because the only thing in this directory is
 * build tooling and adding a test runner to run six assertions would be worse than the
 * assertions.
 *
 * The cases here are the ones that would silently ship a wrong number: ordering that is
 * lexicographic, a substring search that reads "feature" in prose as a feature commit,
 * and a substitution that rewrites the wrong line of a manifest.
 */
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { bumpFor, bumped, commitType, compare, format, parse, stamp } from './version.mjs';

test('a plain triple parses, with or without the tag prefix', () => {
  assert.deepEqual(parse('0.1.0'), { major: 0, minor: 1, patch: 0 });
  assert.deepEqual(parse('v0.1.0'), { major: 0, minor: 1, patch: 0 });
  assert.deepEqual(parse('  v12.34.56 '), { major: 12, minor: 34, patch: 56 });
});

test('anything that is not three numbers is rejected', () => {
  for (const raw of ['', 'v', '1', '1.2', '1.2.3.4', '1.2.x', 'latest-windows', '0.2.0-rc1']) {
    assert.equal(parse(raw), null, `${raw} should not parse`);
  }
});

test('ordering is numeric, not lexicographic', () => {
  // The trap: as strings, "0.9.0" sorts above "0.10.0".
  assert.ok(compare(parse('0.10.0'), parse('0.9.0')) > 0);
  assert.ok(compare(parse('0.2.0'), parse('0.1.9')) > 0);
  assert.ok(compare(parse('1.0.0'), parse('0.99.99')) > 0);
  assert.equal(compare(parse('0.1.0'), parse('v0.1.0')), 0);
});

test('a conventional commit gives up its type and its breaking marker', () => {
  assert.deepEqual(commitType('feat: a thing'), { type: 'feat', breaking: false });
  assert.deepEqual(commitType('fix(dvr): a thing'), { type: 'fix', breaking: false });
  assert.deepEqual(commitType('feat!: a thing'), { type: 'feat', breaking: true });
  assert.deepEqual(commitType('fix(db)!: a thing'), { type: 'fix', breaking: true });
});

test('prose is not a conventional commit', () => {
  assert.equal(commitType('merged main'), null);
  assert.equal(commitType('we (finally) shipped: the thing'), null);
  assert.equal(commitType('feat(unclosed: thing'), null);
});

test('a feature anywhere in the batch makes it a minor', () => {
  assert.equal(bumpFor(['fix: a', 'feat(dvr): b', 'ci: c']), 'minor');
  assert.equal(bumpFor(['feat: a']), 'minor');
});

test('everything else is a patch, including nothing at all', () => {
  assert.equal(bumpFor(['fix: a']), 'patch');
  assert.equal(bumpFor(['ci: a', 'docs: b', 'refactor(db): c']), 'patch');
  assert.equal(bumpFor(['merged main', 'wip']), 'patch');
  assert.equal(bumpFor([]), 'patch');
});

test('the word feature in prose is not a feature commit', () => {
  // What a substring search would get wrong. Both of these are patches.
  assert.equal(bumpFor(['fix: add the feature flag we forgot']), 'patch');
  assert.equal(bumpFor(['docs: describe the feat of strength']), 'patch');
  assert.equal(bumpFor(['defeat the flake']), 'patch');
});

test('a breaking change is a minor, never a major', () => {
  assert.equal(bumpFor(['feat!: rename everything']), 'minor');
  assert.deepEqual(bumped(parse('0.4.2'), 'minor'), { major: 0, minor: 5, patch: 0 });
});

test('a minor clears the patch', () => {
  assert.deepEqual(bumped(parse('0.1.7'), 'minor'), { major: 0, minor: 2, patch: 0 });
  assert.deepEqual(bumped(parse('0.1.7'), 'patch'), { major: 0, minor: 1, patch: 8 });
});

test('stamping a Cargo manifest touches only the workspace version', () => {
  const before = [
    '[workspace.package]',
    'edition = "2021"',
    'version = "0.1.0"',
    '',
    '[workspace.dependencies]',
    'serde = { version = "1", features = ["derive"] }',
    'rusqlite = { version = "0.32", features = ["bundled"] }',
  ].join('\n');

  const after = stamp(before, /^version = "(\d+\.\d+\.\d+)"$/m, parse('0.2.0'));

  assert.ok(after.includes('version = "0.2.0"'));
  // A dependency pin is not the project's version and must not move.
  assert.ok(after.includes('serde = { version = "1", features = ["derive"] }'));
  assert.ok(after.includes('rusqlite = { version = "0.32", features = ["bundled"] }'));
  assert.ok(after.includes('edition = "2021"'));
});

test('stamping a JSON manifest keeps the key and the comma', () => {
  const before = '{\n  "name": "aurora-tv",\n  "version": "0.1.0",\n  "private": true\n}';
  const after = stamp(before, /"version": "(\d+\.\d+\.\d+)"/, parse('1.2.3'));
  assert.equal(after, '{\n  "name": "aurora-tv",\n  "version": "1.2.3",\n  "private": true\n}');
});

test('stamping a file with no version in it says so rather than guessing', () => {
  assert.equal(stamp('nothing here', /"version": "(\d+\.\d+\.\d+)"/, parse('1.0.0')), null);
});

test('the formatted version round-trips', () => {
  for (const raw of ['0.0.0', '0.1.0', '1.20.300']) {
    assert.equal(format(parse(raw)), raw);
  }
});
