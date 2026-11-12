import { mockIPC } from "@tauri-apps/api/mocks";
import ReactDOM from "react-dom/client";

import { SettingsDialog } from "../components/SettingsDialog";
import type { SectionId } from "../components/settings/types";
import { useApplyAppearanceSettings } from "../lib/appearanceSettings";
import { APPEARANCE_SETTINGS_STORAGE_KEY } from "../lib/storageKeys";
import "../index.css";
import "../settings-runtime.css";

// Fixture-only entry: no daemon, relay, account, or terminal is contacted.
// Query params: section (default Appearance), theme, accent, density, width, height.
const params = new URLSearchParams(location.search);
const theme = params.get("theme") ?? "charcoal";
const accent = params.get("accent") ?? "default";
const density = params.get("density") ?? "compact";

localStorage.setItem(
  APPEARANCE_SETTINGS_STORAGE_KEY,
  JSON.stringify({ theme, accentColor: accent, density }),
);

const parsedWidth = Number(params.get("width"));
const parsedHeight = Number(params.get("height"));
if (Number.isFinite(parsedWidth) && parsedWidth > 0) {
  document.documentElement.style.width = String(Math.round(parsedWidth)) + "px";
}
if (Number.isFinite(parsedHeight) && parsedHeight > 0) {
  document.documentElement.style.height = String(Math.round(parsedHeight)) + "px";
}

Object.defineProperty(globalThis, "isTauri", { value: true, configurable: true });

mockIPC((command) => {
  switch (command) {
    case "cmd_terminal_preferences_get":
      return { fontFamily: "JetBrains Mono", source: "ghostty", status: "imported" };
    case "cmd_cli_launcher_status":
      return { installed: true, path: "/usr/local/bin/ferryx", version: "2026.1112.1" };
    case "cmd_remote_gateway_status":
      return { enabled: false, devices: [] };
    case "plugin:event|listen":
      return 1;
    case "plugin:event|unlisten":
      return null;
    default:
      return null;
  }
});

const initialSection = (params.get("section") ?? "appearance") as SectionId;

function Harness() {
  // Same hook App.tsx mounts: without it nothing writes data-theme, so a card click would look inert.
  useApplyAppearanceSettings();
  return (
    <div className="h-screen w-screen bg-background">
      <SettingsDialog open onClose={() => undefined} initialSection={initialSection} />
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<Harness />);
