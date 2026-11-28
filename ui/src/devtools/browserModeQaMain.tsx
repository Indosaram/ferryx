// Browser-rendered harness for Browser mode: the real Maho drawer and canvas from
// `useMahoBrowserShell`, over mocked IPC. A native webview cannot exist in a plain browser,
// so the page area shows the app background where the engine would draw.
// URL params: `?state=empty|page`, `?theme=light|dark`, `?open=command`.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import React from "react";
import ReactDOM from "react-dom/client";

import "../index.css";
import { BROWSER_MODE_SHORTCUT_EVENT, useMahoBrowserShell } from "../components/browser/maho/MahoBrowserShell";
import type { BrowserAgentActivity } from "../components/browser/maho/MahoAgentControl";
import { APPEARANCE_SETTINGS_STORAGE_KEY } from "../lib/storageKeys";
import { browserStore } from "../state/browserStore";

const params = new URLSearchParams(window.location.search);
const theme = params.get("theme") === "light" ? "light" : "dark";
document.documentElement.dataset.theme = theme;
window.localStorage.setItem(APPEARANCE_SETTINGS_STORAGE_KEY, JSON.stringify({ theme }));

mockWindows("main");
mockIPC((command) => {
  if (command === "cmd_browser_download_list" || command === "cmd_browser_list") return [];
  return null;
});

if (params.get("state") !== "empty") {
  const spaceId = browserStore.getState().activeSpaceId;
  const seed = [
    { browserId: "gh", url: "https://github.com/Indosaram/ferryx", title: "Indosaram/ferryx" },
    { browserId: "docs", url: "https://www.tauri.app/start/", title: "Tauri 2" },
    { browserId: "mdn", url: "https://developer.mozilla.org/en-US/", title: "MDN Web Docs" },
  ];
  for (const tab of seed) browserStore.upsertTab({ ...tab, spaceId, section: "today" });
  browserStore.touch("gh");
}

const activity: BrowserAgentActivity = {
  activeTasks: [],
  pendingApprovals: [],
  controlledBrowserIds: [],
  handoffRequests: [],
  badgeCount: 0,
  errorCode: null,
  busy: false,
  controllers: [],
  controllerLabelFor: () => null,
  controllerIdFor: () => null,
  takeOver: async () => undefined,
  doneHandoff: async () => undefined,
  revokeController: async () => undefined,
  resolveApproval: async () => undefined,
};

function Harness() {
  const shell = useMahoBrowserShell({ active: true, drawerOpen: true, activity });
  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <div className="w-12 shrink-0 border-r border-border bg-sidebar" data-testid="harness-activity-rail" />
      {shell.drawer}
      {shell.canvas}
      {shell.overlays}
    </div>
  );
}

if (params.get("open") === "command") {
  window.setTimeout(() => {
    window.dispatchEvent(new CustomEvent(BROWSER_MODE_SHORTCUT_EVENT, { detail: { action: "command-bar" } }));
  }, 300);
}

const el = document.getElementById("root");
if (el) {
  ReactDOM.createRoot(el).render(
    <React.StrictMode>
      <Harness />
    </React.StrictMode>,
  );
}
