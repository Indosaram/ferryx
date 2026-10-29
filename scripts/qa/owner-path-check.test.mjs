import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import { cfgTestLines, findOwnerPaths } from "./owner-path-check.mjs";

const SCRIPT = join(import.meta.dir, "owner-path-check.mjs");

function repo(files) {
  const root = mkdtempSync(join(tmpdir(), "owner-path-"));
  spawnSync("git", ["init", "-q", root]);
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), content);
  }
  spawnSync("git", ["-C", root, "add", "-A"]);
  return root;
}

const RUST = [
  "/// Placeholder doc for `/home/<user>/x`.",
  "pub fn f() {}",
  "",
  "#[cfg(test)]",
  "mod tests {",
  "    #[test]",
  "    fn sample() { assert!(super::g(\"/home/indo/project\")); }",
  "}",
  "",
].join("\n");

test("a clean tree passes, with cfg(test) Rust samples and AGENTS.md allowed", () => {
  const root = repo({ "docs/guide.md": "Install under /home/<user>/ferryx.\n", "src/lib.rs": RUST, "AGENTS.md": "| maho-win | `C:\\Users\\sook\\ferryx` |\n" });
  try {
    const { hits, allowed } = findOwnerPaths(root);
    expect(hits).toEqual([]);
    expect(allowed.map((a) => a.file).sort()).toEqual(["AGENTS.md", "src/lib.rs"]);
    expect(spawnSync(process.execPath, [SCRIPT, root]).status).toBe(0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test.each([
  ["docs/guide.md", "Run /home/indo/bin/ferryx-relay\n"],
  ["docs/win.md", "cd C:\\Users\\sook\\ferryx\n"],
  ["docs/win2.md", "cd C:/Users/sook/ferryx\n"],
  ["scripts/x.example.json", '{"originAddress": "abc.cfargotunnel.com"}\n'],
  ["README.md", "/Users/indo/project\n"],
  ["src/lib.rs", `pub const ROOT: &str = "/home/indo";\n${RUST}`],
])("planting an owner path in %s fails the check", (path, content) => {
  const root = repo({ [path]: content });
  try {
    const { hits } = findOwnerPaths(root);
    expect(hits.map((h) => h.file)).toEqual([path]);
    expect(spawnSync(process.execPath, [SCRIPT, root]).status).toBe(1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("excluded trees stay out of scope", () => {
  const root = repo({ "a/fixtures/x.json": '"/home/indo"\n', ".omo/n.md": "/home/indo\n", "ui/a.test.ts": '"/home/indo"\n', "src/x_tests.rs": '"/home/indo"\n' });
  try {
    expect(findOwnerPaths(root).hits).toEqual([]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("cfg(test) detection covers the module body and nothing after it", () => {
  const lines = cfgTestLines(`${RUST}pub fn after() {}\n`);
  expect(lines.has(7)).toBe(true);
  expect(lines.has(2)).toBe(false);
  expect(lines.has(10)).toBe(false);
});
