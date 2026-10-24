import { execFileSync } from "node:child_process";
import { cpSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import {
  MAIN_ACTIVITY, NDK_VERSION, patchAgp, patchAppGradle, patchBuildTask, patchColors,
  patchGradleProperties, patchGradleWrapper, patchTheme,
} from "./android-patches.mjs";

const mobile = fileURLToPath(new URL("..", import.meta.url));
const cli = path.join(mobile, "../node_modules/@tauri-apps/cli/tauri.js");
const gen = path.join(mobile, "src-tauri/gen/android");

const ndkHome = process.env.NDK_HOME ?? "";
if (!ndkHome.includes(NDK_VERSION)) {
  console.warn(`WARNING: NDK_HOME=${ndkHome || "<unset>"}; release builds need NDK ${NDK_VERSION} for 16 KB page alignment.`);
}

execFileSync(process.execPath, [cli, "android", "init", "--ci"], { cwd: mobile, stdio: "inherit" });

function patch(file, transform) {
  const target = path.join(gen, file);
  writeFileSync(target, transform(readFileSync(target, "utf8")));
}
patch("build.gradle.kts", patchAgp);
patch("buildSrc/build.gradle.kts", patchAgp);
patch("gradle/wrapper/gradle-wrapper.properties", patchGradleWrapper);
patch("app/build.gradle.kts", patchAppGradle);
patch("gradle.properties", patchGradleProperties);
patch("app/src/main/res/values/colors.xml", patchColors);
patch("app/src/main/res/values/themes.xml", patchTheme);
if (existsSync(path.join(gen, "app/src/main/res/values-night/themes.xml"))) {
  patch("app/src/main/res/values-night/themes.xml", patchTheme);
}
patch("app/src/main/java/com/ferryx/mobile/MainActivity.kt", () => MAIN_ACTIVITY);
patch("buildSrc/src/main/java/com/ferryx/mobile/kotlin/BuildTask.kt", s => patchBuildTask(s, cli));

// Committed launcher icons (mipmaps + adaptive XML + background colour) from `bun scripts/icons.mjs`.
cpSync(path.join(mobile, "src-tauri/icons/android"), path.join(gen, "app/src/main/res"), { recursive: true });
console.log("android project patched: SDK 36, AGP/Gradle pinned, release signing, icons, dark splash.");
