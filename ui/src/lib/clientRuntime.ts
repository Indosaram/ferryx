/** A native mobile shell still uses remote HTTP/WebSocket transport, never desktop IPC. */
export function isMobileCompanion(): boolean {
  return import.meta.env.VITE_FERRYX_MOBILE === "1";
}

/** The native companion running on iOS/iPadOS, where outbound purchase links are not shown. */
export function isIosCompanion(): boolean {
  return isMobileCompanion() && typeof navigator !== "undefined" && /iPhone|iPad|iPod/.test(navigator.userAgent);
}

export function isDesktopShell(hasTauri: boolean): boolean {
  return hasTauri && !isMobileCompanion();
}
