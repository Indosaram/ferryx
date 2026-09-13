import { test, expect } from "bun:test";
import { chmod, mkdir, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, posix, win32 } from "node:path";

function targetRootFor(target, paths) {
  return paths.dirname(target);
}

for (const paths of [posix, win32]) {
  test(`signing target has exactly one debug component (${paths.sep})`, () => {
    const target = paths.join("src-tauri", "target", "debug");
    expect(paths.join(targetRootFor(target, paths), "debug")).toBe(target);
  });
}

for (const signingExit of [0, 7]) {
  test(`debug runner uses Developer ID and respects signing exit ${signingExit}`, async () => {
    const root = await mkdtemp(join(tmpdir(), "ferryx dev signing-"));
    const scripts = join(root, "scripts");
    const target = join(root, "src-tauri", "target", "debug");
    const tools = join(root, "tools");
    let child;
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
        executable(join(tools, "uname"), "#!/bin/sh\nprintf '%s\\n' uname >> tool-calls\nprintf 'Darwin\\n'\n"),
        executable(join(tools, "cargo"), "#!/bin/sh\nprintf '%s\\n' cargo \"$@\" >> tool-calls\n"),
        executable(join(tools, "codesign"), "#!/bin/sh\nprintf '%s\\n' codesign >> tool-calls\nprintf '%s\\n' \"$@\" > \"$SIGN_ARGS\"\nexit \"$SIGN_EXIT\"\n"),
      ]);
      // Let Bash own its path namespace; no native drive letters in POSIX PATH.
      // Functions intercept these commands even if a fixture tool is missing:
      // the explicit Bash invocation fails rather than finding a real build/signer.
      child = Bun.spawn(["bash", "--noprofile", "--norc", "-c", `
set -euo pipefail
uname() { bash ./tools/uname "$@"; }
cargo() { bash ./tools/cargo "$@"; }
codesign() { bash ./tools/codesign "$@"; }
printf '%s\\n' "$PWD" > shell-root
export CARGO_TARGET_DIR="$PWD/$1"
source ./scripts/macos-dev-runner.sh run
`, "signing-fixture", targetRootFor("src-tauri/target/debug", posix)], {
        cwd: root,
        env: {
          ...Bun.env,
          BASH_ENV: "",
          ENV: "",
          SIGN_ARGS: "sign-args",
          SIGN_EXIT: String(signingExit),
        },
        stdout: "pipe",
        stderr: "pipe",
        timeout: 20_000,
      });
      const [stdout, stderr, exitCode] = await Promise.all([
        new Response(child.stdout).text(),
        new Response(child.stderr).text(),
        child.exited,
      ]);
      const args = (await readFile(join(root, "sign-args"), "utf8")).trim().split("\n");
      const shellRoot = (await readFile(join(root, "shell-root"), "utf8")).trim();
      expect(args).toEqual([
        "--force", "--sign", "Developer ID Application: Indo Yoon (5DUM8WPB4C)",
        `${shellRoot}/src-tauri/target/debug/Ferryx.app`,
      ]);
      expect((await readFile(join(root, "tool-calls"), "utf8")).trim().split("\n"))
        .toEqual(["uname", "cargo", "build", "codesign"]);
      expect(exitCode, stderr).toBe(signingExit);
      expect(stdout.includes("APP_LAUNCHED")).toBe(signingExit === 0);
    } finally {
      if (child) {
        if (child.exitCode === null) child.kill();
        await child.exited;
      }
      await rm(root, { recursive: true, force: true });
      console.info(`signing fixture cleaned: exit=${child?.exitCode}, root=${root}`);
    }
  }, 30_000);
}
