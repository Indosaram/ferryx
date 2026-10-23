// Runs scripts/hub-cutover.sh and scripts/hub-watchdog.sh against stand-ins for launchctl,
// sqlite3, pgrep, curl, uname, mktemp (BSD semantics), and the ferryx CLI.
// `node --test scripts/hub-cutover.test.mjs` works on Linux and macOS.
import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const cutover = path.join(here, "hub-cutover.sh");
const watchdog = path.join(here, "hub-watchdog.sh");
const realMktemp = spawnSync("bash", ["-c", "command -v mktemp"]).stdout.toString().trim();

function stub(dir, name, body) {
  const file = path.join(dir, name);
  fs.writeFileSync(file, `#!/usr/bin/env bash\n${body}\n`, { mode: 0o755 });
}

function sandbox({ status = "healthy", start = "ok", stop = "ok", health = "200" } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hub-cutover-test-"));
  const bin = path.join(root, "bin");
  const home = path.join(root, "home");
  const tmp = path.join(root, "tmp");
  const log = path.join(root, "calls.log");
  const loaded = path.join(root, "loaded");
  for (const dir of [bin, home, tmp, loaded, path.join(home, "Library/LaunchAgents")]) {
    fs.mkdirSync(dir, { recursive: true });
  }
  for (const label of ["ai.omon.gateway", "ai.omon.gateway-watchdog"]) {
    fs.writeFileSync(path.join(home, "Library/LaunchAgents", `${label}.plist`), "<plist/>");
    fs.writeFileSync(path.join(loaded, label), "");
  }
  const db = path.join(root, "omon_gateway.db");
  fs.writeFileSync(db, "live");
  stub(bin, "uname", 'echo Darwin');
  stub(bin, "pgrep", 'exit 1');
  stub(bin, "plutil", 'exit 1');
  // BSD mktemp keeps X's that are not at the end of the template literally, so a template
  // like omo.XXXXXXXX.backup names the same file every run. Model that as a failure.
  stub(bin, "mktemp", `last="\${@: -1}"
if [[ ! "$last" =~ XXX+$ ]]; then echo "mktemp: non-trailing X template $last" >&2; exit 1; fi
exec ${realMktemp} "$@"`);
  stub(bin, "launchctl", `echo "launchctl $*" >>"${log}"
case "$1" in
  print) [[ -e "${loaded}/\${2##*/}" ]] ;;
  bootout) rm -f "${loaded}/\${2##*/}" ;;
  bootstrap) label="$(basename "$3" .plist)"; touch "${loaded}/$label" ;;
esac`);
  stub(bin, "sqlite3", `echo "sqlite3 $*" >>"${log}"
dest="\${2#.backup \\'}"; dest="\${dest%\\'}"; cp "$1" "$dest"`);
  stub(bin, "curl", `echo "${health}"`);
  stub(bin, "ferryx", `echo "ferryx $*" >>"${log}"
case "$2" in
  migrate) echo "issues 1"; [[ -f "$4" ]] ;;
  start) [[ "${start}" == ok ]] && echo '{"status":"ok","op":"start"}' || { echo '{"status":"failed"}'; exit 1; } ;;
  stop) [[ "${stop}" == ok ]] && echo '{"status":"ok","op":"stop"}' || exit 1 ;;
  status)
    if [[ "${status}" == healthy ]]; then echo '{"status":"ok","healthy":true}'; exit 0; fi
    echo '{"status":"ok","healthy":false,"problems":["slack socket mode is not connected"]}'
    [[ "$3" == --check ]] && exit 1; exit 0 ;;
esac`);
  const run = (args, extraEnv = {}) => {
    const result = spawnSync("bash", [cutover, ...args], {
      env: {
        PATH: `${bin}:/usr/bin:/bin`,
        HOME: home,
        TMPDIR: tmp,
        FERRYX_BIN: "ferryx",
        HUB_VERIFY_SECONDS: "1",
        HUB_VERIFY_INTERVAL: "0.1",
        ...extraEnv,
      },
      encoding: "utf8",
    });
    const calls = fs.existsSync(log) ? fs.readFileSync(log, "utf8") : "";
    return { ...result, calls };
  };
  const isLoaded = (label) => fs.existsSync(path.join(loaded, label));
  const leftovers = () => fs.readdirSync(tmp);
  return { root, home, db, run, isLoaded, leftovers, cleanup: () => fs.rmSync(root, { recursive: true, force: true }) };
}

test("a healthy cutover snapshots into a private dir, verifies the hub, and leaves omo unloaded", (t) => {
  const box = sandbox();
  t.after(box.cleanup);
  const result = box.run(["cutover", "--db", box.db, "--yes"]);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /cutover done/);
  assert.match(result.calls, /sqlite3 .*omo-cutover\.[A-Za-z0-9]{8}\/snapshot\.backup/);
  assert.match(result.calls, /sqlite3 .*omo-cutover\.[A-Za-z0-9]{8}\/delta\.backup/);
  assert.match(result.calls, /ferryx hub status --check/);
  assert.equal(box.isLoaded("ai.omon.gateway"), false);
  assert.equal(box.isLoaded("ai.omon.gateway-watchdog"), false);
  assert.deepEqual(box.leftovers(), [], "snapshots of private data are removed");
});

test("an unhealthy hub is stopped before omo is loaded again", (t) => {
  const box = sandbox({ status: "unhealthy" });
  t.after(box.cleanup);
  const result = box.run(["cutover", "--db", box.db, "--yes"]);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /slack socket mode is not connected/);
  const stopAt = result.calls.indexOf("ferryx hub stop");
  const reloadAt = result.calls.indexOf("launchctl bootstrap");
  assert.ok(stopAt >= 0, "hub stop was sent");
  assert.ok(reloadAt > stopAt, "omo is loaded only after the hub stopped");
  assert.equal(box.isLoaded("ai.omon.gateway"), true);
  assert.deepEqual(box.leftovers(), []);
});

test("an unhealthy hub that will not stop keeps omo unloaded", (t) => {
  const box = sandbox({ status: "unhealthy", stop: "fail" });
  t.after(box.cleanup);
  const result = box.run(["cutover", "--db", box.db, "--yes"]);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /omo was NOT loaded again/);
  assert.doesNotMatch(result.calls, /launchctl bootstrap/);
  assert.equal(box.isLoaded("ai.omon.gateway"), false);
});

test("a failed hub start reply loads omo again", (t) => {
  const box = sandbox({ start: "fail" });
  t.after(box.cleanup);
  const result = box.run(["cutover", "--db", box.db, "--yes"]);
  assert.equal(result.status, 1);
  assert.equal(box.isLoaded("ai.omon.gateway"), true);
  assert.doesNotMatch(result.calls, /hub status/);
});

test("a health URL that is down fails verification even when status passes", (t) => {
  const box = sandbox({ health: "000" });
  t.after(box.cleanup);
  const result = box.run(["cutover", "--db", box.db, "--yes"]);
  assert.equal(result.status, 1);
  assert.equal(box.isLoaded("ai.omon.gateway"), true);
});

test("--no-verify skips the status wait", (t) => {
  const box = sandbox({ status: "unhealthy" });
  t.after(box.cleanup);
  const result = box.run(["cutover", "--db", box.db, "--no-verify", "--yes"]);
  assert.equal(result.status, 0, result.stderr);
  assert.doesNotMatch(result.calls, /hub status/);
  assert.equal(box.isLoaded("ai.omon.gateway"), false);
});

test("every mktemp template in the cutover script ends in X's (BSD mktemp)", () => {
  const source = fs.readFileSync(cutover, "utf8");
  const templates = [...source.matchAll(/\$\(mktemp(?: -[a-z]+)* "([^"]+)"\)/g)].map((m) => m[1]);
  assert.ok(templates.length > 0);
  for (const template of templates) {
    assert.match(template, /X{6,}$/, template);
  }
});

test("rollback does not load omo when hub stop fails", (t) => {
  const box = sandbox({ stop: "fail" });
  t.after(box.cleanup);
  box.run(["cutover", "--db", box.db, "--yes"]);
  const result = box.run(["rollback", "--yes"]);
  assert.equal(result.status, 1);
  assert.equal(box.isLoaded("ai.omon.gateway"), false);
});

test("install-watchdog renders absolute paths and loads the job", (t) => {
  const box = sandbox();
  t.after(box.cleanup);
  const result = box.run(["install-watchdog", "--yes"]);
  assert.equal(result.status, 0, result.stderr);
  const plist = fs.readFileSync(
    path.join(box.home, "Library/LaunchAgents/com.ferryx.hub-watchdog.plist"),
    "utf8",
  );
  assert.doesNotMatch(plist, /__[A-Z_]+__/);
  assert.match(plist, new RegExp(`<string>${watchdog.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}</string>`));
  assert.match(plist, /<key>WATCHDOG_FERRYX_BIN<\/key>\s*<string>\/.*\/ferryx<\/string>/);
  assert.equal(box.isLoaded("com.ferryx.hub-watchdog"), true);
});

function runWatchdog(box, extraEnv) {
  return spawnSync("bash", [watchdog], {
    env: {
      PATH: `${path.join(box.root, "bin")}:/usr/bin:/bin`,
      HOME: box.home,
      WATCHDOG_DRY_ALERT: "1",
      WATCHDOG_FAIL_LIMIT: "1",
      ...extraEnv,
    },
    encoding: "utf8",
  });
}

test("the watchdog alerts when HTTP is up but hub status fails", (t) => {
  const box = sandbox({ status: "unhealthy" });
  t.after(box.cleanup);
  const ok = runWatchdog(box, {});
  assert.equal(ok.status, 0, "without WATCHDOG_FERRYX_BIN only HTTP is checked");
  const result = runWatchdog(box, { WATCHDOG_FERRYX_BIN: "ferryx" });
  assert.equal(result.status, 1);
  assert.match(result.stdout, /DRY_ALERT health ferryx hub hub status --check failed 1 checks/);
});

test("rollback unloads the ferryx watchdog and loads omo after the hub stopped", (t) => {
  const box = sandbox();
  t.after(box.cleanup);
  box.run(["cutover", "--db", box.db, "--yes"]);
  box.run(["install-watchdog", "--yes"]);
  assert.equal(box.isLoaded("com.ferryx.hub-watchdog"), true);
  const result = box.run(["rollback", "--yes"]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(box.isLoaded("com.ferryx.hub-watchdog"), false);
  assert.equal(box.isLoaded("ai.omon.gateway"), true);
  const calls = result.calls;
  assert.ok(calls.lastIndexOf("ferryx hub stop") < calls.lastIndexOf("launchctl bootstrap"));
});
