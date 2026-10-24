import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  AGP_VERSION, GRADLE_VERSION, NDK_VERSION, patchAgp, patchAppGradle, patchGradleWrapper, patchTheme,
} from "./android-patches.mjs";
import { patchInfoPlist, patchLaunchScreen, patchProjectYml } from "./ios-patches.mjs";

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

  test("Info.plist gains ITSAppUsesNonExemptEncryption=false, or flips an existing true", () => {
    const plist = `<?xml version="1.0"?>\n<plist version="1.0">\n<dict>\n\t<key>CFBundleName</key>\n\t<string>Ferryx</string>\n</dict>\n</plist>\n`;
    expect(patchInfoPlist(plist)).toMatch(/<key>ITSAppUsesNonExemptEncryption<\/key>\n\t<false\/>\n<\/dict>/);
    const existing = plist.replace("</dict>", "\t<key>ITSAppUsesNonExemptEncryption</key>\n\t<true/>\n</dict>");
    expect(patchInfoPlist(existing)).toMatch(/<key>ITSAppUsesNonExemptEncryption<\/key>\n\t<false\/>/);
  });

  test("launch screen uses #23262D instead of the system background", () => {
    const out = patchLaunchScreen(fixture("LaunchScreen.storyboard"));
    expect(out).toContain(`<color key="backgroundColor" red="0.1373" green="0.1490" blue="0.1765" alpha="1"`);
    expect(out).not.toContain(`<color key="backgroundColor" systemColor="systemBackgroundColor"/>`);
  });

  test("committed privacy manifest declares no tracking and linked email for app functionality", () => {
    const manifest = readFileSync(new URL("../ios-templates/PrivacyInfo.xcprivacy", import.meta.url), "utf8");
    expect(manifest).toMatch(/<key>NSPrivacyTracking<\/key>\s*<false\/>/);
    expect(manifest).toMatch(/NSPrivacyCollectedDataTypeEmailAddress<\/string>\s*<key>NSPrivacyCollectedDataTypeLinked<\/key>\s*<true\/>/);
    expect(manifest).toContain("NSPrivacyCollectedDataTypePurposeAppFunctionality");
    expect(manifest).toMatch(/NSPrivacyAccessedAPICategoryUserDefaults<\/string>\s*<key>NSPrivacyAccessedAPITypeReasons<\/key>\s*<array>\s*<string>CA92\.1<\/string>/);
  });
});
