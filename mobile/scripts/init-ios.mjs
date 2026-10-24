import { execFileSync } from "node:child_process";
import { copyFileSync, cpSync, existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { patchInfoPlist, patchLaunchScreen, patchProjectYml } from "./ios-patches.mjs";

const mobile = fileURLToPath(new URL("..", import.meta.url));
const cli = path.join(mobile, "../node_modules/@tauri-apps/cli/tauri.js");
const apple = path.join(mobile, "src-tauri/gen/apple");
const team = process.env.APPLE_DEVELOPMENT_TEAM || undefined;

if (!team) console.warn("WARNING: APPLE_DEVELOPMENT_TEAM is unset; set it (or pick a team in Xcode) before archiving.");
// `ios init` only generates the Xcode project (XcodeGen); it does not compile the app.
if (!existsSync(path.join(apple, "project.yml"))) {
  execFileSync(process.execPath, [cli, "ios", "init", "--ci"], { cwd: mobile, stdio: "inherit" });
}

function patch(file, transform) {
  const target = path.join(apple, file);
  writeFileSync(target, transform(readFileSync(target, "utf8")));
}
const yml = readFileSync(path.join(apple, "project.yml"), "utf8");
// Idempotent: re-running the script on an already patched project leaves it alone.
if (!yml.includes("PrivacyInfo.xcprivacy")) patch("project.yml", s => patchProjectYml(s, team));
patch("LaunchScreen.storyboard", patchLaunchScreen);
copyFileSync(path.join(mobile, "ios-templates/PrivacyInfo.xcprivacy"), path.join(apple, "PrivacyInfo.xcprivacy"));
// Committed, alpha-free AppIcon set from `bun scripts/icons.mjs`.
cpSync(path.join(mobile, "src-tauri/icons/ios"), path.join(apple, "Assets.xcassets/AppIcon.appiconset"), { recursive: true });

// Regenerate the .xcodeproj from the patched spec, then patch the emitted Info.plist.
execFileSync("xcodegen", ["generate", "--spec", "project.yml"], { cwd: apple, stdio: "inherit" });
const plistDir = readdirSync(apple).find(n => n.endsWith("_iOS") && existsSync(path.join(apple, n, "Info.plist")));
if (!plistDir) throw new Error("generated *_iOS/Info.plist not found");
patch(path.join(plistDir, "Info.plist"), patchInfoPlist);
console.log(`ios project patched: iPhone-only, privacy manifest, export compliance, dark launch screen, team ${team ?? "<unset>"}.`);
