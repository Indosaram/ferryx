import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  AGP_VERSION, GRADLE_VERSION, NDK_VERSION, patchAgp, patchAppGradle, patchGradleWrapper, patchTheme,
} from "./android-patches.mjs";
import { patchEntitlements, patchInfoPlist, patchLaunchScreen, patchProjectYml } from "./ios-patches.mjs";

// Fixtures are the raw tauri-cli 2.11.4 mobile templates the init scripts patch.
const fixture = name => readFileSync(new URL(`./fixtures/${name}`, import.meta.url), "utf8");
// Lowered SDK levels prove the patch sets them, not that the template already had them.
const appGradle = fixture("app.build.gradle.kts").replace("compileSdk = 36", "compileSdk = 34").replace("targetSdk = 36", "targetSdk = 34");

describe("android patches", () => {
  test("app gradle targets and compiles against API 36 with NDK r28", () => {
    const out = patchAppGradle(appGradle);
    expect(out).toMatch(/compileSdk = 36\b/);
    expect(out).toMatch(/targetSdk = 36\b/);
    expect(out).toContain(`ndkVersion = "${NDK_VERSION}"`);
  });

  test("release build type uses the keystore signing config when present", () => {
    const out = patchAppGradle(appGradle);
    expect(out).toContain(`System.getenv("FERRYX_ANDROID_KEYSTORE_PROPERTIES")`);
    expect(out).toMatch(/signingConfigs \{[\s\S]*create\("release"\)[\s\S]*storeFile = file\(/);
    expect(out).toMatch(/getByName\("release"\) \{\n\s+signingConfigs\.findByName\("release"\)\?\.let \{ signingConfig = it \}/);
  });

  test("versionCode prefers FERRYX_ANDROID_VERSION_CODE over tauri.properties", () => {
    const out = patchAppGradle(appGradle);
    expect(out).toContain(`System.getenv("FERRYX_ANDROID_VERSION_CODE")`);
    expect(out).toMatch(/versionCode = ferryxVersionCode \?: tauriProperties/);
  });

  test("app gradle patch fails loudly when the template drifts", () => {
    expect(() => patchAppGradle("plugins {}\n")).toThrow(/compileSdk/);
  });

  test("AGP and Gradle wrapper are pinned", () => {
    expect(patchAgp(`classpath("com.android.tools.build:gradle:8.11.0")`)).toBe(`classpath("com.android.tools.build:gradle:${AGP_VERSION}")`);
    expect(patchGradleWrapper("distributionUrl=https\\://services.gradle.org/distributions/gradle-8.9-bin.zip"))
      .toContain(`gradle-${GRADLE_VERSION}-bin.zip`);
  });

  test("theme paints the window and splash with the shell colour", () => {
    const out = patchTheme(fixture("themes.xml"));
    expect(out).toContain(`<item name="android:windowBackground">@color/ferryx_shell</item>`);
    expect(out).toContain(`<item name="android:windowSplashScreenBackground" tools:targetApi="31">@color/ferryx_shell</item>`);
  });
});

describe("ios patches", () => {
  const yml = fixture("project.yml");

  test("project spec is iPhone-only and declares export compliance", () => {
    const out = patchProjectYml(yml, undefined);
    expect(out).toMatch(/settings:\n      base:\n        TARGETED_DEVICE_FAMILY: "1"\n/);
    expect(out).toContain("        ITSAppUsesNonExemptEncryption: false\n");
    // The call surface records audio; without the purpose string iOS kills the app on first record.
    expect(out).toContain("        NSMicrophoneUsageDescription: Talk to the supervisor by voice.\n");
    expect(out).not.toContain("DEVELOPMENT_TEAM: undefined");
  });

  test("tauri.conf.json sets minimumSystemVersion >= 15.0 for Xcode 27 SDK compatibility", () => {
    const conf = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
    expect(Number.parseFloat(conf.bundle.iOS.minimumSystemVersion)).toBeGreaterThanOrEqual(15);
  });

  test("development team comes from the provided env value", () => {
    expect(patchProjectYml(yml, "ABCDE12345")).toContain("        DEVELOPMENT_TEAM: ABCDE12345\n");
  });

  test("privacy manifest is bundled as a resource", () => {
    expect(patchProjectYml(yml, undefined)).toContain("      - path: PrivacyInfo.xcprivacy\n        buildPhase: resources\n");
  });

  // A project generated before the team was known still carries the old signing settings, so
  // re-running init must add the team rather than skip. Measured: xcodebuild refused the build with
  // "Signing for ferryx-mobile_iOS requires a development team".
  test("patching a second time adds a team the first run did not have, without duplicating lines", () => {
    const first = patchProjectYml(yml, undefined);
    const second = patchProjectYml(first, "ABCDE12345");
    expect(second).toContain("        DEVELOPMENT_TEAM: ABCDE12345\n");
    expect(second.match(/TARGETED_DEVICE_FAMILY/g)).toHaveLength(1);
    expect(second.match(/ITSAppUsesNonExemptEncryption/g)).toHaveLength(1);
    expect(second.match(/NSMicrophoneUsageDescription/g)).toHaveLength(1);
    expect(second.match(/PrivacyInfo\.xcprivacy/g)).toHaveLength(1);
  });

  test("changing the team rewrites it instead of stacking a second value", () => {
    const a = patchProjectYml(yml, "AAAAAAAAAA");
    const b = patchProjectYml(a, "BBBBBBBBBB");
    expect(b).toContain("        DEVELOPMENT_TEAM: BBBBBBBBBB\n");
    expect(b).not.toContain("AAAAAAAAAA");
    expect(b.match(/DEVELOPMENT_TEAM/g)).toHaveLength(1);
  });

  test("Info.plist gains ITSAppUsesNonExemptEncryption=false, or flips an existing true", () => {
    const plist = `<?xml version="1.0"?>\n<plist version="1.0">\n<dict>\n\t<key>CFBundleName</key>\n\t<string>Ferryx</string>\n</dict>\n</plist>\n`;
    expect(patchInfoPlist(plist)).toMatch(/<key>ITSAppUsesNonExemptEncryption<\/key>\n\t<false\/>\n<\/dict>/);
    const existing = plist.replace("</dict>", "\t<key>ITSAppUsesNonExemptEncryption</key>\n\t<true/>\n</dict>");
    expect(patchInfoPlist(existing)).toMatch(/<key>ITSAppUsesNonExemptEncryption<\/key>\n\t<false\/>/);
  });

  test("launch screen uses #0a0a0a instead of the system background", () => {
    const out = patchLaunchScreen(fixture("LaunchScreen.storyboard"));
    expect(out).toContain(`<color key="backgroundColor" red="0.0392" green="0.0392" blue="0.0392" alpha="1"`);
    expect(out).not.toContain(`<color key="backgroundColor" systemColor="systemBackgroundColor"/>`);
  });

  test("committed privacy manifest declares no tracking and linked email for app functionality", () => {
    const manifest = readFileSync(new URL("../ios-templates/PrivacyInfo.xcprivacy", import.meta.url), "utf8");
    expect(manifest).toMatch(/<key>NSPrivacyTracking<\/key>\s*<false\/>/);
    expect(manifest).toMatch(/NSPrivacyCollectedDataTypeEmailAddress<\/string>\s*<key>NSPrivacyCollectedDataTypeLinked<\/key>\s*<true\/>/);
    expect(manifest).toContain("NSPrivacyCollectedDataTypePurposeAppFunctionality");
    expect(manifest).toMatch(/NSPrivacyAccessedAPICategoryUserDefaults<\/string>\s*<key>NSPrivacyAccessedAPITypeReasons<\/key>\s*<array>\s*<string>CA92\.1<\/string>/);
  });

  // `gen/apple` is gitignored, so a regenerated project is the normal case: without this patch
  // the entitlements file comes back without `aps-environment` and APNs registration fails.
  test("entitlements gain aps-environment so APNs can issue a device token", () => {
    const empty = `<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0">\n<dict>\n</dict>\n</plist>\n`;
    expect(patchEntitlements(empty)).toMatch(/<key>aps-environment<\/key>\n\t<string>development<\/string>/);
  });

  test("entitlements patch rewrites an existing production value to development", () => {
    const signed = `<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0">\n<dict>\n\t<key>aps-environment</key>\n\t<string>production</string>\n</dict>\n</plist>\n`;
    expect(patchEntitlements(signed)).toMatch(/<string>development<\/string>/);
    expect(patchEntitlements(signed)).not.toMatch(/<string>production<\/string>/);
  });

  test("entitlements patch is idempotent", () => {
    const once = patchEntitlements(`<plist version="1.0">\n<dict>\n</dict>\n</plist>\n`);
    expect(patchEntitlements(once)).toBe(once);
  });

  test("entitlements patch fails loudly when the template drifts", () => {
    expect(() => patchEntitlements("not a plist")).toThrow(/entitlements root dict/);
  });

  // Measured on a fresh `tauri ios init` (tauri-cli 2.11.x, macOS 27.2): the generated
  // entitlements file is 0 bytes, which the root-dict anchor above cannot match. Treat an empty
  // file as a template and emit a whole plist, or the init dies before building.
  test("entitlements patch fills an empty file with a complete plist", () => {
    for (const blank of ["", "   \n\t\n"]) {
      const patched = patchEntitlements(blank);
      expect(patched).toContain("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
      expect(patched).toContain("<!DOCTYPE plist");
      expect(patched).toMatch(/<key>aps-environment<\/key>\n\t<string>development<\/string>/);
      expect(patched.trimEnd().endsWith("</plist>")).toBe(true);
    }
  });

  test("an empty entitlements file patches idempotently", () => {
    const once = patchEntitlements("");
    expect(patchEntitlements(once)).toBe(once);
  });

  // Measured on the actual `tauri ios init` output: the template ships the root dict
  // self-closing (a bare dict with a single slash), which a `<dict>\s*` anchor never matches.
  test("entitlements patch fills a self-closing root dict", () => {
    const tauri =
      '<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0">\n<dict/>\n</plist>\n';
    const patched = patchEntitlements(tauri);
    expect(patched).not.toContain("<dict/>");
    expect(patched).toMatch(/<dict>\n\t<key>aps-environment<\/key>\n\t<string>development<\/string>\n<\/dict>/);
    expect(patchEntitlements(patched)).toBe(patched);
  });
});
