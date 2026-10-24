// The "supervisor typing" row signal, shared by the desktop sidebar and the remote sidebar so the
// two can never disagree on which worktree the hub supervisor is typing into.
import { getHostPlatform } from "../../lib/platform";
import { actingSessionIds, type SupervisorSnapshot } from "../../lib/supervisorModel";

function isCaseInsensitivePlatform(): boolean {
  const platform = getHostPlatform();
  return platform === "macos" || platform === "windows";
}

export function normalizeWorktreePath(p: string, caseInsensitive = isCaseInsensitivePlatform()): string {
  let normalized = p.trim().replace(/\\/g, "/");
  normalized = normalized.replace(/\/+/g, "/");
  if (normalized.length > 1 && normalized.endsWith("/")) {
    normalized = normalized.slice(0, -1);
  }
  if (caseInsensitive) {
    normalized = normalized.toLowerCase();
  }
  return normalized;
}

export function isSameWorktreePath(pathA: string, pathB: string, caseInsensitive?: boolean): boolean {
  return normalizeWorktreePath(pathA, caseInsensitive) === normalizeWorktreePath(pathB, caseInsensitive);
}

/** True when a supervised lane rooted at this worktree was typed into within the acting window. */
export function supervisorTypingIn(snapshot: SupervisorSnapshot, now: number, worktreePath: string): boolean {
  const acting = actingSessionIds(snapshot, now);
  if (acting.size === 0) return false;
  return snapshot.lanes.some((lane) => lane.sessionId !== null && acting.has(lane.sessionId) && isSameWorktreePath(lane.cwd, worktreePath));
}
