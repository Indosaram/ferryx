/**
 * A terminal cwd is only usable when it is an absolute path for the platform. Shell probes that
 * capture command output instead of a directory used to plant values like
 * `cwd|rtd info error: No such file or directory` on sessions; such a value must never be
 * inherited by another pane's spawn, served from cache, or restored from persistence.
 */
export function isAbsoluteTerminalCwd(value: string | null | undefined): value is string {
  if (typeof value !== "string" || value.length === 0) return false;
  if (/[\u0000-\u001f\u007f]/.test(value)) return false;
  if (value.startsWith("/")) return true;
  if (value.startsWith("\\\\")) return true;
  return /^[A-Za-z]:[\\/]/.test(value);
}
