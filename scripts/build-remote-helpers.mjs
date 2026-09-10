import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync, copyFileSync } from "node:fs";
import { resolve, join } from "node:path";

const REPO_ROOT = resolve(import.meta.dirname, "..");
const RESOURCES_DIR = join(REPO_ROOT, "src-tauri", "resources", "helpers");

const TARGET_SPECS = [
  {
    target: "x86_64-unknown-linux-gnu",
    filename: "ferryx-remote-helper",
    candidatePaths: [
      join(REPO_ROOT, "remote-helper", "target", "x86_64-unknown-linux-gnu", "release", "ferryx-remote-helper"),
      join(REPO_ROOT, "remote-helper", "target", "x86_64-unknown-linux-gnu", "debug", "ferryx-remote-helper"),
    ],
  },
  {
    target: "aarch64-unknown-linux-gnu",
    filename: "ferryx-remote-helper",
    candidatePaths: [
      join(REPO_ROOT, "remote-helper", "target", "aarch64-unknown-linux-gnu", "release", "ferryx-remote-helper"),
      join(REPO_ROOT, "remote-helper", "target", "aarch64-unknown-linux-gnu", "debug", "ferryx-remote-helper"),
    ],
  },
  {
    target: "x86_64-pc-windows-msvc",
    filename: "ferryx-remote-helper.exe",
    candidatePaths: [
      join(REPO_ROOT, "remote-helper", "target", "x86_64-pc-windows-msvc", "release", "ferryx-remote-helper.exe"),
      join(REPO_ROOT, "remote-helper", "target", "x86_64-pc-windows-gnu", "release", "ferryx-remote-helper.exe"),
      join(REPO_ROOT, "remote-helper", "target", "x86_64-pc-windows-msvc", "debug", "ferryx-remote-helper.exe"),
      join(REPO_ROOT, "remote-helper", "target", "x86_64-pc-windows-gnu", "debug", "ferryx-remote-helper.exe"),
    ],
  },
];

export function computeSha256(buffer) {
  return createHash("sha256").update(buffer).digest("hex");
}

export function stageHelpers() {
  if (!existsSync(RESOURCES_DIR)) {
    mkdirSync(RESOURCES_DIR, { recursive: true });
  }

  const artifacts = [];

  for (const spec of TARGET_SPECS) {
    const targetDir = join(RESOURCES_DIR, spec.target);
    const destFile = join(targetDir, spec.filename);

    let foundSrc = null;
    for (const cand of spec.candidatePaths) {
      if (existsSync(cand)) {
        foundSrc = cand;
        break;
      }
    }

    if (foundSrc) {
      if (!existsSync(targetDir)) {
        mkdirSync(targetDir, { recursive: true });
      }
      copyFileSync(foundSrc, destFile);
      const data = readFileSync(destFile);
      artifacts.push({
        target: spec.target,
        filename: spec.filename,
        sha256: computeSha256(data),
        byteLength: data.byteLength,
      });
      console.log(`Staged ${spec.target} from ${foundSrc} (${data.byteLength} bytes)`);
    } else if (existsSync(destFile)) {
      const data = readFileSync(destFile);
      artifacts.push({
        target: spec.target,
        filename: spec.filename,
        sha256: computeSha256(data),
        byteLength: data.byteLength,
      });
      console.log(`Preserved existing staged asset: ${spec.target} (${data.byteLength} bytes)`);
    } else {
      console.log(`Skipped ${spec.target}: no compiled artifact found`);
    }
  }

  const manifest = {
    schemaVersion: 1,
    helperVersion: "2026.908.1",
    protocolVersion: 1,
    artifacts,
  };

  const manifestPath = join(RESOURCES_DIR, "manifest.json");
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n", "utf8");
  console.log(`Generated manifest at ${manifestPath} with ${artifacts.length} artifacts`);
  return manifest;
}

if (import.meta.main) {
  stageHelpers();
}
