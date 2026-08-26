import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { applyGridFrame, decodeGridAttrs, parseGridFrame, type GridFrame } from "./terminalGridProtocol";

type DumpRecord = { readonly case: string; readonly frame: unknown };

const EXPECTED_CASES = [
  "ascii",
  "cjk",
  "cursor_addressed",
  "diff",
  "full_resync",
  "resize_full",
  "sgr_bold_red",
];

const manifestPath = resolve(dirname(fileURLToPath(import.meta.url)), "../../../src-tauri/Cargo.toml");

const failures: string[] = [];

function check(name: string, condition: boolean, detail?: string) {
  if (condition) return;
  failures.push(detail ? `${name}: ${detail}` : name);
}

const stdout = execFileSync(
  "cargo",
  ["run", "--quiet", "--manifest-path", manifestPath, "--example", "remote_grid_frame_dump"],
  { encoding: "utf8", maxBuffer: 32 * 1024 * 1024, stdio: ["ignore", "pipe", "pipe"] },
);

const records: DumpRecord[] = stdout
  .split("\n")
  .filter((line) => line.trim().length > 0)
  .map((line) => JSON.parse(line) as DumpRecord);

const parsed = new Map<string, GridFrame>();
for (const record of records) {
  const frame = parseGridFrame(JSON.stringify(record.frame));
  if (!frame) {
    failures.push(`${record.case}: frontend parser rejected a real backend frame`);
    continue;
  }
  parsed.set(record.case, frame);
}

const dumpedCases = records.map((record) => record.case).sort();
check(
  "case coverage",
  JSON.stringify(dumpedCases) === JSON.stringify([...EXPECTED_CASES].sort()),
  `dumped ${JSON.stringify(dumpedCases)}`,
);

function frame(name: string): GridFrame | undefined {
  return parsed.get(name);
}

const ascii = frame("ascii");
check("ascii type", ascii?.type === "grid", ascii?.type);
check("ascii text", ascii?.lines[0]?.runs[0]?.text === "hello world", ascii?.lines[0]?.runs[0]?.text);

const boldRun = frame("sgr_bold_red")?.lines[0]?.runs[0];
check("sgr text", boldRun?.text === "RED", boldRun?.text);
check("sgr bold bit", decodeGridAttrs(boldRun?.attrs ?? 0).bold);
check("sgr fg triple", boldRun?.fg?.length === 3, JSON.stringify(boldRun?.fg));

const cjkText = frame("cjk")
  ?.lines[0]?.runs.map((run) => run.text)
  .join("");
check("cjk wide graphemes", Boolean(cjkText?.includes("한글") && cjkText?.includes("테스트")), cjkText);

const diff = frame("diff");
check("diff type", diff?.type === "gridDiff", diff?.type);
check(
  "diff touches only changed line",
  JSON.stringify(diff?.lines.map((line) => line.index)) === "[1]",
  JSON.stringify(diff?.lines.map((line) => line.index)),
);

if (ascii && diff) {
  const patched = applyGridFrame(applyGridFrame(null, ascii), diff);
  check("diff keeps untouched line", patched.lines[0]?.runs[0]?.text === "hello world");
  check("diff applies changed line", patched.lines[1]?.runs[0]?.text === "second");
}

const resized = frame("resize_full");
check("resize type", resized?.type === "grid", resized?.type);
check("resize geometry", resized?.cols === 40 && resized?.rows === 6, `${resized?.cols}x${resized?.rows}`);
check("resize row count", resized ? applyGridFrame(null, resized).lines.length === 6 : false);

const cursor = frame("cursor_addressed")?.cursor;
check("cursor row", cursor?.y === 1, String(cursor?.y));
check("cursor visible", cursor?.visible === true);

if (failures.length > 0) {
  console.error(`grid frame seam FAILED (${failures.length}):`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}

console.log(`grid frame seam OK: ${records.length} real backend frames parsed and asserted`);
