import type { NotificationEntry } from "./types";

export type AttentionSection = "needs-you" | "finished" | "alerts";
export type AttentionFilter = "all" | "unread" | "needs-you";

export interface AttentionGroup {
  section: AttentionSection;
  title: string;
  items: NotificationEntry[];
}

const SECTION_ORDER: AttentionSection[] = ["needs-you", "finished", "alerts"];

const SECTION_TITLES: Record<AttentionSection, string> = {
  "needs-you": "Needs you",
  finished: "Finished",
  alerts: "Mentions",
};

export function isUnread(entry: NotificationEntry): boolean {
  return "unread" in entry.read;
}

export function attentionSectionOf(entry: NotificationEntry): AttentionSection {
  if (entry.reason === "waiting") return "needs-you";
  if (entry.reason === "done") return "finished";
  return "alerts";
}

/** A permanently lit badge stops carrying signal, so only blocked-on-you work counts. */
export function isActionable(entry: NotificationEntry): boolean {
  return entry.reason === "waiting" || (entry.reason === "done" && isUnread(entry));
}

export function actionableCount(entries: readonly NotificationEntry[]): number {
  return entries.reduce((count, entry) => (isActionable(entry) ? count + 1 : count), 0);
}

export function filterEntries(
  entries: readonly NotificationEntry[],
  filter: AttentionFilter,
): NotificationEntry[] {
  if (filter === "unread") return entries.filter(isUnread);
  if (filter === "needs-you") return entries.filter(isActionable);
  return [...entries];
}

export function sortEntries(entries: readonly NotificationEntry[]): NotificationEntry[] {
  return [...entries].sort((a, b) => {
    const unreadDiff = Number(isUnread(b)) - Number(isUnread(a));
    if (unreadDiff !== 0) return unreadDiff;
    const timeDiff = b.lastOccurredAt - a.lastOccurredAt;
    if (timeDiff !== 0) return timeDiff;
    return a.id.localeCompare(b.id);
  });
}

export function groupEntries(entries: readonly NotificationEntry[]): AttentionGroup[] {
  const sorted = sortEntries(entries);
  return SECTION_ORDER.map((section) => ({
    section,
    title: SECTION_TITLES[section],
    items: sorted.filter((entry) => attentionSectionOf(entry) === section),
  })).filter((group) => group.items.length > 0);
}
