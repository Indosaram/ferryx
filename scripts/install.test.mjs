import { afterEach, expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const INSTALL_SH = join(dirname(fileURLToPath(import.meta.url)), "install.sh");
const ARTIFACT = "ferryx-cli-linux-amd64";

const sandboxes = [];
afterEach(() => {
  while (sandboxes.length) rmSync(sandboxes.pop(), { recursive: true, force: true });
});

// A payload the installer accepts on linux/amd64: its first 20 bytes must be a 64-bit
// little-endian ELF with e_machine 0x3e, and the "--help" self-check must then exit 0. A real
// x86_64 ELF is the only payload that satisfies both on Linux, because bash declines its
// ENOEXEC script fallback for anything carrying ELF magic (exit 126, "cannot execute binary
// file"). So copy the host's own /bin/true when it is one, and fall back to the synthetic
// header-plus-script that only the shells with the ENOEXEC fallback (macOS) can run.
function fakeLinuxAmd64Binary() {
  for (const candidate of ["/bin/true", "/usr/bin/true"]) {
    let bytes;
    try {
      bytes = readFileSync(candidate);
    } catch {
      continue;
    }
    if (
      bytes.length >= 20 &&
      bytes[0] === 0x7f && bytes.toString("latin1", 1, 4) === "ELF" &&
      bytes[4] === 2 && bytes[5] === 1 &&
      (bytes[16] | (bytes[17] << 8)) >>> 0 >= 2 && (bytes[16] | (bytes[17] << 8)) >>> 0 <= 3 &&
      (bytes[18] | (bytes[19] << 8)) === 0x3e
    ) {
      return bytes;
    }
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

// Builds a sandbox: PATH shims for uname and curl, and a fixture dir served via file:// URLs.
// The curl shim maps every requested URL to file://<fixtures>/<basename>, so the relay and the
// GitHub release resolve to the same fixture bytes.
function sandbox({ arch, binary, sums, os = "Linux" }) {
  const root = mkdtempSync(join(tmpdir(), "ferryx-install-test-"));
  sandboxes.push(root);
  const bin = join(root, "shim");
  const fixtures = join(root, "fixtures");
  const installDir = join(root, "prefix", "bin");
  mkdirSync(bin);
  mkdirSync(fixtures);

  writeFileSync(
    join(bin, "uname"),
    `#!/bin/sh\ncase "$1" in\n  -s) echo ${os} ;;\n  -m) echo ${arch} ;;\n  *) echo ${os} ;;\nesac\n`,
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

test("macOS Intel exits before attempting to install an arm64 binary", async () => {
  const box = sandbox({ arch: "x86_64", os: "Darwin" });
  const result = await runInstaller(box);
  expect(result.code).toBe(2);
  expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(false);
});

test("Linux x86_64 with a matching checksum installs the binary", async () => {
  const binary = fakeLinuxAmd64Binary();
  const box = sandbox({
    arch: "x86_64",
    binary,
    sums: `${sha256(Buffer.from("other"))}  ferryx-cli-darwin-arm64\n${sha256(binary)}  ${ARTIFACT}\n`,
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

test("Linux x86_64 bundle archive installs ferryx-cli together with its ffmpeg runtime directory", async () => {
  const binary = fakeLinuxAmd64Binary();
  const stage = mkdtempSync(join(tmpdir(), "ferryx-install-bundle-stage-"));
  sandboxes.push(stage);
  mkdirSync(join(stage, "ffmpeg"));
  writeFileSync(join(stage, "ferryx-cli"), binary);
  chmodSync(join(stage, "ferryx-cli"), 0o755);
  writeFileSync(join(stage, "ffmpeg", "libavcodec.so.63"), "mock-ffmpeg-so");
  writeFileSync(join(stage, "ffmpeg", "libpipewire-0.3.so.0"), "mock-pipewire-so");
  const tarPath = join(stage, `${ARTIFACT}.tar.gz`);
  execFileSync("tar", ["-C", stage, "-czf", tarPath, "ferryx-cli", "ffmpeg"]);
  const bundleBytes = readFileSync(tarPath);

  const box = sandbox({
    arch: "x86_64",
    sums: `${sha256(bundleBytes)}  ${ARTIFACT}.tar.gz\n`,
  });
  writeFileSync(join(box.root, "fixtures", `${ARTIFACT}.tar.gz`), bundleBytes);

  const result = await runInstaller(box);
  expect(result.stderr).toBe("");
  expect(result.code).toBe(0);
  expect(result.stdout).toContain(`SHA256 verified: ${sha256(bundleBytes)}`);
  expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(true);
  expect(readFileSync(join(box.installDir, "ffmpeg", "libavcodec.so.63"), "utf8")).toBe("mock-ffmpeg-so");
  expect(readFileSync(join(box.installDir, "ffmpeg", "libpipewire-0.3.so.0"), "utf8")).toBe("mock-pipewire-so");
});
