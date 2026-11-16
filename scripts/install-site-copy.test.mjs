import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// https://ferryx.dev/install.sh and install.ps1 are served from site/public, while the relay
// serves scripts/install.sh and install.ps1 (include_str! in relay_server.rs). The site copies
// once drifted and kept shipping an installer without the SHA256SUMS check, so every installer
// change must land in both places byte for byte.
const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

for (const name of ["install.sh", "install.ps1"]) {
  test(`site/public/${name} is identical to scripts/${name}`, () => {
    const site = readFileSync(join(ROOT, "site", "public", name), "utf8");
    const canonical = readFileSync(join(ROOT, "scripts", name), "utf8");
    expect(site).toBe(canonical);
  });
}
