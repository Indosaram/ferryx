// Regenerates every committed shell icon from the 1024px source, then flattens the
// iOS set: App Store Connect rejects an AppIcon (notably the 1024 marketing icon)
// that carries an alpha channel, even when every pixel is opaque.
import { execFileSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

export const SHELL_BACKGROUND = "#0a0a0a";
const mobile = fileURLToPath(new URL("..", import.meta.url));
const iosIcons = path.join(mobile, "src-tauri/icons/ios");

execFileSync(process.execPath, ["scripts/tauri.mjs", "icon", "icons.json"], { cwd: mobile, stdio: "inherit" });
for (const name of readdirSync(iosIcons).filter(n => n.endsWith(".png"))) {
  const file = path.join(iosIcons, name);
  execFileSync("magick", [file, "-background", SHELL_BACKGROUND, "-alpha", "remove", "-alpha", "off", file]);
}
const marketing = execFileSync("sips", ["-g", "hasAlpha", path.join(iosIcons, "AppIcon-512@2x.png")], { encoding: "utf8" });
if (!/hasAlpha: no/.test(marketing)) throw new Error(`iOS marketing icon still has alpha:\n${marketing}`);
console.log("icons regenerated; iOS AppIcon set flattened onto", SHELL_BACKGROUND);
