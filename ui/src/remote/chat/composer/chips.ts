/**
 * Inline context chips and citations (t3code composer.md "Context in your message",
 * "Quote an assistant response") against the frozen ChipItem/ChipSources contract.
 *
 * A chip lives in the draft as readable markdown: `[label](ferryx://chip/<kind>/<id>)`.
 * It copies and pastes with the text around it and deletes like a character. A citation is
 * `[quote](ferryx://cite/<messageId> "comment")`; the pencil edits the comment, which rides
 * along in copy, stash and send.
 *
 * Labels are escaped the way t3code escapes chip text before writing link syntax
 * (`escapeMarkdownText`, assistantCitations.ts:176-182), so a label holding `[` or `]` still
 * parses back as one link instead of breaking at the first bracket.
 */
import type { ChipItem } from "./chipTypes";
import { escapeLinkLabel } from "./markdown";

/** The trigger character that opens each chip menu; "/" is the existing slash menu. */
export type ChipTrigger = "@" | "#" | "$" | "/";

export interface ChipTokenRange {
  readonly start: number;
  readonly end: number;
  readonly query: string;
  readonly trigger: ChipTrigger;
}

/**
 * The chip trigger whose query ends at the caret, or null. The trigger must start the message
 * or follow whitespace, and the query may hold anything but whitespace. A bare "/" is the
 * provider slash menu's business, so it never opens this menu.
 */
export function chipTriggerAt(text: string, caret: number, trigger: ChipTrigger): ChipTokenRange | null {
  const at = Math.min(Math.max(0, Math.trunc(caret)), text.length);
  const before = text.slice(0, at);
  const pattern = new RegExp(`(?:^|\\s)(${escapeRegExp(trigger)})([^\\s]*)$`);
  const match = pattern.exec(before);
  if (!match) return null;
  const query = match[2];
  if (trigger === "/" && query.length === 0) return null;
  return { start: at - query.length - 1, end: at, query, trigger };
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?${}()|[\]\\]/g, "\\$&");
}

/** Static sources resolve a trigger's candidates directly; threads and PRs are queried. */
export function staticChipCandidates(
  trigger: ChipTrigger,
  sources: { models?: readonly ChipItem[]; skills?: readonly ChipItem[] },
): readonly ChipItem[] {
  if (trigger === "@") return sources.models ?? [];
  if (trigger === "$") return sources.skills ?? [];
  return [];
}

export function chipMarkdown(item: ChipItem): string {
  return `[${escapeLinkLabel(item.label)}](ferryx://chip/${item.kind}/${encodeURIComponent(item.id)})`;
}

export interface CitationInsertLike {
  readonly messageId: string;
  readonly text: string;
}

/**
 * The readable markdown a citation chip serializes to; the comment rides in the link title.
 *
 * The whole quote is in the label — the agent is told which words were quoted, and the quote
 * counts toward the message limit — with the line breaks encoded as `\n` so the label stays on
 * one line the chip parser and the editor can find.
 */
export function citationMarkdown(citation: CitationInsertLike, comment: string): string {
  const label = escapeLinkLabel(citation.text);
  const url = `ferryx://cite/${encodeURIComponent(citation.messageId)}`;
  return comment.length > 0 ? `[${label}](${url} "${comment.replace(/"/g, "'")}")` : `[${label}](${url})`;
}

/** Read a citation link back: its message id and comment title, or null for other links. */
export function parseCitation(
  url: string,
  title: string | null | undefined,
): { messageId: string; comment: string } | null {
  const match = /^ferryx:\/\/cite\/([^"/?#]*)(?:\/.*)?$/.exec(url);
  if (!match) return null;
  return { messageId: decodeURIComponent(match[1]), comment: title ?? "" };
}

/**
 * The chip triggers this composer owns beyond the existing slash menu: "@" opens the thread
 * and model picker, "#" pull requests, "$" skills.
 */
export function isChipTrigger(char: string): char is ChipTrigger {
  return char === "@" || char === "#" || char === "$";
}
