import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  findPreviousReleaseTag,
  parseConventionalCommits,
  fetchGitHubReleaseNotes,
  generateChangelog,
  runChangelogCli,
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

const fakeTags = () => "v2026.09.20.1\nv2026.09.17.1\ndaemon-lifetime-handover-v1\nv2026.09.11.1\n";
const fakeLog = ({ tag }) => ({
  "v2026.09.20.1": "aaa1111\tfeat(remote): third release",
  "v2026.09.17.1": "bbb2222\tfix(pty): second release",
  "v2026.09.11.1": "ccc3333\tfirst release",
})[tag];

function runCli(argv) {
  const out = [];
  const err = [];
  const code = runChangelogCli(argv, {
    stdout: (s) => out.push(s),
    stderr: (s) => err.push(s),
    listTagsFn: fakeTags,
    execGitFn: fakeLog,
  });
  return { code, out: out.join(""), err: err.join("") };
}

test("--write-changelog: prepends only missing tag sections, newest first", () => {
  const file = join(mkdtempSync(join(tmpdir(), "ferryx-changelog-")), "CHANGELOG.md");
  writeFileSync(file, "# Changelog\n\nintro\n\n## v2026.09.11.1\n\n- kept by hand (ccc3333)\n");

  const { code } = runCli(["--write-changelog", "--file", file]);
  assert.equal(code, 0);
  const written = readFileSync(file, "utf8");
  const headings = [...written.matchAll(/^## (\S+)/gm)].map((m) => m[1]);
  assert.deepEqual(headings, ["v2026.09.20.1", "v2026.09.17.1", "v2026.09.11.1"]);
  assert.match(written, /### Features\n\n- \*\*remote\*\*: third release \(aaa1111\)/);
  assert.match(written, /compare\/v2026\.09\.17\.1\.\.\.v2026\.09\.20\.1/);
  assert.match(written, /- kept by hand \(ccc3333\)/);
  assert.doesNotMatch(written, /daemon-lifetime-handover-v1/);

  // A second run is a no-op.
  assert.equal(runCli(["--write-changelog", "--file", file]).code, 0);
  assert.equal(readFileSync(file, "utf8"), written);
});

test("--write-changelog --dry-run prints the selected range without writing", () => {
  const file = join(mkdtempSync(join(tmpdir(), "ferryx-changelog-")), "CHANGELOG.md");
  const { code, out } = runCli(["--write-changelog", "--dry-run", "--from", "v2026.09.17.1", "--file", file]);
  assert.equal(code, 0);
  assert.deepEqual([...out.matchAll(/^## (\S+)/gm)].map((m) => m[1]), ["v2026.09.20.1", "v2026.09.17.1"]);
  assert.throws(() => readFileSync(file, "utf8"));
});

test("--write-changelog fails with a message on an unknown or inverted tag range", () => {
  const unknown = runCli(["--write-changelog", "--dry-run", "--from", "v2099.01.01.1"]);
  assert.equal(unknown.code, 1);
  assert.match(unknown.err, /--from v2099\.01\.01\.1 is not a release tag/);

  const inverted = runCli(["--write-changelog", "--dry-run", "--from", "v2026.09.20.1", "--to", "v2026.09.11.1"]);
  assert.equal(inverted.code, 1);
  assert.match(inverted.err, /empty tag range/);
});
