import React from "react";
import { createRoot } from "react-dom/client";
import { AddProjectDialog } from "./src/components/ProjectDialogs";
import "./src/index.css";

const hosts = [
  { id: "windows", label: "Windows QA", hostname: "windows.example", source: "manual", authMethod: "agent" },
  { id: "linux", label: "Linux QA", hostname: "linux.example", source: "manual", authMethod: "agent" },
];
const params = new URLSearchParams(location.search);
const calls: unknown[] = [];
(window as unknown as { isTauri: boolean }).isTauri = true;
Object.assign(window, {
  __TAURI_INTERNALS__: {
    invoke: async (cmd: string, args: { request?: { hostId: string; path?: string; repoPath?: string } }) => {
      calls.push({ cmd, args });
      if (cmd === "cmd_ssh_list_hosts") return hosts;
      if (cmd === "cmd_ssh_list_directories") {
        const home = args.request?.hostId === "windows" ? "C:\\Users\\developer" : "/home/developer";
        const separator = home.startsWith("C:") ? "\\" : "/";
        const path = (args.request?.path ?? home).replace(/^~/, home).replace(/\\/g, "/").replace(/\/$/, "");
        const homeSlash = home.replace(/\\/g, "/");
        if (path.endsWith("/denied")) throw { code: "IO_ERROR", message: "Permission denied" };
        if (path.endsWith("/loading")) return new Promise(() => {});
        const names = path === homeSlash ? ["code", "Documents", "denied", "empty", "loading", ".config"]
          : path.endsWith("/code") ? ["ferryx", "frontend", "folder-with-a-very-long-name-for-path-width-verification"]
          : [];
        const canonical = separator === "\\" ? path.replace(/\//g, "\\") : path;
        return {
          path: canonical, homePath: home, parentPath: home, truncated: params.has("truncated"),
          entries: names.map((name) => ({ name, path: `${canonical}${separator}${name}`, hidden: name.startsWith(".") })),
        };
      }
      if (cmd === "cmd_project_register_remote") {
        return { hostId: args.request?.hostId, workspaceId: "ssh:qa", repoRoot: args.request?.repoPath, gitRoot: null };
      }
      throw new Error(`Unexpected QA IPC: ${cmd}`);
    },
  },
  qaCalls: calls,
});
const root = document.getElementById("root");
if (root) createRoot(root).render(
  <AddProjectDialog initialHostId={params.get("host") ?? "windows"}
    onClose={() => { document.body.dataset.closed = "true"; }}
    onRegistered={(project) => { document.body.dataset.registered = JSON.stringify(project); }} />,
);
