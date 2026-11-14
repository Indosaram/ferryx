import { afterEach, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPTS_DIR = dirname(fileURLToPath(import.meta.url));
const INSTALL_SH = join(SCRIPTS_DIR, "install.sh");
const SOURCE_OVERRIDES =
  "Download origin: https://relay.ferryx.dev (override with FERRYX_ORIGIN); release: https://github.com/Indosaram/ferryx/releases/latest/download (pin a tag with FERRYX_VERSION).";
const ARTIFACT = "ferryx-cli-linux-amd64";

const sandboxes = [];
afterEach(() => {
  while (sandboxes.length) rmSync(sandboxes.pop(), { recursive: true, force: true });
});

// A file whose first 20 bytes pass the installer's linux/amd64 ELF header check and whose
// "--help" self-check exits 0. On a linux/amd64 host that is the real /bin/true: bash 5.2+ and
// dash refuse to run an ELF-tagged file as a script. Elsewhere the shell runs the stub below as a
// script when execve rejects it (ENOEXEC); the newline sits before the first NUL byte so the shell
// does not treat it as binary.
function fakeLinuxAmd64Binary() {
  if (existsSync("/bin/true")) {
    const host = readFileSync("/bin/true");
    const amd64Elf = host.subarray(0, 4).toString("latin1") === "\x7fELF" && host[4] === 2 && host.readUInt16LE(18) === 0x3e;
    if (amd64Elf) return host;
  }
  const header = Buffer.from([
    0x7f, 0x45, 0x4c, 0x46, 0x02, 0x01, 0x0a, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23,
    0x02, 0x00, 0x3e, 0x00,
  ]);
  return Buffer.concat([header, Buffer.from("\nexit 0\n")]);
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// Builds a sandbox: PATH shims for uname and curl (uname fails for the flag in `unameFails`), plus
// ldd when `ldd` is given, an `install` that always fails when `failInstall` is set, and an `id`
// reporting `uid`; and a fixture dir served via file:// URLs.
// The curl shim maps every requested URL to file://<fixtures>/<basename>, so the relay and the
// GitHub release resolve to the same fixture bytes.
function sandbox({ arch, binary, sums, ldd, failInstall, unameFails, uid }) {
  const root = mkdtempSync(join(tmpdir(), "ferryx-install-test-"));
  sandboxes.push(root);
  const bin = join(root, "shim");
  const fixtures = join(root, "fixtures");
  const installDir = join(root, "prefix", "bin");
  mkdirSync(bin);
  mkdirSync(fixtures);

  writeFileSync(
    join(bin, "uname"),
    `#!/bin/sh\n${unameFails ? `[ "$1" = "${unameFails}" ] && exit 1\n` : ""}case "$1" in\n  -s) echo Linux ;;\n  -m) echo ${arch} ;;\n  *) echo Linux ;;\nesac\n`,
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
  if (ldd) {
    writeFileSync(join(bin, "ldd"), `#!/bin/sh\necho '${ldd}' >&2\nexit 1\n`);
    chmodSync(join(bin, "ldd"), 0o755);
  }
  if (uid !== undefined) {
    writeFileSync(join(bin, "id"), `#!/bin/sh\necho ${uid}\n`);
    chmodSync(join(bin, "id"), 0o755);
  }
  if (failInstall) {
    writeFileSync(join(bin, "install"), "#!/bin/sh\necho 'install: cannot create regular file' >&2\nexit 1\n");
    chmodSync(join(bin, "install"), 0o755);
  }

  if (binary) writeFileSync(join(fixtures, ARTIFACT), binary);
  if (sums !== undefined) writeFileSync(join(fixtures, "SHA256SUMS"), sums);

  return { root, bin, installDir };
}

async function runInstaller({ bin, installDir }, env = {}) {
  // Keys set to undefined in `env` are removed from the installer's environment.
  const proc = Bun.spawn(["sh", INSTALL_SH, "--no-up"], {
    env: Object.fromEntries(Object.entries({
      PATH: `${bin}:/usr/bin:/bin:/usr/sbin:/sbin`,
      HOME: installDir,
      FERRYX_INSTALL_DIR: installDir,
      ...env,
    }).filter(([, value]) => value !== undefined)),
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
  test(`Linux ${arch} exits 2 and says arm64 is not supported`, async () => {
    const box = sandbox({ arch });
    const result = await runInstaller(box);
    expect(result.code).toBe(2);
    expect(result.stderr).toContain(
      `Error: Linux arm64 (${arch}) is not supported; ferryx-cli ships for Linux x86_64 only.`,
    );
    expect(result.stderr).toContain(SOURCE_OVERRIDES);
    expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(false);
  });
}

test("Linux x86_64 on musl libc exits 2 before downloading anything", async () => {
  const box = sandbox({ arch: "x86_64", binary: fakeLinuxAmd64Binary(), ldd: "musl libc (x86_64)" });
  const result = await runInstaller(box);
  expect(result.code).toBe(2);
  expect(result.stderr).toContain(
    "Error: Linux with musl libc (Alpine and similar) is not supported; ferryx-cli needs glibc 2.17 or newer.",
  );
  expect(result.stderr).toContain(SOURCE_OVERRIDES);
  expect(result.stdout).not.toContain("Downloading");
  expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(false);
});

test("an invalid FERRYX_ORIGIN exits 1, naming the value and the overrides", async () => {
  const box = sandbox({ arch: "x86_64" });
  const result = await runInstaller(box, { FERRYX_ORIGIN: "http://relay.example.com" });
  expect(result.code).toBe(1);
  expect(result.stderr).toContain("Error: Invalid FERRYX_ORIGIN 'http://relay.example.com'.");
  expect(result.stderr).toContain(
    "Download origin: http://relay.example.com (override with FERRYX_ORIGIN); release: https://github.com/Indosaram/ferryx/releases/latest/download (pin a tag with FERRYX_VERSION).",
  );
  expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(false);
});

for (const flag of ["-s", "-m"]) {
  test(`a failing 'uname ${flag}' exits 1 with a message and the overrides`, async () => {
    const box = sandbox({ arch: "x86_64", unameFails: flag });
    const result = await runInstaller(box);
    expect(result.code).toBe(1);
    expect(result.stderr).toContain(`'uname ${flag}' failed.`);
    expect(result.stderr).toContain(SOURCE_OVERRIDES);
  });
}

test("a non-root user without HOME or FERRYX_INSTALL_DIR exits 1 before downloading", async () => {
  const binary = fakeLinuxAmd64Binary();
  const box = sandbox({ arch: "x86_64", binary, sums: `${sha256(binary)}  ${ARTIFACT}\n`, uid: 1000 });
  const result = await runInstaller(box, { HOME: undefined, FERRYX_INSTALL_DIR: undefined });
  expect(result.code).toBe(1);
  expect(result.stderr).toContain("Error: HOME is not set; set FERRYX_INSTALL_DIR to choose the install directory.");
  expect(result.stderr).toContain(SOURCE_OVERRIDES);
  expect(result.stdout).not.toContain("Downloading");
});

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
  expect(result.stderr).toContain(SOURCE_OVERRIDES);
  expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(false);
});

test("a failed staged install exits 1 with the overrides and leaves no binary behind", async () => {
  const binary = fakeLinuxAmd64Binary();
  const box = sandbox({ arch: "x86_64", binary, sums: `${sha256(binary)}  ${ARTIFACT}\n`, failInstall: true });
  const result = await runInstaller(box);
  expect(result.code).toBe(1);
  expect(result.stderr).toContain(`Error: Cannot install to '${join(box.installDir, "ferryx-cli")}'`);
  expect(result.stderr).toContain(SOURCE_OVERRIDES);
  expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(false);
  expect(existsSync(join(box.installDir, "ferryx-cli.new"))).toBe(false);
});

// site/public copies are generated by scripts/gen-site-installers.mjs (site prebuild); the relay
// serves scripts/ directly, so both origins must publish identical bytes.
for (const name of ["install.sh", "install.ps1"]) {
  test(`site/public/${name} is byte-identical to scripts/${name}`, () => {
    const source = readFileSync(join(SCRIPTS_DIR, name));
    const published = readFileSync(join(SCRIPTS_DIR, "..", "site", "public", name));
    expect(published.equals(source)).toBe(true);
  });
}
