// Parity + contract tests for the keeper's LaunchAgent.
//
// Two separate risks, one file:
//
//  1. **Drift.** `scripts/lib/keeper-launchd-job.mjs` restates the plist `keeper_launchd.rs` renders.
//     A restatement nobody compares is a comment, so the first test reads the Rust `format!` literal
//     out of the source and requires the two documents to be byte-identical. Any change to a label,
//     a restart rule or a descriptor limit on either side fails here.
//
//  2. **The property itself.** The plan requires an INDEPENDENTLY managed keeper service: own label,
//     own process group, executable outside the app bundle. `verifyKeeperLaunchdJob` judges a real
//     install's captured plist, and these cases pin that it refuses each way the property can be
//     broken -- including the tempting "one job for both" arrangement.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  DAEMON_PLIST_LABEL,
  KEEPER_PLIST_LABEL,
  KEEPER_QA_PLIST_LABEL,
  KEEPER_RETAINED_GENERATIONS,
  keeperGenerationDir,
  keeperGenerationExecutable,
  parsePlistXml,
  renderKeeperLaunchdPlist,
  verifyKeeperLaunchdJob,
} from "./lib/keeper-launchd-job.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const KEEPER_LAUNCHD_RS = join(REPO_ROOT, "src-tauri", "src", "daemon", "keeper_launchd.rs");

const HOME = "/Users/operator";
const VERSION = "2026.1116.1";
const LOG_DIR = "/Users/operator/Library/Logs/Ferryx";
const EXECUTABLE = keeperGenerationExecutable({ home: HOME, version: VERSION });

/**
 * The `format!` template inside `generate_keeper_launchd_plist`, with its placeholders intact.
 *
 * Read from source rather than duplicated so the parity test cannot drift along with the copy it is
 * meant to guard. The literal is the only `r#"<?xml ... "#` in the file, and it ends at the first
 * `"#,` after its start.
 */
function rustPlistTemplate() {
  const source = readFileSync(KEEPER_LAUNCHD_RS, "utf8");
  const open = source.indexOf('r#"<?xml');
  assert.notEqual(open, -1, "keeper_launchd.rs no longer contains the plist raw-string literal");
  const bodyStart = open + 'r#"'.length;
  const close = source.indexOf('"#,', bodyStart);
  assert.notEqual(close, -1, "the plist raw-string literal is unterminated");
  return source.slice(bodyStart, close);
}

/** Applies the Rust template's placeholders in the order `format!` resolves them. */
function renderFromRustTemplate(template, { label, executablePath, logDir, runAtLoad }) {
  const withNamed = template
    .replace("{label}", label)
    .replace("{executable_path}", executablePath)
    .replace("{run_at_load}", runAtLoad ? "<true/>" : "<false/>");
  // The two positional `{}` are the stdout and stderr paths, in that order.
  return withNamed.replace("{}", `${logDir}/keeper.stdout.log`).replace("{}", `${logDir}/keeper.stderr.log`);
}

test("the keeper plist renderer matches the Rust format! literal byte for byte", () => {
  const template = rustPlistTemplate();
  for (const runAtLoad of [true, false]) {
    const rendered = renderKeeperLaunchdPlist({
      label: KEEPER_PLIST_LABEL,
      executablePath: EXECUTABLE,
      logDir: LOG_DIR,
      runAtLoad,
    });
    const expected = renderFromRustTemplate(template, {
      label: KEEPER_PLIST_LABEL,
      executablePath: EXECUTABLE,
      logDir: LOG_DIR,
      runAtLoad,
    });
    assert.equal(
      rendered,
      expected,
      `the JS renderer and keeper_launchd::generate_keeper_launchd_plist disagree for runAtLoad=${runAtLoad}`,
    );
  }
});

test("the Rust renderer and the checker agree on the job's own label and the daemon's", () => {
  const template = rustPlistTemplate();
  const source = readFileSync(KEEPER_LAUNCHD_RS, "utf8");
  assert.match(source, new RegExp(`PLIST_LABEL: &str = "${KEEPER_PLIST_LABEL.replace(/\./g, "\\.")}"`));
  assert.match(source, new RegExp(`QA_PLIST_LABEL: &str = "${KEEPER_QA_PLIST_LABEL.replace(/\./g, "\\.")}"`));
  const daemonSource = readFileSync(join(REPO_ROOT, "src-tauri", "src", "daemon", "launchd.rs"), "utf8");
  assert.match(daemonSource, new RegExp(`PLIST_LABEL: &str = "${DAEMON_PLIST_LABEL.replace(/\./g, "\\.")}"`));
  assert.notEqual(KEEPER_PLIST_LABEL, DAEMON_PLIST_LABEL, "one job, one label");
  assert.notEqual(KEEPER_QA_PLIST_LABEL, KEEPER_PLIST_LABEL, "a fixture job must not collide with the real one");
  // The template names no program of its own: the executable is always the caller's.
  assert.match(template, /\{executable_path\}/);
  assert.equal(KEEPER_RETAINED_GENERATIONS, 3, "the plan's retention window is the last three generations");
});

test("the generated plist passes the independence check", () => {
  const plistText = renderKeeperLaunchdPlist({
    label: KEEPER_PLIST_LABEL,
    executablePath: EXECUTABLE,
    logDir: LOG_DIR,
    runAtLoad: true,
  });
  const result = verifyKeeperLaunchdJob({ plistText, home: HOME, version: VERSION });
  assert.deepEqual(result.problems, []);
  assert.equal(result.ok, true);
  assert.equal(result.executable, EXECUTABLE);
  assert.equal(result.job.AbandonProcessGroup, true);
  assert.equal(result.job.KeepAlive.SuccessfulExit, false);
});

test("the generation directory is outside the app bundle and named by the app version", () => {
  // The whole point of the generation layout: replacing Ferryx.app must not replace the keeper.
  const dir = keeperGenerationDir({ home: HOME, version: VERSION });
  assert.equal(dir, "/Users/operator/.ferryx/keeper/2026.1116.1");
  assert.equal(`${dir}/ferryx-ptykeeper`, EXECUTABLE);
  assert.ok(!EXECUTABLE.includes(".app/"), "a keeper inside the bundle dies with the app");
  assert.throws(() => keeperGenerationDir({ home: "", version: VERSION }), /KEEPER_JOB_NO_HOME/);
  assert.throws(() => keeperGenerationDir({ home: HOME, version: "" }), /KEEPER_JOB_NO_VERSION/);
});

test("refuses a keeper job installed under the daemon's own label", () => {
  // One label is one job: sharing the daemon's means launchctl manages a single process, so the
  // keeper would be replaced (and its PTY masters lost) whenever the app replaces its daemon.
  const plistText = renderKeeperLaunchdPlist({
    label: DAEMON_PLIST_LABEL,
    executablePath: EXECUTABLE,
    logDir: LOG_DIR,
    runAtLoad: true,
  });
  const result = verifyKeeperLaunchdJob({ plistText, home: HOME, version: VERSION });
  assert.equal(result.ok, false);
  const labelProblem = result.problems.find((entry) => entry.code === "KEEPER_JOB_LABEL");
  assert.ok(labelProblem, result.problems.map((entry) => entry.code).join(", "));
  assert.match(labelProblem.detail, /daemon's own label/);
});

test("refuses the QA label in a shipped job and the production label in a fixture", () => {
  const qaText = renderKeeperLaunchdPlist({
    label: KEEPER_QA_PLIST_LABEL,
    executablePath: EXECUTABLE,
    logDir: LOG_DIR,
    runAtLoad: true,
  });
  const shipped = verifyKeeperLaunchdJob({ plistText: qaText, home: HOME, version: VERSION });
  assert.ok(shipped.problems.some((entry) => entry.code === "KEEPER_JOB_LABEL"));
  assert.match(
    shipped.problems.find((entry) => entry.code === "KEEPER_JOB_LABEL").detail,
    /QA label/,
  );
  // ... and the same document judged as the fixture it is passes, so the QA label is not banned.
  assert.deepEqual(
    verifyKeeperLaunchdJob({ plistText: qaText, home: HOME, version: VERSION, expectedLabel: KEEPER_QA_PLIST_LABEL })
      .problems,
    [],
  );
});

test("refuses a keeper that execs inside the app bundle", () => {
  const insideBundle = "/Applications/Ferryx.app/Contents/Resources/keeper/ferryx-ptykeeper";
  const plistText = renderKeeperLaunchdPlist({
    label: KEEPER_PLIST_LABEL,
    executablePath: insideBundle,
    logDir: LOG_DIR,
    runAtLoad: true,
  });
  const result = verifyKeeperLaunchdJob({ plistText, home: HOME, version: VERSION });
  const problem = result.problems.find((entry) => entry.code === "KEEPER_JOB_EXECUTABLE");
  assert.ok(problem, result.problems.map((entry) => entry.code).join(", "));
  assert.match(problem.detail, /inside an app bundle/);
  assert.match(problem.detail, /\.ferryx\/keeper\/2026\.1116\.1\/ferryx-ptykeeper/);
});

test("refuses a keeper execing another generation than this archive installs", () => {
  // A keeper job pinned to the previous release keeps running the old keeper against a new daemon,
  // which is the compatibility hazard the generation layout exists to avoid.
  const plistText = renderKeeperLaunchdPlist({
    label: KEEPER_PLIST_LABEL,
    executablePath: keeperGenerationExecutable({ home: HOME, version: "2026.1115.1" }),
    logDir: LOG_DIR,
    runAtLoad: true,
  });
  const result = verifyKeeperLaunchdJob({ plistText, home: HOME, version: VERSION });
  const problem = result.problems.find((entry) => entry.code === "KEEPER_JOB_EXECUTABLE");
  assert.ok(problem);
  assert.match(problem.detail, /generation this archive installs is .*2026\.1116\.1/);
});

test("refuses arguments on the keeper's ProgramArguments", () => {
  // The endpoint stays derived from the runtime (`crash_supervisor::KEEPER_SOCKET_NAME`), and the
  // keeper has no `--daemon*` mode: any extra element is either a pinned socket or the daemon's job.
  const base = renderKeeperLaunchdPlist({
    label: KEEPER_PLIST_LABEL,
    executablePath: EXECUTABLE,
    logDir: LOG_DIR,
    runAtLoad: true,
  });
  const withSocket = base.replace(
    `<string>${EXECUTABLE}</string>`,
    `<string>${EXECUTABLE}</string>
        <string>--socket</string>
        <string>/tmp/ferryx-501/keeper.sock</string>`,
  );
  const result = verifyKeeperLaunchdJob({ plistText: withSocket, home: HOME, version: VERSION });
  const problem = result.problems.find((entry) => entry.code === "KEEPER_JOB_ARGUMENTS_NOT_SINGLE");
  assert.ok(problem, result.problems.map((entry) => entry.code).join(", "));
  assert.equal(result.executable, EXECUTABLE);

  const flagAsProgram = base.replace(`<string>${EXECUTABLE}</string>`, `<string>--daemon-crash-supervisor</string>`);
  const flagResult = verifyKeeperLaunchdJob({ plistText: flagAsProgram, home: HOME, version: VERSION });
  assert.ok(flagResult.problems.some((entry) => entry.code === "KEEPER_JOB_ARGUMENT_FLAG"));
});

test("refuses a restart rule that restarts after a deliberate exit", () => {
  // The keeper exits 0 when a second instance finds the instance lock held. Under a plain
  // `KeepAlive` (or `SuccessfulExit: true`) launchd would relaunch it forever, fighting the keeper
  // that legitimately owns the socket.
  const base = renderKeeperLaunchdPlist({
    label: KEEPER_PLIST_LABEL,
    executablePath: EXECUTABLE,
    logDir: LOG_DIR,
    runAtLoad: true,
  });
  const unconditional = base.replace(
    `    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>`,
    `    <key>KeepAlive</key>
    <true/>`,
  );
  assert.notEqual(unconditional, base, "fixture precondition: KeepAlive was rewritten");
  const result = verifyKeeperLaunchdJob({ plistText: unconditional, home: HOME, version: VERSION });
  assert.ok(result.problems.some((entry) => entry.code === "KEEPER_JOB_KEEP_ALIVE"));

  const restartsOnZero = base.replace(`<key>SuccessfulExit</key>
        <false/>`, `<key>SuccessfulExit</key>
        <true/>`);
  const zeroResult = verifyKeeperLaunchdJob({ plistText: restartsOnZero, home: HOME, version: VERSION });
  const problem = zeroResult.problems.find((entry) => entry.code === "KEEPER_JOB_RESTARTS_ON_EXIT_ZERO");
  assert.ok(problem, zeroResult.problems.map((entry) => entry.code).join(", "));
  assert.match(problem.detail, /only an abnormal death restart/);
});

test("refuses a job that lets launchd signal the PTY children", () => {
  const base = renderKeeperLaunchdPlist({
    label: KEEPER_PLIST_LABEL,
    executablePath: EXECUTABLE,
    logDir: LOG_DIR,
    runAtLoad: true,
  });
  const drop = base.replace(`    <key>AbandonProcessGroup</key>
    <true/>
`, "");
  const result = verifyKeeperLaunchdJob({ plistText: drop, home: HOME, version: VERSION });
  const problem = result.problems.find((entry) => entry.code === "KEEPER_JOB_PROCESS_GROUP");
  assert.ok(problem);
  assert.match(problem.detail, /signal the PTY children/);
});

test("refuses a job whose descriptor limit is the default and one with no log paths", () => {
  const base = renderKeeperLaunchdPlist({
    label: KEEPER_PLIST_LABEL,
    executablePath: EXECUTABLE,
    logDir: LOG_DIR,
    runAtLoad: true,
  });
  const noLimit = base.replace(`    <key>SoftResourceLimits</key>
    <dict>
        <key>NumberOfFiles</key>
        <integer>4096</integer>
    </dict>
`, "");
  const limitResult = verifyKeeperLaunchdJob({ plistText: noLimit, home: HOME, version: VERSION });
  assert.ok(limitResult.problems.some((entry) => entry.code === "KEEPER_JOB_DESCRIPTOR_LIMIT"));

  // The daemon's own capture file names would interleave two processes into one stream.
  const sharedLog = base.replace("<string>/Users/operator/Library/Logs/Ferryx/keeper.stderr.log</string>",
    "<string>/Users/operator/Library/Logs/Ferryx/keeper.stdout.log</string>");
  const logResult = verifyKeeperLaunchdJob({ plistText: sharedLog, home: HOME, version: VERSION });
  assert.ok(
    logResult.problems.some(
      (entry) => entry.code === "KEEPER_JOB_LOG_PATH" && /same file/.test(entry.detail),
    ),
  );

  const daemonNames = base
    .replace("keeper.stdout.log</string>", "daemon.stdout.log</string>")
    .replace("keeper.stderr.log</string>", "daemon.stderr.log</string>");
  const namesResult = verifyKeeperLaunchdJob({ plistText: daemonNames, home: HOME, version: VERSION });
  assert.ok(namesResult.problems.some((entry) => entry.code === "KEEPER_JOB_LOG_PATH"));
});

test("an unreadable capture is a refusal, not an empty job", () => {
  // A checker that returned `{}` for broken input would pass every "must be present" rule.
  const result = verifyKeeperLaunchdJob({ plistText: "<plist><dict><key>Label", home: HOME, version: VERSION });
  assert.equal(result.ok, false);
  assert.equal(result.job, null);
  assert.equal(result.problems[0].code, "KEEPER_JOB_PLIST_UNREADABLE");
});

test("the plist reader handles nested dicts, arrays, integers and booleans", () => {
  const parsed = parsePlistXml(`<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.ferryx.ptykeeper</string>
    <key>Nested</key>
    <dict>
        <key>List</key>
        <array>
            <string>a</string>
            <integer>2</integer>
            <true/>
        </array>
        <key>Flag</key>
        <false/>
    </dict>
</dict>
</plist>`);
  assert.deepEqual(parsed, {
    Label: "com.ferryx.ptykeeper",
    Nested: { List: ["a", 2, true], Flag: false },
  });
});
