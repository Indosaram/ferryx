// Pure transforms applied to the `tauri ios init` output (gen/apple is gitignored).
// Each throws when its anchor is missing so template drift fails loudly.

export const LAUNCH_BACKGROUND = { red: 10 / 255, green: 10 / 255, blue: 10 / 255 }; // #0a0a0a

function replaceRequired(source, pattern, replacement, label) {
  if (!pattern.test(source)) throw new Error(`ios patch anchor missing: ${label}`);
  return source.replace(pattern, replacement);
}

/**
 * project.yml (XcodeGen spec): iPhone-only, optional development team, export
 * compliance key, and the privacy manifest as an explicit resource.
 * `team` is APPLE_DEVELOPMENT_TEAM (undefined leaves signing to Xcode/owner).
 */
export function patchProjectYml(source, team) {
  // Idempotent: strip the lines this patch owns so re-running it (with a different team, or after a
  // project was generated earlier) rewrites them instead of appending duplicates. DEVELOPMENT_TEAM is
  // only stripped when a team is being written, so an init without one cannot drop a template value.
  let owned = source
    .replace(/^ *TARGETED_DEVICE_FAMILY: "1"\n/gm, "")
    .replace(/^ *ITSAppUsesNonExemptEncryption: false\n/gm, "")
    .replace(/^ *NSMicrophoneUsageDescription: .*\n/gm, "");
  if (team) owned = owned.replace(/^ *DEVELOPMENT_TEAM: [A-Z0-9]+\n/gm, "");
  let s = replaceRequired(
    owned,
    /(\n    settings:\n      base:\n)/,
    `$1        TARGETED_DEVICE_FAMILY: "1"\n${team ? `        DEVELOPMENT_TEAM: ${team}\n` : ""}`,
    "app target settings.base",
  );
  s = replaceRequired(
    s,
    /(\n        LSRequiresIPhoneOS: true\n)/,
    "$1        ITSAppUsesNonExemptEncryption: false\n        NSMicrophoneUsageDescription: Talk to the supervisor by voice.\n",
    "Info.plist properties",
  );
  return replaceRequired(s, /(\n      - path: LaunchScreen\.storyboard\n)/, "$1      - path: PrivacyInfo.xcprivacy\n        buildPhase: resources\n", "target sources");
}

/** Info.plist: export compliance (HTTPS/WSS only uses exempt OS-provided encryption). */
export function patchInfoPlist(source) {
  if (source.includes("<key>ITSAppUsesNonExemptEncryption</key>")) {
    return source.replace(/(<key>ITSAppUsesNonExemptEncryption<\/key>\s*)<(true|false)\/>/, "$1<false/>");
  }
  return replaceRequired(source, /<\/dict>\s*<\/plist>\s*$/, "\t<key>ITSAppUsesNonExemptEncryption</key>\n\t<false/>\n</dict>\n</plist>\n", "Info.plist root dict");
}

/** LaunchScreen.storyboard: shell background instead of the white system background. */
export function patchLaunchScreen(source) {
  const { red, green, blue } = LAUNCH_BACKGROUND;
  const color = `<color key="backgroundColor" red="${red.toFixed(4)}" green="${green.toFixed(4)}" blue="${blue.toFixed(4)}" alpha="1" colorSpace="custom" customColorSpace="sRGB"/>`;
  return replaceRequired(source, /<color key="backgroundColor"[^>]*\/>/, color, "launch view backgroundColor");
}

/**
 * The app target's entitlements file: `aps-environment` is what lets iOS hand the app an APNs
 * device token. Without it `registerForRemoteNotifications` fails at runtime, so `gen/apple`
 * being regenerated must not silently drop the capability. `development` is correct for builds
 * signed with a development profile; App Store and TestFlight builds are re-signed by Apple
 * with `production` entitlements.
 *
 * `tauri ios init` may write this file empty (0 bytes), so an empty or whitespace-only source is
 * a legitimate template, not drift: it gets a whole plist. Anything else without a root dict is
 * still a loud failure.
 */
export function patchEntitlements(source) {
  const key = "<key>aps-environment</key>";
  if (source.includes(key)) {
    return source.replace(/(<key>aps-environment<\/key>\s*)<string>[^<]*<\/string>/, "$1<string>development</string>");
  }
  if (!source.trim()) return emptyEntitlements();
  // `tauri ios init` writes the root dict self-closing, which has no body to insert into.
  if (/<dict\s*\/>/.test(source)) {
    return source.replace(/<dict\s*\/>/, `<dict>\n\t<key>aps-environment</key>\n\t<string>development</string>\n</dict>`);
  }
  return replaceRequired(source, /<dict>\s*/, "<dict>\n\t<key>aps-environment</key>\n\t<string>development</string>\n", "entitlements root dict");
}

/** A complete entitlements plist, for when `tauri ios init` leaves the file empty. */
function emptyEntitlements() {
  return `<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0">\n<dict>\n\t<key>aps-environment</key>\n\t<string>development</string>\n</dict>\n</plist>\n`;
}
