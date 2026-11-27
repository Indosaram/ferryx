import assert from "node:assert/strict";
import test from "node:test";
import { spawn } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPTS = dirname(fileURLToPath(import.meta.url));
const LIB = join(SCRIPTS, "lib/build-host-claims.sh");

// Every case runs against a private lock directory. The real
// /tmp/ferryx-build-host-locks is never read or written by this suite.
function sandbox(t) {
  const root = mkdtempSync(join(tmpdir(), "fx-claim-"));
  const dir = join(root, "locks");
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return { root, dir };
}

function run(dir, script, extraEnv = {}) {
  return new Promise(resolve => {
    const child = spawn("bash", ["-c", `set -u\n. "${LIB}"\n${script}`, "fx"], {
      env: { ...process.env, FERRYX_BUILD_LOCKDIR: dir, LC_ALL: "C", ...extraEnv },
    });
    let out = "", err = "";
    child.stdout.on("data", d => (out += d));
    child.stderr.on("data", d => (err += d));
    child.on("close", code => resolve({ code, out, err }));
  });
}

// Above the largest pid any supported kernel can allocate (Linux 2^22-1, macOS 99998).
const DEAD_PID = 4194305;
const legacyPayload = (kind, val) => `${kind} ${val}\n`;

const sq = s => `'${s.replace(/'/g, `'\\''`)}'`;
const live = `pid ${process.pid}`;

test("legacy until-lease written by an old picker is honoured as held", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "h1"), legacyPayload("until", Math.floor(Date.now() / 1000) + 600));
  const r = await run(dir, 'fx_claim_acquire h1 "pid $$"');
  assert.equal(r.code, 2);
  assert.equal(readFileSync(join(dir, "h1"), "utf8").startsWith("until "), true);
});

test("legacy live pid-lease written by an old picker is honoured as held", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "h2"), legacyPayload("pid", process.pid));
  const r = await run(dir, 'fx_claim_acquire h2 "until 1"');
  assert.equal(r.code, 2);
  assert.equal(readFileSync(join(dir, "h2"), "utf8"), legacyPayload("pid", process.pid));
});

test("a claim this code writes is read as held by the legacy picker reader", async t => {
  const { dir } = sandbox(t);
  const own = await run(dir, `fx_claim_acquire h3 ${sq(live)} && cat "$(fx_lockdir)/h3"`);
  assert.equal(own.code, 0);
  assert.match(own.out.trim(), /^pid \d+$/, "claim file must stay two-field legacy shape");
  const legacyReader = `
    lock=${JSON.stringify(join(dir, "h3"))}
    claimed() {
      local kind val; [ -f "$1" ] || return 1; read -r kind val < "$1" || return 1
      case "$kind" in
        pid) kill -0 "$val" 2>/dev/null ;;
        until) [ "$(date +%s)" -lt "$val" ] ;;
        *) return 1 ;;
      esac
    }
    claimed "$lock"`;
  const legacy = await new Promise(resolve => {
    const child = spawn("bash", ["-c", legacyReader]);
    child.on("close", code => resolve(code));
  });
  assert.equal(legacy, 0, "legacy reader must see the new claim as held");
});

test("legacy rm -f release still clears a claim this code wrote", async t => {
  const { dir } = sandbox(t);
  await run(dir, `fx_claim_acquire h4 ${sq(live)}`);
  assert.equal(existsSync(join(dir, "h4")), true);
  await new Promise(resolve => {
    const child = spawn("bash", ["-c", `rm -f ${JSON.stringify(join(dir, "h4"))}`]);
    child.on("close", resolve);
  });
  assert.equal(existsSync(join(dir, "h4")), false);
  const again = await run(dir, `fx_claim_acquire h4 ${sq(live)}`);
  assert.equal(again.code, 0, "a legacy-released host must be acquirable again");
});

test("concurrent acquirers produce exactly one winner", async t => {
  const { dir } = sandbox(t);
  const results = await Promise.all(Array.from({ length: 12 }, () =>
    run(dir, `fx_claim_acquire racer ${sq(live)} || { [ $? -eq 2 ] && echo held; }`)));
  const winners = results.filter(r => r.code === 0 && r.out.trim() === "");
  assert.equal(winners.length, 1);
  assert.match(readFileSync(join(dir, "racer"), "utf8").trim(), /^pid \d+$/);
});

test("an unreadable or truncated claim is held, never free", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "trunc"), "");
  writeFileSync(join(dir, "garbage"), "not-a-claim\n");
  writeFileSync(join(dir, "badnum"), "until later\n");
  mkdirSync(join(dir, "adir"));
  for (const host of ["trunc", "garbage", "badnum", "adir"]) {
    const r = await run(dir, `fx_claim_acquire ${host} "pid $$"`);
    assert.equal(r.code, 2, `${host} must read as held`);
  }
  assert.equal(readFileSync(join(dir, "trunc"), "utf8"), "");
  assert.equal(readFileSync(join(dir, "garbage"), "utf8"), "not-a-claim\n");
});

test("release refuses a live claim owned by another pid", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "mine"), legacyPayload("pid", process.pid));
  const r = await run(dir, 'fx_claim_release mine $$');
  assert.equal(r.code, 5);
  assert.equal(existsSync(join(dir, "mine")), true);
});

test("release of an own pid claim succeeds once and is idempotent", async t => {
  const { dir } = sandbox(t);
  const first = await run(dir, 'fx_claim_acquire own "pid $$"; fx_claim_release own $$');
  assert.equal(first.code, 0);
  assert.equal(existsSync(join(dir, "own")), false);
  const second = await run(dir, "fx_claim_release own $$");
  assert.equal(second.code, 0);
});

test("release clears a stale legacy lease left by an interrupted run", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "stale"), legacyPayload("until", Math.floor(Date.now() / 1000) - 5));
  const r = await run(dir, "fx_claim_release stale -");
  assert.equal(r.code, 0);
  assert.equal(existsSync(join(dir, "stale")), false);
});

test("release never removes a live legacy pid claim, even when it owns the name", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "foreign"), legacyPayload("pid", process.pid));
  const r = await run(dir, "fx_claim_release foreign - --force");
  assert.equal(r.code, 5);
  assert.equal(existsSync(join(dir, "foreign")), true);
});

test("--force never removes a claim this process does not own", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "junk"), "junk\n");
  const plain = await run(dir, "fx_claim_release junk -");
  assert.equal(plain.code, 5);
  assert.equal(existsSync(join(dir, "junk")), true);
  const forced = await run(dir, "fx_claim_release junk - --force");
  assert.equal(forced.code, 5, "--force must not widen a release");
  assert.equal(readFileSync(join(dir, "junk"), "utf8"), "junk\n");
});

// A parseable-but-unreadable claim becomes reapable on the grace timer instead of
// being force-deleted: the timer is evidence that nothing is using the host, and
// --force is not. An acquire is the only caller that wants the host enough to reap.
test("a malformed claim is reaped by acquire after the grace window, never by release", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "junk2"), "junk\n");
  const refused = await run(dir, `fx_claim_release junk2 - --force`);
  assert.equal(refused.code, 5);
  const young = await run(dir, `fx_claim_acquire junk2 ${sq(live)}`);
  assert.equal(young.code, 2, "a young unreadable claim is held");
  const aged = await run(dir, `fx_claim_acquire junk2 ${sq(live)}`, { FERRYX_BUILD_CLAIM_GRACE: "0" });
  assert.equal(aged.code, 0, "past the grace window the host is reapable again");
  // The writer terminates a claim with a newline (`printf '%s\n' "$payload" >"$tmp"`); the reader
  // tolerates its absence so an old truncating writer still parses. Both shapes are the format, so
  // this file - written by the acquire above - must be accepted with or without the final newline.
  assert.match(readFileSync(join(dir, "junk2"), "utf8"), /^pid \d+\n?$/);
});

// Only a wall-clock lease has an end that is a fact. A dead owner's pid claim does
// not, because its remote command may still be running - so release refuses it and
// leaves reaping to an acquire that actually wants the host.
test("ownerless release refuses a stale pid claim and clears only an expired lease", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "deadpid"), legacyPayload("pid", DEAD_PID));
  const stale = await run(dir, "fx_claim_release deadpid -", { FERRYX_BUILD_CLAIM_GRACE: "0" });
  assert.equal(stale.code, 5, "a stale pid claim is not a release target");
  assert.equal(readFileSync(join(dir, "deadpid"), "utf8"), legacyPayload("pid", DEAD_PID));
  const absent = await run(dir, "fx_claim_release expired -");
  assert.equal(absent.code, 0, "nothing to release is not a failure");
  writeFileSync(join(dir, "expired"), legacyPayload("until", Math.floor(Date.now() / 1000) - 1));
  const cleared = await run(dir, "fx_claim_release expired -");
  assert.equal(cleared.code, 0);
  assert.equal(existsSync(join(dir, "expired")), false);
});

test("a live lease is never cleared by an ownerless release, expired or not", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  const future = Math.floor(Date.now() / 1000) + 3600;
  writeFileSync(join(dir, "held"), legacyPayload("until", future));
  const r = await run(dir, "fx_claim_release held -");
  assert.equal(r.code, 5);
  assert.equal(readFileSync(join(dir, "held"), "utf8"), legacyPayload("until", future));
});

test("release refuses host names that would escape the lock directory", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  const sentinel = join(dir, "sentinel");
  writeFileSync(sentinel, "");
  for (const host of ["../etc/passwd", "a/b", ".", "..", "-rf", "HOST", "h*"]) {
    const r = await run(dir, `fx_claim_release ${JSON.stringify(host)} - --force`);
    assert.equal(r.code, 1, `${host} must be refused`);
  }
  assert.equal(existsSync(sentinel), true);
});

test("renew trades an own pid lease for a lease that survives interruption", async t => {
  const { dir } = sandbox(t);
  const r = await run(dir, 'fx_claim_acquire r "pid $$" && fx_claim_renew r $$ "until 9999999999"');
  assert.equal(r.code, 0);
  assert.equal(readFileSync(join(dir, "r"), "utf8"), "until 9999999999\n");
  const other = await run(dir, 'fx_claim_renew r $$ "until 1"');
  assert.equal(other.code, 1, "renew must not touch a claim owned by another pid");
  assert.equal(readFileSync(join(dir, "r"), "utf8"), "until 9999999999\n");
});

test("the pre-flight ownership check rejects a claim a legacy picker replaced", async t => {
  const { dir } = sandbox(t);
  const owned = await run(dir, `fx_claim_acquire own2 ${sq(live)}; fx_claim_owned_by own2 ${process.pid}`);
  assert.equal(owned.code, 0);
  const stolen = await run(dir, 'fx_claim_owned_by own2 1');
  assert.equal(stolen.code, 1, "another owner must not pass the check");
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "legacy"), legacyPayload("until", Math.floor(Date.now() / 1000) + 600));
  const legacyUntil = await run(dir, `fx_claim_owned_by legacy ${process.pid}`);
  assert.equal(legacyUntil.code, 1, "a lease we did not write is not ours");
});

test("a stale pid claim is reapable and a live one is not", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "dead"), legacyPayload("pid", DEAD_PID));
  const reaped = await run(dir, `fx_claim_acquire dead ${sq(live)}`, { FERRYX_BUILD_CLAIM_GRACE: "0" });
  assert.equal(reaped.code, 0);
  const holder = await run(dir, `fx_claim_acquire live ${sq(live)}; cat "$(fx_lockdir)/live"`);
  assert.equal(holder.code, 0);
  const blocked = await run(dir, `fx_claim_acquire live ${sq(live)}`);
  assert.equal(blocked.code, 2);
});

test("quoted argv survives a remote shell round trip word for word", async t => {
  const { dir } = sandbox(t);
  const words = ["cargo", "test", "a b", "semi;colon", "$(id)", "`id`", "*", "a'b", 'a"b', "back\\slash", "", "-n", "&&"];
  const script = [
    `q=$(fx_quote_argv ${words.map(sq).join(" ")})`,
    `eval "set -- $q"`,
    `printf '%s\\0' "$@"`,
  ].join("\n");
  const r = await run(dir, script);
  assert.equal(r.code, 0);
  assert.deepEqual(r.out.split("\0").slice(0, -1), words);
});

test("a claim written without a trailing newline still parses", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "nonl"), live);
  const held = await run(dir, 'fx_claim_acquire nonl "until 1"');
  assert.equal(held.code, 2);
  writeFileSync(join(dir, "nonl"), `pid ${DEAD_PID}`);
  const reaped = await run(dir, `fx_claim_acquire nonl ${sq(live)}`, { FERRYX_BUILD_CLAIM_GRACE: "0" });
  assert.equal(reaped.code, 0);
});

test("a pid claim whose owner just died is held for the grace window", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "killed"), legacyPayload("pid", DEAD_PID));
  const fresh = await run(dir, `fx_claim_acquire killed ${sq(live)}`);
  assert.equal(fresh.code, 2, "a freshly dead owner may still have a remote command running");
  const expired = await run(dir, `fx_claim_acquire killed ${sq(live)}`, { FERRYX_BUILD_CLAIM_GRACE: "0" });
  assert.equal(expired.code, 0, "with the grace window closed the host is reapable again");
});

test("no-owner release never clears a live lease it did not write", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  const future = Math.floor(Date.now() / 1000) + 3600;
  writeFileSync(join(dir, "lease"), legacyPayload("until", future));
  for (const force of ["-", "- --force"]) {
    const r = await run(dir, `fx_claim_release lease ${force}`);
    assert.equal(r.code, 5, `no-owner release must refuse a live lease (${force})`);
    assert.equal(readFileSync(join(dir, "lease"), "utf8"), legacyPayload("until", future));
  }
});

// A lease names no process, so without its epoch the session that took it could only wait
// it out. The epoch the picker printed is the token: it ends exactly that lease, and a
// different epoch - another session's lease on the same name - is still refused.
test("a live lease is released by its own epoch and refused for any other", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  const future = Math.floor(Date.now() / 1000) + 3600;
  writeFileSync(join(dir, "lease"), legacyPayload("until", future));
  const wrong = await run(dir, `fx_claim_release lease - ${future + 1}`);
  assert.equal(wrong.code, 5, "another lease's epoch is a refusal");
  assert.equal(readFileSync(join(dir, "lease"), "utf8"), legacyPayload("until", future));
  writeFileSync(join(dir, "pidclaim"), legacyPayload("pid", process.pid));
  const pid = await run(dir, `fx_claim_release pidclaim - ${process.pid}`);
  assert.equal(pid.code, 5, "an epoch never matches a pid claim");
  assert.equal(existsSync(join(dir, "pidclaim")), true);
  const own = await run(dir, `fx_claim_release lease - ${future}`);
  assert.equal(own.code, 0);
  assert.equal(existsSync(join(dir, "lease")), false);
  const again = await run(dir, `fx_claim_release lease - ${future}`);
  assert.equal(again.code, 0, "releasing a lease that is already gone is not an error");
});

// Not a fix: a legacy writer's check-then-truncate cannot be made safe from this
// side. The best available property is that the overwrite is *detected* before
// the build starts, so a second run fails closed instead of sharing the host.
test("a legacy truncating write over this code's claim is detected, not prevented", async t => {
  const { dir } = sandbox(t);
  const r = await run(dir, [
    `fx_claim_acquire taken ${sq(live)}`,
    `printf 'until %s\\n' $(( $(date +%s) + 600 )) > "$(fx_lockdir)/taken"`,
    `fx_claim_owned_by taken ${process.pid} && echo still-ours || echo taken-over`,
  ].join("\n"));
  assert.equal(r.out.trim(), "taken-over", "the overwrite must be visible to the ownership check");
  assert.equal(readFileSync(join(dir, "taken"), "utf8").startsWith("until "), true);
});

test("marker status takes the last marker and stays empty when none is sent", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "ok"), "log line\n__FERRYX_EXIT__0\n");
  writeFileSync(join(dir, "fail"), "boom\n__FERRYX_EXIT__7\n");
  writeFileSync(join(dir, "none"), "boom\n");
  writeFileSync(join(dir, "spoof"), "__FERRYX_EXIT__0\n__FERRYX_EXIT__3\n");
  writeFileSync(join(dir, "crlf"), "__FERRYX_EXIT__4\r\n");
  assert.equal((await run(dir, `fx_marker_status ${JSON.stringify(join(dir, "ok"))}`)).out.trim(), "0");
  assert.equal((await run(dir, `fx_marker_status ${JSON.stringify(join(dir, "fail"))}`)).out.trim(), "7");
  assert.equal((await run(dir, `fx_marker_status ${JSON.stringify(join(dir, "none"))}`)).out.trim(), "");
  assert.equal((await run(dir, `fx_marker_status ${JSON.stringify(join(dir, "spoof"))}`)).out.trim(), "3");
  assert.equal((await run(dir, `fx_marker_status ${JSON.stringify(join(dir, "crlf"))}`)).out.trim(), "");
  assert.equal((await run(dir, `fx_marker_status ${JSON.stringify(join(dir, "absent"))}`)).out.trim(), "");
});

test("marker stripping leaves no marker line and keeps real output", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  const f = join(dir, "mix");
  writeFileSync(f, "compiling\n__FERRYX_EXIT__2\ndone\n");
  const r = await run(dir, `sed '/^__FERRYX_EXIT__[0-9][0-9]*$/d' ${JSON.stringify(f)}`);
  assert.equal(r.out, "compiling\ndone\n");
});

// --- compare-and-remove -------------------------------------------------------

// The restore branch of fx_claim_detach: when the file that was renamed away no
// longer satisfies the predicate, it goes back under its own name byte for byte.
// This is what keeps a release from discarding a claim a concurrent run installed.
test("a detach whose predicate fails puts the claim back unchanged", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  const bytes = legacyPayload("pid", process.pid);
  writeFileSync(join(dir, "restore"), bytes);
  const r = await run(dir, [
    `fx_claim_guard restore && fx_claim_detach restore fx_claim_owned_file 1; rc=$?`,
    `fx_claim_unguard restore; echo "rc=$rc"`,
  ].join("\n"));
  assert.match(r.out, /rc=1/, "a claim owned by another pid is not removed");
  assert.equal(readFileSync(join(dir, "restore"), "utf8"), bytes, "bytes survive the failed compare");
  assert.equal(existsSync(join(dir, ".restore.guard")), false, "the guard is dropped");
  assert.deepEqual(readdirSync(dir).filter(n => n.includes(".restore.tomb")), [],
    "no tombstone is left behind");
});

test("a reap of a stale claim has exactly one winner and leaves no guard or tombstone", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "reaped"), legacyPayload("pid", DEAD_PID));
  // Every contender installs the same live owner, so only the stale-reap decision
  // can let one through: a second winner would mean the reap dropped a claim.
  const results = await Promise.all(Array.from({ length: 16 }, () =>
    run(dir, `fx_claim_acquire reaped ${sq(live)}`, { FERRYX_BUILD_CLAIM_GRACE: "0" })));
  const winners = results.filter(r => r.code === 0);
  assert.equal(winners.length, 1, "one reap wins, the rest see the installed claim");
  results.filter(r => r.code !== 0).forEach(r => assert.equal(r.code, 2));
  assert.match(readFileSync(join(dir, "reaped"), "utf8").trim(), /^pid \d+$/);
  const leftovers = readdirSync(dir).filter(n => n.startsWith(".reaped"));
  assert.deepEqual(leftovers, [], "guard and tombstone names are cleaned up");
});

test("release and acquire racing on one host never lose the installed claim", async t => {
  const { dir } = sandbox(t);
  mkdirSync(dir, { recursive: true });
  // Owner releases its own claim while other runs acquire the same name. Whatever
  // the interleaving, the name must hold a well-formed claim or be absent - never a
  // half-written file, and never a claim whose owner was told it still holds it.
  const script = `
    fx_claim_acquire racer "pid $$" >/dev/null 2>&1
    fx_claim_release racer $$ >/dev/null 2>&1; echo "rel=$?"`;
  const rounds = Array.from({ length: 10 }, () => run(dir, script));
  const contenders = Array.from({ length: 10 }, (_, i) =>
    run(dir, `fx_claim_acquire racer "pid ${2000 + i}" >/dev/null 2>&1; echo "acq=$?"`));
  const all = await Promise.all([...rounds, ...contenders]);
  // Each contender echoes exactly one exit code; `echo` adds the newline, which is not part of it.
  all.forEach(r => assert.match(r.out.trim(), /^(rel|acq)=\d$/, "each contender reports one exit code"));
  if (existsSync(join(dir, "racer"))) {
    assert.match(readFileSync(join(dir, "racer"), "utf8").trim(), /^(pid \d+|until \d+)$/);
  }
  const leftovers = readdirSync(dir).filter(n => n.startsWith(".racer"));
  assert.deepEqual(leftovers, [], "no guard, new or tombstone file survives the race");
});

// ln(1) reports success when its second argument is an existing directory, linking
// *inside* it. Without an explicit existence check a stray directory in the lock dir
// would read as a host this run had just acquired.
test("a directory where a claim name belongs is held, and nothing is linked into it", async t => {
  const { dir } = sandbox(t);
  mkdirSync(join(dir, "adir"), { recursive: true });
  const r = await run(dir, `fx_claim_acquire adir ${sq(live)}`);
  assert.equal(r.code, 2, "a directory claim is unparsable, so it is held");
  assert.deepEqual(readdirSync(join(dir, "adir")), [], "no claim is written inside it");
});

// The guard only has to serialise the reap path, so it is exercised through a stale
// claim: a live writer stops the reap, and a writer that is gone is taken over.
test("a guard held by a live writer blocks the reap; an abandoned one is taken over", async t => {
  const { dir } = sandbox(t);
  mkdirSync(join(dir, ".guarded.guard"), { recursive: true });
  writeFileSync(join(dir, ".guarded.guard", "pid"), `${process.pid}\n`);
  writeFileSync(join(dir, "guarded"), legacyPayload("pid", DEAD_PID));
  const blocked = await run(dir, `fx_claim_acquire guarded ${sq(live)}`, { FERRYX_BUILD_CLAIM_GRACE: "0" });
  assert.equal(blocked.code, 2, "a live guard stops the reap");
  assert.equal(readFileSync(join(dir, "guarded"), "utf8"), legacyPayload("pid", DEAD_PID));
  assert.equal(readFileSync(join(dir, ".guarded.guard", "pid"), "utf8"), `${process.pid}\n`,
    "a live guard is never broken");
});

// The pre-repair code wrote the guard as a directory holding a `pid` child, and a
// claim as an unconditional rm target. A tree upgraded mid-run can hold both.
test("an abandoned directory-layout guard is cleared so it cannot wedge a host", async t => {
  const { dir } = sandbox(t);
  mkdirSync(join(dir, ".old.guard"), { recursive: true });
  writeFileSync(join(dir, ".old.guard", "pid"), `${DEAD_PID}\n`);
  writeFileSync(join(dir, "old"), legacyPayload("pid", DEAD_PID));
  const r = await run(dir, `fx_claim_acquire old ${sq(live)}`, { FERRYX_BUILD_CLAIM_GRACE: "0" });
  assert.equal(r.code, 0, "the stale claim is reapable once the guard is abandoned");
  assert.equal(existsSync(join(dir, ".old.guard")), false, "the abandoned guard is removed");
  assert.match(readFileSync(join(dir, "old"), "utf8").trim(), /^pid \d+$/);
});

// The picker maps the library's three outcomes onto its own exit codes, and phase G
// of the runbook runs exactly this command. A wrong mapping turns "not ours" into
// "bad host name" and invites a retry that shares the machine.
test("picker --release maps each library outcome to its documented exit code", async t => {
  const { root, dir } = sandbox(t);
  const scripts = join(root, "scripts");
  mkdirSync(join(scripts, "lib"), { recursive: true });
  copyFileSync(LIB, join(scripts, "lib", "build-host-claims.sh"));
  copyFileSync(join(SCRIPTS, "pick-build-host.sh"), join(scripts, "pick-build-host.sh"));
  chmodSync(join(scripts, "pick-build-host.sh"), 0o755);
  const invoke = (args, extraEnv = {}) => new Promise(resolve => {
    const child = spawn("bash", [join(scripts, "pick-build-host.sh"), ...args], {
      env: {
        ...process.env,
        FERRYX_BUILD_LOCKDIR: dir,
        FERRYX_BUILD_HOSTS_FILE: join(root, "roster"),
        FERRYX_BUILD_CLAIM_GRACE: "0",
        LC_ALL: "C",
        ...extraEnv,
      },
    });
    let err = "";
    child.stderr.on("data", d => (err += d));
    child.on("close", code => resolve({ code, err }));
  });
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "mine"), legacyPayload("pid", process.pid));
  const other = await invoke(["--release", "mine"], { FERRYX_BUILD_OWNER_PID: "1" });
  assert.equal(other.code, 5, "another pid's claim is a refusal, not a usage error");
  assert.equal(existsSync(join(dir, "mine")), true);
  writeFileSync(join(dir, "own"), legacyPayload("pid", 4242));
  const ok = await invoke(["--release", "own"], { FERRYX_BUILD_OWNER_PID: "4242" });
  assert.equal(ok.code, 0);
  assert.equal(existsSync(join(dir, "own")), false);
  const absent = await invoke(["--release", "nothing-here"]);
  assert.equal(absent.code, 0, "releasing a free host is not an error");
  for (const bad of ["../etc/passwd", "a/b", "-rf", "HOST", "."]) {
    const r = await invoke(["--release", bad], { FERRYX_BUILD_OWNER_PID: "4242" });
    assert.equal(r.code, 2, `${bad} is a usage error`);
  }
  const noarg = await invoke(["--release"]);
  assert.equal(noarg.code, 2);
  const future = Math.floor(Date.now() / 1000) + 3600;
  writeFileSync(join(dir, "leased"), legacyPayload("until", future));
  const bare = await invoke(["--release", "leased"]);
  assert.equal(bare.code, 5, "a live lease without its epoch stays held");
  assert.match(bare.err, /--lease <epoch>/, "the refusal names the way to release a lease");
  const badLease = await invoke(["--release", "leased", "--lease", "soon"]);
  assert.equal(badLease.code, 2, "a non-numeric epoch is a usage error");
  const mine = await invoke(["--release", "leased", "--lease", String(future)]);
  assert.equal(mine.code, 0);
  assert.equal(existsSync(join(dir, "leased")), false);
});

// --- build-remote.sh, end to end over stub transports -------------------------

// A private copy of the real build-remote.sh, with the roster-reading picker and
// the ssh binary replaced by stubs. Nothing here reaches a network or a real host:
// the claim library under test is the shipped one.
function remoteSandbox(t, mode) {
  const { root, dir } = sandbox(t);
  const scripts = join(root, "scripts");
  const bin = join(root, "bin");
  mkdirSync(join(scripts, "lib"), { recursive: true });
  mkdirSync(bin, { recursive: true });
  copyFileSync(LIB, join(scripts, "lib", "build-host-claims.sh"));
  copyFileSync(join(SCRIPTS, "build-remote.sh"), join(scripts, "build-remote.sh"));
  chmodSync(join(scripts, "build-remote.sh"), 0o755);
  writeFileSync(join(scripts, "pick-build-host.sh"), [
    "#!/usr/bin/env bash",
    "set -u",
    `HERE=$(cd "$(dirname "$0")" && pwd)`,
    ". \"$HERE/lib/build-host-claims.sh\"",
    '[ -n "${FERRYX_BUILD_OWNER_PID:-}" ] || exit 1',
    `fx_claim_acquire stubhost "pid $FERRYX_BUILD_OWNER_PID" || exit 3`,
    `echo stubhost`,
  ].join("\n"));
  chmodSync(join(scripts, "pick-build-host.sh"), 0o755);
  const roster = join(root, "roster");
  writeFileSync(roster, "stubhost ssh-unix stub-target\n");
  // The stub ssh consumes its stdin, then emits exactly what the mode says. The
  // "steal" mode replaces the claim mid-run, which is what a legacy picker does.
  writeFileSync(join(bin, "ssh"), [
    "#!/usr/bin/env bash",
    "cat >/dev/null",
    'case "$STUBMODE" in',
    `  ok) echo "__FERRYX_EXIT__0" ;;`,
    `  fail) echo "remote boom"; echo "__FERRYX_EXIT__7" ;;`,
    `  nomarker) echo "remote log, then silence" ;;`,
    `  steal) printf 'until %s\\n' "$(( $(date +%s) + 600 ))" > "$FERRYX_BUILD_LOCKDIR/stubhost"; echo "__FERRYX_EXIT__0" ;;`,
    "esac",
  ].join("\n"));
  chmodSync(join(bin, "ssh"), 0o755);
  const invoke = (extraEnv = {}) => new Promise(resolve => {
    const child = spawn("bash", [join(scripts, "build-remote.sh"), "s", "--", "true"], {
      env: {
        ...process.env,
        PATH: `${bin}:${process.env.PATH}`,
        FERRYX_BUILD_LOCKDIR: dir,
        FERRYX_BUILD_HOSTS_FILE: roster,
        FERRYX_BUILD_CLAIM_GRACE: "0",
        LC_ALL: "C",
        STUBMODE: mode,
        ...extraEnv,
      },
    });
    let err = "";
    child.stderr.on("data", d => (err += d));
    child.on("close", code => resolve({ code, err }));
  });
  return { dir, invoke };
}

test("build-remote releases its own claim on a marker-reported success", async t => {
  const { dir, invoke } = remoteSandbox(t, "ok");
  const r = await invoke();
  assert.equal(r.code, 0, r.err);
  assert.equal(existsSync(join(dir, "stubhost")), false, "the claim is gone after a clean run");
});

test("build-remote reports the remote status from the marker", async t => {
  const { dir, invoke } = remoteSandbox(t, "fail");
  const r = await invoke();
  assert.equal(r.code, 7, r.err);
  assert.equal(existsSync(join(dir, "stubhost")), false, "a failed build still releases");
});

// The repaired defect: no marker means "the command may still be running", so the
// host is kept on a wall-clock lease instead of being handed to the next session.
test("a missing marker fails the run and leaves a lease instead of releasing", async t => {
  const { dir, invoke } = remoteSandbox(t, "nomarker");
  const r = await invoke();
  assert.equal(r.code, 1, "an absent marker is a failure, not a success");
  const claim = readFileSync(join(dir, "stubhost"), "utf8").trim();
  assert.match(claim, /^until \d+$/, "the host stays held on a time lease");
  assert.ok(Number(claim.split(" ")[1]) > Math.floor(Date.now() / 1000),
    "the lease is in the future, so the host is not reusable now");
});

// The other repaired defect: a release that refuses must not be swallowed. Here the
// claim is replaced under the run's feet, which is what a legacy picker does.
test("build-remote surfaces a refused release instead of exiting clean", async t => {
  const { dir, invoke } = remoteSandbox(t, "steal");
  const r = await invoke();
  assert.equal(r.code, 5, r.err);
  assert.match(r.err, /not ours/, "the refusal is reported, not discarded");
  const claim = readFileSync(join(dir, "stubhost"), "utf8").trim();
  assert.match(claim, /^until \d+$/, "the claim that replaced ours is untouched");
});
