import type { ReferencePart } from "../referenceTypes";
import type { ToolCallVisibility } from "../../../lib/paneChatPreferences";

/**
 * Tool calls in chat (t3code, docs/user/activity-log.md "Hide tool calls"):
 * `all` shows everything; `hide-successful` keeps a call while it runs and only if
 * it fails; `hide-all` hides every call; `zen` additionally hides the agent's
 * in-between messages so a turn shows the user's message and the agent's final
 * reply. Approvals, questions, and errors always show.
 *
 * The mode and its stored value are one thing: the union and the guard are defined
 * by `lib/paneChatPreferences` and re-exported here, so the persisted setting and
 * the rendered filter can never drift apart.
 */
export { TOOL_CALL_VISIBILITIES, isToolCallVisibility } from "../../../lib/paneChatPreferences";
/** Re-exported from the binding this module imports, so the union has one source. */
export type { ToolCallVisibility };

/** A work part a visibility mode may hide: calls and their diffs and results. */
function isHideableWork(part: ReferencePart): boolean {
  return (
    part.kind === "tool" || part.kind === "diff" || part.kind === "taskResult"
  );
}

/** Errors always show, in every mode, exactly as the reference promises. */
function partFailed(part: ReferencePart): boolean {
  if (part.kind === "tool") {
    return Boolean(part.error) || part.status === "failed";
  }
  if (part.kind === "taskResult") {
    return part.tasks.some((task) => task.status === "failed");
  }
  return false;
}

function partStillRunning(part: ReferencePart): boolean {
  if (part.kind === "tool") return part.status === "running";
  if (part.kind === "taskResult") return part.tasks.some((task) => task.status === "running");
  return false;
}

/**
 * The work parts a mode keeps. Thinking survives `hide-all` (the reference shows
 * Thinking while it hides every call); nothing but zen drops thinking.
 */
export function filterPartsForVisibility(
  parts: readonly ReferencePart[],
  mode: ToolCallVisibility,
): ReferencePart[] {
  if (mode === "all") return [...parts];
  return parts.filter((part) => {
    if (part.kind === "thinking") return true;
    if (!isHideableWork(part)) return true;
    if (partFailed(part)) return true;
    // hide-successful keeps a call while it runs; hide-all drops it even mid-run,
    // so after the error check only hide-successful's running calls survive.
    if (mode === "hide-successful" && partStillRunning(part)) return true;
    return false;
  });
}

/**
 * Zen turns. `true` hides the turn body (its prose and work); the caller still
 * draws approvals, questions, and errors. The turn's final reply, and every user
 * message, always show. Find overrides zen entirely.
 */
export function zenHidesTurn(
  role: "user" | "assistant" | "system",
  isFinalReply: boolean,
  findActive: boolean,
): boolean {
  if (findActive) return false;
  if (role !== "assistant") return false;
  return !isFinalReply;
}

/**
 * The parts zen keeps on an in-between reply: only errors (approvals, questions,
 * and errors always show). A running call is not an error; a diff has no error
 * flag and never survives zen on its own.
 */
export function zenErrorParts(parts: readonly ReferencePart[]): ReferencePart[] {
  return parts.filter((part) => {
    if (part.kind === "tool") return Boolean(part.error) || part.status === "failed";
    if (part.kind === "taskResult") {
      return part.tasks.some((task) => task.status === "failed");
    }
    return false;
  });
}
