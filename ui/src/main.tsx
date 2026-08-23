import React from "react";
import ReactDOM from "react-dom/client";
import { installSettingsRuntimeBridge } from "./lib/settingsRuntimeBridge";
import { applyCachedTerminalBackground } from "./lib/terminalSettings";
import "./index.css";
import "./settings-runtime.css";

installSettingsRuntimeBridge();
applyCachedTerminalBackground();

const isTauriApp = typeof window !== "undefined" && Boolean((window as any).__TAURI_INTERNALS__);

async function boot() {
  const el = document.getElementById("root");
  if (!el) return;

  if (isTauriApp) {
    const { default: App } = await import("./App");
    ReactDOM.createRoot(el).render(
      <React.StrictMode>
        <App />
      </React.StrictMode>,
    );
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
