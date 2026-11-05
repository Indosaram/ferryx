/**
 * Rev3 history reconciliation for one visible target.
 *
 * Anchor storage: after a full page, the anchor is the oldest open turn, or the newest
 * closed turn when none is open. `previousTurnFingerprint` is the sha256 of the previous
 * turn's exact wire JSON when the response text can be sliced, otherwise canonical JSON.
 * It is null when the anchor is the first turn. A delta stores the server's `nextAnchor`.
 * A reset, an unaligned page, a target change, or a missing stream id clears it.
 *
 * Coalescing: at most one in-flight read per target. A matching frame, visibility restore,
 * or reconnect that arrives during that read sets one dirty bit. When the read settles,
 * exactly one follow-up runs if the bit is set. An ordinary poll that overlaps a read is
 * skipped and does not set the bit; the next poll is the floor.
 */
import { sha256 } from "@noble/hashes/sha2.js";
import {
  parseReferenceHistoryChanged,
} from "../agentConversation";
import {
  referenceTargetKey,
  sameReferenceTarget,
  type ReferenceHistoryAnchor,
  type ReferenceHistoryChanged,
  type ReferenceHistoryDeltaResponse,
  type ReferenceHistoryPage,
  type ReferenceHistorySource,
  type ReferencePart,
  type ReferenceTargetRef,
  type ReferenceTurn,
} from "./referenceTypes";

const NO_ANCHOR_SOURCES: ReadonlySet<ReferenceHistorySource> = new Set([
  "pi-transcript",
  "cursor-transcript",
  "opencode-transcript",
  "crush-transcript",
  "cline-transcript",
  "scrollback",
]);

export type HistoryReadReason = "poll" | "frame" | "visibility" | "reconnect" | "full";

export interface HistoryReadTicket {
  readonly seq: number;
  readonly targetKey: string;
  readonly after: ReferenceHistoryAnchor | null;
}

export type HistorySettleInput =
  | {
      readonly kind: "page";
      readonly page: ReferenceHistoryPage;
      readonly fingerprints?: readonly (string | null)[];
    }
  | { readonly kind: "delta"; readonly delta: Extract<ReferenceHistoryDeltaResponse, { mode: "delta" }> }
  | { readonly kind: "reset" }
  | { readonly kind: "malformed" }
  | { readonly kind: "failed" }
  | { readonly kind: "dropped" };

export type HistorySettleResult =
  | { readonly kind: "stale" }
  | { readonly kind: "page"; readonly page: ReferenceHistoryPage; readonly followUp: HistoryReadTicket | null }
  | { readonly kind: "full"; readonly ticket: HistoryReadTicket }
  | { readonly kind: "idle"; readonly followUp: HistoryReadTicket | null };

export function referenceTurnFingerprintFromJson(json: string): string {
  const digest = sha256(new TextEncoder().encode(json));
  let hex = "";
  for (const byte of digest) hex += byte.toString(16).padStart(2, "0");
  return hex;
}

/** Top-level `turns` elements, as the response spelled them. Null when the text cannot be sliced. */
export function referenceTurnWireSlices(body: string): readonly string[] | null {
  let i = 0;
  let depth = 0;
  let inString = false;
  let escape = false;
  while (i < body.length) {
    const ch = body[i];
    if (inString) {
      if (escape) escape = false;
      else if (ch === "\\") escape = true;
      else if (ch === '"') inString = false;
      i += 1;
      continue;
    }
    if (ch === '"') {
      if (depth === 1) {
        const end = jsonStringEnd(body, i);
        if (end < 0) return null;
        if (body.slice(i, end + 1) === '"turns"') {
          let j = end + 1;
          while (j < body.length && isJsonSpace(body[j])) j += 1;
          if (body[j] !== ":") return null;
          j += 1;
          while (j < body.length && isJsonSpace(body[j])) j += 1;
          if (body[j] !== "[") return null;
          return scanJsonArray(body, j);
        }
        i = end + 1;
        continue;
      }
      inString = true;
      i += 1;
      continue;
    }
    if (ch === "{" || ch === "[") depth += 1;
    else if (ch === "}" || ch === "]") depth -= 1;
    i += 1;
  }
  return null;
}

export function referenceHistoryTurnFingerprints(
  body: string,
  turns: readonly ReferenceTurn[],
): readonly (string | null)[] {
  const slices = referenceTurnWireSlices(body);
  if (slices && slices.length === turns.length) {
    return slices.map((slice) => referenceTurnFingerprintFromJson(slice));
  }
  return turns.map((turn) => {
    const json = canonicalTurnJson(turn);
    return json === null ? null : referenceTurnFingerprintFromJson(json);
  });
}

/** Stream id from the page cursor, else from `{streamId}:inc-N` or `{streamId}:{16 hex}`. */
export function referenceHistoryStreamId(page: ReferenceHistoryPage): string | null {
  const cursorId = page.cursor?.streamId;
  if (typeof cursorId === "string" && cursorId.length > 0) return cursorId;
  const inc = /^(.*):inc-[0-9]+$/.exec(page.generation);
  if (inc && inc[1].length > 0) return inc[1];
  const genesis = /^(.*):[0-9a-f]{16}$/.exec(page.generation);
  if (genesis && genesis[1].length > 0) return genesis[1];
  return null;
}

export function referenceHistoryAnchorFromPage(
  page: ReferenceHistoryPage,
  fingerprints: readonly (string | null)[] = page.turns.map((turn) => {
    const json = canonicalTurnJson(turn);
    return json === null ? null : referenceTurnFingerprintFromJson(json);
  }),
): ReferenceHistoryAnchor | null {
  if (page.availability !== "native" || NO_ANCHOR_SOURCES.has(page.source)) return null;
  if (page.turns.length === 0 || fingerprints.length !== page.turns.length) return null;
  if (page.turns.some((turn) => typeof turn.id !== "string" || turn.id.length === 0)) return null;
  const streamId = referenceHistoryStreamId(page);
  if (streamId === null) return null;
  let open = -1;
  for (let index = 0; index < page.turns.length; index += 1) {
    if (referenceTurnIsOpen(page.turns[index], index === page.turns.length - 1)) {
      open = index;
      break;
    }
  }
  const index = open >= 0 ? open : page.turns.length - 1;
  const anchorTurnId = page.turns[index].id;
  if (typeof anchorTurnId !== "string" || anchorTurnId.length === 0) return null;
  const previous = index === 0 ? null : fingerprints[index - 1];
  if (index > 0 && (previous === null || previous === undefined)) return null;
  return {
    streamId,
    generation: page.generation,
    anchorTurnId,
    previousTurnFingerprint: previous ?? null,
  };
}

/** Replace from `replaceFromTurnId` and append. Null when the anchor turn is not held. */
export function applyReferenceHistoryDelta(
  turns: readonly ReferenceTurn[],
  delta: Extract<ReferenceHistoryDeltaResponse, { mode: "delta" }>,
): { readonly turns: readonly ReferenceTurn[]; readonly anchor: ReferenceHistoryAnchor } | null {
  const index = turns.findIndex((turn) => turn.id === delta.replaceFromTurnId);
  if (index < 0) return null;
  if (delta.turns.length === 0 || delta.turns[0]?.id !== delta.replaceFromTurnId) return null;
  return {
    turns: [...turns.slice(0, index), ...delta.turns],
    anchor: delta.nextAnchor,
  };
}

export function mergeHistoryPage(
  page: ReferenceHistoryPage,
  turns: readonly ReferenceTurn[],
  generation: string,
  source: ReferenceHistorySource,
): ReferenceHistoryPage {
  return { ...page, turns, generation, source };
}

export interface ReferenceHistorySync {
  setDeltaEnabled(enabled: boolean): void;
  bind(target: ReferenceTargetRef | null): void;
  readonly anchor: ReferenceHistoryAnchor | null;
  readonly inFlight: boolean;
  readonly dirty: boolean;
  invalidateFlight(): void;
  noteCleared(): void;
  noteDeferred(): void;
  acceptsFrame(raw: unknown): boolean;
  begin(reason: HistoryReadReason): HistoryReadTicket | null;
  onFrame(raw: unknown): HistoryReadTicket | null;
  settle(ticket: HistoryReadTicket, input: HistorySettleInput): HistorySettleResult;
}

export function createReferenceHistorySync(): ReferenceHistorySync {
  let enabled = false;
  let target: ReferenceTargetRef | null = null;
  let boundKey = "";
  let anchor: ReferenceHistoryAnchor | null = null;
  let turns: readonly ReferenceTurn[] = [];
  let lastPage: ReferenceHistoryPage | null = null;
  let fingerprints: readonly (string | null)[] = [];
  let inFlight = false;
  let dirty = false;
  let seq = 0;
  let flightSeq = 0;
  let flightAfter: ReferenceHistoryAnchor | null = null;

  const rebuild = (): void => {
    anchor = enabled && lastPage !== null
      ? referenceHistoryAnchorFromPage(lastPage, fingerprints)
      : null;
  };

  const takeFollowUp = (): HistoryReadTicket | null => {
    if (!dirty || target === null) return null;
    return begin("frame");
  };

  const beginFull = (): HistoryReadTicket => {
    anchor = null;
    const ticket = begin("full");
    if (ticket === null) {
      throw new Error("a full history read started while another read was in flight");
    }
    return ticket;
  };

  function begin(reason: HistoryReadReason): HistoryReadTicket | null {
    if (target === null) return null;
    if (inFlight) {
      if (reason !== "poll") dirty = true;
      return null;
    }
    seq += 1;
    flightSeq = seq;
    inFlight = true;
    dirty = false;
    const after = enabled && anchor !== null && reason !== "full" ? anchor : null;
    flightAfter = after;
    return { seq: flightSeq, targetKey: boundKey, after };
  }

  return {
    setDeltaEnabled(next: boolean): void {
      enabled = next;
      if (next) rebuild();
    },
    bind(next: ReferenceTargetRef | null): void {
      const key = next === null ? "" : referenceTargetKey(next);
      if (key !== boundKey) {
        boundKey = key;
        target = next;
        anchor = null;
        turns = [];
        lastPage = null;
        fingerprints = [];
        dirty = false;
        seq += 1;
        flightSeq = seq;
        inFlight = false;
        flightAfter = null;
        return;
      }
      target = next;
    },
    get anchor() {
      return anchor;
    },
    get inFlight() {
      return inFlight;
    },
    get dirty() {
      return dirty;
    },
    invalidateFlight(): void {
      seq += 1;
      flightSeq = seq;
      inFlight = false;
      dirty = false;
      flightAfter = null;
    },
    noteCleared(): void {
      anchor = null;
      turns = [];
      lastPage = null;
      fingerprints = [];
      dirty = false;
      seq += 1;
      flightSeq = seq;
      inFlight = false;
      flightAfter = null;
    },
    noteDeferred(): void {
      dirty = true;
    },
    acceptsFrame(raw: unknown): boolean {
      const frame = readChangedFrame(raw);
      return frame !== null && enabled && target !== null && sameReferenceTarget(target, frame.target);
    },
    begin,
    onFrame(raw: unknown): HistoryReadTicket | null {
      if (!this.acceptsFrame(raw)) return null;
      return begin("frame");
    },
    settle(ticket: HistoryReadTicket, input: HistorySettleInput): HistorySettleResult {
      if (ticket.seq !== flightSeq || ticket.targetKey !== boundKey) return { kind: "stale" };
      if (input.kind === "dropped") {
        inFlight = false;
        flightAfter = null;
        return { kind: "idle", followUp: null };
      }
      if (input.kind === "failed") {
        inFlight = false;
        flightAfter = null;
        return { kind: "idle", followUp: takeFollowUp() };
      }
      if (input.kind === "reset" || (input.kind === "malformed" && flightAfter !== null)) {
        inFlight = false;
        flightAfter = null;
        return { kind: "full", ticket: beginFull() };
      }
      if (input.kind === "malformed") {
        inFlight = false;
        flightAfter = null;
        return { kind: "idle", followUp: null };
      }
      if (input.kind === "delta") {
        if (ticket.after !== null && input.delta.generation !== ticket.after.generation) {
          inFlight = false;
          flightAfter = null;
          return { kind: "full", ticket: beginFull() };
        }
        const applied = applyReferenceHistoryDelta(turns, input.delta);
        if (applied === null || lastPage === null) {
          inFlight = false;
          flightAfter = null;
          return { kind: "full", ticket: beginFull() };
        }
        turns = applied.turns;
        anchor = applied.anchor;
        lastPage = mergeHistoryPage(lastPage, applied.turns, input.delta.generation, input.delta.source);
        inFlight = false;
        flightAfter = null;
        return { kind: "page", page: lastPage, followUp: takeFollowUp() };
      }
      turns = input.page.turns;
      lastPage = input.page;
      fingerprints = input.fingerprints ?? referenceHistoryAnchorFingerprints(input.page);
      rebuild();
      inFlight = false;
      flightAfter = null;
      return { kind: "page", page: input.page, followUp: takeFollowUp() };
    },
  };
}

function referenceHistoryAnchorFingerprints(page: ReferenceHistoryPage): readonly (string | null)[] {
  return page.turns.map((turn) => {
    const json = canonicalTurnJson(turn);
    return json === null ? null : referenceTurnFingerprintFromJson(json);
  });
}

function readChangedFrame(raw: unknown): ReferenceHistoryChanged | null {
  let body = raw;
  if (typeof raw === "string") {
    try {
      body = JSON.parse(raw);
    } catch {
      return null;
    }
  }
  if (body === null || typeof body !== "object" || Array.isArray(body)) return null;
  if ((body as { type?: unknown }).type !== "referenceHistoryChanged") return null;
  try {
    return parseReferenceHistoryChanged(body);
  } catch {
    return null;
  }
}

export function referenceTurnIsOpen(turn: ReferenceTurn, newest: boolean): boolean {
  const unanswered = turn.parts.some((part) => {
    if (part.kind !== "tool") return false;
    if (part.status === "running") return true;
    const settled =
      part.status === "completed" ||
      part.status === "failed" ||
      part.status === "cancelled" ||
      part.status === "unverifiable";
    return typeof part.callId === "string" && part.output === "" && part.error !== true && !settled;
  });
  const openCommentary =
    turn.parts.some((part) => part.kind === "text" && part.phase === "commentary") &&
    !turn.parts.some((part) => part.kind === "text" && part.phase === "finalAnswer");
  // A submitted user turn is never an open tail. Assistant text or thinking may still grow.
  const tailText =
    newest &&
    turn.role !== "user" &&
    turn.parts.some((part) => part.kind === "text" || part.kind === "thinking");
  return unanswered || openCommentary || tailText;
}

function canonicalTurnJson(turn: ReferenceTurn): string | null {
  const parts = turn.parts.map(canonicalPart);
  if (parts.some((part) => part === null)) return null;
  const record: Record<string, unknown> = { role: turn.role };
  if (typeof turn.startedAt === "string") record.startedAt = turn.startedAt;
  if (typeof turn.endedAt === "string") record.endedAt = turn.endedAt;
  if (turn.source === "typed" || turn.source === "runtime") record.source = turn.source;
  record.parts = parts;
  if (turn.abandoned) {
    const abandoned: Record<string, unknown> = {
      count: turn.abandoned.count,
      branches: turn.abandoned.branches,
    };
    if (typeof turn.abandoned.summary === "string") abandoned.summary = turn.abandoned.summary;
    record.abandoned = abandoned;
  }
  if (typeof turn.id === "string") record.id = turn.id;
  if (turn.meta) {
    const meta: Record<string, unknown> = { provider: turn.meta.provider };
    if (typeof turn.meta.model === "string") meta.model = turn.meta.model;
    if (turn.meta.usage) {
      const usage: Record<string, unknown> = {};
      const counts = turn.meta.usage;
      if (typeof counts.inputTokens === "number") usage.inputTokens = counts.inputTokens;
      if (typeof counts.outputTokens === "number") usage.outputTokens = counts.outputTokens;
      if (typeof counts.cacheReadTokens === "number") usage.cacheReadTokens = counts.cacheReadTokens;
      if (typeof counts.cacheWriteTokens === "number") usage.cacheWriteTokens = counts.cacheWriteTokens;
      meta.usage = usage;
    }
    record.meta = meta;
  }
  return JSON.stringify(record);
}

function canonicalPart(part: ReferencePart): Record<string, unknown> | null {
  const id = typeof part.id === "string" ? { id: part.id } : {};
  switch (part.kind) {
    case "text":
      return {
        kind: "text",
        text: part.text,
        ...(part.phase === "commentary" || part.phase === "finalAnswer" ? { phase: part.phase } : {}),
        ...id,
      };
    case "thinking":
      return { kind: "thinking", text: part.text, ...id };
    case "skill":
      return { kind: "skill", skill: canonicalSkill(part.skill), ...id };
    case "tool": {
      const tool: Record<string, unknown> = {
        kind: "tool",
        name: part.name,
        summary: part.summary,
        input: part.input,
        output: part.output,
      };
      if (typeof part.error === "boolean") tool.error = part.error;
      if (part.skill) tool.skill = canonicalSkill(part.skill);
      if (typeof part.outputRef === "string") tool.outputRef = part.outputRef;
      if (typeof part.outputSize === "number") tool.outputSize = part.outputSize;
      if (part.images && part.images.length > 0) tool.images = part.images.map((image) => ({ mediaType: image.mediaType, ref: image.ref }));
      if (typeof part.id === "string") tool.id = part.id;
      if (typeof part.callId === "string") tool.callId = part.callId;
      if (typeof part.status === "string") tool.status = part.status;
      if (part.outputPayload) tool.outputPayload = canonicalPayload(part.outputPayload);
      return tool;
    }
    case "image":
      return { kind: "image", mediaType: part.mediaType, ref: part.ref, ...id };
    case "compact":
      return { kind: "compact", text: part.text, ...id };
    case "notice":
      return {
        kind: "notice",
        text: part.text,
        ...(typeof part.source === "string" ? { source: part.source } : {}),
        ...id,
      };
    case "taskResult":
      return { kind: "taskResult", tasks: part.tasks.map(canonicalTask), ...id };
    case "diff":
      return { kind: "diff", patch: canonicalPayload(part.patch), ...id };
    default:
      return null;
  }
}

function canonicalSkill(skill: { name: string; evidence: string; status: string; path?: string | null }): Record<string, unknown> {
  return {
    name: skill.name,
    evidence: skill.evidence,
    status: skill.status,
    ...(typeof skill.path === "string" ? { path: skill.path } : {}),
  };
}

function canonicalTask(task: ReferenceTurn extends never ? never : {
  id: string;
  title: string;
  status: string;
  result: string;
  agent?: string | null;
  model?: string | null;
  durationMs?: number | null;
  turns?: number | null;
  toolCalls?: number | null;
  tokens?: number | null;
  resultCut?: boolean | null;
  parentCallId?: string | null;
}): Record<string, unknown> {
  const ordered: Record<string, unknown> = { id: task.id, title: task.title };
  if (typeof task.agent === "string") ordered.agent = task.agent;
  if (typeof task.model === "string") ordered.model = task.model;
  ordered.status = task.status;
  if (typeof task.durationMs === "number") ordered.durationMs = task.durationMs;
  if (typeof task.turns === "number") ordered.turns = task.turns;
  if (typeof task.toolCalls === "number") ordered.toolCalls = task.toolCalls;
  if (typeof task.tokens === "number") ordered.tokens = task.tokens;
  ordered.result = task.result;
  if (typeof task.resultCut === "boolean") ordered.resultCut = task.resultCut;
  if (typeof task.parentCallId === "string") ordered.parentCallId = task.parentCallId;
  return ordered;
}

function canonicalPayload(payload: { ref: string; byteLength: number; digest: string; truncated: boolean }): Record<string, unknown> {
  return { ref: payload.ref, byteLength: payload.byteLength, digest: payload.digest, truncated: payload.truncated };
}

function isJsonSpace(ch: string | undefined): boolean {
  return ch === " " || ch === "\n" || ch === "\r" || ch === "\t";
}

function jsonStringEnd(body: string, start: number): number {
  let i = start + 1;
  while (i < body.length) {
    if (body[i] === "\\") {
      i += 2;
      continue;
    }
    if (body[i] === '"') return i;
    i += 1;
  }
  return -1;
}

function scanJsonArray(body: string, open: number): readonly string[] | null {
  const slices: string[] = [];
  let i = open + 1;
  let depth = 0;
  let inString = false;
  let escape = false;
  let elementStart = -1;
  while (i < body.length) {
    const ch = body[i];
    if (inString) {
      if (escape) escape = false;
      else if (ch === "\\") escape = true;
      else if (ch === '"') inString = false;
      i += 1;
      continue;
    }
    if (ch === '"') {
      inString = true;
      if (depth === 0 && elementStart < 0) elementStart = i;
      i += 1;
      continue;
    }
    if (depth === 0 && isJsonSpace(ch)) {
      i += 1;
      continue;
    }
    if (depth === 0 && ch === ",") {
      i += 1;
      continue;
    }
    if (ch === "]" && depth === 0) return slices;
    if (ch === "{" || ch === "[") {
      if (depth === 0) elementStart = i;
      depth += 1;
      i += 1;
      continue;
    }
    if (ch === "}" || ch === "]") {
      depth -= 1;
      if (depth === 0 && elementStart >= 0) {
        slices.push(body.slice(elementStart, i + 1));
        elementStart = -1;
      }
      if (depth < 0) return null;
      i += 1;
      continue;
    }
    if (depth === 0 && elementStart < 0) elementStart = i;
    i += 1;
    if (depth === 0 && elementStart >= 0 && (ch === "t" || ch === "f" || ch === "n" || ch === "-" || (ch >= "0" && ch <= "9"))) {
      // primitive element: consume until comma or close
      while (i < body.length && body[i] !== "," && body[i] !== "]" && !isJsonSpace(body[i])) i += 1;
      slices.push(body.slice(elementStart, i));
      elementStart = -1;
    }
  }
  return null;
}


