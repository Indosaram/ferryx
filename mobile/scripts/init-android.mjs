import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const mobile = fileURLToPath(new URL("..", import.meta.url));
const cli = path.join(mobile, "../node_modules/@tauri-apps/cli/tauri.js");
execFileSync(process.execPath, [cli, "android", "init", "--ci"], { cwd: mobile, stdio: "inherit" });
// The CLI's generated templates can drift independently of this source tree.
// Pin to the available Android 35/36 toolchain instead of silently requiring preview SDKs.
const gen = path.join(mobile, "src-tauri/gen/android");
function patch(file, transform) {
  const target = path.join(gen, file);
  writeFileSync(target, transform(readFileSync(target, "utf8")));
}
patch("build.gradle.kts", s => s.replace(/com.android.tools.build:gradle:[^"\s]+/, "com.android.tools.build:gradle:8.7.3"));
patch("buildSrc/build.gradle.kts", s => s.replace(/com.android.tools.build:gradle:[^"\s]+/, "com.android.tools.build:gradle:8.7.3"));
patch("gradle/wrapper/gradle-wrapper.properties", s => s.replace(/gradle-[\d.]+-bin.zip/, "gradle-8.9-bin.zip"));
patch("app/build.gradle.kts", s => s
  .replace(/compileSdk = \d+/, "compileSdk = 35")
  .replace(/targetSdk = \d+/, "targetSdk = 35")
  .replace(/optimization\s*\{\s*enable = true\s*\}/, "isMinifyEnabled = true")
  .replace(/lifecycle-process:[\d.]+/, "lifecycle-process:2.8.7"));
patch("gradle.properties", s => s
  .replace(/^org.gradle.jvmargs=.*$/m, "org.gradle.jvmargs=-Xmx768m -XX:MaxMetaspaceSize=384m")
  + "\norg.gradle.workers.max=1\nkotlin.compiler.execution.strategy=in-process\norg.gradle.daemon=false\n");

patch("buildSrc/src/main/java/com/ferryx/mobile/kotlin/BuildTask.kt", s => s.replace('listOf("tauri", "android", "android-studio-script")', `listOf("${cli.replaceAll("\\", "/")}", "android", "android-studio-script")`));
