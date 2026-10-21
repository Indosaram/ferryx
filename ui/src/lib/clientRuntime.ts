/** A native mobile shell still uses remote HTTP/WebSocket transport, never desktop IPC. */
export function isMobileCompanion(): boolean {
  return import.meta.env.VITE_FERRYX_MOBILE === "1";
}

export function isDesktopShell(hasTauri: boolean): boolean {
  return hasTauri && !isMobileCompanion();
}
