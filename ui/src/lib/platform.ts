export type HostPlatform = "macos" | "linux" | "windows" | "unknown";

export type NavigatorPlatformSource = {
  userAgentData?: { platform?: string };
  platform?: string;
  userAgent?: string;
};

/** Platform reported by a Node-like runtime (tests, SSR); null in a browser webview. */
export function getRuntimeProcessPlatform(): HostPlatform | null {
  if (typeof process !== "undefined" && process.platform) {
    if (process.platform === "darwin") return "macos";
    if (process.platform === "win32") return "windows";
    if (process.platform === "linux") return "linux";
  }
  return null;
}

/** Classifies a navigator-shaped object; the single webview platform rule for the whole UI. */
export function classifyNavigatorPlatform(nav: NavigatorPlatformSource | undefined | null): HostPlatform {
  if (!nav) return "unknown";
  const combined = `${nav.userAgentData?.platform ?? ""} ${nav.platform ?? ""} ${nav.userAgent ?? ""}`;

  if (/Win/i.test(combined)) return "windows";
  if (/Mac|iPhone|iPad|iPod/i.test(combined)) return "macos";
  if (/Linux|X11|CrOS/i.test(combined)) return "linux";
  return "unknown";
}

export function getHostPlatform(): HostPlatform {
  const runtime = getRuntimeProcessPlatform();
  if (runtime) return runtime;
  if (typeof navigator === "undefined") return "unknown";
  return classifyNavigatorPlatform(navigator as NavigatorPlatformSource);
}

export function isInstalledBrowserCookieImportSupported(): boolean {
  const platform = getHostPlatform();
  return platform === "macos" || platform === "linux";
}

export function isMacHost(): boolean {
  return getHostPlatform() === "macos";
}

export function isWindowsHost(): boolean {
  return getHostPlatform() === "windows";
}
