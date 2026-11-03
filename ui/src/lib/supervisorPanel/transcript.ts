// The supervisor's app transcript as the panel shows it: the owner's prompts, the supervisor's
// Reply rows and its long reports, read from the existing transcript routes. Desktop reads the
// daemon view through `cmd_supervisor_read`; the remote web and the phone read
// `GET /api/v1/supervisor/transcript`. Both return the same stored page; nothing is kept twice.
import { isTauriRuntime, readSupervisorTranscriptPage, saveSupervisorTranscriptFile } from "../tauri";
import type { EvidenceRef } from "./contract";

export type TranscriptMessage = {
  readonly id: number;
  readonly role: "owner" | "supervisor";
  readonly text: string;
  readonly evidence: readonly EvidenceRef[];
  readonly requestId: string;
  readonly createdAtMs: number;
};

export type TranscriptPage = {
  readonly messages: readonly TranscriptMessage[];
  readonly nextCursor: number;
  readonly hasMore: boolean;
};

export type TranscriptPageLoader = (cursor: number) => Promise<TranscriptPage>;

/** Request-id prefix of a worker report the supervisor stored (`report:<wake event id>`). */
const REPORT_REQUEST_PREFIX = "report:";
/** More pages than this in one read means the cursor is not advancing; stop rather than spin. */
const MAX_PAGES = 500;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function parseMessage(value: unknown): TranscriptMessage | null {
  if (!isRecord(value)) return null;
  const { id, role, text, requestId, createdAtMs, evidence } = value;
  if (typeof id !== "number" || typeof text !== "string" || typeof requestId !== "string") return null;
  if (role !== "owner" && role !== "supervisor") return null;
  return {
    id,
    role,
    text,
    requestId,
    createdAtMs: typeof createdAtMs === "number" ? createdAtMs : 0,
    evidence: Array.isArray(evidence) ? (evidence as EvidenceRef[]) : [],
  };
}

export function parseTranscriptPage(value: unknown): TranscriptPage | null {
  if (!isRecord(value) || !Array.isArray(value.messages)) return null;
  const messages = value.messages.map(parseMessage);
  if (messages.some((message) => message === null)) return null;
  return {
    messages: messages as TranscriptMessage[],
    nextCursor: typeof value.nextCursor === "number" ? value.nextCursor : 0,
    hasMore: value.hasMore === true,
  };
}

/** The desktop's page loader: the daemon's `transcript` read over the Tauri command. */
export async function loadDesktopTranscriptPage(cursor: number): Promise<TranscriptPage> {
  const page = parseTranscriptPage(await readSupervisorTranscriptPage(cursor));
  if (!page) throw new Error("The supervisor transcript reply was not a transcript page");
  return page;
}

/** Every message after `cursor`, page by page, with the cursor to continue from next time. */
export async function readTranscriptAfter(
  load: TranscriptPageLoader,
  cursor: number,
): Promise<{ readonly messages: readonly TranscriptMessage[]; readonly cursor: number }> {
  const messages: TranscriptMessage[] = [];
  let next = cursor;
  for (let pages = 0; pages < MAX_PAGES; pages += 1) {
    const page = await load(next);
    messages.push(...page.messages);
    const advanced = page.nextCursor > next;
    next = Math.max(next, page.nextCursor);
    if (!page.hasMore || !advanced) break;
  }
  return { messages, cursor: next };
}

export function isReportMessage(message: TranscriptMessage): boolean {
  return message.role === "supervisor" && message.requestId.startsWith(REPORT_REQUEST_PREFIX);
}

/**
 * A long report's beginning and its last line, so both ends read in the row without opening it.
 * A short report is shown whole (`tail` null). Markdown heading marks are dropped for reading.
 */
export function reportExcerpt(text: string, headLines = 3): { readonly head: string; readonly tail: string | null } {
  const lines = text
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => line.replace(/^#{1,6}\s+/, ""));
  if (lines.length <= headLines + 1) return { head: lines.join("\n"), tail: null };
  return { head: lines.slice(0, headLines).join("\n"), tail: lines[lines.length - 1] };
}

function roleLabel(message: TranscriptMessage): string {
  if (message.role === "owner") return "Owner prompt";
  return isReportMessage(message) ? "Supervisor report" : "Supervisor reply";
}

/** The export file's text: one block per stored row, oldest first, in the stored words. */
export function formatTranscript(messages: readonly TranscriptMessage[]): string {
  const blocks = messages.map(
    (message) => `[${new Date(message.createdAtMs).toISOString()}] ${roleLabel(message)}\n${message.text}\n`,
  );
  return `Supervisor transcript\n\n${blocks.join("\n")}`;
}

export function transcriptFileName(nowMs: number): string {
  return `supervisor-transcript-${new Date(nowMs).toISOString().replace(/[:.]/g, "-")}.txt`;
}

export type ExportResult = { readonly ok: true; readonly where: string } | { readonly ok: false; readonly message: string };

function downloadInBrowser(fileName: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain;charset=utf-8" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

/**
 * Saves the transcript as a file. The desktop webview has no download handler, so the desktop
 * writes it into the Downloads folder through a command; the remote web downloads it in the browser.
 */
export async function exportTranscriptFile(messages: readonly TranscriptMessage[], nowMs: number): Promise<ExportResult> {
  const fileName = transcriptFileName(nowMs);
  const text = formatTranscript(messages);
  try {
    if (isTauriRuntime()) {
      const path = await saveSupervisorTranscriptFile(fileName, text);
      return { ok: true, where: path };
    }
    downloadInBrowser(fileName, text);
    return { ok: true, where: fileName };
  } catch (error) {
    const message = error instanceof Error && error.message ? error.message : "The transcript was not saved.";
    return { ok: false, message };
  }
}
