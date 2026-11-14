import { afterEach, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdtempSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, utimesSync, writeFileSync } from "node:fs";
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

// The fallback stub of fakeLinuxAmd64Binary with a different script body.
function amd64ElfStub(script) {
  const header = Buffer.from([
    0x7f, 0x45, 0x4c, 0x46, 0x02, 0x01, 0x0a, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23, 0x23,
    0x02, 0x00, 0x3e, 0x00,
  ]);
  return Buffer.concat([header, Buffer.from(`\n${script}\n`)]);
}

function hostAmd64Elf(path) {
  if (!existsSync(path)) return null;
  const bytes = readFileSync(path);
  const amd64Elf = bytes.subarray(0, 4).toString("latin1") === "\x7fELF" && bytes[4] === 2 && bytes.readUInt16LE(18) === 0x3e;
  return amd64Elf ? bytes : null;
}

// A linux/amd64 binary the dynamic loader rejects for a too-old glibc, the way the published
// ferryx-cli fails on Ubuntu 22.04: "version `GLIBC_x' not found (required by ...)", exit 1.
// On a linux/amd64 host this is /bin/true with one GLIBC_2.NN version requirement renamed to
// GLIBC_9.99 (same length), so the real loader produces the error. Elsewhere a stub prints it.
function glibcTooOldBinary() {
  const host = hostAmd64Elf("/bin/true");
  if (host) {
    const text = host.toString("latin1");
    const match = /GLIBC_2\.\d\d\0/.exec(text);
    if (match) {
      const patched = Buffer.from(host);
      patched.write("GLIBC_9.99\0", match.index, "latin1");
      return { binary: patched, missing: "9.99" };
    }
  }
  return {
    binary: amd64ElfStub(
      "echo \"$0: /lib/x86_64-linux-gnu/libc.so.6: version \\`GLIBC_2.39' not found (required by $0)\" >&2\nexit 1",
    ),
    missing: "2.39",
  };
}

// A linux/amd64 binary the loader rejects for a missing non-glibc symbol version:
// "version `CXXABI_9.9' not found (required by ...)", exit 1. On a linux/amd64 host this is
// /bin/true with one GLIBC_2.NN requirement renamed to CXXABI_9.9 (same length).
function cxxabiMissingBinary() {
  const host = hostAmd64Elf("/bin/true");
  if (host) {
    const match = /GLIBC_2\.\d\d\0/.exec(host.toString("latin1"));
    if (match) {
      const patched = Buffer.from(host);
      patched.write("CXXABI_9.9\0", match.index, "latin1");
      return patched;
    }
  }
  return amd64ElfStub(
    "echo \"$0: /lib/x86_64-linux-gnu/libstdc++.so.6: version \\`CXXABI_9.9' not found (required by $0)\" >&2\nexit 1",
  );
}

// A linux/amd64 binary that runs but prints nothing and exits 1 (/bin/false on linux/amd64).
function silentFailingBinary() {
  return hostAmd64Elf("/bin/false") ?? amd64ElfStub("exit 1");
}

// A static linux/amd64 ELF (no dynamic loader) that writes the Ferryx usage banner to stdout and
// then runs `tail`: by default a store to address 0, so it crashes with SIGSEGV (shell status 139)
// after printing the banner.
const SEGFAULT_TAIL = [0xc7, 0x04, 0x25, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]; // mov dword [0], 0
const EXIT_200_TAIL = [0xbf, 0xc8, 0x00, 0x00, 0x00, 0xb8, 0x3c, 0x00, 0x00, 0x00, 0x0f, 0x05]; // exit(200)
function bannerThenSegfaultBinary(tail = SEGFAULT_TAIL) {
  const msg = Buffer.from("Ferryx CLI is running in headless mode.\nUsage: ferryx-cli <up>\n", "latin1");
  const base = 0x400000;
  const codeOff = 0x78; // ELF header (0x40) + one program header (0x38)
  const code = Buffer.from([
    0xb8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1 (write)
    0xbf, 0x01, 0x00, 0x00, 0x00, // mov edi, 1 (stdout)
    0x48, 0x8d, 0x35, 0x00, 0x00, 0x00, 0x00, // lea rsi, [rip + msg]
    0xba, 0x00, 0x00, 0x00, 0x00, // mov edx, len
    0x0f, 0x05, // syscall
    ...tail,
  ]);
  code.writeInt32LE(code.length - 17, 13); // msg follows the code; rip after the lea is code+17
  code.writeUInt32LE(msg.length, 18);
  const total = codeOff + code.length + msg.length;
  const h = Buffer.alloc(codeOff);
  h.write("\x7fELF", 0, "latin1");
  h[4] = 2; // ELFCLASS64
  h[5] = 1; // little endian
  h[6] = 1; // EV_CURRENT
  h.writeUInt16LE(2, 16); // ET_EXEC
  h.writeUInt16LE(0x3e, 18); // EM_X86_64
  h.writeUInt32LE(1, 20);
  h.writeBigUInt64LE(BigInt(base + codeOff), 24); // e_entry
  h.writeBigUInt64LE(0x40n, 32); // e_phoff
  h.writeUInt16LE(0x40, 52); // e_ehsize
  h.writeUInt16LE(0x38, 54); // e_phentsize
  h.writeUInt16LE(1, 56); // e_phnum
  h.writeUInt32LE(1, 0x40); // PT_LOAD
  h.writeUInt32LE(5, 0x44); // R+X
  h.writeBigUInt64LE(BigInt(base), 0x50); // p_vaddr
  h.writeBigUInt64LE(BigInt(base), 0x58); // p_paddr
  h.writeBigUInt64LE(BigInt(total), 0x60); // p_filesz
  h.writeBigUInt64LE(BigInt(total), 0x68); // p_memsz
  h.writeBigUInt64LE(0x1000n, 0x70); // p_align
  return Buffer.concat([h, code, msg]);
}
const LINUX_AMD64 = process.platform === "linux" && process.arch === "x64";

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// The per-run staging files (.ferryx-cli.new.XXXXXX) install.sh left in `dir`.
function stagingLeftovers(dir) {
  return existsSync(dir) ? readdirSync(dir).filter((name) => name.startsWith(".ferryx-cli.new.")) : [];
}

// Builds a sandbox: PATH shims for uname and curl (uname fails for the flag in `unameFails`), plus
// ldd when `ldd` is given, an `install` that always fails when `failInstall` is set, and an `id`
// reporting `uid`; and a fixture dir served via file:// URLs.
// The curl shim maps every requested URL to file://<fixtures>/<basename>, so the relay and the
// GitHub release resolve to the same fixture bytes.
function sandbox({ arch, binary, sums, ldd, failInstall, nonExecInstall, mvSync, unameFails, uid }) {
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
  let sync;
  if (mvSync) {
    // An mv that, for the final staged-file rename only, announces itself on the at-mv FIFO and
    // then blocks until a line arrives on the go FIFO: the test decides what runs in between.
    sync = { atMv: join(root, "at-mv.fifo"), go: join(root, "go.fifo") };
    execFileSync("mkfifo", [sync.atMv, sync.go]);
    writeFileSync(
      join(bin, "mv"),
      `#!/bin/sh\ncase "$1 $2" in\n  *.ferryx-cli.new.*) echo at-mv > "${sync.atMv}"; read go < "${sync.go}" ;;\nesac\nexec /bin/mv "$@"\n`,
    );
    chmodSync(join(bin, "mv"), 0o755);
  }
  if (nonExecInstall) {
    // "install -m 0755 SRC DST" that stages DST without the execute bit, so running it fails
    // with EACCES ("Permission denied", exit 126) the way a noexec mount does.
    writeFileSync(join(bin, "install"), '#!/bin/sh\ncp "$3" "$4" && chmod 0644 "$4"\n');
    chmodSync(join(bin, "install"), 0o755);
  }

  if (binary) writeFileSync(join(fixtures, ARTIFACT), binary);
  if (sums !== undefined) writeFileSync(join(fixtures, "SHA256SUMS"), sums);

  return { root, bin, installDir, sync };
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
  expect(result.stdout).toContain("runs on this host (--version exit 0)");
  expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(true);
});

const EXISTING_INSTALL = Buffer.from("#!/bin/sh\necho previously installed ferryx-cli\n");

test("a binary the loader rejects for a too-old glibc exits 2 and leaves the existing install byte-identical", async () => {
  const { binary, missing } = glibcTooOldBinary();
  const box = sandbox({ arch: "x86_64", binary, sums: `${sha256(binary)}  ${ARTIFACT}\n` });
  mkdirSync(box.installDir, { recursive: true });
  writeFileSync(join(box.installDir, "ferryx-cli"), EXISTING_INSTALL);
  const result = await runInstaller(box);
  expect(result.code).toBe(2);
  expect(result.stderr).toContain(
    `Error: this Linux is not supported: ferryx-cli needs glibc ${missing} or newer (GLIBC_${missing} not found), and this host has `,
  );
  expect(result.stderr).toMatch(/and this host has \S/);
  expect(result.stderr).toContain(`GLIBC_${missing}' not found (required by`);
  expect(result.stderr).toContain("the existing installation, if any, was left untouched.");
  expect(result.stderr).toContain(SOURCE_OVERRIDES);
  expect(result.stdout).not.toContain("Successfully installed");
  expect(readFileSync(join(box.installDir, "ferryx-cli")).equals(EXISTING_INSTALL)).toBe(true);
  expect(stagingLeftovers(box.installDir)).toEqual([]);
});

test("a binary that runs but neither succeeds nor prints the CLI usage exits 1 and installs nothing", async () => {
  const binary = silentFailingBinary();
  const box = sandbox({ arch: "x86_64", binary, sums: `${sha256(binary)}  ${ARTIFACT}\n` });
  const result = await runInstaller(box);
  expect(result.code).toBe(1);
  expect(result.stderr).toContain("Error: the downloaded ferryx-cli failed its self-check ('--version' exit 1");
  expect(result.stderr).toContain(SOURCE_OVERRIDES);
  expect(existsSync(box.installDir)).toBe(false);
  expect(existsSync(join(box.root, "prefix"))).toBe(false);
});

test("a non-glibc loader failure exits 2 even when TMPDIR and the install dir contain 'Ferryx CLI'", async () => {
  const binary = cxxabiMissingBinary();
  const box = sandbox({ arch: "x86_64", binary, sums: `${sha256(binary)}  ${ARTIFACT}\n` });
  const tmp = join(box.root, "Ferryx CLI tmp");
  const installDir = join(box.root, "Ferryx CLI", "bin");
  mkdirSync(tmp);
  mkdirSync(installDir, { recursive: true });
  writeFileSync(join(installDir, "ferryx-cli"), EXISTING_INSTALL);
  const result = await runInstaller(box, { TMPDIR: tmp, FERRYX_INSTALL_DIR: installDir });
  expect(result.code).toBe(2);
  expect(result.stderr).toContain(
    "Error: this system is not supported: the dynamic loader rejected ferryx-cli because no installed library provides CXXABI_9.9",
  );
  expect(result.stderr).toContain("CXXABI_9.9' not found (required by");
  expect(result.stderr).toContain(SOURCE_OVERRIDES);
  expect(result.stdout).not.toContain("Successfully installed");
  expect(readFileSync(join(installDir, "ferryx-cli")).equals(EXISTING_INSTALL)).toBe(true);
  expect(stagingLeftovers(installDir)).toEqual([]);
});

test("an install dir that cannot run programs (exit 126, Permission denied) exits 1 with a noexec hint", async () => {
  const binary = fakeLinuxAmd64Binary();
  const box = sandbox({ arch: "x86_64", binary, sums: `${sha256(binary)}  ${ARTIFACT}\n`, nonExecInstall: true });
  mkdirSync(box.installDir, { recursive: true });
  writeFileSync(join(box.installDir, "ferryx-cli"), EXISTING_INSTALL);
  const result = await runInstaller(box);
  expect(result.code).toBe(1);
  expect(result.stderr).toContain(
    `Error: cannot run programs from '${box.installDir}' (Permission denied, exit 126; is it mounted noexec?). Set FERRYX_INSTALL_DIR to a directory that allows running programs.`,
  );
  expect(result.stderr).not.toContain("not supported");
  expect(result.stderr).toContain(SOURCE_OVERRIDES);
  expect(readFileSync(join(box.installDir, "ferryx-cli")).equals(EXISTING_INSTALL)).toBe(true);
  expect(stagingLeftovers(box.installDir)).toEqual([]);
});

test.skipIf(!LINUX_AMD64)("a binary that prints the usage banner and then crashes (SIGSEGV) exits 1 and installs nothing", async () => {
  const binary = bannerThenSegfaultBinary();
  const box = sandbox({ arch: "x86_64", binary, sums: `${sha256(binary)}  ${ARTIFACT}\n` });
  mkdirSync(box.installDir, { recursive: true });
  writeFileSync(join(box.installDir, "ferryx-cli"), EXISTING_INSTALL);
  const result = await runInstaller(box);
  expect(result.code).toBe(1);
  expect(result.stderr).toContain(
    "Error: the downloaded ferryx-cli failed its self-check ('--version' exit 139; a status above 128 is a signal death).",
  );
  expect(result.stderr).toContain("Ferryx CLI is running in headless mode.");
  expect(result.stderr).toContain(SOURCE_OVERRIDES);
  expect(result.stdout).not.toContain("Successfully installed");
  expect(readFileSync(join(box.installDir, "ferryx-cli")).equals(EXISTING_INSTALL)).toBe(true);
  expect(stagingLeftovers(box.installDir)).toEqual([]);
});

test.skipIf(!LINUX_AMD64)("a plain exit 200 after the usage banner is refused without naming a signal", async () => {
  const binary = bannerThenSegfaultBinary(EXIT_200_TAIL);
  const box = sandbox({ arch: "x86_64", binary, sums: `${sha256(binary)}  ${ARTIFACT}\n` });
  const result = await runInstaller(box);
  expect(result.code).toBe(1);
  expect(result.stderr).toContain(
    "Error: the downloaded ferryx-cli failed its self-check ('--version' exit 200; a status above 128 is a signal death).",
  );
  expect(result.stderr).not.toContain("signal 72");
  expect(existsSync(join(box.installDir, "ferryx-cli"))).toBe(false);
});

test("staging files older than an hour are removed; a recent one (a concurrent run) is kept", async () => {
  const binary = fakeLinuxAmd64Binary();
  const box = sandbox({ arch: "x86_64", binary, sums: `${sha256(binary)}  ${ARTIFACT}\n` });
  mkdirSync(box.installDir, { recursive: true });
  const stale = join(box.installDir, ".ferryx-cli.new.Stale1");
  const recent = join(box.installDir, ".ferryx-cli.new.Fresh1");
  writeFileSync(stale, "killed run");
  writeFileSync(recent, "concurrent run");
  const twoHoursAgo = new Date(Date.now() - 2 * 60 * 60 * 1000);
  utimesSync(stale, twoHoursAgo, twoHoursAgo);
  const result = await runInstaller(box);
  expect(result.code).toBe(0);
  expect(readFileSync(join(box.installDir, "ferryx-cli")).equals(binary)).toBe(true);
  expect(stagingLeftovers(box.installDir)).toEqual([".ferryx-cli.new.Fresh1"]);
});

test("a leftover ferryx-cli.new directory neither blocks the install nor is touched", async () => {
  const binary = fakeLinuxAmd64Binary();
  const box = sandbox({ arch: "x86_64", binary, sums: `${sha256(binary)}  ${ARTIFACT}\n` });
  mkdirSync(join(box.installDir, "ferryx-cli.new"), { recursive: true });
  writeFileSync(join(box.installDir, "ferryx-cli.new", "user-file"), "keep");
  const result = await runInstaller(box);
  expect(result.code).toBe(0);
  expect(readFileSync(join(box.installDir, "ferryx-cli")).equals(binary)).toBe(true);
  expect(readFileSync(join(box.installDir, "ferryx-cli.new", "user-file"), "utf8")).toBe("keep");
  expect(stagingLeftovers(box.installDir)).toEqual([]);
});

test("three parallel runs of the same release into one dir all install it", async () => {
  const binary = fakeLinuxAmd64Binary();
  const box = sandbox({ arch: "x86_64", binary, sums: `${sha256(binary)}  ${ARTIFACT}\n` });
  const results = await Promise.all([0, 1, 2].map(() => runInstaller(box)));
  expect(results.map((r) => r.code)).toEqual([0, 0, 0]);
  expect(readFileSync(join(box.installDir, "ferryx-cli")).equals(binary)).toBe(true);
  expect(stagingLeftovers(box.installDir)).toEqual([]);
});

test("a refused run between another run's verify and its rename cannot swap in its own binary", async () => {
  const good = fakeLinuxAmd64Binary();
  const bad = cxxabiMissingBinary();
  const a = sandbox({ arch: "x86_64", binary: good, sums: `${sha256(good)}  ${ARTIFACT}\n`, mvSync: true });
  const b = sandbox({ arch: "x86_64", binary: bad, sums: `${sha256(bad)}  ${ARTIFACT}\n` });
  const installDir = join(a.root, "shared", "bin");
  mkdirSync(installDir, { recursive: true });
  writeFileSync(join(installDir, "ferryx-cli"), EXISTING_INSTALL);

  // A verifies its candidate, then blocks in its final mv; B runs start to finish meanwhile.
  const runA = runInstaller(a, { FERRYX_INSTALL_DIR: installDir });
  const atMv = Bun.spawn(["cat", a.sync.atMv], { stdout: "ignore" });
  const first = await Promise.race([atMv.exited.then(() => "a-at-mv"), runA.then(() => "a-exited")]);
  if (first !== "a-at-mv") atMv.kill();
  expect(first).toBe("a-at-mv");
  const resultB = await runInstaller(b, { FERRYX_INSTALL_DIR: installDir });
  expect(resultB.code).toBe(2);
  expect(readFileSync(join(installDir, "ferryx-cli")).equals(EXISTING_INSTALL)).toBe(true);
  await Bun.spawn(["sh", "-c", 'echo go > "$1"', "sh", a.sync.go]).exited;

  const resultA = await runA;
  expect(resultA.code).toBe(0);
  expect(resultA.stdout).toContain("Successfully installed");
  expect(readFileSync(join(installDir, "ferryx-cli")).equals(good)).toBe(true);
  expect(stagingLeftovers(installDir)).toEqual([]);
}, 30000);

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
  expect(stagingLeftovers(box.installDir)).toEqual([]);
});

// install.ps1's Install-FerryxCandidate, driven under pwsh with the self-check run mocked per
// executable: the candidate in the temp dir and the installed ferryx-cli.exe each get their own
// exit code and output. Runs where pwsh is installed (the PowerShell container in QA).
const PWSH = Bun.which("pwsh");
const INSTALL_PS1 = join(SCRIPTS_DIR, "install.ps1");
const PS1_DRIVER = `param([string]$Script, [string]$TempExe, [string]$InstallDir, [string]$WorkDir)
$FerryxOrigin = "https://relay.ferryx.dev"
$FerryxVersion = "latest"
$ast = [System.Management.Automation.Language.Parser]::ParseFile($Script, [ref]$null, [ref]$null)
$wanted = @("Get-FerryxReleaseBaseUrl", "Write-FerryxSourceOverrides", "Write-FerryxError", "Test-FerryxUnrunnableExitCode", "Invoke-FerryxSelfCheck", "Remove-FerryxCreatedDirectories", "Install-FerryxCandidate")
foreach ($f in $ast.FindAll({ $args[0] -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $true)) {
    if ($wanted -contains $f.Name) { . ([scriptblock]::Create($f.Extent.Text)) }
}
function Invoke-FerryxSelfCheckRun {
    param($Executable, $WorkDir)
    if ($Executable -eq $TempExe) { $prefix = "MOCK_TEMP" } else { $prefix = "MOCK_TARGET" }
    if ($prefix -eq "MOCK_TARGET" -and $env:MOCK_CONCURRENT) {
        # A concurrent successful run installs its exe and cleans up this run's backup meanwhile.
        Get-ChildItem -LiteralPath (Split-Path -Parent $Executable) -Filter ".ferryx-cli.exe.old.*" -File -Force | Microsoft.PowerShell.Management\\Remove-Item -Force
        Microsoft.PowerShell.Management\\Remove-Item -LiteralPath $Executable -Force
        [System.IO.File]::WriteAllText($Executable, $env:MOCK_CONCURRENT)
    }
    [pscustomobject]@{ ExitCode = [int][Environment]::GetEnvironmentVariable($prefix + "_CODE"); Output = [Environment]::GetEnvironmentVariable($prefix + "_OUT") }
}
if ($env:MOCK_FAIL_RESTORE) {
    # The restore (backup -> ferryx-cli.exe) silently does nothing, as a denied rename would.
    function Move-Item {
        [CmdletBinding()]
        param([string]$LiteralPath, [string]$Destination, [switch]$Force)
        if ($LiteralPath -like "*.ferryx-cli.exe.old.*" -and $Destination -like "*ferryx-cli.exe") { return }
        Microsoft.PowerShell.Management\\Move-Item @PSBoundParameters
    }
}
if ($env:MOCK_LOCKED_TARGET) {
    # Deleting the installed ferryx-cli.exe silently does nothing, as a file locked by AV would.
    function Remove-Item {
        [CmdletBinding()]
        param([Parameter(ValueFromPipeline = $true)]$InputObject, [string]$LiteralPath, [switch]$Force)
        process {
            if ($LiteralPath -like "*ferryx-cli.exe" -and -not ($LiteralPath -like "*.ferryx-cli.exe.*")) { return }
            if ($InputObject) { $InputObject | Microsoft.PowerShell.Management\\Remove-Item -Force } else { Microsoft.PowerShell.Management\\Remove-Item @PSBoundParameters }
        }
    }
}
$status = Install-FerryxCandidate -TempExe $TempExe -InstallDir $InstallDir -WorkDir $WorkDir
[Console]::Out.WriteLine("STATUS=" + $status)
`;

const NEW_EXE = Buffer.from("MZ new ferryx-cli.exe");
const OLD_EXE = Buffer.from("MZ previous ferryx-cli.exe");
const USAGE = "Ferryx CLI is running in headless mode.\nUsage: ferryx-cli <up>";

async function runPs1Install({ existing, plant, stale, fresh, failRestore, lockedTarget, concurrent, temp, target }) {
  const root = mkdtempSync(join(tmpdir(), "ferryx-install-ps1-test-"));
  sandboxes.push(root);
  const work = join(root, "work");
  const installDir = join(root, "Ferryx", "bin");
  mkdirSync(work);
  writeFileSync(join(work, "ferryx-cli.exe"), NEW_EXE);
  writeFileSync(join(root, "driver.ps1"), PS1_DRIVER);
  if (existing) {
    mkdirSync(installDir, { recursive: true });
    writeFileSync(join(installDir, "ferryx-cli.exe"), OLD_EXE);
  }
  if (plant) {
    // Leftover ferryx-cli.exe.new / .old DIRECTORIES holding a user file.
    for (const name of ["ferryx-cli.exe.new", "ferryx-cli.exe.old"]) {
      mkdirSync(join(installDir, name), { recursive: true });
      writeFileSync(join(installDir, name, "user-file"), "keep");
    }
  }
  if (stale) {
    // A backup an earlier run could not delete while the previous exe was running.
    const staleBackup = join(installDir, ".ferryx-cli.exe.old.0123456789abcdef");
    writeFileSync(staleBackup, OLD_EXE);
    const twoHoursAgo = new Date(Date.now() - 2 * 60 * 60 * 1000);
    utimesSync(staleBackup, twoHoursAgo, twoHoursAgo);
  }
  if (fresh) {
    // A concurrent run's live backup, written moments ago.
    writeFileSync(join(installDir, ".ferryx-cli.exe.old.fedcba9876543210"), OLD_EXE);
  }
  const proc = Bun.spawn(
    [PWSH, "-NoProfile", "-File", join(root, "driver.ps1"), "-Script", INSTALL_PS1, "-TempExe", join(work, "ferryx-cli.exe"), "-InstallDir", installDir, "-WorkDir", work],
    {
      env: {
        PATH: process.env.PATH,
        HOME: root,
        MOCK_TEMP_CODE: String(temp.code),
        MOCK_TEMP_OUT: temp.out,
        MOCK_TARGET_CODE: String(target.code),
        MOCK_TARGET_OUT: target.out,
        ...(failRestore ? { MOCK_FAIL_RESTORE: "1" } : {}),
        ...(lockedTarget ? { MOCK_LOCKED_TARGET: "1" } : {}),
        ...(concurrent ? { MOCK_CONCURRENT: concurrent } : {}),
      },
      stdin: "ignore",
      stdout: "pipe",
      stderr: "pipe",
    },
  );
  const [stdout, stderr] = await Promise.all([new Response(proc.stdout).text(), new Response(proc.stderr).text(), proc.exited]);
  const status = /STATUS=(-?\d+)/.exec(stdout);
  const exe = join(installDir, "ferryx-cli.exe");
  return {
    status: status ? Number(status[1]) : null,
    stdout,
    stderr,
    dirExists: existsSync(installDir),
    parentExists: existsSync(join(root, "Ferryx")),
    exeIsFile: existsSync(exe) && statSync(exe).isFile(),
    exe: existsSync(exe) && statSync(exe).isFile() ? readFileSync(exe) : null,
    planted: ["ferryx-cli.exe.new", "ferryx-cli.exe.old"].map((name) =>
      existsSync(join(installDir, name, "user-file")) ? readFileSync(join(installDir, name, "user-file"), "utf8") : null,
    ),
    leftovers: existsSync(installDir) ? readdirSync(installDir).filter((name) => name.startsWith(".ferryx-cli.exe.")) : [],
  };
}

test.skipIf(!PWSH)("install.ps1: a candidate the Windows loader rejects (0xC0000135) returns 2 and creates no install dir", async () => {
  const r = await runPs1Install({ existing: false, temp: { code: -1073741515, out: "" }, target: { code: 0, out: USAGE } });
  expect(r.status).toBe(2);
  expect(r.stderr).toContain("0xC0000135 STATUS_DLL_NOT_FOUND");
  expect(r.stderr).toContain(SOURCE_OVERRIDES);
  expect(r.dirExists).toBe(false);
  expect(r.parentExists).toBe(false);
});

test.skipIf(!PWSH)("install.ps1: a rejected candidate leaves the existing exe byte-identical", async () => {
  const r = await runPs1Install({ existing: true, temp: { code: -1073741515, out: "" }, target: { code: 0, out: USAGE } });
  expect(r.status).toBe(2);
  expect(r.exe.equals(OLD_EXE)).toBe(true);
  expect(r.leftovers).toEqual([]);
});

test.skipIf(!PWSH)("install.ps1: a failed post-install check restores the previous exe", async () => {
  const r = await runPs1Install({ existing: true, temp: { code: 1, out: USAGE }, target: { code: 1, out: "" } });
  expect(r.status).toBe(1);
  expect(r.stderr).toContain("The previous ferryx-cli.exe was restored.");
  expect(r.stderr).toContain(SOURCE_OVERRIDES);
  expect(r.exe.equals(OLD_EXE)).toBe(true);
  expect(r.leftovers).toEqual([]);
});

test.skipIf(!PWSH)("install.ps1: a failed restore says so and names the kept backup", async () => {
  const r = await runPs1Install({ existing: true, failRestore: true, temp: { code: 1, out: USAGE }, target: { code: 1, out: "" } });
  expect(r.status).toBe(1);
  expect(r.stderr).not.toContain("was restored");
  expect(r.stderr).toContain("Restoring the previous ferryx-cli.exe failed; it was kept as '");
  expect(r.exeIsFile).toBe(false);
  expect(r.leftovers.length).toBe(1);
  expect(r.leftovers[0]).toMatch(/^\.ferryx-cli\.exe\.old\.[0-9a-f]{32}$/);
  expect(r.stderr).toContain(r.leftovers[0]);
});

test.skipIf(!PWSH)("install.ps1: a successful install removes .ferryx-cli.exe.old.* backups older than an hour, not a fresh one", async () => {
  const r = await runPs1Install({ existing: true, stale: true, fresh: true, temp: { code: 1, out: USAGE }, target: { code: 0, out: USAGE } });
  expect(r.status).toBe(0);
  expect(r.exe.equals(NEW_EXE)).toBe(true);
  expect(r.leftovers).toEqual([".ferryx-cli.exe.old.fedcba9876543210"]);
});

test.skipIf(!PWSH)("install.ps1: an undeletable failed exe plus a failed restore is not reported as restored", async () => {
  const r = await runPs1Install({ existing: true, lockedTarget: true, failRestore: true, temp: { code: 1, out: USAGE }, target: { code: 1, out: "" } });
  expect(r.status).toBe(1);
  expect(r.stderr).not.toContain("was restored");
  expect(r.stderr).toContain("Restoring the previous ferryx-cli.exe failed; it was kept as '");
  expect(r.exe.equals(NEW_EXE)).toBe(true);
  expect(r.leftovers.length).toBe(1);
  expect(r.stderr).toContain(r.leftovers[0]);
});

test.skipIf(!PWSH)("install.ps1: a failed check whose backup a concurrent run removed leaves that run's exe in place", async () => {
  const r = await runPs1Install({ existing: true, concurrent: "MZ concurrent ferryx-cli.exe", temp: { code: 1, out: USAGE }, target: { code: 1, out: "" } });
  expect(r.status).toBe(1);
  expect(r.stderr).not.toContain("was restored");
  expect(r.stderr).toContain("is gone (another install may have replaced it)");
  expect(r.exe.toString()).toBe("MZ concurrent ferryx-cli.exe");
  expect(r.leftovers).toEqual([]);
});

test.skipIf(!PWSH)("install.ps1: a failed post-install check on a fresh install leaves no exe and no new dir", async () => {
  const r = await runPs1Install({ existing: false, temp: { code: 1, out: USAGE }, target: { code: -1073741515, out: "" } });
  expect(r.status).toBe(2);
  expect(r.stderr).toContain("Nothing was left installed.");
  expect(r.dirExists).toBe(false);
  expect(r.parentExists).toBe(false);
});

test.skipIf(!PWSH)("install.ps1: a passing candidate replaces the previous exe and leaves no .old/.new", async () => {
  const r = await runPs1Install({ existing: true, temp: { code: 1, out: USAGE }, target: { code: 0, out: "ferryx-cli 2026.1027.5" } });
  expect(r.status).toBe(0);
  expect(r.stdout).toContain("==> Successfully installed ferryx-cli to ");
  expect(r.exe.equals(NEW_EXE)).toBe(true);
  expect(r.leftovers).toEqual([]);
});

test.skipIf(!PWSH)("install.ps1: output that only contains the exe path in a crash message is not the usage banner", async () => {
  const r = await runPs1Install({
    existing: true,
    temp: { code: 1, out: "C:\\Users\\u\\AppData\\Local\\Temp\\ferryx-install-1\\ferryx-cli.exe: some crash\r\nUsage: was not printed\r\n" },
    target: { code: 0, out: USAGE },
  });
  expect(r.status).toBe(1);
  expect(r.stderr).toContain("output did not look like the Ferryx CLI usage");
  expect(r.exe.equals(OLD_EXE)).toBe(true);
  expect(r.leftovers).toEqual([]);
});

test.skipIf(!PWSH)("install.ps1: a crash (0xC0000005) after printing the usage banner is refused", async () => {
  const r = await runPs1Install({ existing: true, temp: { code: -1073741819, out: USAGE }, target: { code: 0, out: USAGE } });
  expect(r.status).toBe(1);
  expect(r.stderr).toContain("crashed during its self-check (--help exit -1073741819, status 0xC0000005)");
  expect(r.exe.equals(OLD_EXE)).toBe(true);
});

test.skipIf(!PWSH)("install.ps1: leftover ferryx-cli.exe.new/.old directories are neither used nor touched", async () => {
  const r = await runPs1Install({ existing: true, plant: true, temp: { code: 1, out: USAGE }, target: { code: 0, out: USAGE } });
  expect(r.status).toBe(0);
  expect(r.exeIsFile).toBe(true);
  expect(r.exe.equals(NEW_EXE)).toBe(true);
  expect(r.planted).toEqual(["keep", "keep"]);
  expect(r.leftovers).toEqual([]);
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
