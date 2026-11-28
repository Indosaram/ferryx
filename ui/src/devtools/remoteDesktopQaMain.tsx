import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import React, { useState } from "react";
import ReactDOM from "react-dom/client";

import "../index.css";
import { applyAppearanceSettings, loadAppearanceSettings } from "../lib/appearanceSettings";
import { APPEARANCE_SETTINGS_STORAGE_KEY } from "../lib/storageKeys";
import { GlobalRemoteDesktopCanvas } from "../components/remoteDesktop/GlobalRemoteDesktopCanvas";
import { RemoteDesktopActivitySidebar } from "../components/remoteDesktop/RemoteDesktopActivitySidebar";
import { remoteDesktopStore } from "../state/remoteDesktopStore";
import { remoteHostStore } from "../state/remoteHostStore";

const params = new URLSearchParams(window.location.search);
const theme = params.get("theme") === "light" ? "light" : "dark";
document.documentElement.dataset.theme = theme;
window.localStorage.setItem(APPEARANCE_SETTINGS_STORAGE_KEY, JSON.stringify({ theme }));
applyAppearanceSettings(loadAppearanceSettings());

mockWindows("main");
mockIPC((command) => {
  if (command === "cmd_rd_is_supported") return true;
  if (command === "cmd_rd_host_status") {
    return {
      running: true,
      enabled: true,
      listeningAddr: "127.0.0.1:44411",
      activeClients: 0,
      pin: "482913",
    };
  }
  if (command === "cmd_rd_list_trusted_clients") return [];
  if (command === "cmd_rd_list_audio_devices") return [];
  if (command === "cmd_rd_connect") return { rdSessionId: "rd-qa-1", supportsStreamOptions: true };
  return null;
});

remoteHostStore.setHosts([
  {
    hostId: "studio-mac",
    machineId: "studio-mac",
    name: "Studio Mac",
    displayName: "Studio Mac",
    address: "100.65.239.35",
    transport: "tailscale",
    authStatus: "paired",
    online: true,
    grantScope: "machine",
    generation: "1",
    latencyMs: 8,
  },
  {
    hostId: "omarchy-work",
    machineId: "omarchy-work",
    name: "Omarchy Linux",
    displayName: "Omarchy Linux",
    address: "https://relay.ferryx.dev",
    transport: "relay",
    authStatus: "paired",
    online: true,
    grantScope: "machine",
    generation: "1",
    latencyMs: 24,
  },
  {
    hostId: "maho-win",
    machineId: "maho-win",
    name: "Windows Box",
    displayName: "Windows Box",
    address: "100.78.73.120",
    transport: "tailscale",
    authStatus: "paired",
    online: false,
    grantScope: "machine",
    generation: "1",
  },
]);

if (params.get("state") === "session") {
  void remoteDesktopStore.connect("studio-mac", "Studio Mac");
}

function Harness() {
  const [activeHostId, setActiveHostId] = useState<string | null>(
    params.get("state") === "session" ? "studio-mac" : null,
  );
  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <div className="w-12 shrink-0 border-r border-border bg-sidebar" data-testid="harness-activity-rail" />
      <RemoteDesktopActivitySidebar activeHostId={activeHostId} onSelectHost={(id) => setActiveHostId(id ?? null)} />
      <GlobalRemoteDesktopCanvas
        active
        activeHostId={activeHostId}
        onSelectHost={(id) => setActiveHostId(id ?? null)}
      />
    </div>
  );
}

const el = document.getElementById("root");
if (el) {
  ReactDOM.createRoot(el).render(
    <React.StrictMode>
      <Harness />
    </React.StrictMode>,
  );
}
