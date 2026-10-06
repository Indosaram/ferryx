import { useEffect } from "react";
import { createRoot } from "react-dom/client";

import { AddWorktreeDialog } from "../components/ProjectDialogs";
import "../index.css";

const preview = {
  number: 12,
  title: "Native pane drag leaves a stale overlay behind",
  url: "https://github.com/acme/widgets/issues/12",
  body:
    "Steps to reproduce:\n" +
    "1. Drag a pane handle across two splits.\n" +
    "2. Release over the drop zone.\n\n" +
    "The overlay stays painted until the next resize.",
  bodyTruncated: false,
  repository: "acme/widgets",
  suggestedSlug: "issue-12-native-pane-drag-leaves",
};

let callbackId = 0;
const scenario = new URLSearchParams(window.location.search).get("scenario") ?? "preview";

(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
  invoke: async (cmd: string) => {
    if (cmd === "cmd_project_branches") {
      return [
        { name: "develop", isCurrent: false },
        { name: "main", isCurrent: true },
      ];
    }
    if (cmd === "cmd_github_issue_preview") {
      if (scenario === "intake-error") {
        throw {
          code: "CLI_EXECUTABLE_NOT_FOUND",
          message: "The GitHub CLI (gh) was not found on PATH. Install it and run \`gh auth login\`.",
        };
      }
      return preview;
    }
    if (cmd === "cmd_ssh_list_hosts") return [];
    if (cmd === "plugin:event|listen") return 1;
    return null;
  },
  transformCallback: () => (callbackId += 1),
  unregisterCallback: () => {},
  convertFileSrc: (path: string) => path,
};

if (scenario === "clipboard-failure") {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: () => Promise.reject(new Error("denied")) },
  });
  Object.defineProperty(document, "execCommand", { configurable: true, value: () => false });
}

function Harness() {
  useEffect(() => {
    const input = document.querySelector<HTMLInputElement>('[aria-label="GitHub issue"]');
    if (!input) return;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    setter?.call(input, "12");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    window.setTimeout(() => {
      const click = (label: string) =>
        Array.from(document.querySelectorAll("button"))
          .find((button) => button.textContent?.includes(label))
          ?.click();
      click("Load issue");
      if (scenario === "clipboard-failure") window.setTimeout(() => click("Copy issue context"), 200);
    }, 60);
  }, []);
  return (
    <AddWorktreeDialog
      project={{ workspaceId: "ferryx", repoRoot: "/repo/ferryx", gitRoot: "/repo/ferryx" }}
      onClose={() => {}}
      onCreated={() => {}}
    />
  );
}

document.documentElement.classList.add("dark");
createRoot(document.getElementById("root") as HTMLElement).render(<Harness />);
