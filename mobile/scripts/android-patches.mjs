// Pure transforms applied to the `tauri android init` output (gen/ is gitignored,
// so every native release setting lives here). Each function maps the generated
// file text to the patched text and throws when an anchor is missing, so a CLI
// template drift fails loudly instead of shipping an unpatched project.

// API 36 needs AGP >= 8.9.1 (AGP 8.11 release notes: "maximum API level ... 36",
// minimum Gradle 8.13). We pin the latest 8.x line known on Google Maven.
export const ANDROID_SDK = 36;
export const AGP_VERSION = "8.13.2";
export const GRADLE_VERSION = "8.14.3";
// r28+ links native libraries with 16 KB ELF alignment (Play requirement for API 35+).
export const NDK_VERSION = "28.2.13676358";
export const SHELL_BACKGROUND = "#0a0a0a";

function replaceRequired(source, pattern, replacement, label) {
  if (!pattern.test(source)) throw new Error(`android patch anchor missing: ${label}`);
  return source.replace(pattern, replacement);
}

export function patchAgp(source) {
  return replaceRequired(source, /com\.android\.tools\.build:gradle:[^"\s]+/, `com.android.tools.build:gradle:${AGP_VERSION}`, "AGP classpath");
}

export function patchGradleWrapper(source) {
  return replaceRequired(source, /gradle-[\d.]+-bin\.zip/, `gradle-${GRADLE_VERSION}-bin.zip`, "gradle wrapper");
}

const SIGNING_LOADER = `
// Release signing (Ferryx): keystore.properties holds storeFile/storePassword/keyAlias/keyPassword.
val ferryxKeystoreProperties = Properties().apply {
    val path = System.getenv("FERRYX_ANDROID_KEYSTORE_PROPERTIES")
        ?: rootProject.file("../../../keystore.properties").path
    val propFile = file(path)
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
        // storeFile is resolved relative to keystore.properties, not the Gradle module.
        getProperty("storeFile")?.let { setProperty("storeFile", propFile.parentFile.resolve(it).path) }
    } else {
        logger.warn("WARNING: Ferryx release signing disabled: no keystore.properties at $path (set FERRYX_ANDROID_KEYSTORE_PROPERTIES). Release outputs will be UNSIGNED and cannot be uploaded to Play.")
    }
}
val ferryxVersionCode = System.getenv("FERRYX_ANDROID_VERSION_CODE")?.toInt()
`;

const SIGNING_CONFIG = `    signingConfigs {
        if (ferryxKeystoreProperties.getProperty("storeFile") != null) {
            create("release") {
                storeFile = file(ferryxKeystoreProperties.getProperty("storeFile"))
                storePassword = ferryxKeystoreProperties.getProperty("storePassword")
                keyAlias = ferryxKeystoreProperties.getProperty("keyAlias")
                keyPassword = ferryxKeystoreProperties.getProperty("keyPassword")
            }
        }
    }
`;

/** app/build.gradle.kts: SDK 36, NDK r28, env versionCode, keystore-backed release signing. */
export function patchAppGradle(source) {
  let s = replaceRequired(source, /compileSdk = \d+/, `compileSdk = ${ANDROID_SDK}`, "compileSdk");
  s = replaceRequired(s, /targetSdk = \d+/, `targetSdk = ${ANDROID_SDK}`, "targetSdk");
  s = replaceRequired(s, /\nandroid \{\n/, `\n${SIGNING_LOADER}\nandroid {\n    ndkVersion = "${NDK_VERSION}"\n${SIGNING_CONFIG}`, "android block");
  s = replaceRequired(
    s,
    /versionCode = (tauriProperties\.getProperty\("tauri\.android\.versionCode", "1"\)\.toInt\(\))/,
    "versionCode = ferryxVersionCode ?: $1",
    "versionCode",
  );
  s = replaceRequired(
    s,
    /getByName\("release"\) \{\n/,
    `getByName("release") {\n            signingConfigs.findByName("release")?.let { signingConfig = it }\n`,
    "release buildType",
  );
  // Older templates used the AGP 9 DSL; keep the plain flag for AGP 8.
  return s.replace(/optimization\s*\{\s*enable = true\s*\}/, "isMinifyEnabled = true");
}

export function patchGradleProperties(source) {
  return replaceRequired(source, /^org\.gradle\.jvmargs=.*$/m, "org.gradle.jvmargs=-Xmx768m -XX:MaxMetaspaceSize=384m", "jvmargs")
    + "\nandroid.useAndroidX=true\norg.gradle.workers.max=1\nkotlin.compiler.execution.strategy=in-process\norg.gradle.daemon=false\n";
}

/** values/themes.xml + values-night/themes.xml: dark shell background on launch (splash) and window. */
export function patchTheme(source) {
  const items = [
    `        <item name="android:windowBackground">@color/ferryx_shell</item>`,
    `        <item name="android:windowSplashScreenBackground" tools:targetApi="31">@color/ferryx_shell</item>`,
    `        <item name="android:statusBarColor">@color/ferryx_shell</item>`,
    `        <item name="android:navigationBarColor">@color/ferryx_shell</item>`,
  ].join("\n");
  return replaceRequired(source, /(<style name="Theme\.[^"]+"[^>]*>)[\s\S]*?(<\/style>)/, `$1\n${items}\n    $2`, "app theme style");
}

export function patchColors(source) {
  return replaceRequired(source, /<\/resources>/, `    <color name="ferryx_shell">${SHELL_BACKGROUND}</color>\n</resources>`, "colors resources");
}

// Back navigation: Tauri's AppPlugin registers an OnBackPressedCallback that calls
// WebView.goBack() while history exists (works with predictive back at targetSdk 36),
// so the UI only needs to push history entries for in-app screens.
export const MAIN_ACTIVITY = `package com.ferryx.mobile

import android.os.Bundle
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge(SystemBarStyle.dark(0xFF0A0A0A.toInt()), SystemBarStyle.dark(0xFF0A0A0A.toInt()))
    super.onCreate(savedInstanceState)
    val content = findViewById<android.view.View>(android.R.id.content)
    // Inset padding exposes the window background; match the app shell instead of a light strip.
    content.setBackgroundColor(0xFF0A0A0A.toInt())
    // Edge-to-edge keeps the WebView full height under the soft keyboard; pad by the
    // IME/system-bar insets so the visual viewport tracks the keyboard.
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
      view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, ime.bottom))
      WindowInsetsCompat.CONSUMED
    }
  }
}
`;

export function patchBuildTask(source, cli) {
  return replaceRequired(
    source,
    /listOf\("tauri", "android", "android-studio-script"\)/,
    `listOf("${cli.replaceAll("\\", "/")}", "android", "android-studio-script")`,
    "BuildTask tauri command",
  );
}
