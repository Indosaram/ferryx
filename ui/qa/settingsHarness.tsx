import React from "react";
import ReactDOM from "react-dom/client";
import { SettingsDialog } from "../src/components/SettingsDialog";
import type { RegisteredProject } from "../src/lib/tauri";
import type { Worktree } from "../src/lib/types";
import { installSettingsRuntimeBridge } from "../src/lib/settingsRuntimeBridge";
import { applyCachedTerminalBackground } from "../src/lib/terminalSettings";
import "../src/index.css";
import "../src/settings-runtime.css";

// Setup Tauri internals and invoke stubs
let callbackId = 1;
const callbacks = new Map<number, (res: unknown) => void>();

const mockInvoke = async (cmd: string, args?: Record<string, unknown>): Promise<unknown> => {
  // console.debug("[Tauri Mock Invoke]", cmd, args);
  switch (cmd) {
    case "cmd_terminal_preferences":
    case "cmd_terminal_apply_overrides":
      return {
        fontFamily: 'MesloLGS NF, "Noto Sans KR", monospace',
        fontSize: 13,
        macosOptionAsAlt: false,
        cursorStyle: "block",
        theme: {
          background: "#1e1e1e",
          foreground: "#cccccc",
          cursor: "#ffffff",
          cursorAccent: "#000000",
          selectionBackground: "#333333",
          black: "#000000",
          red: "#cd3131",
          green: "#0dbc79",
          yellow: "#e5e510",
          blue: "#2472c8",
          magenta: "#bc3fbc",
          cyan: "#11a8cd",
          white: "#e5e5e5",
          brightBlack: "#666666",
          brightRed: "#f14c4c",
          brightGreen: "#23d18b",
          brightYellow: "#f5f543",
          brightBlue: "#3b8eea",
          brightMagenta: "#d670d6",
          brightCyan: "#29b8db",
          brightWhite: "#e5e5e5",
        },
        source: "ghostty",
        status: "imported",
        sourcePath: "/Users/indo/.config/ghostty/config",
      };

    case "cmd_project_branches":
      return [
        { name: "main", isCurrent: true },
        { name: "feature/settings-dialog-redesign", isCurrent: false },
        { name: "fix/terminal-cursor-blink", isCurrent: false },
      ];

    case "cmd_worktree_list":
      return [
        {
          path: "/Users/indo/code/project/orca-lite",
          head: "01a04595",
          branch: "refs/heads/orca/orca-lite/main",
          bare: false,
          detached: false,
          locked: null,
          prunable: null,
        },
      ];

    case "cmd_worktree_status":
      return { isDirty: false, files: [] };

    case "cmd_worktree_delete_preview":
      return {
        branch: "feature/settings-dialog-redesign",
        head: "01a04595",
        upstream: null,
        merged: true,
        ahead: 0,
        behind: 0,
      };

    case "cmd_agents_detect": {
      const candidates = (args?.names as string[]) || [
        "claude",
        "codegpt",
        "aider",
        "cline",
        "continue",
        "goose",
        "copilot",
      ];
      return candidates.map((name) => ({
        name,
        available: name === "claude" || name === "codegpt" || name === "aider",
        path: `/usr/local/bin/${name}`,
        version: "1.0.0",
      }));
    }

    case "cmd_browser_list":
      return [];

    case "cmd_browser_set_zoom":
      return args?.zoomFactor ?? 1;

    case "cmd_browser_focus":
      return null;

    case "cmd_cli_launcher_status":
      return {
        installed: true,
        path: "/usr/local/bin/orca",
        isSymlink: true,
      };

    case "cmd_cli_launcher_install":
      return {
        installed: true,
        path: "/usr/local/bin/orca",
        isSymlink: true,
      };

    case "cmd_remote_status":
      return {
        enabled: true,
        port: 43821,
        localIp: "192.168.1.100",
        tailscale: {
          running: true,
          selfDns: "orca-desktop.tailscale.net",
        },
      };

    case "cmd_remote_devices":
      return [
        {
          deviceId: "dev-iphone-1234",
          deviceName: "Indo's iPhone",
          deviceType: "ios",
          connectedAt: Date.now() - 3600000,
          lastSeenAt: Date.now() - 60000,
          ip: "100.64.0.2",
        },
      ];

    case "cmd_tailscale_status":
      return {
        running: true,
        selfDns: "orca-desktop.tailscale.net",
        ip: "100.64.0.1",
      };

    case "cmd_remote_pairing_create":
      return {
        code: "948-201",
        expiresAt: Math.floor(Date.now() / 1000) + 300,
      };

    case "cmd_notification_get_permission_status":
      return {
        authorization: "authorized",
        sound: true,
        badge: true,
        alert: true,
      };

    case "cmd_notification_request_permission":
      return {
        authorization: "authorized",
        sound: true,
        badge: true,
        alert: true,
      };

    case "cmd_notification_probe_delivery":
      return { success: true };

    case "cmd_notification_play_sound":
      return { success: true };

    case "cmd_notification_pick_audio":
      return null;

    case "cmd_notification_open_system_settings":
      return { success: true };

    case "cmd_notification_set_badge_count":
      return { count: 0 };

    case "plugin:app|version":
      return "0.1.0";

    case "plugin:app|name":
      return "Ferryx";

    case "plugin:app|tauri_version":
      return "2.0.0";

    case "plugin:updater|check":
      return null;

    case "plugin:dialog|open":
      return null;

    case "plugin:dialog|save":
      return null;

    case "plugin:dialog|message":
      return true;

    case "plugin:event|listen":
      return Math.floor(Math.random() * 100000);

    case "plugin:event|unlisten":
    case "plugin:event|emit":
    case "plugin:event|emit_to":
      return undefined;

    default:
      return {};
  }
};

(window as unknown as { isTauri: boolean }).isTauri = true;
(window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {
  invoke: mockInvoke,
  transformCallback: (callback: (res: unknown) => void, once?: boolean) => {
    const id = callbackId++;
    callbacks.set(id, (res: unknown) => {
      if (once) callbacks.delete(id);
      callback(res);
    });
    return id;
  },
  unregisterCallback: (id: number) => {
    callbacks.delete(id);
  },
  convertFileSrc: (filePath: string) => filePath,
};

(window as unknown as { __TAURI_EVENT_PLUGIN_INTERNALS__: Record<string, unknown> }).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
  unregisterListener: () => {},
};

installSettingsRuntimeBridge();
applyCachedTerminalBackground();

const mockProjects: RegisteredProject[] = [
  {
    workspaceId: "orca-lite",
    repoRoot: "/Users/indo/code/project/orca-lite",
    gitRoot: "/Users/indo/code/project/orca-lite",
  },
];

const mockWorktree: Worktree = {
  path: "/Users/indo/code/project/orca-lite",
  head: "01a04595",
  branch: "refs/heads/orca/orca-lite/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function HarnessApp() {
  return (
    <div className="h-screen w-screen overflow-hidden bg-background text-foreground">
      <SettingsDialog
        open={true}
        onClose={() => console.log("Settings onClose invoked")}
        projects={mockProjects}
        activeProjectId="orca-lite"
        activeWorktree={mockWorktree}
        onSelectProject={(project) => console.log("Select project:", project)}
        onAddProject={() => console.log("Add project")}
        onAddWorktree={() => console.log("Add worktree")}
      />
    </div>
  );
}

// Global helper for QA scripts
declare global {
  interface Window {
    __qaClickSection: (label: string) => Promise<void>;
    __qaReady: boolean;
  }
}

window.__qaClickSection = async (label: string): Promise<void> => {
  const nav = document.querySelector('[data-testid="settings-nav"]');
  if (!nav) {
    throw new Error('Nav element [data-testid="settings-nav"] not found in DOM');
  }

  const buttons = Array.from(nav.querySelectorAll("button"));
  const target = buttons.find((btn) => {
    const text = btn.textContent?.trim() || "";
    return text.toLowerCase() === label.toLowerCase() || text.toLowerCase().includes(label.toLowerCase());
  });

  if (!target) {
    const available = buttons.map((b) => b.textContent?.trim()).join(", ");
    throw new Error(`Nav button with label "${label}" not found. Available buttons: [${available}]`);
  }

  target.click();

  // Await two animation frames plus an extra tick to allow React transitions/renders to settle
  await new Promise<void>((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        setTimeout(resolve, 150);
      });
    });
  });
};

const rootEl = document.getElementById("root");
if (rootEl) {
  const root = ReactDOM.createRoot(rootEl);
  root.render(<HarnessApp />);
  window.__qaReady = true;
}
