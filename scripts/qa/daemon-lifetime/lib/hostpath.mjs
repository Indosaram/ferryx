// Host-path joins for paths that live on the target host, not on the controller.
// The controller runs on macOS, so a bare path.join would build posix paths for a Windows host.
import path from 'node:path';

/** True when p is a Windows absolute path (drive letter or UNC). */
export function isWindowsPath(p) {
  return /^[A-Za-z]:[\\/]/.test(p) || p.startsWith('\\\\');
}

/** Joins with the flavour of the base path: win32 for a Windows base, posix otherwise. */
export function hjoin(base, ...parts) {
  return isWindowsPath(base) ? path.win32.join(base, ...parts) : path.posix.join(base, ...parts);
}
