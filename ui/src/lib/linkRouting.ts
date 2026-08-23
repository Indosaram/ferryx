import { isHttpUrl, loadBrowserSettings } from "./browserSettings";
import { openExternalUrl } from "./browserTauri";

export const TERMINAL_LINK_ACTION_EVENT = "ferryx:terminal-link-actions";

export type LinkRoutingSource = "app" | "terminal" | "browser-popup" | "markdown" | "editor";
export type LinkDestination = "builtin" | "external";

export type LinkRoutingOptions = {
  shiftKey?: boolean;
  source?: LinkRoutingSource;
  destination?: LinkDestination;
};

export type TerminalLinkActionRequest = {
  url: string;
};

type BuiltInBrowserOpener = (url: string) => void | Promise<void>;
let builtInBrowserOpener: BuiltInBrowserOpener | null = null;

export function registerBuiltInBrowserLinkOpener(opener: BuiltInBrowserOpener): () => void {
  builtInBrowserOpener = opener;
  return () => {
    if (builtInBrowserOpener === opener) builtInBrowserOpener = null;
  };
}

export async function routeHttpLink(url: string, options: LinkRoutingOptions = {}): Promise<LinkDestination> {
  const normalized = url.trim();
  if (!isHttpUrl(normalized)) throw new Error("Only http(s) links can be opened by the browser router.");

  const settings = loadBrowserSettings();
  const destination = options.destination
    ?? (options.shiftKey && settings.shiftOpensSystemBrowser
      ? "external"
      : settings.openLinksInBuiltInBrowser && builtInBrowserOpener
        ? "builtin"
        : "external");

  if (destination === "builtin") {
    if (!builtInBrowserOpener) {
      await openExternalUrl(normalized);
      return "external";
    }
    await builtInBrowserOpener(normalized);
    return "builtin";
  }

  await openExternalUrl(normalized);
  return "external";
}

export async function requestTerminalLinkOpen(url: string, shiftKey = false): Promise<"chooser" | LinkDestination> {
  const settings = loadBrowserSettings();
  if (shiftKey && settings.shiftOpensSystemBrowser) {
    return routeHttpLink(url, { shiftKey: true, source: "terminal" });
  }
  if (!settings.showTerminalLinkActions) {
    return routeHttpLink(url, { source: "terminal" });
  }
  if (typeof window === "undefined") {
    return routeHttpLink(url, { source: "terminal" });
  }
  window.dispatchEvent(new CustomEvent<TerminalLinkActionRequest>(TERMINAL_LINK_ACTION_EVENT, {
    detail: { url },
  }));
  return "chooser";
}
