import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { RemoteApp } from "./remote/RemoteApp";
import "./index.css";

const isTauriApp = typeof window !== "undefined" && Boolean((window as any).__TAURI_INTERNALS__);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isTauriApp ? <App /> : <RemoteApp />}
  </React.StrictMode>
);

if (typeof window !== "undefined" && "serviceWorker" in navigator && !isTauriApp) {
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").catch(() => {});
  });
}
