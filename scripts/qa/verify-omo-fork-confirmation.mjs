import assert from "node:assert/strict";
import { mkdtemp, mkdir, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const mainPath = process.argv[2];
if (!mainPath) throw new Error("Usage: bun verify-omo-fork-confirmation.mjs <senpi main.ts|main.js>");
const extension = mainPath.endsWith(".ts") ? "ts" : "js";
const mainUrl = pathToFileURL(resolve(mainPath)).href;
const settingsUrl = pathToFileURL(join(dirname(resolve(mainPath)), `core/settings-manager.${extension}`)).href;
const results = [];

for (const scenario of ["y", "yes", "N", "enter", "eof", "same-project"]) {
  const root = await realpath(await mkdtemp(join(tmpdir(), "ferryx-omo-confirm-")));
  let child;
  let timer;
  try {
    const parentCwd = join(root, "project");
    const originalCwd = join(parentCwd, "nested");
    const agentDir = join(root, "agent");
    await mkdir(originalCwd, { recursive: true });
    const encoded = `--${originalCwd.replace(/^[/\\]/, "").replace(/[/\\:]/g, "-")}--`;
    const sessions = join(agentDir, "sessions", encoded);
    await mkdir(sessions, { recursive: true });
    const id = crypto.randomUUID();
    const originalFile = join(sessions, `2026-09-08T00-00-00-000Z_${id}.jsonl`);
    await writeFile(originalFile, `${JSON.stringify({
      type: "session", version: 3, id, cwd: originalCwd, timestamp: "2026-09-08T00:00:00.000Z",
    })}\n`);
    const code = `
      const { createSessionManager } = await import(${JSON.stringify(mainUrl)});
      const { SettingsManager } = await import(${JSON.stringify(settingsUrl)});
      const manager = await createSessionManager(
        { session: ${JSON.stringify(id)} }, process.cwd(), undefined, SettingsManager.inMemory(), "interactive"
      );
      console.log("QA_RESULT=" + JSON.stringify({ id: manager.getSessionId(), cwd: manager.getCwd() }));
      process.exit(0);
    `;
    let output = "";
    let answered = false;
    let timedOut = false;
    child = Bun.spawn([process.execPath, "-e", code], {
      cwd: scenario === "same-project" ? originalCwd : parentCwd,
      env: {
        PATH: process.env.PATH,
        HOME: root,
        USERPROFILE: root,
        TERM: "xterm-256color",
        SENPI_CODING_AGENT_DIR: agentDir,
        PI_OFFLINE: "1",
      },
      terminal: {
        cols: 160,
        rows: 32,
        data(terminal, bytes) {
          output += new TextDecoder().decode(bytes);
          if (!answered && scenario !== "same-project" && output.includes("[y/N] ")) {
            answered = true;
            terminal.write(scenario === "eof" ? "\x04" : scenario === "enter" ? "\r" : `${scenario}\r`);
          }
        },
      },
    });
    timer = setTimeout(() => {
      timedOut = true;
      child.kill();
    }, 20000);
    const exitCode = await child.exited;
    clearTimeout(timer);
    assert.equal(timedOut, false, `${scenario}: timed out\n${output}`);
    assert.equal(exitCode, 0, `${scenario}: exit ${exitCode}\n${output}`);
    const line = output.split(/\r?\n/).find((entry) => entry.startsWith("QA_RESULT="));
    const result = line ? JSON.parse(line.slice("QA_RESULT=".length)) : null;
    const files = [];
    for await (const file of new Bun.Glob("**/*.jsonl").scan(join(agentDir, "sessions"))) files.push(file);
    if (scenario === "y" || scenario === "yes") {
      assert.ok(result, `${scenario}: fork did not complete\n${output}`);
      assert.notEqual(result.id, id);
      assert.equal(result.cwd, parentCwd);
      assert.equal(files.length, 2);
    } else if (scenario === "same-project") {
      assert.deepEqual(result, { id, cwd: originalCwd });
      assert.equal(files.length, 1);
      assert.equal(answered, false);
    } else {
      assert.equal(result, null);
      assert.equal(files.length, 1);
    }
    results.push({ scenario, exitCode, outcome: result ? scenario === "same-project" ? "resumed" : "forked" : "cancelled" });
  } finally {
    clearTimeout(timer);
    if (child) {
      if (child.exitCode === null) child.kill();
      await child.exited;
      child.terminal?.close();
    }
    await rm(root, { recursive: true, force: true });
  }
}

console.log(JSON.stringify({ mainPath: resolve(mainPath), results }, null, 2));
