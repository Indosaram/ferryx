// Pure transforms applied to the `tauri ios init` output (gen/apple is gitignored).
// Each throws when its anchor is missing so template drift fails loudly.

export const LAUNCH_BACKGROUND = { red: 35 / 255, green: 38 / 255, blue: 45 / 255 }; // #23262D

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
  let s = replaceRequired(
    source,
    /(\n    settings:\n      base:\n)/,
    `$1        TARGETED_DEVICE_FAMILY: "1"\n${team ? `        DEVELOPMENT_TEAM: ${team}\n` : ""}`,
    "app target settings.base",
  );
  s = replaceRequired(
    s,
    /(\n        LSRequiresIPhoneOS: true\n)/,
    "$1        ITSAppUsesNonExemptEncryption: false\n",
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
