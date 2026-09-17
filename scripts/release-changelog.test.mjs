import test from "node:test";
import assert from "node:assert/strict";
import {
  findPreviousReleaseTag,
  parseConventionalCommits,
  fetchGitHubReleaseNotes,
  generateChangelog,
} from "./lib/release-changelog.mjs";

test("parseConventionalCommits: groups commits by type and strips scopes", () => {
  const log = `
abc1234\tfeat(ssh): provision remote helper
def5678\tfix(remote): prevent socket timeout
1234567\tperf(daemon): speed up startup
89abcde\trefactor(ui): cleanup layout components
fedcba9\tdocs(readme): update install guide
456789a\tchore(deps): bump versions
7654321\tunconventional commit without scope
`;

  const parsed = parseConventionalCommits(log);
  assert.equal(parsed.feat.length, 1);
  assert.equal(parsed.feat[0].scope, "ssh");
  assert.equal(parsed.feat[0].subject, "provision remote helper");
  assert.equal(parsed.feat[0].hash, "abc1234");

  assert.equal(parsed.fix.length, 1);
  assert.equal(parsed.fix[0].scope, "remote");
  assert.equal(parsed.fix[0].subject, "prevent socket timeout");

  assert.equal(parsed.perf.length, 1);
  assert.equal(parsed.refactor.length, 1);
  assert.equal(parsed.docs.length, 1);
  assert.equal(parsed.chore.length, 1);
  assert.equal(parsed.other.length, 1);
  assert.equal(parsed.other[0].subject, "unconventional commit without scope");
});

test("parseConventionalCommits: handles subjects with escaped newlines cleanly", () => {
  const log = "abc1234\tfeat(session): implement lazy restore\\n\\n- More details\\n- Extra note";
  const parsed = parseConventionalCommits(log);
  assert.equal(parsed.feat.length, 1);
  assert.equal(parsed.feat[0].subject, "implement lazy restore");
});

test("findPreviousReleaseTag: executes fallback or executor function", () => {
  const customExec = (tag) => (tag === "v2026.09.17.1" ? "v2026.09.11.1" : null);
  const prev = findPreviousReleaseTag("v2026.09.17.1", customExec);
  assert.equal(prev, "v2026.09.11.1");
});

test("generateChangelog: generates full markdown with Conventional Commits", () => {
  const mockGitFn = () => `
abc1234\tfeat(browser): add screencast tab
def5678\tfix(terminal): repair input deadlocks
`;

  const changelog = generateChangelog({
    repo: "Indosaram/ferryx",
    tag: "v2026.09.17.1",
    prevTag: "v2026.09.11.1",
    execGitFn: mockGitFn,
    execGhFn: () => null, // simulate no PR notes from gh api
  });

  assert.match(changelog, /# Ferryx release v2026\.09\.17\.1/);
  assert.match(changelog, /## Features/);
  assert.match(changelog, /- \*\*browser\*\*: add screencast tab \(abc1234\)/);
  assert.match(changelog, /## Bug Fixes/);
  assert.match(changelog, /- \*\*terminal\*\*: repair input deadlocks \(def5678\)/);
  assert.match(changelog, /\*\*Full Changelog\*\*: https:\/\/github\.com\/Indosaram\/ferryx\/compare\/v2026\.09\.11\.1\.\.\.v2026\.09\.17\.1/);
});

test("generateChangelog: seamlessly incorporates GitHub PR notes when available", () => {
  const mockGitFn = () => "abc1234\tfix(core): quick fix";
  const mockGhFn = () => `
## What's Changed
* feat: awesome feature by @developer in https://github.com/Indosaram/ferryx/pull/42
`;

  const changelog = generateChangelog({
    repo: "Indosaram/ferryx",
    tag: "v2026.09.17.1",
    prevTag: "v2026.09.11.1",
    execGitFn: mockGitFn,
    execGhFn: mockGhFn,
  });

  assert.match(changelog, /## What's Changed/);
  assert.match(changelog, /pull\/42/);
  assert.match(changelog, /## Bug Fixes/);
  assert.match(changelog, /- \*\*core\*\*: quick fix \(abc1234\)/);
});
