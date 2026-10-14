import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Copy, ExternalLink, FileText, FolderOpen, Globe, X } from "lucide-react";

import { copyTextToClipboard } from "../lib/clipboard";
import { extractIpcErrorMessage } from "../lib/sshHosts";
import { revealPath } from "../lib/tauri";
import {
  TERMINAL_FILE_LINK_ACTION_EVENT,
  type TerminalLinkOpenOverrides,
} from "../lib/terminalLinkTarget";
import { toast } from "./ui/sonner";

export type TerminalFileLinkToken =
  | { type: "file"; path: string; absolutePath?: string; line?: number; col?: number }
  | { type: "url"; target: string };

export interface TerminalFileLinkActionDetail {
  token: TerminalFileLinkToken;
  open: (shiftKey: boolean, overrides?: TerminalLinkOpenOverrides) => Promise<void>;
  /** The file lives on an SSH/paired host: hide actions that act on the local disk. */
  remote?: boolean;
}

export function TerminalFileLinkActions() {
  useEffect(() => {
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<TerminalFileLinkActionDetail>).detail;
      if (!detail?.token) return;

      const token = detail.token;
      const isFile = token.type === "file";
      const isUrl = token.type === "url";
      const isRemoteFile = isFile && detail.remote === true;

      if (!isFile && !isUrl) return;

      if (isFile && !token.path) return;
      if (isUrl && !token.target) return;

      const displayTarget = isFile
        ? token.line != null
          ? `${token.path}:${token.line}${token.col != null ? `:${token.col}` : ""}`
          : token.path
        : token.target;

      toast.custom(
        (t) => {
          const handleOpen = async (shiftKey: boolean, overrides?: TerminalLinkOpenOverrides) => {
            toast.dismiss(t);
            try {
              await (overrides ? detail.open(shiftKey, overrides) : detail.open(shiftKey));
            } catch (error) {
              toast.error(extractIpcErrorMessage(error, "Could not open terminal link."));
            }
          };

          // "Open with default app" must bypass the configured file-link editor: detail.open
          // forwards that preference, so a resolved local path goes straight to the system opener.
          const handleOpenWithDefaultApp = async () => {
            if (!isFile || !token.absolutePath) {
              await handleOpen(true, { editor: "system" });
              return;
            }
            toast.dismiss(t);
            try {
              await invoke<boolean>("cmd_open_file_path", {
                path: token.absolutePath,
                editor: "system",
              });
            } catch (error) {
              toast.error(extractIpcErrorMessage(error, "Could not open terminal link."));
            }
          };

          const handleCopy = async (text: string) => {
            toast.dismiss(t);
            try {
              const ok = await copyTextToClipboard(text);
              if (!ok) {
                toast.error("Could not copy to clipboard.");
                return;
              }
              toast.success(isFile ? "Path copied" : "Link copied");
            } catch {
              toast.error("Could not copy to clipboard.");
            }
          };

          const handleReveal = async (path: string) => {
            toast.dismiss(t);
            try {
              await revealPath(path);
            } catch (error) {
              toast.error(extractIpcErrorMessage(error, "Could not reveal the file."));
            }
          };

          return (
            <div
              role="dialog"
              aria-label="Terminal link actions"
              tabIndex={-1}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  event.stopPropagation();
                  toast.dismiss(t);
                }
              }}
              className="w-[min(26rem,calc(100vw-2rem))] rounded-xl border border-border bg-popover p-3 text-popover-foreground shadow-2xl"
            >
              <div className="flex items-start gap-3">
                {isFile ? (
                  <FileText className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
                ) : (
                  <Globe className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
                )}
                <div className="min-w-0 flex-1">
                  <div className="text-xs font-semibold">
                    {isFile ? "Open terminal file" : "Open terminal link"}
                  </div>
                  <div
                    className="mt-1 truncate font-mono text-[10px] text-muted-foreground"
                    title={displayTarget}
                  >
                    {displayTarget}
                  </div>
                </div>
                <button
                  type="button"
                  aria-label="Close link actions"
                  onClick={() => toast.dismiss(t)}
                  className="rounded p-1 hover:bg-accent"
                >
                  <X className="size-3.5" />
                </button>
              </div>

              <div
                role="menu"
                aria-label="Link actions"
                className="mt-3 flex flex-wrap justify-end gap-2"
              >
                {isFile ? (
                  <>
                    <button
                      type="button"
                      role="menuitem"
                      aria-label="Copy path"
                      onClick={() => void handleCopy(token.absolutePath ?? token.path)}
                      className="flex h-8 items-center gap-1.5 rounded-md border border-border px-2.5 text-[11px] hover:bg-accent"
                    >
                      <Copy className="size-3.5" />
                      Copy path
                    </button>
                    {isRemoteFile ? null : (
                      <>
                        <button
                          type="button"
                          role="menuitem"
                          aria-label="Reveal in folder"
                          onClick={() => void handleReveal(token.absolutePath ?? token.path)}
                          className="flex h-8 items-center gap-1.5 rounded-md border border-border px-2.5 text-[11px] hover:bg-accent"
                        >
                          <FolderOpen className="size-3.5" />
                          Reveal in folder
                        </button>
                        <button
                          type="button"
                          role="menuitem"
                          aria-label="Open with default app"
                          onClick={() => void handleOpenWithDefaultApp()}
                          className="flex h-8 items-center gap-1.5 rounded-md border border-border px-2.5 text-[11px] hover:bg-accent"
                        >
                          <ExternalLink className="size-3.5" />
                          Open with default app
                        </button>
                      </>
                    )}
                    <button
                      type="button"
                      role="menuitem"
                      aria-label="Open"
                      onClick={() => void handleOpen(false)}
                      className="flex h-8 items-center gap-1.5 rounded-md bg-foreground px-2.5 text-[11px] text-background hover:opacity-90"
                    >
                      Open
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      role="menuitem"
                      aria-label="Copy link"
                      onClick={() => void handleCopy(token.target)}
                      className="flex h-8 items-center gap-1.5 rounded-md border border-border px-2.5 text-[11px] hover:bg-accent"
                    >
                      <Copy className="size-3.5" />
                      Copy link
                    </button>
                    <button
                      type="button"
                      role="menuitem"
                      aria-label="Open in browser"
                      onClick={() => void handleOpen(true)}
                      className="flex h-8 items-center gap-1.5 rounded-md border border-border px-2.5 text-[11px] hover:bg-accent"
                    >
                      <ExternalLink className="size-3.5" />
                      Open in browser
                    </button>
                    <button
                      type="button"
                      role="menuitem"
                      aria-label="Open"
                      onClick={() => void handleOpen(false)}
                      className="flex h-8 items-center gap-1.5 rounded-md bg-foreground px-2.5 text-[11px] text-background hover:opacity-90"
                    >
                      Open
                    </button>
                  </>
                )}
              </div>
            </div>
          );
        },
        { duration: Infinity },
      );
    };

    window.addEventListener(TERMINAL_FILE_LINK_ACTION_EVENT, handler);
    return () => window.removeEventListener(TERMINAL_FILE_LINK_ACTION_EVENT, handler);
  }, []);

  return null;
}
