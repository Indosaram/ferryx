import test from "node:test";
import assert from "node:assert/strict";

import { DEFAULT_SIGNING_IDENTITY, preflight, updaterKeyFromEnv } from "./build-notarized-local.mjs";

const SECRET = "untrusted-test-secret-value";

function fakeHost({
  version = "2026.1022.6",
  installed = "2026.1022.5",
  dirty = "",
  helpers = 0,
  identity = true,
  envText = `TAURI_SIGNING_PRIVATE_KEY=${SECRET}\nTAURI_SIGNING_PRIVATE_KEY_PASSWORD=pw\n`,
  tauri = 0,
} = {}) {
  const calls = [];
  const files = {
    "/wt/src-tauri/tauri.conf.json": JSON.stringify({ version }),
    "/main/.env": envText,
  };
  return {
    calls,
    worktree: "/wt",
    mainRepo: "/main",
    env: {},
    readFile: (file) => {
      if (!(file in files)) throw new Error(`ENOENT ${file}`);
      return files[file];
    },
    exists: (file) => file in files || (file === "/Applications/Ferryx.app" && installed !== null),
    exec: (cmd, args) => {
      calls.push([cmd, ...args].join(" "));
      if (cmd === "cargo" && args[0] === "tauri") return { exit: tauri, out: "" };
      if (cmd === "git") return { exit: 0, out: dirty };
      if (cmd === "plutil") return { exit: 0, out: `${installed}\n` };
      if (cmd === "node") return { exit: helpers, out: helpers ? "fingerprint mismatch" : "verified" };
      if (cmd === "security") {
        return { exit: 0, out: identity ? `  1) ABC "${DEFAULT_SIGNING_IDENTITY}"\n` : "0 valid identities found\n" };
      }
      return { exit: 0, out: "" };
    },
  };
}

test("a ready host passes the preflight and checks the helper fingerprint", () => {
  const host = fakeHost();
  const result = preflight(host);
  assert.deepEqual(result.failures, []);
  assert.equal(result.ok, true);
  assert.ok(host.calls.some((call) => call.includes("build-remote-helpers.mjs --check-bundled /wt")));
});

test("an equal or older version than the installed app fails before the build", () => {
  const equal = preflight(fakeHost({ installed: "2026.1022.6" }));
  assert.equal(equal.ok, false);
  assert.match(equal.failures.join("\n"), /equals the installed version/);
  const older = preflight(fakeHost({ installed: "2026.1023.1" }));
  assert.match(older.failures.join("\n"), /older than the installed version/);
  const fresh = preflight(fakeHost({ installed: null }));
  assert.equal(fresh.ok, true, "no installed app is fine");
});

test("each missing prerequisite is named", () => {
  const result = preflight(
    fakeHost({ dirty: " M src/lib.rs", helpers: 1, identity: false, envText: "", tauri: 1 }),
  );
  const text = result.failures.join("\n");
  assert.match(text, /uncommitted changes/);
  assert.match(text, /bundled remote helpers do not match source/);
  assert.match(text, /TAURI_SIGNING_PRIVATE_KEY missing/);
  assert.match(text, /signing identity not in the keychain/);
  assert.match(text, /cargo tauri is not available/);
});

test("the updater key value never appears in preflight output", () => {
  const ok = preflight(fakeHost());
  const bad = preflight(fakeHost({ installed: "2026.1022.6", identity: false }));
  for (const result of [ok, bad]) {
    assert.doesNotMatch(JSON.stringify(result), new RegExp(SECRET));
  }
  assert.equal(updaterKeyFromEnv(`TAURI_SIGNING_PRIVATE_KEY=${SECRET}`).key, SECRET);
  assert.equal(updaterKeyFromEnv("").key, "");
});
