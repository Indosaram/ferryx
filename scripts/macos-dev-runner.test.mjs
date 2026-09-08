import { test, expect } from "bun:test";
import { chmod, mkdir, mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

for (const signingExit of [0, 7]) {
  test(`debug runner uses Developer ID and respects signing exit ${signingExit}`, async () => {
    expect((await stat(new URL("./macos-dev-runner.sh", import.meta.url))).mode & 0o111).not.toBe(0);
    const root = await mkdtemp(join(tmpdir(), "ferryx-dev-signing-"));
    const scripts = join(root, "scripts");
    const target = join(root, "src-tauri", "target", "debug");
    const tools = join(root, "tools");
    try {
      await Promise.all([
        mkdir(scripts),
        mkdir(tools),
        mkdir(join(target, "Contents"), { recursive: true }),
        mkdir(join(root, "src-tauri", "icons"), { recursive: true }),
      ]);
      const executable = async (path, contents) => {
        await Bun.write(path, contents);
        await chmod(path, 0o755);
      };
      await Promise.all([
        Bun.write(join(scripts, "macos-dev-runner.sh"), await readFile(new URL("./macos-dev-runner.sh", import.meta.url))),
        Bun.write(join(target, "Contents", "Info.plist"), "<plist/>"),
        Bun.write(join(root, "src-tauri", "icons", "icon.icns"), "fixture"),
        executable(join(target, "ferryx"), "#!/bin/sh\nprintf 'APP_LAUNCHED\\n'\n"),
        executable(join(tools, "uname"), "#!/bin/sh\nprintf 'Darwin\\n'\n"),
        executable(join(tools, "cargo"), "#!/bin/sh\nexit 0\n"),
        executable(join(tools, "codesign"), "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$SIGN_ARGS\"\nexit \"$SIGN_EXIT\"\n"),
      ]);
      const process = Bun.spawn(["bash", join(scripts, "macos-dev-runner.sh"), "run"], {
        cwd: root,
        env: {
          ...Bun.env,
          PATH: `${tools}:${Bun.env.PATH}`,
          CARGO_TARGET_DIR: target.replace(/\/debug$/, ""),
          SIGN_ARGS: join(root, "sign-args"),
          SIGN_EXIT: String(signingExit),
        },
        stdout: "pipe",
        stderr: "pipe",
      });
      const [stdout, stderr, exitCode] = await Promise.all([
        new Response(process.stdout).text(),
        new Response(process.stderr).text(),
        process.exited,
      ]);
      const args = (await readFile(join(root, "sign-args"), "utf8")).trim().split("\n");
      expect(args[args.indexOf("--sign") + 1]).toBe("Developer ID Application: Indo Yoon (5DUM8WPB4C)");
      expect(exitCode, stderr).toBe(signingExit);
      expect(stdout.includes("APP_LAUNCHED")).toBe(signingExit === 0);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  }, 30_000);
}
