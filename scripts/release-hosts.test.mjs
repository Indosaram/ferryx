import assert from "node:assert/strict";
import childProcess, { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { syncBuiltinESMExports } from "node:module";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  loadHostConfig,
  validateHostConfig,
  runProcess,
  runHostScript,
  quoteSh,
  quotePowerShell,
} from "./lib/release-hosts.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "..");
const EXAMPLE_CONFIG_PATH = join(__dirname, "release-hosts.example.json");

function makeValidConfig(overrides = {}) {
  return {
    schemaVersion: 1,
    repository: "/Users/indo/code/project/orca-lite",
    ghosttyRepository: "/Users/indo/code/project/orca-lite/src-tauri/vendor/ghostty",
    repo: "Indosaram/ferryx",
    hosts: {
      macbook: {
        ssh: null,
        platform: "darwin",
        root: "/Users/indo/ferryx-release-builds",
        minFreeBytes: 32212254720,
        notaryProfile: "FerryxNotary",
        signingIdentity: "Developer ID Application: Indo Yoon (5DUM8WPB4C)",
      },
      omaki: {
        ssh: "omaki",
        platform: "linux",
        root: "/home/indo/ferryx-releases",
        minFreeBytes: 21474836480,
      },
      "maho-win": {
        ssh: "maho-win",
        platform: "win32",
        root: "C:/Users/sook/ferryx-releases",
        minFreeBytes: 21474836480,
      },
    },
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// 1. Config Validation Tests
// ---------------------------------------------------------------------------

test("validateHostConfig: accepts canonical valid host config", () => {
  const config = makeValidConfig();
  const normalized = validateHostConfig(config);
  assert.equal(normalized.schemaVersion, 1);
  assert.equal(normalized.repo, "Indosaram/ferryx");
  assert.equal(normalized.repository, "/Users/indo/code/project/orca-lite");
  assert.equal(normalized.hosts.macbook.platform, "darwin");
  assert.equal(normalized.hosts.omaki.platform, "linux");
  assert.equal(normalized.hosts["maho-win"].platform, "win32");
});

test("loadHostConfig: successfully loads and parses example config file", () => {
  const config = loadHostConfig(EXAMPLE_CONFIG_PATH);
  assert.equal(config.schemaVersion, 1);
  assert.equal(config.repo, "Indosaram/ferryx");
  assert.equal(config.hosts.macbook.platform, "darwin");
  assert.equal(config.hosts.macbook.ssh, null);
  assert.equal(config.hosts.omaki.platform, "linux");
  assert.equal(config.hosts["maho-win"].platform, "win32");
});

test("validateHostConfig: rejects non-object configs", () => {
  assert.throws(() => validateHostConfig(null), /Host config must be an object/);
  assert.throws(() => validateHostConfig("string"), /Host config must be an object/);
  assert.throws(() => validateHostConfig([]), /Host config must be an object/);
});

test("validateHostConfig: rejects missing or invalid schemaVersion", () => {
  assert.throws(
    () => validateHostConfig(makeValidConfig({ schemaVersion: 2 })),
    /Invalid schemaVersion: expected 1/
  );
  assert.throws(
    () => validateHostConfig(makeValidConfig({ schemaVersion: "1" })),
    /Invalid schemaVersion: expected 1/
  );
});

test("validateHostConfig: rejects invalid repo", () => {
  assert.throws(
    () => validateHostConfig(makeValidConfig({ repo: "Other/repo" })),
    /Invalid repo: expected 'Indosaram\/ferryx'/
  );
});

test("validateHostConfig: rejects relative repository or ghosttyRepository paths", () => {
  assert.throws(
    () => validateHostConfig(makeValidConfig({ repository: "./relative/path" })),
    /repository must be an absolute path/
  );
  assert.throws(
    () => validateHostConfig(makeValidConfig({ ghosttyRepository: "relative/path" })),
    /ghosttyRepository must be an absolute path/
  );
});

test("validateHostConfig: rejects missing required hosts or unknown hosts", () => {
  const missingOmaki = makeValidConfig();
  delete missingOmaki.hosts.omaki;
  assert.throws(() => validateHostConfig(missingOmaki), /Missing required host: 'omaki'/);

  const extraHost = makeValidConfig();
  extraHost.hosts["extra-pc"] = {
    ssh: "extra",
    platform: "linux",
    root: "/tmp/extra",
    minFreeBytes: 1000,
  };
  assert.throws(() => validateHostConfig(extraHost), /Unknown host in config: 'extra-pc'/);
});

test("validateHostConfig: rejects unknown root properties", () => {
  assert.throws(
    () => validateHostConfig(makeValidConfig({ unknownProp: "bad" })),
    /Unknown property in host config: 'unknownProp'/
  );
  assert.throws(
    () => validateHostConfig(makeValidConfig({ $schema: "http://example.com" })),
    /Unknown property in host config: '\$schema'/
  );
});

test("validateHostConfig: rejects secrets, tokens, or credential fields in keys and values", () => {
  assert.throws(
    () => validateHostConfig(makeValidConfig({ apiToken: "secret123" })),
    /Secret or credential field detected/
  );
  assert.throws(
    () => validateHostConfig(makeValidConfig({ privateKey: "pem" })),
    /Secret or credential field detected/
  );

  const withTokenValue = makeValidConfig();
  withTokenValue.hosts.macbook.signingIdentity = "ghp_123456789012345678901234567890123456";
  assert.throws(() => validateHostConfig(withTokenValue), /Secret or credential pattern detected in value/);

  const withPrivateKeyVal = makeValidConfig();
  withPrivateKeyVal.hosts.omaki.path = "-----BEGIN OPENSSH PRIVATE KEY-----";
  assert.throws(() => validateHostConfig(withPrivateKeyVal), /Secret or credential pattern detected in value/);

  const withKeyPath = makeValidConfig();
  withKeyPath.hosts.omaki.path = "/Users/indo/.ssh/id_ed25519";
  assert.throws(() => validateHostConfig(withKeyPath), /Credential file path detected in config/);
});

test("validateHostConfig: enforces ssh null on macbook and non-empty destination on remotes", () => {
  const macbookWithSsh = makeValidConfig();
  macbookWithSsh.hosts.macbook.ssh = "macbook.local";
  assert.throws(
    () => validateHostConfig(macbookWithSsh),
    /macbook ssh destination must be null/
  );

  const omakiEmptySsh = makeValidConfig();
  omakiEmptySsh.hosts.omaki.ssh = "";
  assert.throws(() => validateHostConfig(omakiEmptySsh), /Invalid SSH destination for host 'omaki'/);

  const omakiOptionSsh = makeValidConfig();
  omakiOptionSsh.hosts.omaki.ssh = "-oProxyCommand=rm -rf /";
  assert.throws(() => validateHostConfig(omakiOptionSsh), /Invalid SSH destination for host 'omaki'/);

  const omakiSpacesSsh = makeValidConfig();
  omakiSpacesSsh.hosts.omaki.ssh = "omaki host with spaces";
  assert.throws(() => validateHostConfig(omakiSpacesSsh), /Invalid SSH destination for host 'omaki'/);
});

test("validateHostConfig: enforces platform match per host", () => {
  const badMacPlatform = makeValidConfig();
  badMacPlatform.hosts.macbook.platform = "linux";
  assert.throws(
    () => validateHostConfig(badMacPlatform),
    /Host 'macbook' platform must be 'darwin'/
  );

  const badWinPlatform = makeValidConfig();
  badWinPlatform.hosts["maho-win"].platform = "linux";
  assert.throws(
    () => validateHostConfig(badWinPlatform),
    /Host 'maho-win' platform must be 'win32'/
  );
});

test("validateHostConfig: validates build root path rules", () => {
  const relRoot = makeValidConfig();
  relRoot.hosts.macbook.root = "relative/builds";
  assert.throws(() => validateHostConfig(relRoot), /root must be an absolute path/);

  const winBackslashRoot = makeValidConfig();
  winBackslashRoot.hosts["maho-win"].root = "C:\\Users\\sook\\ferryx-releases";
  assert.throws(
    () => validateHostConfig(winBackslashRoot),
    /Windows root must use forward slashes/
  );

  const rootEqualsRepo = makeValidConfig();
  rootEqualsRepo.hosts.macbook.root = rootEqualsRepo.repository;
  assert.throws(
    () => validateHostConfig(rootEqualsRepo),
    /Host root must not equal repository/
  );
});

test("validateHostConfig: enforces positive integer minFreeBytes", () => {
  const zeroBytes = makeValidConfig();
  zeroBytes.hosts.macbook.minFreeBytes = 0;
  assert.throws(() => validateHostConfig(zeroBytes), /minFreeBytes must be a positive integer/);

  const floatBytes = makeValidConfig();
  floatBytes.hosts.macbook.minFreeBytes = 12345.67;
  assert.throws(() => validateHostConfig(floatBytes), /minFreeBytes must be a positive integer/);

  const negativeBytes = makeValidConfig();
  negativeBytes.hosts.macbook.minFreeBytes = -100;
  assert.throws(() => validateHostConfig(negativeBytes), /minFreeBytes must be a positive integer/);
});

test("validateHostConfig: rejects notaryProfile and signingIdentity on non-darwin hosts", () => {
  const linuxWithNotary = makeValidConfig();
  linuxWithNotary.hosts.omaki.notaryProfile = "NotaryLinux";
  assert.throws(
    () => validateHostConfig(linuxWithNotary),
    /notaryProfile is only permitted on macOS \(darwin\)/
  );

  const winWithIdentity = makeValidConfig();
  winWithIdentity.hosts["maho-win"].signingIdentity = "DevID";
  assert.throws(
    () => validateHostConfig(winWithIdentity),
    /signingIdentity is only permitted on macOS \(darwin\)/
  );
});

test("validateHostConfig: validates expectedTools schema", () => {
  const withValidTools = makeValidConfig();
  withValidTools.hosts.macbook.expectedTools = {
    bun: "1.4.0",
    node: "22.22.3",
    zig: "0.16.0",
    rust: "1.92.0",
    tauri: "2.10.1",
  };
  const normalized = validateHostConfig(withValidTools);
  assert.equal(normalized.hosts.macbook.expectedTools.bun, "1.4.0");

  const withUnknownTool = makeValidConfig();
  withUnknownTool.hosts.macbook.expectedTools = {
    python: "3.12.0",
  };
  assert.throws(
    () => validateHostConfig(withUnknownTool),
    /Unknown tool in expectedTools: 'python'/
  );
});

// ---------------------------------------------------------------------------
// 2. Shell Quoting Tests
// ---------------------------------------------------------------------------

test("quoteSh: correctly handles POSIX quoting edge cases", () => {
  assert.equal(quoteSh(""), "''");
  assert.equal(quoteSh("simple"), "'simple'");
  assert.equal(quoteSh("hello world"), "'hello world'");
  assert.equal(quoteSh("don't"), "'don'\\''t'");
  assert.equal(quoteSh('foo"bar"$BAZ`cmd`'), '\'foo"bar"$BAZ`cmd`\'');
  assert.equal(quoteSh("line1\nline2"), "'line1\nline2'");
  assert.equal(quoteSh("한글 및 공백 경로"), "'한글 및 공백 경로'");
});

test("quotePowerShell: correctly handles PowerShell quoting without JS backslash escapes", () => {
  assert.equal(quotePowerShell(""), "''");
  assert.equal(quotePowerShell("simple"), "'simple'");
  assert.equal(quotePowerShell("hello world"), "'hello world'");
  assert.equal(quotePowerShell("it's a test"), "'it''s a test'");
  assert.equal(
    quotePowerShell("C:\\Users\\sook\\ferryx-releases\\app.exe"),
    "'C:\\Users\\sook\\ferryx-releases\\app.exe'"
  );
  assert.equal(quotePowerShell("$env:COMPUTERNAME"), "'$env:COMPUTERNAME'");
  assert.equal(quotePowerShell("한글 디렉터리"), "'한글 디렉터리'");
});

// ---------------------------------------------------------------------------
// 3. Process Transport (runProcess) Tests
// ---------------------------------------------------------------------------

test("runProcess: captures stdout and stderr on clean exit", async () => {
  const result = await runProcess("node", [
    "-e",
    "process.stdout.write('hello stdout\\n'); process.stderr.write('hello stderr\\n');",
  ]);
  assert.equal(result.exitCode, 0);
  assert.equal(result.stdout, "hello stdout\n");
  assert.equal(result.stderr, "hello stderr\n");
});

test("runProcess: passes stdin input to child process", async () => {
  const result = await runProcess(
    "node",
    ["-e", "process.stdin.pipe(process.stdout);"],
    { input: "input stream payload 42\n" }
  );
  assert.equal(result.exitCode, 0);
  assert.equal(result.stdout, "input stream payload 42\n");
});

test("runProcess: subscribes to error/exit BEFORE writing input", async () => {
  // Process exits immediately with 0 without reading stdin.
  // Should not crash or throw unhandled EPIPE error.
  const result = await runProcess(
    "node",
    ["-e", "process.exit(0);"],
    { input: "large input ".repeat(10000) }
  );
  assert.equal(result.exitCode, 0);
});

test("runProcess: rejects non-zero exit with ProcessExecutionError and does not echo env secrets", async () => {
  let thrown = null;
  try {
    await runProcess(
      "node",
      [
        "-e",
        "process.stderr.write('fatal execution error\\n'); process.exit(42);",
      ],
      {
        env: {
          ...process.env,
          SUPER_SECRET_TOKEN: "super_secret_value_12345",
        },
      }
    );
  } catch (err) {
    thrown = err;
  }

  assert.ok(thrown, "Expected runProcess to reject on non-zero exit code");
  assert.equal(thrown.name, "ProcessExecutionError");
  assert.equal(thrown.exitCode, 42);
  assert.equal(thrown.stderr, "fatal execution error\n");
  assert.match(thrown.message, /Command failed with exit code 42/);
  // Ensure secrets from env are NOT serialized into the error message
  assert.ok(!thrown.message.includes("super_secret_value_12345"));
  assert.ok(!thrown.message.includes("SUPER_SECRET_TOKEN"));
});

test("runProcess: enforces bounded timeout and terminates hanging child process tree", async () => {
  const start = Date.now();
  let thrown = null;
  try {
    await runProcess(
      "node",
      [
        "-e",
        "const http = require('http'); setInterval(() => {}, 1000);",
      ],
      { timeoutMs: 300 }
    );
  } catch (err) {
    thrown = err;
  }

  const elapsed = Date.now() - start;
  assert.ok(thrown, "Expected process to reject on timeout");
  assert.equal(thrown.name, "ProcessTimeoutError");
  assert.equal(thrown.timedOut, true);
  assert.ok(elapsed >= 250 && elapsed < 3000, `Elapsed time ${elapsed}ms should be bounded`);
});

// ---------------------------------------------------------------------------
// 4. Host Script Execution (runHostScript) Tests
// ---------------------------------------------------------------------------

test("runHostScript: macbook runs locally via bash -s and uname succeeds", async () => {
  const config = makeValidConfig();
  const result = await runHostScript(config.hosts.macbook, {
    posix: "uname",
  });
  assert.equal(result.exitCode, 0);
  assert.equal(result.stdout.trim(), "Darwin");
});

test("runHostScript: macbook fails closed on script non-zero exit", async () => {
  const config = makeValidConfig();
  await assert.rejects(
    async () => {
      await runHostScript(config.hosts.macbook, {
        posix: "echo 'failing command' >&2; exit 17",
      });
    },
    (err) => {
      assert.equal(err.name, "ProcessExecutionError");
      assert.equal(err.exitCode, 17);
      assert.match(err.stderr, /failing command/);
      return true;
    }
  );
});

test("runHostScript: omaki Linux SSH transport invokes BatchMode and bash -s via executable SSH fixture", async () => {
  const fixtureDir = mkdtempSync(join(tmpdir(), "ferryx-ssh-omaki-"));
  const sshLogFile = join(fixtureDir, "ssh-calls.json");
  const fakeSshBin = join(fixtureDir, "ssh");

  // Create an executable SSH fixture that records invocation args and executes bash -s locally
  const fakeSshScript = `#!/usr/bin/env node
const fs = require('node:fs');
const { spawnSync } = require('node:child_process');

const args = process.argv.slice(2);
let input = fs.readFileSync(0, 'utf8');

const calls = fs.existsSync(${JSON.stringify(sshLogFile)})
  ? JSON.parse(fs.readFileSync(${JSON.stringify(sshLogFile)}, 'utf8'))
  : [];
calls.push({ args, input });
fs.writeFileSync(${JSON.stringify(sshLogFile)}, JSON.stringify(calls, null, 2));

// Execute the command locally to simulate the remote execution
const res = spawnSync('bash', ['-s'], { input, encoding: 'utf8' });
process.stdout.write(res.stdout);
process.stderr.write(res.stderr);
process.exit(res.status ?? 0);
`;
  writeFileSync(fakeSshBin, fakeSshScript, "utf8");
  chmodSync(fakeSshBin, 0o755);

  try {
    const config = makeValidConfig();
    const result = await runHostScript(
      config.hosts.omaki,
      { posix: "echo 'OMAKI_LINUX_FIXTURE_OK'" },
      { sshCommand: fakeSshBin }
    );

    assert.equal(result.exitCode, 0);
    assert.match(result.stdout, /OMAKI_LINUX_FIXTURE_OK/);

    const calls = JSON.parse(readFileSync(sshLogFile, "utf8"));
    assert.equal(calls.length, 1);
    const call = calls[0];

    // Assert exact SSH options
    assert.ok(call.args.includes("-o"));
    assert.ok(call.args.includes("BatchMode=yes"));
    assert.ok(call.args.includes("ConnectTimeout=10"));
    assert.ok(call.args.includes("ServerAliveInterval=5"));
    assert.ok(call.args.includes("ServerAliveCountMax=2"));
    assert.ok(call.args.includes("omaki"));
    assert.ok(call.args.includes("bash"));
    assert.ok(call.args.includes("-s"));
    assert.equal(call.input, "echo 'OMAKI_LINUX_FIXTURE_OK'");
  } finally {
    rmSync(fixtureDir, { recursive: true, force: true });
  }
});

test("runHostScript: maho-win Windows SSH transport invokes base64 UTF16LE EncodedCommand via executable SSH fixture", async () => {
  const fixtureDir = mkdtempSync(join(tmpdir(), "ferryx-ssh-maho-"));
  const sshLogFile = join(fixtureDir, "ssh-calls.json");
  const fakeSshBin = join(fixtureDir, "ssh");

  // Create an executable SSH fixture that decodes the PowerShell -EncodedCommand argument
  const fakeSshScript = `#!/usr/bin/env node
const fs = require('node:fs');

const args = process.argv.slice(2);
const encodedIdx = args.indexOf('-EncodedCommand');
let decodedCommand = '';
if (encodedIdx !== -1 && args[encodedIdx + 1]) {
  const b64 = args[encodedIdx + 1];
  decodedCommand = Buffer.from(b64, 'base64').toString('utf16le');
}

const calls = fs.existsSync(${JSON.stringify(sshLogFile)})
  ? JSON.parse(fs.readFileSync(${JSON.stringify(sshLogFile)}, 'utf8'))
  : [];
calls.push({ args, decodedCommand });
fs.writeFileSync(${JSON.stringify(sshLogFile)}, JSON.stringify(calls, null, 2));

process.stdout.write('MAHO_WIN_POWERSHELL_SIMULATION_OK\\n');
process.exit(0);
`;
  writeFileSync(fakeSshBin, fakeSshScript, "utf8");
  chmodSync(fakeSshBin, 0o755);

  try {
    const config = makeValidConfig();
    const result = await runHostScript(
      config.hosts["maho-win"],
      { powershell: "Write-Output 'Windows Build Step'" },
      { sshCommand: fakeSshBin }
    );

    assert.equal(result.exitCode, 0);
    assert.match(result.stdout, /MAHO_WIN_POWERSHELL_SIMULATION_OK/);

    const calls = JSON.parse(readFileSync(sshLogFile, "utf8"));
    assert.equal(calls.length, 1);
    const call = calls[0];

    // Assert exact SSH options and PowerShell flags
    assert.ok(call.args.includes("-o"));
    assert.ok(call.args.includes("BatchMode=yes"));
    assert.ok(call.args.includes("ConnectTimeout=10"));
    assert.ok(call.args.includes("ServerAliveInterval=5"));
    assert.ok(call.args.includes("ServerAliveCountMax=2"));
    assert.ok(call.args.includes("maho-win"));
    assert.ok(call.args.includes("powershell"));
    assert.ok(call.args.includes("-NoProfile"));
    assert.ok(call.args.includes("-NonInteractive"));
    assert.ok(call.args.includes("-EncodedCommand"));

    // Verify decoded PowerShell payload has SilentlyContinue and exit status handling
    assert.ok(call.decodedCommand.includes("$ProgressPreference = 'SilentlyContinue'"));
    assert.ok(call.decodedCommand.includes("Write-Output 'Windows Build Step'"));
    assert.ok(
      call.decodedCommand.includes("$LASTEXITCODE") ||
      call.decodedCommand.includes("exit")
    );
  } finally {
    rmSync(fixtureDir, { recursive: true, force: true });
  }
});

test("runHostScript: rejects invalid script requests for host platform", async () => {
  const config = makeValidConfig();

  await assert.rejects(
    async () => {
      await runHostScript(config.hosts.macbook, {});
    },
    /POSIX script required for darwin host/
  );

  await assert.rejects(
    async () => {
      await runHostScript(config.hosts.omaki, { powershell: "dir" });
    },
    /POSIX script required for linux host/
  );

  await assert.rejects(
    async () => {
      await runHostScript(config.hosts["maho-win"], { posix: "ls" });
    },
    /PowerShell script required for win32 host/
  );
});

// ---------------------------------------------------------------------------
// 5. Optional Real Remote Read-Only Smoke Check
// ---------------------------------------------------------------------------

test("runHostScript: live read-only remote smoke probe (if available)", async (t) => {
  // Probe if maho-win is directly reachable
  const check = spawnSync("ssh", [
    "-o", "BatchMode=yes",
    "-o", "ConnectTimeout=2",
    "maho-win",
    "powershell -NoProfile -Command Write-Output REACHABLE"
  ], { encoding: "utf8" });

  if (check.status !== 0) {
    t.skip("maho-win host not reachable for live smoke check");
    return;
  }

  const config = makeValidConfig();
  const result = await runHostScript(config.hosts["maho-win"], {
    powershell: "Write-Output 'REMOTE_LIVE_MAHO_WIN_OK'",
  });
  assert.equal(result.exitCode, 0);
  assert.match(result.stdout, /REMOTE_LIVE_MAHO_WIN_OK/);
});

// ---------------------------------------------------------------------------
// 6. Sensitive Environment Redaction & Stdin Hardening Tests
// ---------------------------------------------------------------------------

test("runProcess: redacts sensitive child environment variables from stdout and stderr on clean exit across chunks", async () => {
  const fakePrivateKey = "fake_tauri_privkey_" + Buffer.from("pk_secret_12345_bytes").toString("base64");
  const fakeToken = "ghp_" + "A".repeat(36);
  const fakePassword = "fake_pwd_p@ssw0rd!_987";
  const fakeSecret = "fake_signing_secret_bytes_xyz";
  const fakeAppleKey = "fake_apple_api_key_content_abc";

  const env = {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: fakePrivateKey,
    GITHUB_TOKEN: fakeToken,
    STORE_PASSWORD: fakePassword,
    SIGNING_SECRET: fakeSecret,
    APPLE_API_KEY: fakeAppleKey,
  };

  const script = [
    // Split fakePrivateKey across two stdout writes to test chunk boundary handling
    "process.stdout.write('Tauri Signer Help: --private-key <KEY> [env: TAURI_SIGNING_PRIVATE_KEY=' + process.env.TAURI_SIGNING_PRIVATE_KEY.slice(0, 14));",
    "process.stdout.write(process.env.TAURI_SIGNING_PRIVATE_KEY.slice(14) + '] end\\n');",
    // Split fakeToken across two stderr writes
    "process.stderr.write('Warning: using token ' + process.env.GITHUB_TOKEN.slice(0, 10));",
    "process.stderr.write(process.env.GITHUB_TOKEN.slice(10) + ' for upload\\n');",
    // Print password, secret, apple key
    "process.stdout.write('Secrets: pass=' + process.env.STORE_PASSWORD + ' secret=' + process.env.SIGNING_SECRET + ' apple=' + process.env.APPLE_API_KEY + '\\n');",
    "process.exit(0);",
  ].join("\n");

  const result = await runProcess("node", ["-e", script], { env });

  assert.equal(result.exitCode, 0);

  // Assert NO raw sensitive value appears in stdout or stderr
  for (const secret of [fakePrivateKey, fakeToken, fakePassword, fakeSecret, fakeAppleKey]) {
    assert.ok(!result.stdout.includes(secret), `Raw secret should not leak in stdout: ${secret}`);
    assert.ok(!result.stderr.includes(secret), `Raw secret should not leak in stderr: ${secret}`);
  }

  // Assert redaction marker is present
  assert.ok(result.stdout.includes("[REDACTED]"));
  assert.ok(result.stderr.includes("[REDACTED]"));

  // Assert non-secret markers are preserved intact
  assert.ok(result.stdout.includes("TAURI_SIGNING_PRIVATE_KEY="));
  assert.ok(result.stdout.includes("Tauri Signer Help:"));
  assert.ok(result.stderr.includes("Warning: using token "));
});

test("runProcess: redacts sensitive child environment variables from error message, stdout, stderr, and command on non-zero exit across chunks", async () => {
  const fakePrivateKey = "fake_tauri_privkey_err_" + Buffer.from("err_key_bytes").toString("base64");
  const fakeSecret = "fake_error_secret_val_456";

  const env = {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: fakePrivateKey,
    SIGNING_SECRET: fakeSecret,
  };

  const script = [
    "process.stdout.write('Output before failure: ' + process.env.TAURI_SIGNING_PRIVATE_KEY.slice(0, 10));",
    "process.stdout.write(process.env.TAURI_SIGNING_PRIVATE_KEY.slice(10) + '\\n');",
    "process.stderr.write('Fatal error trace: ' + process.env.SIGNING_SECRET.slice(0, 8));",
    "process.stderr.write(process.env.SIGNING_SECRET.slice(8) + '\\n');",
    "process.exit(17);",
  ].join("\n");

  let caught = null;
  try {
    await runProcess("node", ["-e", script], { env });
  } catch (err) {
    caught = err;
  }

  assert.ok(caught, "Expected runProcess to reject on non-zero exit");
  assert.equal(caught.name, "ProcessExecutionError");
  assert.equal(caught.exitCode, 17);

  // Assert NO raw sensitive value appears anywhere in public error fields
  for (const secret of [fakePrivateKey, fakeSecret]) {
    assert.ok(!caught.message.includes(secret), "Raw secret should not leak into err.message");
    assert.ok(!caught.stdout.includes(secret), "Raw secret should not leak into err.stdout");
    assert.ok(!caught.stderr.includes(secret), "Raw secret should not leak into err.stderr");
    assert.ok(!String(caught.command).includes(secret), "Raw secret should not leak into err.command");
  }

  assert.ok(caught.stdout.includes("[REDACTED]"));
  assert.ok(caught.stderr.includes("[REDACTED]"));
});

test("runProcess: redacts sensitive values inherited from process.env when options.env is omitted", async () => {
  const fakeInheritedKey = "fake_inherited_privkey_99999";
  process.env.TAURI_SIGNING_PRIVATE_KEY = fakeInheritedKey;

  try {
    const script = [
      "process.stdout.write('Inherited key: ' + process.env.TAURI_SIGNING_PRIVATE_KEY.slice(0, 8));",
      "process.stdout.write(process.env.TAURI_SIGNING_PRIVATE_KEY.slice(8) + '\\n');",
    ].join("\n");

    const result = await runProcess("node", ["-e", script]);

    assert.equal(result.exitCode, 0);
    assert.ok(!result.stdout.includes(fakeInheritedKey), "Raw inherited secret should not leak into stdout");
    assert.ok(result.stdout.includes("[REDACTED]"));
  } finally {
    delete process.env.TAURI_SIGNING_PRIVATE_KEY;
  }
});

test("runProcess: respects explicit per-command env={} without inheriting process.env sensitive values", async () => {
  const canarySecret = "canary_privkey_do_not_leak_000";
  process.env.TAURI_SIGNING_PRIVATE_KEY = canarySecret;

  try {
    // When options.env is provided, only actual child env is used for redaction rules
    const result = await runProcess(
      "node",
      ["-e", "process.stdout.write('ISOLATED_ENV_OUTPUT\\n');"],
      { env: { PATH: process.env.PATH } }
    );

    assert.equal(result.exitCode, 0);
    assert.equal(result.stdout, "ISOLATED_ENV_OUTPUT\n");
    assert.ok(!result.stdout.includes("[REDACTED]"));
  } finally {
    delete process.env.TAURI_SIGNING_PRIVATE_KEY;
  }
});

test("runProcess: preserves normal build output and non-sensitive env variables without redacting", async () => {
  const nonSensitiveEnv = {
    PATH: process.env.PATH,
    VERSION: "v2026.09.08.1",
    BUILD_NUMBER: "42",
    PUBLIC_KEY: "rwT7yH0a6yq3Xb2N/checked_in_public_key",
    TARGET_PLATFORM: "darwin-arm64",
    MARKER: "CLEAN_BUILD_MARKER",
  };

  const script = "process.stdout.write(`Version: ${process.env.VERSION}, Target: ${process.env.TARGET_PLATFORM}, PubKey: ${process.env.PUBLIC_KEY}, Marker: ${process.env.MARKER}\\n`);";
  const result = await runProcess("node", ["-e", script], { env: nonSensitiveEnv });

  assert.equal(result.exitCode, 0);
  assert.ok(result.stdout.includes("Version: v2026.09.08.1"));
  assert.ok(result.stdout.includes("Target: darwin-arm64"));
  assert.ok(result.stdout.includes("PubKey: rwT7yH0a6yq3Xb2N/checked_in_public_key"));
  assert.ok(result.stdout.includes("Marker: CLEAN_BUILD_MARKER"));
  assert.ok(!result.stdout.includes("[REDACTED]"));
});

test("runProcess reports non-EPIPE input errors after terminating its child", async (t) => {
  const realSpawn = childProcess.spawn;
  let child;
  t.mock.method(childProcess, "spawn", (...args) => {
    child = realSpawn(...args);
    queueMicrotask(() => {
      const error = Object.assign(new Error("input write fault"), { code: "EIO" });
      child.stdin.emit("error", error);
    });
    return child;
  });
  syncBuiltinESMExports();
  try {
    await assert.rejects(
      runProcess(process.execPath, ["-e", "process.stdin.resume()"], {
        input: "input",
        timeoutMs: 2000,
      }),
      /input write fault/,
    );
    assert.ok(child.exitCode !== null || child.signalCode !== null);
  } finally {
    t.mock.restoreAll();
    syncBuiltinESMExports();
  }
});
