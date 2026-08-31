import React from "react";
import ReactDOM from "react-dom/client";
import { installSettingsRuntimeBridge } from "./lib/settingsRuntimeBridge";
import { isMacShortcutPlatform } from "./lib/shortcuts";
import { bootTrace } from "./lib/tauri";
import { applyCachedTerminalBackground } from "./lib/terminalSettings";
import "./index.css";
import "./settings-runtime.css";

document.documentElement.classList.toggle("platform-macos", isMacShortcutPlatform());
installSettingsRuntimeBridge();
applyCachedTerminalBackground();

const isTauriApp = typeof window !== "undefined" && Boolean((window as any).__TAURI_INTERNALS__);
void bootTrace(isTauriApp ? "tauri.detected" : "tauri.absent", { href: location.href });
window.addEventListener("error", (event) => {
  void bootTrace("window.error", {
    message: event.message,
    filename: event.filename?.split("/").pop(),
    line: event.lineno,
  });
});
window.addEventListener("unhandledrejection", (event) => {
  void bootTrace("unhandled.rejection", { reason: String(event.reason).slice(0, 200) });
});

async function boot() {
  const el = document.getElementById("root");
  if (!el) return;

  if (isTauriApp) {
    void bootTrace("app.import.start");
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

void boot();

if (typeof window !== "undefined" && "serviceWorker" in navigator && !isTauriApp) {
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").catch(() => {});
  });
}
