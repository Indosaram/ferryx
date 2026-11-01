// One result shape for every platform action. Cards and the DB get facts the code checked,
// never raw page text or agent transcripts (an agent run's transcript stays in its Aside session).
export type OutcomeStatus = "done" | "already" | "dry_run" | "failed";

export interface Outcome {
  status: OutcomeStatus;
  /** Machine code, e.g. APPLIED, LOGIN_REQUIRED. Decisions use this, never the human text. */
  code: string;
  /** Short human-readable lines shown on the card. */
  facts: string[];
  /** Aside agent session id; the full transcript is in ~/.aside/u/<n>/sessions/<date>_<id>/. */
  session?: string;
}

const make = (status: OutcomeStatus) => (code: string, facts: string[] = [], session?: string): Outcome =>
  ({ status, code, facts: facts.filter(Boolean), ...(session ? { session } : {}) });
export const done = make("done");
export const already = make("already");
export const dryRun = make("dry_run");
export const failed = (code: string, reason = "", session?: string): Outcome => make("failed")(code, [reason], session);

const ANSI = /\x1b\[[0-9;?]*[ -\/]*[@-~]/g;
/** One clean line: no terminal colour codes, no line breaks, at most `max` characters. */
export function brief(text: string, max = 200): string {
  const s = text.replace(ANSI, "").replace(/\s+/g, " ").trim();
  return s.length <= max ? s : s.slice(0, max - 1) + "…";
}

/** The most telling line of an error: its last "Error" line, else its first line. */
export function briefError(e: unknown, max = 300): string {
  const lines = String((e as Error)?.message ?? e).replace(ANSI, "").split("\n").map((l) => l.trim()).filter(Boolean);
  const errorLine = [...lines].reverse().find((l) => /error/i.test(l));
  return brief(errorLine ?? lines[0] ?? "알 수 없는 오류", max);
}

export function render(label: string, o: Outcome): string {
  const head = {
    done: `✓ ${label} 완료`,
    already: `✓ ${label} — 이미 처리돼 있습니다`,
    dry_run: `${label} 드라이런 — 실제로 제출하지 않았습니다`,
    failed: `⚠️ ${label} 실패`,
  }[o.status];
  const lines = [head, ...o.facts.map((f) => `• ${f}`)];
  if (o.session) lines.push(`Aside 세션: ${o.session}`);
  return lines.join("\n");
}
