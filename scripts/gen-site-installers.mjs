#!/usr/bin/env bun
// Copies the CLI installers into the site so https://ferryx.dev/install.{sh,ps1} serves the same
// bytes the relay embeds (src-tauri/src/remote/relay_server.rs include_str!). scripts/ holds the
// single source of truth: never hand-edit site/public/install.{sh,ps1}.
import { chmodSync, copyFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export const INSTALLERS = ["install.sh", "install.ps1"].map((name) => ({
  source: join(REPO_ROOT, "scripts", name),
  target: join(REPO_ROOT, "site", "public", name),
}));

export function generate(installers = INSTALLERS) {
  for (const { source, target } of installers) {
    copyFileSync(source, target);
    chmodSync(target, statSync(source).mode & 0o777);
  }
  return installers;
}

if (import.meta.main ?? process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    for (const { source, target } of generate()) {
      console.log(`gen-site-installers: ${relative(REPO_ROOT, source)} -> ${relative(REPO_ROOT, target)}`);
    }
  } catch (error) {
    console.error(`gen-site-installers: ${error.message}`);
    process.exit(1);
  }
}
