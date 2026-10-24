import type { TargetRef } from "../../../lib/scopedContracts";
import type { StatusDotState } from "../../../components/ui/StatusDot";
import type { NotificationEntry, NotificationReadState } from "../../../lib/notificationCenter/types";
import type { Agent } from "./client";

export type AttentionSection = "needs-you" | "finished" | "working" | "mentions";
export type AttentionFilter = "all" | "unread" | "needs-you";

export interface AttentionRow {
  key: string;
  workspaceId: string;
  sessionId: string;
  label: string;
  section: AttentionSection;
  read: NotificationReadState;
  lastOccurredAt: number;
  target: TargetRef | null;
  entry: NotificationEntry | null;
  agent: Agent | null;
}

const SECTION_ORDER: AttentionSection[] = ["needs-you", "finished", "working", "mentions"];

export const ATTENTION_SECTION_TITLES: Record<AttentionSection, string> = {
  "needs-you": "Needs you",
  finished: "Finished",
  working: "Working",
  mentions: "Mentions",
};

export function isRowUnread(row: AttentionRow): boolean {
  return "unread" in row.read;
}

export function isRowActionable(row: AttentionRow): boolean {
  if (row.section === "needs-you") return true;
  return row.section === "finished" && isRowUnread(row);
}

function sectionOf(entry: NotificationEntry | null, agent: Agent | null): AttentionSection {
  if (entry?.reason === "bell") return "mentions";
  if (entry?.reason === "supervisor" && "unread" in entry.read) return "needs-you";
  if (agent?.state === "working") return "working";
  if (agent?.state === "waiting" || entry?.reason === "waiting") return "needs-you";
  return "finished";
}

function labelOf(entry: NotificationEntry | null, agent: Agent | null, sessionId: string): string {
  const labels = entry?.labels;
  const fromEntry =
    labels?.agentLabel?.trim() || labels?.terminalTitle?.trim() || labels?.workspaceLabel?.trim();
  return fromEntry || agent?.label?.trim() || sessionId;
}

/** Merges the durable event record (read state) with the live inventory (sessions that were already waiting at boot). */
export function buildAttentionRows(
  entries: readonly NotificationEntry[],
  agents: readonly Agent[],
  localKeyOf: (agent: Agent) => string | null,
): AttentionRow[] {
  const rows = new Map<string, AttentionRow>();

  for (const agent of agents) {
    const sessionId = localKeyOf(agent);
    if (!sessionId) continue;
    const entry =
      entries.find(
        (candidate) =>
          candidate.workspaceId === agent.workspaceId && candidate.sessionId === sessionId,
      ) ?? null;
    const section = sectionOf(entry, agent);
    const key = `${agent.workspaceId}:${sessionId}`;
    rows.set(key, {
      key,
      workspaceId: agent.workspaceId,
      sessionId,
      label: labelOf(entry, agent, sessionId),
      section,
      read: entry?.read ?? { unread: true },
      lastOccurredAt: entry?.lastOccurredAt ?? 0,
      target: agent.target,
      entry,
      agent,
    });
  }

  for (const entry of entries) {
    const key = `${entry.workspaceId}:${entry.sessionId}`;
    if (rows.has(key)) continue;
    rows.set(key, {
      key,
      workspaceId: entry.workspaceId,
      sessionId: entry.sessionId,
      label: labelOf(entry, null, entry.sessionId),
      section: sectionOf(entry, null),
      read: entry.read,
      lastOccurredAt: entry.lastOccurredAt,
      target: null,
      entry,
      agent: null,
    });
  }

  return [...rows.values()].filter((row) => row.section !== "finished" || row.entry !== null);
}

export function filterRows(
  rows: readonly AttentionRow[],
  filter: AttentionFilter,
): AttentionRow[] {
  if (filter === "unread") return rows.filter(isRowUnread);
  if (filter === "needs-you") return rows.filter(isRowActionable);
  return [...rows];
}

function sectionRank(section: AttentionSection): number {
  return SECTION_ORDER.indexOf(section);
}

export function sortRows(rows: readonly AttentionRow[]): AttentionRow[] {
  return [...rows].sort((a, b) => {
    const sectionDiff = sectionRank(a.section) - sectionRank(b.section);
    if (sectionDiff !== 0) return sectionDiff;
    const unreadDiff = Number(isRowUnread(b)) - Number(isRowUnread(a));
    if (unreadDiff !== 0) return unreadDiff;
    const timeDiff = b.lastOccurredAt - a.lastOccurredAt;
    if (timeDiff !== 0) return timeDiff;
    return a.key.localeCompare(b.key);
  });
}

export interface AttentionGroup {
  section: AttentionSection;
  title: string;
  rows: AttentionRow[];
}

/** Rows that exist only in the inventory borrow the entry shape so one row renderer serves both. */
export function rowDisplayEntry(row: AttentionRow): NotificationEntry {
  if (row.entry) return row.entry;
  return {
    id: row.key,
    workspaceId: row.workspaceId,
    sessionId: row.sessionId,
    labels: { agentLabel: row.label },
    subject: "agent",
    reason: row.section === "needs-you" ? "waiting" : "done",
    firstOccurredAt: row.lastOccurredAt,
    lastOccurredAt: row.lastOccurredAt,
    updateOrder: 0,
    revision: 0,
    occurrenceCount: 1,
    read: row.read,
  };
}

export function rowDotState(row: AttentionRow): StatusDotState {
  if (row.agent?.state === "working") return "working";
  if (row.agent?.state === "waiting") return "waiting";
  if (row.agent?.state === "failed") return "failed";
  if (row.agent?.state === "exited") return "exited";
  if (row.entry?.reason === "waiting" || row.entry?.reason === "supervisor") return "waiting";
  if (row.entry?.reason === "done") return "done";
  return "unread";
}

export function actionableEntryCount(entries: readonly NotificationEntry[]): number {
  return entries.reduce((count, entry) => {
    if (entry.reason === "waiting") return count + 1;
    if (entry.reason === "supervisor" && "unread" in entry.read) return count + 1;
    if (entry.reason === "done" && "unread" in entry.read) return count + 1;
    return count;
  }, 0);
}

export function groupRows(rows: readonly AttentionRow[]): AttentionGroup[] {
  const sorted = sortRows(rows);
  return SECTION_ORDER.map((section) => ({
    section,
    title: ATTENTION_SECTION_TITLES[section],
    rows: sorted.filter((row) => row.section === section),
  })).filter((group) => group.rows.length > 0);
}
