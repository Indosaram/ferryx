import type { AttentionGroup } from "./supervisorPanel/contract";

/**
 * The attention group a surface shows a session under. The daemon judges it (`attention.group`);
 * this only maps the state of an older peer, or of a pane the daemon has no entry for, which is
 * the one place a state may become a group.
 */
export function groupOf(session: {
  attention?: { group: AttentionGroup } | null;
  state?: string;
  seen?: boolean;
}): AttentionGroup {
  if (session.attention?.group) return session.attention.group;
  switch (session.state) {
    case "working":
      return "working";
    case "blocked":
      return "needsYou";
    case "waiting":
      return session.seen ? "seen" : "needsYou";
    case "done":
      return session.seen ? "seen" : "unread";
    default:
      return "seen";
  }
}
