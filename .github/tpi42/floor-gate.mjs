// Feeds measured `dumpbin /dependents` lists into the real buildHost floor gate for maho-win
// (fixture runner: no remote host, the measurements are the real ones from this Windows runner).
// usage: bun floor-gate.mjs <cli-dependents.json> <dynamic-fixture-dependents.json>
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { buildHost } from "../../scripts/lib/release-platforms.mjs";

const [cliPath, dynPath] = process.argv.slice(2);
const cliDeps = JSON.parse(readFileSync(cliPath, "utf8"));
const dynDeps = JSON.parse(readFileSync(dynPath, "utf8"));
const MSIX_FIXTURE = ["KERNEL32.dll"]; // not under test here

function makeRun(tmp) {
  const runDir = join(tmp, "run");
  for (const d of ["receipts", "bundles", join("artifacts", "windows")]) mkdirSync(join(runDir, d), { recursive: true });
  writeFileSync(join(runDir, "bundles", "source.bundle"), "FIXTURE_SOURCE_BUNDLE");
  writeFileSync(join(runDir, "bundles", "ghostty.bundle"), "FIXTURE_GHOSTTY_BUNDLE");
  writeFileSync(join(runDir, "plan.json"), JSON.stringify({
    schemaVersion: 1, runId: "rel-run-tpi42", repo: "Indosaram/ferryx",
    commitSha: "5d5499806a1b207849778f488b4e3e7b821a751b", tag: "v2026.09.08.1", appVersion: "2026.908.1",
    msixVersion: "2026.908.1.0", channels: { store: true, nsisMigration: false },
    requiredTargets: ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"],
    toolchains: { node: ">=22.0.0", bun: ">=1.4.0", zig: "0.16.0" },
    sourceDateEpoch: 1788868800, createdAt: "2026-09-08T12:00:00.000Z",
  }));
  const hostRoot = join(tmp, "host-root");
  mkdirSync(hostRoot);
  const config = {
    schemaVersion: 1, repository: join(tmp, "no-repo"), ghosttyRepository: join(tmp, "no-ghostty"), repo: "Indosaram/ferryx",
    hosts: { "maho-win": { platform: "fixture", ssh: null, root: hostRoot, minFreeBytes: 1024 } },
  };
  return { runDir, config };
}
const runner = (cli) => async ({ artifactsOutDir }) => {
  writeFileSync(join(artifactsOutDir, "Ferryx_x64.msix"), "MOCK_MSIX");
  writeFileSync(join(artifactsOutDir, "ferryx-cli-windows-amd64.exe"), "MOCK_CLI");
  return {
    exitCode: 0,
    toolchains: { node: "22.22.3", bun: "1.4.0", zig: "0.16.0", rust: "1.92.0" },
    floors: { msix: { dependents: MSIX_FIXTURE }, "cli-windows-amd64": { dependents: cli } },
  };
};

let failures = 0;
for (const [name, deps, expectPass] of [["real-crt-static-cli", cliDeps, true], ["dynamic-crt-fixture", dynDeps, false]]) {
  const tmp = mkdtempSync(join(tmpdir(), `tpi42-${name}-`));
  try {
    const { runDir, config } = makeRun(tmp);
    console.log(`== ${name}: cli-windows-amd64 dependents ${JSON.stringify(deps)}`);
    try {
      const receipt = await buildHost({ hostName: "maho-win", config, runDir, runner: runner(deps), floorWaiver: null });
      const entry = receipt.floors.checks.find((c) => c.kind === "cli-windows-amd64");
      console.log(`RECEIPT floors cli-windows-amd64: ${JSON.stringify(entry)}`);
      console.log(`RESULT ${name}: buildHost passed`);
      if (!expectPass) failures += 1;
    } catch (err) {
      console.log(`RESULT ${name}: buildHost failed: ${err.message}`);
      if (expectPass) failures += 1;
    }
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}
console.log(`FLOOR_GATE_UNEXPECTED=${failures}`);
process.exit(failures === 0 ? 0 : 1);
