import { isDesktopShell, isMobileCompanion } from "./lib/clientRuntime";
import "./lib/uuid";
import React from "react";
import ReactDOM from "react-dom/client";
import { BUILD_STAMP } from "./lib/buildStamp";
import { SERVICE_WORKER_PATH } from "./lib/pushSubscription";
import { installSettingsRuntimeBridge } from "./lib/settingsRuntimeBridge";
import { isMacShortcutPlatform } from "./lib/shortcuts";
import { installShortcutDiagnostics } from "./lib/shortcutDiagnostics";
import { isSwitchDebugEnabled, switchDebug } from "./lib/switchDebug";
import { bootTrace, describeRejection } from "./lib/tauri";
import { applyCachedTerminalBackground } from "./lib/terminalSettings";
import "./index.css";
import "./settings-runtime.css";

document.documentElement.classList.toggle("platform-macos", isMacShortcutPlatform());
installSettingsRuntimeBridge();
applyCachedTerminalBackground();

if (import.meta.env.DEV && import.meta.env.VITE_DISABLE_REACT_DEVTOOLS !== "1") {
  void import("react-grab");
  void import("react-scan");
}

const isTauriApp = isDesktopShell(typeof window !== "undefined" && Boolean((window as any).__TAURI_INTERNALS__));
// Build identity is exposed for support: a stale phone bundle is then visible on the device itself.
document.documentElement.dataset.ferryxBuild = BUILD_STAMP;
console.info(`Ferryx ${isTauriApp ? "desktop" : "remote client"} build ${BUILD_STAMP}`);
// TEMPORARY-DIAGNOSTIC: install before App import, without changing routing.
// Only when switch-debug tracing is on (dev build or VITE_SWITCH_DEBUG=1), never in a plain release build.
if (isTauriApp && isSwitchDebugEnabled) {
  const stop = installShortcutDiagnostics();
  import.meta.hot?.dispose(stop);
}
void bootTrace(isTauriApp ? "tauri.detected" : "tauri.absent", { href: location.href });
const onWindowError = (event: ErrorEvent) => {
  switchDebug("shortcut.webview.error", { errorType: event.error instanceof Error ? event.error.name : typeof event.error });
  void bootTrace("window.error", {
    message: event.message,
    filename: event.filename?.split("/").pop(),
    line: event.lineno,
  });
};
const onUnhandledRejection = (event: PromiseRejectionEvent) => {
  switchDebug("shortcut.webview.unhandledrejection", { errorType: event.reason instanceof Error ? event.reason.name : typeof event.reason });
  void bootTrace("unhandled.rejection", { reason: describeRejection(event.reason).slice(0, 200) });
};
window.addEventListener("error", onWindowError);
window.addEventListener("unhandledrejection", onUnhandledRejection);
import.meta.hot?.dispose(() => {
  window.removeEventListener("error", onWindowError);
  window.removeEventListener("unhandledrejection", onUnhandledRejection);
});

async function boot() {
  const el = document.getElementById("root");
  if (!el) return;

  if (isTauriApp) {
    void bootTrace("app.import.start");
    // Resolve the new-vs-existing install default before App can persist any workspace state.
    const { loadAdvancedFeatures } = await import("./lib/advancedFeatures");
    loadAdvancedFeatures();
    const { installNativeShortcutTablePush } = await import("./lib/nativeShortcutTable");
    const stopNativeShortcutTablePush = installNativeShortcutTablePush();
    import.meta.hot?.dispose(stopNativeShortcutTablePush);
    const { bootstrapPairedHostInventory } = await import("./lib/pairedHostInventory");
    void bootstrapPairedHostInventory();
    const { bootstrapPairedHostReachability } = await import("./lib/pairedHostReachability");
    void bootstrapPairedHostReachability();
    const { bootstrapPairedHostRoute } = await import("./lib/pairedHostRoute");
    void bootstrapPairedHostRoute();
    const { startTailscaleDiscovery } = await import("./lib/tailscaleDiscovery");
    const stopDiscovery = startTailscaleDiscovery();
    import.meta.hot?.dispose(stopDiscovery);
    const { default: App } = await import("./App");
    void bootTrace("app.imported");
    ReactDOM.createRoot(el).render(
      <React.StrictMode>
        <App />
      </React.StrictMode>,
    );
    void bootTrace("render.done");
    return;
  }

  const { RemoteApp } = await import("./remote/RemoteApp");
  ReactDOM.createRoot(el).render(
    <React.StrictMode>
      <RemoteApp />
    </React.StrictMode>,
  );
}

boot().catch((error: unknown) => {
  console.error("Ferryx boot failed", error);
  void bootTrace("boot.failed", { reason: describeRejection(error).slice(0, 200) });
  // Plain DOM on purpose: React or the app bundle may be what failed to load. Tokens may be
  // missing too, so the screen leans on the inline document colours set in index.html.
  const el = document.getElementById("root");
  if (!el) return;
  const screen = document.createElement("main");
  screen.setAttribute("role", "alert");
  screen.style.cssText =
    "min-height:100dvh;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:1rem;" +
    "padding:max(env(safe-area-inset-top),1.5rem) 1.5rem max(env(safe-area-inset-bottom),1.5rem);" +
    "font:1rem/1.5 system-ui,sans-serif;text-align:center";
  const message = document.createElement("p");
  message.textContent = navigator.onLine === false
    ? "You're offline and Ferryx isn't saved on this device yet. Connect and reload."
    : "Ferryx couldn't start. Reload to try again.";
  const reload = document.createElement("button");
  reload.type = "button";
  reload.textContent = "Reload";
  reload.style.cssText =
    "min-height:2.75rem;padding:0 1.25rem;border-radius:0.5rem;border:1px solid currentColor;background:transparent;color:inherit;font:inherit";
  reload.addEventListener("click", () => location.reload());
  const build = document.createElement("p");
  build.textContent = `Build ${BUILD_STAMP}`;
  build.style.cssText = "font-size:0.75rem;opacity:0.7";
  screen.replaceChildren(message, reload, build);
  el.replaceChildren(screen);
});

if (typeof window !== "undefined" && "serviceWorker" in navigator && !isTauriApp && !isMobileCompanion()) {
  const registerServiceWorker = () => {
    navigator.serviceWorker.register(SERVICE_WORKER_PATH).catch(() => {});
  };
  window.addEventListener("load", registerServiceWorker);
  import.meta.hot?.dispose(() => window.removeEventListener("load", registerServiceWorker));
}
