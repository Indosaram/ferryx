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
  + "\nandroid.useAndroidX=true\norg.gradle.workers.max=1\nkotlin.compiler.execution.strategy=in-process\norg.gradle.daemon=false\n");

// Edge-to-edge keeps the WebView full height under the soft keyboard and system bars,
// so focused inputs end up hidden. Pad the content view by the IME/system-bar insets
// instead; the WebView then shrinks and its visual viewport tracks the keyboard.
patch("app/src/main/java/com/ferryx/mobile/MainActivity.kt", () => `package com.ferryx.mobile

import android.os.Bundle
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge(SystemBarStyle.dark(0xFF23262D.toInt()), SystemBarStyle.dark(0xFF23262D.toInt()))
    super.onCreate(savedInstanceState)
    val content = findViewById<android.view.View>(android.R.id.content)
    // Inset padding exposes the window background; match the app shell instead of a light strip.
    content.setBackgroundColor(0xFF23262D.toInt())
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
      view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, ime.bottom))
      WindowInsetsCompat.CONSUMED
    }
  }
}
`);

patch("buildSrc/src/main/java/com/ferryx/mobile/kotlin/BuildTask.kt", s => s.replace('listOf("tauri", "android", "android-studio-script")', `listOf("${cli.replaceAll("\\", "/")}", "android", "android-studio-script")`));
