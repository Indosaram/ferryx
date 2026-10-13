import { classifyNavigatorPlatform } from "./platform";

export type TerminalLinkHintKind = "file" | "directory" | "url";

export function terminalLinkOpenHint(kind: TerminalLinkHintKind, isMac: boolean): string {
  const mod = isMac ? "Cmd" : "Ctrl";
  switch (kind) {
    case "file":
      return `${mod}+click to open file`;
    case "directory":
      return `${mod}+click to reveal folder`;
    case "url":
      return `${mod}+click to open link`;
  }
}

export function isMacPlatform(nav?: { platform?: string; userAgent?: string }): boolean {
  const effectiveNav =
    nav !== undefined
      ? nav
      : typeof globalThis !== "undefined" && "navigator" in globalThis
        ? globalThis.navigator
        : undefined;

  if (!effectiveNav) {
    return false;
  }

  return classifyNavigatorPlatform(effectiveNav) === "macos";
}

export function isTerminalLinkActionClick(e: {
  button: number;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
  ctrlKey: boolean;
}): boolean {
  return (
    e.button === 0 &&
    !e.altKey &&
    !e.shiftKey &&
    !e.metaKey &&
    !e.ctrlKey
  );
}
