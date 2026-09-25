import { useState } from "react";

import {
  readAgentHistory,
  searchAgentHistory,
  type AgentHistoryEntry,
  type AgentHistoryMessage,
  type AgentHistoryPage,
  type AgentHistoryProvider,
} from "../../lib/agentHistory";

export interface AttentionAskServices {
  search: (request: {
    provider: AgentHistoryProvider;
    cwd: string | null;
    query: string;
    cursor: string | null;
    limit: number;
  }) => Promise<AgentHistoryPage<AgentHistoryEntry>>;
  read: (request: {
    entryKey: string;
    cursor: string | null;
    limit: number;
  }) => Promise<AgentHistoryPage<AgentHistoryMessage>>;
}

export interface AttentionAskRange {
  id: string;
  label: string;
  ms: number | null;
}

export const ATTENTION_ASK_RANGES: AttentionAskRange[] = [
  { id: "15m", label: "15분", ms: 15 * 60 * 1000 },
  { id: "30m", label: "30분", ms: 30 * 60 * 1000 },
  { id: "1h", label: "1시간", ms: 60 * 60 * 1000 },
  { id: "3h", label: "3시간", ms: 3 * 60 * 60 * 1000 },
  { id: "6h", label: "6시간", ms: 6 * 60 * 60 * 1000 },
  { id: "12h", label: "12시간", ms: 12 * 60 * 60 * 1000 },
  { id: "24h", label: "24시간", ms: 24 * 60 * 60 * 1000 },
  { id: "3d", label: "3일", ms: 3 * 24 * 60 * 60 * 1000 },
  { id: "7d", label: "7일", ms: 7 * 24 * 60 * 60 * 1000 },
  { id: "all", label: "전체", ms: null },
];

export const DEFAULT_ASK_RANGE_ID = "3h";

const ASK_PROVIDERS: AgentHistoryProvider[] = ["claude", "codex"];
const ASK_ENTRY_LIMIT = 24;
const ASK_EVIDENCE_LIMIT = 6;
const ASK_SNIPPETS_PER_ENTRY = 3;
const ASK_MESSAGE_LIMIT = 200;

export interface AttentionAskEvidence {
  entry: AgentHistoryEntry;
  snippets: string[];
}

export interface AttentionAskResult {
  evidence: AttentionAskEvidence[];
  scanned: number;
  partial: boolean;
  warnings: string[];
}

function rangeOf(id: string): AttentionAskRange {
  return ATTENTION_ASK_RANGES.find((range) => range.id === id) ?? ATTENTION_ASK_RANGES[3];
}

export function isWithinRange(entry: AgentHistoryEntry, range: AttentionAskRange, now: number): boolean {
  if (range.ms === null) return true;
  if (typeof entry.modifiedMs !== "number") return true;
  return now - entry.modifiedMs <= range.ms;
}

function matchingSnippets(messages: readonly AgentHistoryMessage[], query: string): string[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const snippets: string[] = [];
  for (const message of messages) {
    for (const line of message.text.split("\n")) {
      if (line.toLowerCase().includes(needle) && line.trim()) {
        snippets.push(line.trim().slice(0, 240));
        if (snippets.length >= ASK_SNIPPETS_PER_ENTRY) return snippets;
      }
    }
  }
  return snippets;
}

export async function collectAskEvidence(
  query: string,
  range: AttentionAskRange,
  services: AttentionAskServices,
  now = Date.now(),
): Promise<AttentionAskResult> {
  const warnings: string[] = [];
  let partial = false;
  const candidates: AgentHistoryEntry[] = [];

  for (const provider of ASK_PROVIDERS) {
    const page = await services.search({
      provider,
      cwd: null,
      query,
      cursor: null,
      limit: ASK_ENTRY_LIMIT,
    });
    if (page.partial) partial = true;
    warnings.push(...(page.warnings ?? []));
    candidates.push(...page.items.filter((entry) => isWithinRange(entry, range, now)));
  }

  const scanned = candidates.length;
  candidates.sort((a, b) => (b.modifiedMs ?? 0) - (a.modifiedMs ?? 0));

  const evidence: AttentionAskEvidence[] = [];
  for (const entry of candidates) {
    if (evidence.length >= ASK_EVIDENCE_LIMIT) break;
    const page = await services.read({ entryKey: entry.entryKey, cursor: null, limit: ASK_MESSAGE_LIMIT });
    if (page.partial) partial = true;
    warnings.push(...(page.warnings ?? []));
    const snippets = matchingSnippets(page.items, query);
    if (snippets.length > 0) evidence.push({ entry, snippets });
  }

  return { evidence, scanned, partial, warnings };
}

export interface AttentionAskPanelProps {
  services?: AttentionAskServices;
  className?: string;
}

const defaultServices: AttentionAskServices = {
  search: searchAgentHistory,
  read: readAgentHistory,
};

export function AttentionAskPanel({ services = defaultServices, className }: AttentionAskPanelProps) {
  const [rangeId, setRangeId] = useState(DEFAULT_ASK_RANGE_ID);
  const [question, setQuestion] = useState("");
  const [result, setResult] = useState<AttentionAskResult | null>(null);
  const [asked, setAsked] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const range = rangeOf(rangeId);

  const submit = async () => {
    const query = question.trim();
    if (!query || busy) return;
    setBusy(true);
    setError(null);
    try {
      const next = await collectAskEvidence(query, range, services);
      setResult(next);
      setAsked(query);
    } catch (caught) {
      setResult(null);
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setBusy(false);
    }
  };

  const formatTime = (ms: number | null | undefined) =>
    typeof ms === "number" ? new Date(ms).toLocaleString() : "시간 정보 없음";

  return (
    <div className={className} data-testid="attention-ask">
      <div className="flex flex-wrap gap-1 px-3 py-2" role="group" aria-label="Time range">
        {ATTENTION_ASK_RANGES.map((candidate) => (
          <button
            key={candidate.id}
            type="button"
            data-testid={`ask-range-${candidate.id}`}
            aria-pressed={candidate.id === rangeId}
            onClick={() => setRangeId(candidate.id)}
            className={
              candidate.id === rangeId
                ? "rounded px-2 py-0.5 text-[11px] bg-primary/15 text-primary"
                : "rounded px-2 py-0.5 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
            }
          >
            {candidate.label}
          </button>
        ))}
      </div>

      <form
        className="flex items-center gap-1.5 px-3 pb-2"
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <input
          value={question}
          onChange={(event) => setQuestion(event.target.value)}
          aria-label="Ask past conversations"
          placeholder="무엇을 찾을까요?"
          className="min-w-0 flex-1 rounded border border-input bg-background px-2 py-1 text-xs outline-none focus:border-ring"
        />
        <button
          type="submit"
          disabled={busy || question.trim().length === 0}
          className="rounded bg-primary px-2.5 py-1 text-[11px] font-medium text-primary-foreground disabled:opacity-40"
        >
          {busy ? "찾는 중…" : "질문"}
        </button>
      </form>

      {error ? (
        <p role="alert" className="px-3 pb-2 text-[11px] text-destructive">
          {error}
        </p>
      ) : null}

      {result && result.evidence.length === 0 ? (
        <p role="status" className="px-3 pb-3 text-[11px] text-muted-foreground">
          {range.label} 범위에서 “{asked}”에 해당하는 메시지를 찾지 못해 답변할 근거가 없습니다.
        </p>
      ) : null}

      {result && result.evidence.length > 0 ? (
        <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-3">
          <p className="pb-1.5 text-[11px] text-muted-foreground">
            {range.label} 범위 · 대화 {result.scanned}건에서 근거 {result.evidence.length}건
            {result.partial ? " · 일부만 검색됨" : ""}
          </p>
          <ul className="flex flex-col gap-2">
            {result.evidence.map((item) => (
              <li
                key={item.entry.entryKey}
                data-testid={`ask-evidence-${item.entry.entryKey}`}
                className="rounded border border-border px-2 py-1.5"
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate text-[11px] font-medium">
                    {item.entry.providerSession.id}
                  </span>
                  <span className="shrink-0 text-[10px] text-muted-foreground">
                    {formatTime(item.entry.modifiedMs)}
                  </span>
                </div>
                <span className="block truncate text-[10px] text-muted-foreground">{item.entry.cwd}</span>
                <ul className="pt-1">
                  {item.snippets.map((snippet, index) => (
                    <li key={index} className="text-[11px] leading-snug">
                      {snippet}
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ul>
          {result.warnings.length > 0 ? (
            <p className="pt-2 text-[10px] text-muted-foreground">
              경고 {result.warnings.length}건
            </p>
          ) : null}
        </div>
      ) : null}

      {!result && !error ? (
        <p className="px-3 pb-3 text-[11px] text-muted-foreground">
          기간을 고르고 물어보면 그 범위의 대화에서 근거를 찾아 보여줍니다. 요약은 하지 않습니다.
        </p>
      ) : null}
    </div>
  );
}
