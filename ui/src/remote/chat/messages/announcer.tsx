import React from "react";
import { cn } from "../../../lib/cn";

/**
 * Screen reader announcements (t3code ThreadActivityAnnouncer, activity-log.md
 * "Screen reader announcements"). status-only says a turn started, needs input,
 * and ended; status-and-messages also reads each completed in-between message
 * (cut short when very long); everything adds the step the agent is on. The
 * reasoning text is never read, and the turn's final reply is not read so the
 * reader can read it after hearing Done.
 */

export type AnnounceLevel = "status-only" | "status-and-messages" | "everything";

export const ANNOUNCE_LEVELS: readonly AnnounceLevel[] = [
  "status-only",
  "status-and-messages",
  "everything",
];

export function isAnnounceLevel(value: unknown): value is AnnounceLevel {
  return typeof value === "string" && (ANNOUNCE_LEVELS as readonly string[]).includes(value);
}

/** A message longer than this is cut short, as the reference cuts a long message. */
export const ANNOUNCE_MESSAGE_MAX_CHARS = 400;

export function truncateAnnouncement(text: string, max = ANNOUNCE_MESSAGE_MAX_CHARS): string {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

export type AnnouncementKind = "status" | "message" | "step";

/** What this announcement level reads, and what it skips. */
export function shouldAnnounce(level: AnnounceLevel, kind: AnnouncementKind): boolean {
  if (kind === "status") return true;
  if (kind === "message") return level !== "status-only";
  return level === "everything";
}

/**
 * The text an aria-live region says for one thread event, or null when the
 * level does not read it. Empty text never renders a region.
 */
export function announcementText(
  level: AnnounceLevel,
  kind: AnnouncementKind,
  text: string,
): string | null {
  if (!shouldAnnounce(level, kind)) return null;
  const trimmed = text.trim();
  if (trimmed.length === 0) return null;
  return kind === "status" ? trimmed : truncateAnnouncement(trimmed);
}

export const ThreadActivityAnnouncer: React.FC<{
  /** The latest announcement; re-saying the same text is skipped by the reader. */
  announcement: string;
  level: AnnounceLevel;
  className?: string;
}> = ({ announcement, level, className }) => {
  const text = announcementText(level, "status", announcement);
  if (text === null) return null;
  return (
    <div
      data-testid="thread-announcer"
      role="status"
      aria-live="polite"
      className={cn("sr-only", className)}
    >
      {text}
    </div>
  );
};
