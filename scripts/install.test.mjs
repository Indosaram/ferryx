import { afterEach, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const INSTALL_SH = join(dirname(fileURLToPath(import.meta.url)), "install.sh");
const ARTIFACT = "ferryx-cli-linux-amd64";

const sandboxes = [];
afterEach(() => {
  while (sandboxes.length) rmSync(sandboxes.pop(), { recursive: true, force: true });
});

// A file whose first 20 bytes pass the installer's linux/amd64 ELF header check and which the
// shell runs as a script when execve rejects it (ENOEXEC), so the "--help" self-check exits 0.
// The newline sits before the first NUL byte so the shell does not treat it as binary.
function fakeLinuxAmd64Binary() {
  const header = Buffer.from([
    0x7f, 0x45, 0x4c, 0x46, 0x02, 0x01, 0x0a, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23,
    0x02, 0x00, 0x3e, 0x00,
  ]);
  return Buffer.concat([header, Buffer.from("\nexit 0\n")]);
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// Builds a sandbox: PATH shims for uname and curl, and a fixture dir served via file:// URLs.
// The curl shim maps every requested URL to file://<fixtures>/<basename>, so the relay and the
// GitHub release resolve to the same fixture bytes.
function sandbox({ arch, binary, sums }) {
  const root = mkdtempSync(join(tmpdir(), "ferryx-install-test-"));
  sandboxes.push(root);
  const bin = join(root, "shim");
  const fixtures = join(root, "fixtures");
  const installDir = join(root, "prefix", "bin");
  mkdirSync(bin);
  mkdirSync(fixtures);

  writeFileSync(
    join(bin, "uname"),
    `#!/bin/sh\ncase "$1" in\n  -s) echo Linux ;;\n  -m) echo ${arch} ;;\n  *) echo Linux ;;\nesac\n`,
  );
  writeFileSync(
    join(bin, "curl"),
    [
      "#!/bin/sh",
      "out=''",
      "url=''",
      "while [ \"$#\" -gt 0 ]; do",
      "  case \"$1\" in",
      "    -o) out=\"$2\"; shift 2 ;;",
      "    -*) shift ;;",
      "    *) url=\"$1\"; shift ;;",
      "  esac",
      "done",
      `exec /usr/bin/curl -fsS "file://${fixtures}/\${url##*/}" -o "$out"`,
      "",
    ].join("\n"),
  );
  chmodSync(join(bin, "uname"), 0o755);
  chmodSync(join(bin, "curl"), 0o755);

  if (binary) writeFileSync(join(fixtures, ARTIFACT), binary);
  if (sums !== undefined) writeFileSync(join(fixtures, "SHA256SUMS"), sums);

  return { root, bin, installDir };
}

async function runInstaller({ bin, installDir }) {
  const proc = Bun.spawn(["sh", INSTALL_SH, "--no-up"], {
    env: {
      PATH: `${bin}:/usr/bin:/bin:/usr/sbin:/sbin`,
      HOME: installDir,
      FERRYX_INSTALL_DIR: installDir,
    },
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, stderr, code] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  return { code, stdout, stderr };
}

for (const arch of ["aarch64", "arm64"]) {
  test(`Linux ${arch} exits 2 and points to the source build`, async () => {
    const box = sandbox({ arch });
    const result = await runInstaller(box);
    expect(result.code).toBe(2);
    expect(result.stderr).toContain(
      "Linux arm64 has no prebuilt ferryx-cli yet; build from source: https://github.com/Indosaram/ferryx/blob/main/CONTRIBUTING.md#build-prerequisites",
    );
    expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(false);
  });
}

test("Linux x86_64 with a matching checksum installs the binary", async () => {
  const binary = fakeLinuxAmd64Binary();
  const box = sandbox({
    arch: "x86_64",
    binary,
    sums: `${sha256(Buffer.from("other"))}  ferryx-cli-darwin-universal\n${sha256(binary)}  ${ARTIFACT}\n`,
  });
  const result = await runInstaller(box);
  expect(result.stderr).toBe("");
  expect(result.code).toBe(0);
  expect(result.stdout).toContain(`SHA256 verified: ${sha256(binary)}`);
  expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(true);
});

test("Linux x86_64 with a mismatched checksum exits non-zero and installs nothing", async () => {
  const binary = fakeLinuxAmd64Binary();
  const box = sandbox({ arch: "x86_64", binary, sums: `${sha256(Buffer.from("corrupted"))}  ${ARTIFACT}\n` });
  const result = await runInstaller(box);
  expect(result.code).not.toBe(0);
  expect(result.stderr).toContain("matched the SHA256SUMS");
  expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(false);
});

test("a release without SHA256SUMS exits non-zero and installs nothing", async () => {
  const box = sandbox({ arch: "x86_64", binary: fakeLinuxAmd64Binary() });
  const result = await runInstaller(box);
  expect(result.code).not.toBe(0);
  expect(result.stderr).toContain("could not download SHA256SUMS");
  expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(false);
});
