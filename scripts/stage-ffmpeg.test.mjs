import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  assertLgplReceipt,
  checkStaged,
  isRuntimeLibrary,
  overlayConfig,
  parseArgs,
  readPin,
  stageTarget,
} from "./stage-ffmpeg.mjs";

const PIN = readPin();
const LGPL_CONF = "--prefix=/x --disable-doc --enable-shared --disable-static --enable-vaapi";
const RECEIPT = `FFmpeg ${PIN.source.version} (sha256 ${PIN.source.sha256})\n${LGPL_CONF}\n`;
const LIBS = ["avutil", "swresample", "swscale", "avcodec", "avformat", "avfilter", "avdevice"];

function withTmp(fn) {
  const dir = mkdtempSync(join(tmpdir(), "ferryx-stage-ffmpeg-"));
  return Promise.resolve(fn(dir)).finally(() => rmSync(dir, { recursive: true, force: true }));
}

function linuxPrefix(dir, receipt = RECEIPT) {
  mkdirSync(join(dir, "lib"), { recursive: true });
  mkdirSync(join(dir, "share", "ferryx-legal"), { recursive: true });
  for (const [i, lib] of LIBS.entries()) {
    writeFileSync(join(dir, "lib", `lib${lib}.so.${60 + i}`), `elf ${lib}`);
    writeFileSync(join(dir, "lib", `lib${lib}.so`), "dev symlink stand-in");
  }
  writeFileSync(join(dir, "share", "ferryx-legal", "COPYING.LGPLv2.1"), "LGPL 2.1");
  writeFileSync(join(dir, "share", "ferryx-legal", "LICENSE.md"), "FFmpeg license");
  writeFileSync(join(dir, "build-receipt.txt"), receipt);
  return dir;
}

test("receipt gate accepts LGPL-only configurations and rejects gpl, nonfree, x264 and openh264", () => {
  assert.ok(assertLgplReceipt(RECEIPT, { source: PIN.source }));
  for (const flag of ["--enable-gpl", "--enable-nonfree", "--enable-libx264", "--enable-libfdk-aac", "--enable-libopenh264"]) {
    assert.throws(() => assertLgplReceipt(`${RECEIPT.trim()} ${flag}\n`, { source: PIN.source }), /not LGPL-only/);
  }
  assert.throws(() => assertLgplReceipt(RECEIPT.replace(PIN.source.sha256, "0".repeat(64)), { source: PIN.source }), /pinned source/);
  assert.throws(() => assertLgplReceipt("", { source: PIN.source }), /missing or empty/);
  assert.throws(() => assertLgplReceipt(RECEIPT.replace("--enable-shared", "--enable-static")), /--enable-shared/);
  // --disable-libx264 is not an enable flag.
  assert.ok(assertLgplReceipt(`${RECEIPT.trim()} --disable-libx264 --disable-libx265\n`));
});

test("runtime library names follow each platform's loader convention", () => {
  assert.ok(isRuntimeLibrary("x86_64-unknown-linux-gnu", "libavcodec.so.63"));
  assert.ok(!isRuntimeLibrary("x86_64-unknown-linux-gnu", "libavcodec.so"));
  assert.ok(isRuntimeLibrary("universal-apple-darwin", "libavutil.61.dylib"));
  assert.ok(!isRuntimeLibrary("universal-apple-darwin", "libavutil.dylib"));
  assert.ok(isRuntimeLibrary("x86_64-pc-windows-msvc", "avformat-63.dll"));
  assert.ok(!isRuntimeLibrary("x86_64-pc-windows-msvc", "ffmpeg.exe"));
});

test("parseArgs keys prefixes by triple and rejects unknown targets", () => {
  const options = parseArgs(["--target", "universal-apple-darwin", "--prefix", "aarch64-apple-darwin=/a", "--prefix", "/plain"]);
  assert.equal(options.prefixes["aarch64-apple-darwin"], "/a");
  assert.equal(options.prefixes["*"], "/plain");
  assert.throws(() => parseArgs(["--target", "riscv64-unknown-linux-gnu"]), /unsupported/);
});

test("overlay bundles libraries per platform without touching unrelated config", () => {
  const base = { bundle: { linux: { deb: { depends: ["libgtk-3-0"] } } } };
  const root = "/repo/src-tauri/resources/ffmpeg/payload/x";
  const linux = overlayConfig({ triple: "x86_64-unknown-linux-gnu", root, runtime: ["libavutil.so.61"], legal: ["SOURCE.txt"], baseConfig: base, tauriDir: "/repo/src-tauri" });
  assert.equal(linux.bundle.resources["resources/ffmpeg/payload/x/runtime/libavutil.so.61"], "ffmpeg/libavutil.so.61");
  assert.equal(linux.bundle.resources["resources/ffmpeg/payload/x/legal/SOURCE.txt"], "ffmpeg-legal/SOURCE.txt");
  assert.deepEqual(linux.bundle.linux.deb.depends, ["libgtk-3-0", "libva2", "libva-drm2"]);
  const win = overlayConfig({ triple: "x86_64-pc-windows-msvc", root, runtime: ["avutil-61.dll"], legal: [], baseConfig: base, tauriDir: "/repo/src-tauri" });
  assert.equal(win.bundle.resources["resources/ffmpeg/payload/x/runtime/avutil-61.dll"], "avutil-61.dll");
  assert.equal(win.bundle.linux, undefined);
  const mac = overlayConfig({ triple: "universal-apple-darwin", root, runtime: ["libavutil.61.dylib"], legal: [], baseConfig: base, tauriDir: "/repo/src-tauri" });
  assert.deepEqual(mac.bundle.macOS.frameworks, [`${root}/runtime/libavutil.61.dylib`]);
});

test("linux: stages a receipt-checked prefix, sets $ORIGIN rpath, and check fails closed on tampering", () =>
  withTmp(async (dir) => {
    const prefix = linuxPrefix(join(dir, "prefix"));
    const dest = join(dir, "payload");
    const calls = [];
    const exec = (command, args) => {
      calls.push([command, ...args]);
      return "";
    };
    const triple = "x86_64-unknown-linux-gnu";
    const manifest = await stageTarget({ dest, triple, pin: PIN, prefixes: { "*": prefix }, exec });
    assert.deepEqual(Object.keys(manifest.runtime).sort(), LIBS.map((lib, i) => `lib${lib}.so.${60 + i}`).sort());
    assert.ok(calls.every(([command, flag, value]) => command === "patchelf" && flag === "--set-rpath" && value === "$ORIGIN"));
    assert.equal(calls.length, LIBS.length);
    for (const name of ["build-receipt.txt", "SOURCE.txt", "COPYING.LGPLv2.1", "LICENSE.md"]) {
      assert.ok(existsSync(join(dest, triple, "legal", name)), name);
    }
    assert.match(readFileSync(join(dest, triple, "legal", "SOURCE.txt"), "utf8"), new RegExp(PIN.source.sha256));

    writeFileSync(join(dest, triple, "runtime", "libavcodec.so.63"), "swapped");
    assert.throws(() => checkStaged({ dest, triple, pin: PIN }), /does not match stage\.json/);
  }));

test("linux: refuses a non-LGPL prefix and leaves no payload behind", () =>
  withTmp(async (dir) => {
    const prefix = linuxPrefix(join(dir, "prefix"), `${RECEIPT.trim()} --enable-gpl\n`);
    const dest = join(dir, "payload");
    await assert.rejects(
      stageTarget({ dest, triple: "x86_64-unknown-linux-gnu", pin: PIN, prefixes: { "*": prefix }, exec: () => "" }),
      /not LGPL-only/,
    );
    assert.ok(!existsSync(join(dest, "x86_64-unknown-linux-gnu")));
    assert.throws(() => checkStaged({ dest, triple: "x86_64-unknown-linux-gnu", pin: PIN }), /not staged/);
  }));

test("darwin universal: lipo's both slices and rewrites install names to @rpath", () =>
  withTmp(async (dir) => {
    const prefixes = {};
    for (const slice of ["aarch64-apple-darwin", "x86_64-apple-darwin"]) {
      const prefix = join(dir, slice);
      mkdirSync(join(prefix, "lib"), { recursive: true });
      mkdirSync(join(prefix, "share", "ferryx-legal"), { recursive: true });
      for (const [i, lib] of LIBS.entries()) writeFileSync(join(prefix, "lib", `lib${lib}.${60 + i}.dylib`), `${slice} ${lib}`);
      writeFileSync(join(prefix, "share", "ferryx-legal", "COPYING.LGPLv2.1"), "LGPL 2.1");
      writeFileSync(join(prefix, "share", "ferryx-legal", "LICENSE.md"), "FFmpeg license");
      writeFileSync(join(prefix, "build-receipt.txt"), RECEIPT);
      prefixes[slice] = prefix;
    }
    const calls = [];
    const exec = (command, args) => {
      calls.push([command, ...args]);
      if (command === "lipo") writeFileSync(args[args.length - 1], "fat");
      if (command === "otool") return `${args[1]}:\n\t${prefixes["aarch64-apple-darwin"]}/lib/libavutil.60.dylib (compat)\n`;
      return "";
    };
    const dest = join(dir, "payload");
    const manifest = await stageTarget({ dest, triple: "universal-apple-darwin", pin: PIN, prefixes, exec });
    assert.equal(Object.keys(manifest.runtime).length, LIBS.length);
    assert.equal(calls.filter(([command]) => command === "lipo").length, LIBS.length);
    const installs = calls.filter(([command]) => command === "install_name_tool");
    assert.equal(installs.length, LIBS.length);
    for (const call of installs) {
      assert.equal(call[1], "-id");
      assert.match(call[2], /^@rpath\/lib\w+\.\d+\.dylib$/);
      assert.deepEqual(call.slice(3, 6), ["-change", `${prefixes["aarch64-apple-darwin"]}/lib/libavutil.60.dylib`, "@rpath/libavutil.60.dylib"]);
    }
    assert.match(readFileSync(join(dest, "universal-apple-darwin", "legal", "build-receipt.txt"), "utf8"), /# slice x86_64-apple-darwin/);
    const overlay = JSON.parse(readFileSync(join(dest, "universal-apple-darwin", "tauri.ffmpeg.conf.json"), "utf8"));
    assert.equal(overlay.bundle.macOS.frameworks.length, LIBS.length);
  }));

test("darwin universal: a missing slice prefix fails closed", () =>
  withTmp(async (dir) => {
    await assert.rejects(
      stageTarget({ dest: join(dir, "payload"), triple: "universal-apple-darwin", pin: PIN, prefixes: {}, exec: () => "" }),
      /no FFmpeg prefix for aarch64-apple-darwin/,
    );
  }));

test("windows: stages DLLs from bin/ plus an FFMPEG_DIR sdk, and refuses OpenH264", () =>
  withTmp(async (dir) => {
    const triple = "x86_64-pc-windows-msvc";
    const winConf = "--prefix=/c/ff --toolchain=msvc --target-os=win64 --enable-shared --disable-static --enable-nvenc --enable-amf --enable-d3d11va --enable-mediafoundation";
    const makePrefix = (path, configuration) => {
      mkdirSync(join(path, "bin"), { recursive: true });
      mkdirSync(join(path, "lib"), { recursive: true });
      mkdirSync(join(path, "include", "libavutil"), { recursive: true });
      mkdirSync(join(path, "share", "ferryx-legal"), { recursive: true });
      for (const [i, lib] of LIBS.entries()) {
        writeFileSync(join(path, "bin", `${lib}-${60 + i}.dll`), `MZ ${lib}`);
        writeFileSync(join(path, "lib", `${lib}.lib`), "implib");
      }
      writeFileSync(join(path, "bin", "ffmpeg.exe"), "MZ exe");
      writeFileSync(join(path, "include", "libavutil", "avutil.h"), "h");
      writeFileSync(join(path, "share", "ferryx-legal", "COPYING.LGPLv2.1"), "LGPL 2.1");
      writeFileSync(join(path, "share", "ferryx-legal", "LICENSE.md"), "FFmpeg license");
      writeFileSync(join(path, "build-receipt.txt"), `FFmpeg ${PIN.source.version} (sha256 ${PIN.source.sha256})\n${configuration}\n`);
      return path;
    };
    const dest = join(dir, "payload");
    const calls = [];
    const manifest = await stageTarget({
      dest, triple, pin: PIN, prefixes: { "*": makePrefix(join(dir, "good"), winConf) },
      exec: (command, args) => calls.push([command, ...args]),
    });
    assert.deepEqual(calls, [], "Windows staging rewrites nothing");
    assert.equal(Object.keys(manifest.runtime).length, LIBS.length);
    assert.ok(!Object.keys(manifest.runtime).includes("ffmpeg.exe"));
    assert.equal(manifest.source.sha256, PIN.source.sha256);
    assert.ok(existsSync(join(dest, triple, "sdk", "lib", "avcodec.lib")));
    assert.ok(existsSync(join(dest, triple, "sdk", "include", "libavutil", "avutil.h")));
    const overlay = JSON.parse(readFileSync(join(dest, triple, "tauri.ffmpeg.conf.json"), "utf8"));
    assert.ok(Object.values(overlay.bundle.resources).includes("avcodec-63.dll"), "DLLs land beside ferryx.exe");

    // A receipt that enables OpenH264 is refused on Windows (and everywhere else).
    await assert.rejects(
      stageTarget({ dest: join(dir, "p2"), triple, pin: PIN, prefixes: { "*": makePrefix(join(dir, "openh264"), `${winConf} --enable-libopenh264`) }, exec: () => "" }),
      /not LGPL-only \(--enable-libopenh264\)/,
    );
    assert.ok(!existsSync(join(dir, "p2", triple)));
    // A prefix without import libraries cannot link and is refused.
    const noSdk = makePrefix(join(dir, "nosdk"), winConf);
    rmSync(join(noSdk, "lib"), { recursive: true, force: true });
    await assert.rejects(stageTarget({ dest: join(dir, "p3"), triple, pin: PIN, prefixes: { "*": noSdk }, exec: () => "" }), /MSVC import libraries/);
  }));
