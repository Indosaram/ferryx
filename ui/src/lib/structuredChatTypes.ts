/**
 * Structured agent chat wire types (lane G7, chat-t3-parity plan) and their validated decoder.
 *
 * The Rust twin is `src-tauri/src/structured_chat/types.rs` (lane G1, frozen contract). Every
 * DTO there serializes camelCase; a part is a flattened envelope `{seq, turnId?, tsMs, type, ...}`.
 *
 * Decoding never throws: a part whose envelope is unusable (no numeric `seq`) is dropped, and a
 * part whose kind is unknown or whose fields do not match the contract is kept as a `raw` part so
 * a newer gateway can never crash an older client.
 */

export const STRUCTURED_CHAT_ROUTE_PREFIX = "/api/v1/structured-chat";

export type StructuredPermissionMode = "supervised" | "auto-accept-edits" | "auto" | "full-access";

/** Ascending grant order, as `PermissionMode::all()` lists it. */
export const STRUCTURED_PERMISSION_MODES: readonly StructuredPermissionMode[] = [
  "supervised",
  "auto-accept-edits",
  "auto",
  "full-access",
];

export type StructuredApprovalDecision = "accept" | "acceptForSession" | "decline";

export type StructuredToolCallStatus = "inProgress" | "completed" | "failed" | "declined" | "cancelled";

export type StructuredSessionState = "running" | "ended";

/**
 * How a queue delivers its rows (`QueueDeliveryMode` in the Rust contract, kebab-case). `all`
 * sends every queued row in order; `one-at-a-time` sends one and waits for the turn to settle.
 */
export type QueueDeliveryMode = "all" | "one-at-a-time";

/** Ascending delivery order, as `QueueDeliveryMode` spells it on the wire. */
export const STRUCTURED_QUEUE_DELIVERY_MODES: readonly QueueDeliveryMode[] = ["all", "one-at-a-time"];

export function isStructuredQueueDeliveryMode(value: unknown): value is QueueDeliveryMode {
  return STRUCTURED_QUEUE_DELIVERY_MODES.some((mode) => mode === value);
}

/**
 * The session's settings as `PUT /settings` stores them. Every field past the first three is
 * optional on the wire (an older gateway omits it), so a decoder only ever adds what it read.
 */
export interface StructuredSessionSettings {
  readonly model?: string | null;
  readonly effort?: string | null;
  readonly permissionMode?: StructuredPermissionMode | null;
  readonly executionMode?: string | null;
  readonly agentName?: string | null;
  readonly fastMode?: boolean | null;
  readonly additionalDirs?: readonly string[];
  readonly steeringMode?: QueueDeliveryMode | null;
  readonly followUpMode?: QueueDeliveryMode | null;
  readonly autoCompaction?: boolean | null;
  readonly autoRetry?: boolean | null;
  readonly compactionMode?: string | null;
}

export interface StructuredCreateSessionBody {
  readonly provider: string;
  readonly cwd: string;
  readonly model?: string | null;
  readonly effort?: string | null;
  readonly permissionMode?: StructuredPermissionMode | null;
}

export interface StructuredSessionSummary {
  readonly sessionId: string;
  readonly provider: string;
  readonly cwd: string;
  readonly state: StructuredSessionState;
  readonly settings: StructuredSessionSettings;
  /**
   * The provider turn currently running, when the gateway names it. Absent means "not reported"
   * (an older gateway), which the UI treats as unknown rather than as idle: the journal's own
   * running probe still decides the turn-in-flight indicator in that case.
   */
  readonly activeTurnId?: string | null;
}

export interface StructuredModelInfo {
  readonly id: string;
  readonly label: string;
  readonly efforts: readonly string[];
}

export interface StructuredCapabilities {
  readonly steer: boolean;
  readonly rollback: boolean;
  /**
   * Optional wire bits a newer gateway advertises. Every one is read as `=== true`, so an older
   * gateway that omits it is simply "does not advertise it" - never an error.
   */
  readonly resume?: boolean;
  readonly revertFiles?: boolean;
  readonly historyPaging?: boolean;
  readonly permissionModes: readonly StructuredPermissionMode[];
  readonly models: readonly StructuredModelInfo[];
  readonly slashCommands: readonly string[];
  /** Execution modes (`executionMode` in the settings) the agent accepts, in its own order. */
  readonly executionModes?: readonly string[];
  /** Named agents a session may run as (`agentName` in the settings). */
  readonly agents?: readonly string[];
  /** Reasoning levels the agent accepts beyond a model's own `efforts`. */
  readonly thinkingLevels?: readonly string[];
  readonly fastMode?: boolean;
  readonly additionalDirs?: boolean;
  readonly queueModes?: boolean;
  readonly compactCustomInstructions?: boolean;
  readonly treeNavigation?: boolean;
  readonly fork?: boolean;
  readonly cloneSession?: boolean;
  readonly editUserMessage?: boolean;
  readonly editAssistantMessage?: boolean;
  readonly sessionRename?: boolean;
  readonly sessionStats?: boolean;
  readonly sessionSwitch?: boolean;
  readonly reload?: boolean;
  readonly media?: boolean;
  readonly extensionUi?: boolean;
  readonly goals?: boolean;
  readonly favoriteModels?: boolean;
  readonly scopedModels?: boolean;
}

export interface StructuredFileChangeEntry {
  readonly path: string;
  readonly kind: string;
  readonly diff: string;
}

export type StructuredPartPayload =
  | { readonly type: "userMessage"; readonly text: string }
  | { readonly type: "assistantMessage"; readonly text: string }
  | { readonly type: "reasoning"; readonly text: string }
  | {
      readonly type: "toolCall";
      readonly id: string;
      readonly name: string;
      readonly status: StructuredToolCallStatus;
      readonly output?: string;
      readonly exitCode?: number;
      readonly durationMs?: number;
    }
  | {
      readonly type: "fileChange";
      readonly id: string;
      readonly status: StructuredToolCallStatus;
      readonly changes: readonly StructuredFileChangeEntry[];
    }
  | {
      readonly type: "approvalRequest";
      readonly requestId: string;
      readonly kind: string;
      readonly summary: string;
      readonly reason?: string;
      readonly answered: boolean;
    }
  | { readonly type: "plan"; readonly text: string }
  | { readonly type: "error"; readonly message: string }
  | {
      readonly type: "usage";
      readonly totalTokens: number;
      readonly inputTokens: number;
      readonly cachedInputTokens: number;
      readonly outputTokens: number;
      readonly contextWindow?: number;
    }
  | { readonly type: "newSession" }
  /** A rollback divider: the conversation above it was taken back `turns` whole turns. */
  | { readonly type: "rollback"; readonly turns: number }
  /**
   * A provider extension asking the reader something. `method` names the shape: `select`,
   * `confirm`, `input`, `editor`, `question` (multi-question) or `set_editor_text` (a prefill the
   * client applies to its own draft and answers at once).
   */
  | {
      readonly type: "extensionUiRequest";
      readonly requestId: string;
      readonly method: string;
      readonly title?: string;
      readonly message?: string;
      readonly placeholder?: string;
      readonly prefill?: string;
      readonly options: readonly string[];
      readonly payload?: Readonly<Record<string, unknown>>;
      readonly answered: boolean;
    }
  | { readonly type: "extensionStatus"; readonly statusKey: string; readonly statusText?: string }
  | {
      readonly type: "extensionWidget";
      readonly widgetKey: string;
      readonly lines: readonly string[];
      readonly placement?: string;
    }
  | {
      readonly type: "subagentProgress";
      readonly taskId: string;
      readonly name: string;
      readonly status: string;
      readonly summary?: string;
    }
  | {
      readonly type: "goalUpdate";
      readonly goalId: string;
      readonly title: string;
      readonly status: string;
      readonly progress?: string;
    }
  /** A tool result's media lives on the host, not in the journal; `media` fetches its bytes. */
  | {
      readonly type: "mediaRef";
      readonly toolCallId: string;
      readonly contentIndex: number;
      readonly mimeType: string;
      readonly alt?: string;
    }
  /** A kind this client does not know, or a known kind whose fields broke the contract. */
  | { readonly type: "raw"; readonly rawType: string; readonly value: Readonly<Record<string, unknown>> };

export type StructuredPartType = StructuredPartPayload["type"];

export type StructuredPart = {
  readonly seq: number;
  readonly turnId?: string;
  readonly tsMs: number;
} & StructuredPartPayload;

type Json = Readonly<Record<string, unknown>>;

function isRecord(value: unknown): value is Json {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function str(record: Json, key: string): string | undefined {
  const value = record[key];
  return typeof value === "string" ? value : undefined;
}

function num(record: Json, key: string): number | undefined {
  const value = record[key];
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function optionalNum(record: Json, key: string): number | undefined | null {
  if (record[key] === undefined || record[key] === null) return undefined;
  const value = num(record, key);
  return value === undefined ? null : value;
}

function optionalStr(record: Json, key: string): string | undefined | null {
  if (record[key] === undefined || record[key] === null) return undefined;
  const value = str(record, key);
  return value === undefined ? null : value;
}

function optionalBool(record: Json, key: string): boolean | undefined | null {
  const value = record[key];
  if (value === undefined || value === null) return undefined;
  return typeof value === "boolean" ? value : null;
}

/** A list of non-empty strings, or `[]` when the field is absent or not a list. */
function strList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((row): row is string => typeof row === "string" && row.trim().length > 0);
}

const TOOL_STATUSES: readonly StructuredToolCallStatus[] = ["inProgress", "completed", "failed", "declined", "cancelled"];

function toolStatus(record: Json): StructuredToolCallStatus | undefined {
  const value = record.status;
  return TOOL_STATUSES.find((status) => status === value);
}

export function isStructuredPermissionMode(value: unknown): value is StructuredPermissionMode {
  return STRUCTURED_PERMISSION_MODES.some((mode) => mode === value);
}

function decodePayload(record: Json): StructuredPartPayload | null {
  const type = str(record, "type");
  switch (type) {
    case "userMessage":
    case "assistantMessage":
    case "reasoning":
    case "plan": {
      const text = str(record, "text");
      return text === undefined ? null : { type, text };
    }
    case "toolCall": {
      const id = str(record, "id");
      const name = str(record, "name");
      const status = toolStatus(record);
      const output = optionalStr(record, "output");
      const exitCode = optionalNum(record, "exitCode");
      const durationMs = optionalNum(record, "durationMs");
      if (id === undefined || name === undefined || status === undefined) return null;
      if (output === null || exitCode === null || durationMs === null) return null;
      return {
        type,
        id,
        name,
        status,
        ...(output === undefined ? {} : { output }),
        ...(exitCode === undefined ? {} : { exitCode }),
        ...(durationMs === undefined ? {} : { durationMs }),
      };
    }
    case "fileChange": {
      const id = str(record, "id");
      const status = toolStatus(record);
      const rows = record.changes;
      if (id === undefined || status === undefined || !Array.isArray(rows)) return null;
      const changes: StructuredFileChangeEntry[] = [];
      for (const row of rows) {
        if (!isRecord(row)) return null;
        const path = str(row, "path");
        const kind = str(row, "kind");
        const diff = str(row, "diff");
        if (path === undefined || kind === undefined || diff === undefined) return null;
        changes.push({ path, kind, diff });
      }
      return { type, id, status, changes };
    }
    case "approvalRequest": {
      const requestId = str(record, "requestId");
      const kind = str(record, "kind");
      const summary = str(record, "summary");
      const reason = optionalStr(record, "reason");
      const answered = record.answered;
      if (requestId === undefined || kind === undefined || summary === undefined) return null;
      if (reason === null || typeof answered !== "boolean") return null;
      return { type, requestId, kind, summary, ...(reason === undefined ? {} : { reason }), answered };
    }
    case "error": {
      const message = str(record, "message");
      return message === undefined ? null : { type, message };
    }
    case "usage": {
      const totalTokens = num(record, "totalTokens");
      const inputTokens = num(record, "inputTokens");
      const cachedInputTokens = num(record, "cachedInputTokens");
      const outputTokens = num(record, "outputTokens");
      const contextWindow = optionalNum(record, "contextWindow");
      if (
        totalTokens === undefined ||
        inputTokens === undefined ||
        cachedInputTokens === undefined ||
        outputTokens === undefined ||
        contextWindow === null
      ) {
        return null;
      }
      return {
        type,
        totalTokens,
        inputTokens,
        cachedInputTokens,
        outputTokens,
        ...(contextWindow === undefined ? {} : { contextWindow }),
      };
    }
    case "newSession":
      return { type };
    case "rollback": {
      const turns = num(record, "turns");
      return turns === undefined ? null : { type, turns };
    }
    case "extensionUiRequest": {
      const requestId = str(record, "requestId");
      const method = str(record, "method");
      const answered = record.answered;
      const title = optionalStr(record, "title");
      const message = optionalStr(record, "message");
      const placeholder = optionalStr(record, "placeholder");
      const prefill = optionalStr(record, "prefill");
      const payload = record.payload;
      if (requestId === undefined || method === undefined || typeof answered !== "boolean") return null;
      if (title === null || message === null || placeholder === null || prefill === null) return null;
      if (payload !== undefined && payload !== null && !isRecord(payload)) return null;
      return {
        type,
        requestId,
        method,
        options: strList(record.options),
        answered,
        ...(title === undefined ? {} : { title }),
        ...(message === undefined ? {} : { message }),
        ...(placeholder === undefined ? {} : { placeholder }),
        ...(prefill === undefined ? {} : { prefill }),
        ...(isRecord(payload) ? { payload } : {}),
      };
    }
    case "extensionStatus": {
      const statusKey = str(record, "statusKey");
      const statusText = optionalStr(record, "statusText");
      if (statusKey === undefined || statusText === null) return null;
      return { type, statusKey, ...(statusText === undefined ? {} : { statusText }) };
    }
    case "extensionWidget": {
      const widgetKey = str(record, "widgetKey");
      const placement = optionalStr(record, "placement");
      if (widgetKey === undefined || placement === null) return null;
      return {
        type,
        widgetKey,
        lines: strList(record.lines),
        ...(placement === undefined ? {} : { placement }),
      };
    }
    case "subagentProgress": {
      const taskId = str(record, "taskId");
      const name = str(record, "name");
      const status = str(record, "status");
      const summary = optionalStr(record, "summary");
      if (taskId === undefined || name === undefined || status === undefined || summary === null) return null;
      return { type, taskId, name, status, ...(summary === undefined ? {} : { summary }) };
    }
    case "goalUpdate": {
      const goalId = str(record, "goalId");
      const title = str(record, "title");
      const status = str(record, "status");
      const progress = optionalStr(record, "progress");
      if (goalId === undefined || title === undefined || status === undefined || progress === null) return null;
      return { type, goalId, title, status, ...(progress === undefined ? {} : { progress }) };
    }
    case "mediaRef": {
      const toolCallId = str(record, "toolCallId");
      const contentIndex = num(record, "contentIndex");
      const mimeType = str(record, "mimeType");
      const alt = optionalStr(record, "alt");
      if (toolCallId === undefined || contentIndex === undefined || mimeType === undefined) return null;
      if (alt === null || !Number.isInteger(contentIndex) || contentIndex < 0) return null;
      return {
        type,
        toolCallId,
        contentIndex,
        mimeType,
        ...(alt === undefined ? {} : { alt }),
      };
    }
    default:
      return null;
  }
}

/** One part off the wire; `null` only when the envelope itself (`seq`) is unusable. */
export function decodeStructuredPart(value: unknown): StructuredPart | null {
  if (!isRecord(value)) return null;
  const seq = num(value, "seq");
  if (seq === undefined || seq < 0 || !Number.isInteger(seq)) return null;
  const tsMs = num(value, "tsMs") ?? 0;
  const turnId = str(value, "turnId");
  const envelope = { seq, tsMs, ...(turnId === undefined ? {} : { turnId }) };
  const payload = decodePayload(value);
  if (payload !== null) return { ...envelope, ...payload };
  const { seq: _seq, tsMs: _ts, turnId: _turn, type: _type, ...rest } = value;
  return { ...envelope, type: "raw", rawType: str(value, "type") ?? "unknown", value: rest };
}

/** The `{parts}` body of the history and events reads. */
export function decodeStructuredParts(body: unknown): StructuredPart[] {
  const rows = isRecord(body) ? body.parts : body;
  if (!Array.isArray(rows)) return [];
  const parts: StructuredPart[] = [];
  for (const row of rows) {
    const part = decodeStructuredPart(row);
    if (part !== null) parts.push(part);
  }
  return parts;
}

export function decodeStructuredSettings(value: unknown): StructuredSessionSettings {
  if (!isRecord(value)) return {};
  const model = str(value, "model");
  const effort = str(value, "effort");
  const permissionMode = isStructuredPermissionMode(value.permissionMode) ? value.permissionMode : undefined;
  const executionMode = optionalStr(value, "executionMode");
  const agentName = optionalStr(value, "agentName");
  const fastMode = optionalBool(value, "fastMode");
  const steeringMode = optionalStr(value, "steeringMode");
  const followUpMode = optionalStr(value, "followUpMode");
  const autoCompaction = optionalBool(value, "autoCompaction");
  const autoRetry = optionalBool(value, "autoRetry");
  const compactionMode = optionalStr(value, "compactionMode");
  const additionalDirs = value.additionalDirs === undefined ? undefined : strList(value.additionalDirs);
  return {
    ...(model === undefined ? {} : { model }),
    ...(effort === undefined ? {} : { effort }),
    ...(permissionMode === undefined ? {} : { permissionMode }),
    ...(executionMode === undefined || executionMode === null ? {} : { executionMode }),
    ...(agentName === undefined || agentName === null ? {} : { agentName }),
    ...(fastMode === undefined || fastMode === null ? {} : { fastMode }),
    ...(additionalDirs === undefined || additionalDirs.length === 0 ? {} : { additionalDirs }),
    ...(isStructuredQueueDeliveryMode(steeringMode) ? { steeringMode } : {}),
    ...(isStructuredQueueDeliveryMode(followUpMode) ? { followUpMode } : {}),
    ...(autoCompaction === undefined || autoCompaction === null ? {} : { autoCompaction }),
    ...(autoRetry === undefined || autoRetry === null ? {} : { autoRetry }),
    ...(compactionMode === undefined || compactionMode === null ? {} : { compactionMode }),
  };
}

export function decodeStructuredSession(value: unknown): StructuredSessionSummary | null {
  if (!isRecord(value)) return null;
  const sessionId = str(value, "sessionId");
  const provider = str(value, "provider");
  const cwd = str(value, "cwd");
  const state = value.state === "running" || value.state === "ended" ? value.state : null;
  if (sessionId === undefined || provider === undefined || cwd === undefined || state === null) return null;
  // `activeTurnId` is optional on the wire: a gateway that does not name the running turn leaves
  // it out, and the caller must read that as "not reported", never as "no turn is running".
  const activeTurnId = optionalStr(value, "activeTurnId");
  return {
    sessionId,
    provider,
    cwd,
    state,
    settings: decodeStructuredSettings(value.settings),
    ...(activeTurnId === undefined || activeTurnId === null ? {} : { activeTurnId }),
  };
}

export function decodeStructuredSessions(body: unknown): StructuredSessionSummary[] {
  const rows = isRecord(body) ? body.sessions : null;
  if (!Array.isArray(rows)) return [];
  return rows.map(decodeStructuredSession).filter((row): row is StructuredSessionSummary => row !== null);
}

export function decodeStructuredCapabilities(value: unknown): StructuredCapabilities | null {
  if (!isRecord(value)) return null;
  if (typeof value.steer !== "boolean" || typeof value.rollback !== "boolean") return null;
  const modes = Array.isArray(value.permissionModes) ? value.permissionModes.filter(isStructuredPermissionMode) : [];
  const models: StructuredModelInfo[] = [];
  for (const row of Array.isArray(value.models) ? value.models : []) {
    if (!isRecord(row)) continue;
    const id = str(row, "id");
    if (id === undefined || id.length === 0) continue;
    const efforts = Array.isArray(row.efforts)
      ? row.efforts.filter((effort): effort is string => typeof effort === "string" && effort.length > 0)
      : [];
    models.push({ id, label: str(row, "label") ?? id, efforts });
  }
  const slashCommands = Array.isArray(value.slashCommands)
    ? value.slashCommands.filter((name): name is string => typeof name === "string" && name.trim().length > 0)
    : [];
  // The optional bits a newer gateway advertises; an older one simply omits them, which reads as
  // "not advertised" (false) rather than as a malformed reply.
  const bit = (key: string): boolean | undefined => (value[key] === true ? true : undefined);
  const optionalList = (key: string): readonly string[] | undefined => {
    if (value[key] === undefined) return undefined;
    const list = strList(value[key]);
    return list.length === 0 ? undefined : list;
  };
  const executionModes = optionalList("executionModes");
  const agents = optionalList("agents");
  const thinkingLevels = optionalList("thinkingLevels");
  return {
    steer: value.steer,
    rollback: value.rollback,
    permissionModes: modes,
    models,
    slashCommands,
    ...(typeof value.resume === "boolean" ? { resume: value.resume } : {}),
    ...(typeof value.revertFiles === "boolean" ? { revertFiles: value.revertFiles } : {}),
    ...(typeof value.historyPaging === "boolean" ? { historyPaging: value.historyPaging } : {}),
    ...(executionModes === undefined ? {} : { executionModes }),
    ...(agents === undefined ? {} : { agents }),
    ...(thinkingLevels === undefined ? {} : { thinkingLevels }),
    ...(bit("fastMode") === undefined ? {} : { fastMode: true }),
    ...(bit("additionalDirs") === undefined ? {} : { additionalDirs: true }),
    ...(bit("queueModes") === undefined ? {} : { queueModes: true }),
    ...(bit("compactCustomInstructions") === undefined ? {} : { compactCustomInstructions: true }),
    ...(bit("treeNavigation") === undefined ? {} : { treeNavigation: true }),
    ...(bit("fork") === undefined ? {} : { fork: true }),
    ...(bit("cloneSession") === undefined ? {} : { cloneSession: true }),
    ...(bit("editUserMessage") === undefined ? {} : { editUserMessage: true }),
    ...(bit("editAssistantMessage") === undefined ? {} : { editAssistantMessage: true }),
    ...(bit("sessionRename") === undefined ? {} : { sessionRename: true }),
    ...(bit("sessionStats") === undefined ? {} : { sessionStats: true }),
    ...(bit("sessionSwitch") === undefined ? {} : { sessionSwitch: true }),
    ...(bit("reload") === undefined ? {} : { reload: true }),
    ...(bit("media") === undefined ? {} : { media: true }),
    ...(bit("extensionUi") === undefined ? {} : { extensionUi: true }),
    ...(bit("goals") === undefined ? {} : { goals: true }),
    ...(bit("favoriteModels") === undefined ? {} : { favoriteModels: true }),
    ...(bit("scopedModels") === undefined ? {} : { scopedModels: true }),
  };
}

/** The `seq` a turn, steer or clear returned. */
export function decodeStructuredSeq(body: unknown): number | null {
  if (!isRecord(body)) return null;
  const seq = num(body, "seq");
  return seq === undefined ? null : seq;
}

/** `/api/v1/structured-chat/sessions[/<id>[/<suffix>]]` with every segment URL-encoded. */
export function structuredChatRoute(sessionId?: string, ...segments: readonly string[]): string {
  const parts = [STRUCTURED_CHAT_ROUTE_PREFIX, "sessions"];
  if (sessionId !== undefined) parts.push(encodeURIComponent(sessionId));
  for (const segment of segments) parts.push(encodeURIComponent(segment));
  return parts.join("/");
}

/**
 * The machine codes the routes and the IPC command carry beside a failure
 * (`StructuredChatError::code`). `FORBIDDEN` is the gateway's device-permission refusal and
 * `OPERATION_OUTCOME_UNKNOWN` is a mutation that may have landed but was never confirmed.
 */
export const STRUCTURED_CHAT_INVALID_REQUEST = "INVALID_REQUEST";
export const STRUCTURED_CHAT_NOT_FOUND = "NOT_FOUND";
export const STRUCTURED_CHAT_CONFLICT = "CONFLICT";
export const STRUCTURED_CHAT_STEER_UNSUPPORTED = "STEER_UNSUPPORTED";
export const STRUCTURED_CHAT_COMPACT_UNSUPPORTED = "COMPACT_UNSUPPORTED";
export const STRUCTURED_CHAT_PROVIDER_FAILED = "PROVIDER_FAILED";
export const STRUCTURED_CHAT_FORBIDDEN = "FORBIDDEN";
export const STRUCTURED_CHAT_OUTCOME_UNKNOWN = "OPERATION_OUTCOME_UNKNOWN";

/** Did this failure leave the mutation's outcome unknown, so it must not be replayed on its own? */
export function structuredChatOutcomeUnknown(code: string): boolean {
  return code === STRUCTURED_CHAT_OUTCOME_UNKNOWN;
}

/**
 * One `{parts}` read (history or events) plus the cursor past it. `nextSeq` is the highest `seq`
 * in the page, or the requested cursor when the page was empty, so a poll that comes back empty
 * keeps asking from the same place.
 */
export interface StructuredPageRead {
  readonly parts: readonly StructuredPart[];
  readonly nextSeq: number;
  /**
   * `history` serves a bounded window: older parts exist below this page, and `nextBefore` is the
   * `before=` cursor that walks back to them (`HistoryPage` in the Rust store). An `events` read
   * carries neither, so both are absent there.
   */
  readonly hasOlder?: boolean;
  readonly nextBefore?: number | null;
}

export function decodeStructuredPage(body: unknown, after = 0): StructuredPageRead {
  const parts = decodeStructuredParts(body);
  const page = isRecord(body) ? body : null;
  const nextBefore = page === null ? undefined : optionalNum(page, "nextBefore");
  return {
    parts,
    nextSeq: structuredHighestSeq(parts, after),
    ...(page !== null && typeof page.hasOlder === "boolean" ? { hasOlder: page.hasOlder } : {}),
    ...(nextBefore === undefined || nextBefore === null ? {} : { nextBefore }),
  };
}

/** The highest `seq` in `parts`, never below `floor`. */
export function structuredHighestSeq(parts: readonly StructuredPart[], floor = 0): number {
  let highest = floor;
  for (const part of parts) if (part.seq > highest) highest = part.seq;
  return highest;
}

/** Every part past `seq`, ascending. The routes already filter; a merged page may not. */
export function structuredPartsAfter(parts: readonly StructuredPart[], seq: number): readonly StructuredPart[] {
  return parts.filter((part) => part.seq > seq);
}

/**
 * Merge two pages of the same append-only stream: parts are keyed by `seq`, the newer copy of a
 * `seq` wins (a later read can complete a part an earlier long-poll caught mid-write), and the
 * result is ascending. A replayed or overlapping page therefore never duplicates a row.
 */
/** `seq` strictly increasing: the shape both a journal read and a merge result have. */
function strictlyAscending(parts: readonly StructuredPart[]): boolean {
  for (let index = 1; index < parts.length; index += 1) {
    if (parts[index].seq <= parts[index - 1].seq) return false;
  }
  return true;
}

export function mergeStructuredParts(
  current: readonly StructuredPart[],
  incoming: readonly StructuredPart[],
): readonly StructuredPart[] {
  if (incoming.length === 0) return current;
  // Append-only fast path: an `after=cursor` poll carries only seqs above everything held, so the
  // merge is one array copy instead of a Map build plus a full sort on every poll.
  if (
    strictlyAscending(current) &&
    strictlyAscending(incoming) &&
    (current.length === 0 || incoming[0].seq > current[current.length - 1].seq)
  ) {
    return current.length === 0 ? incoming : [...current, ...incoming];
  }
  const bySeq = new Map<number, StructuredPart>();
  for (const part of current) bySeq.set(part.seq, part);
  // A replayed page that carries the very objects already held changes nothing: hand back the
  // reference the caller's memo already compares against instead of a rebuilt array.
  let changed = false;
  for (const part of incoming) {
    if (bySeq.get(part.seq) === part) continue;
    bySeq.set(part.seq, part);
    changed = true;
  }
  if (!changed) return current;
  return [...bySeq.values()].sort((left, right) => left.seq - right.seq);
}

/**
 * Parts grouped into provider turns. A part whose envelope carries no `turnId` (a `newSession`
 * divider, a session-level error) is its own group, keyed by its `seq`, so grouping never moves
 * a row between turns.
 */
export interface StructuredTurnGroup {
  readonly key: string;
  readonly turnId: string | null;
  readonly parts: readonly StructuredPart[];
}

export function groupStructuredTurns(parts: readonly StructuredPart[]): StructuredTurnGroup[] {
  const groups: { key: string; turnId: string | null; parts: StructuredPart[] }[] = [];
  for (const part of parts) {
    const turnId = part.turnId ?? null;
    const last = groups.length === 0 ? undefined : groups[groups.length - 1];
    if (last !== undefined && turnId !== null && last.turnId === turnId) {
      last.parts.push(part);
      continue;
    }
    groups.push({ key: turnId ?? `seq:${part.seq}`, turnId, parts: [part] });
  }
  return groups;
}

/** The decision order the approval card offers; index `n` of the card answers `n` of these. */
export const STRUCTURED_APPROVAL_DECISIONS: readonly StructuredApprovalDecision[] = [
  "accept",
  "acceptForSession",
  "decline",
];

export function structuredApprovalDecisionAt(index: number): StructuredApprovalDecision | null {
  return STRUCTURED_APPROVAL_DECISIONS[index] ?? null;
}

/** One approval the provider is still waiting on. */
export interface StructuredPendingApproval {
  readonly requestId: string;
  readonly kind: string;
  readonly summary: string;
  readonly reason?: string;
  readonly seq: number;
  readonly tsMs: number;
}

/**
 * Approvals still open, in stream order, each named once. An `approvalRequest` that a later part
 * of the same `requestId` marked answered is not pending even if its first row said otherwise,
 * so a long-poll that catches the answer out of order still retires the card.
 */
export function structuredPendingApprovals(parts: readonly StructuredPart[]): StructuredPendingApproval[] {
  const answered = new Set<string>();
  for (const part of parts) {
    if (part.type === "approvalRequest" && part.answered) answered.add(part.requestId);
  }
  const seen = new Set<string>();
  const rows: StructuredPendingApproval[] = [];
  for (const part of parts) {
    if (part.type !== "approvalRequest" || part.answered) continue;
    if (answered.has(part.requestId) || seen.has(part.requestId)) continue;
    seen.add(part.requestId);
    rows.push({
      requestId: part.requestId,
      kind: part.kind,
      summary: part.summary,
      ...(part.reason === undefined ? {} : { reason: part.reason }),
      seq: part.seq,
      tsMs: part.tsMs,
    });
  }
  return rows;
}

/** The most recent provider token counts in the stream, or `null` when none was recorded. */
export interface StructuredUsageTotal {
  readonly totalTokens: number;
  readonly inputTokens: number;
  readonly cachedInputTokens: number;
  readonly outputTokens: number;
  readonly contextWindow?: number;
}

export function structuredLatestUsage(parts: readonly StructuredPart[]): StructuredUsageTotal | null {
  for (let index = parts.length - 1; index >= 0; index -= 1) {
    const part = parts[index];
    if (part.type !== "usage") continue;
    return {
      totalTokens: part.totalTokens,
      inputTokens: part.inputTokens,
      cachedInputTokens: part.cachedInputTokens,
      outputTokens: part.outputTokens,
      ...(part.contextWindow === undefined ? {} : { contextWindow: part.contextWindow }),
    };
  }
  return null;
}

/**
 * Whole provider turns a rollback starting at `seq` has to walk back: the target part's turn and
 * every turn after it. A part that belongs to no turn (a divider, a session-level error) is not
 * a turn and is not counted, so a target sitting in one walks back the turns that follow it.
 * A `seq` the stream does not hold is `0`.
 */
export function structuredTurnsBackTo(parts: readonly StructuredPart[], seq: number): number {
  const groups = groupStructuredTurns(parts);
  let position = -1;
  for (let index = 0; index < groups.length; index += 1) {
    if (groups[index].parts.some((part) => part.seq === seq)) {
      position = index;
      break;
    }
  }
  if (position < 0) return 0;
  return groups.slice(position).filter((group) => group.turnId !== null).length;
}

/**
 * One slash command the agent lists (`SlashCommandItem`). `syntax` says which trigger opens it:
 * `slash` is `/`, `dollar` is `$` (a skill). `source` names where it came from (`builtin`,
 * `skill`, `agent`, …); the client never invents one.
 */
export interface StructuredSlashCommand {
  readonly name: string;
  readonly description?: string;
  readonly source: string;
  readonly syntax: string;
}

/** One node of the session tree (`SessionTreeNode`): a branch the reader can navigate to. */
export interface StructuredSessionTreeNode {
  readonly id: string;
  readonly parentId?: string;
  readonly role: string;
  readonly preview: string;
  readonly label?: string;
  readonly active: boolean;
}

/** The whole tree plus the leaf the session is on (`SessionTree`). */
export interface StructuredSessionTree {
  readonly currentLeafId?: string;
  readonly nodes: readonly StructuredSessionTreeNode[];
}

/** Token and turn totals for one session (`SessionStats`). */
export interface StructuredSessionStats {
  readonly name?: string;
  readonly turns: number;
  readonly totalTokens: number;
  readonly inputTokens: number;
  readonly outputTokens: number;
  readonly contextWindow?: number;
  readonly costUsd?: number;
}

/** Bytes of one tool-result media item (`MediaBlob`), base64 as the host stores them. */
export interface StructuredMediaBlob {
  readonly mimeType: string;
  readonly dataBase64: string;
}

/** What `fork` and `clone` answer (`ForkResult`): the session and the thread it now owns. */
export interface StructuredForkResult {
  readonly sessionId: string;
  readonly threadId?: string;
}

/**
 * A client's answer to one `extensionUiRequest` (`ExtensionUiResponseBody`). Exactly one shape is
 * meaningful per request method: `value` for `select`/`input`/`editor`, `confirmed` for
 * `confirm`, `answers` for `question`, `cancelled` for a dismissal of any of them.
 */
export interface StructuredExtensionUiResponse {
  readonly value?: string;
  readonly confirmed?: boolean;
  readonly cancelled?: boolean;
  readonly answers?: Readonly<Record<string, unknown>>;
  readonly comment?: string;
}

/** The model lists a reader pins for the session (`ModelPreferencesBody`). */
export interface StructuredModelPreferences {
  readonly favoriteModels?: readonly string[];
  readonly scopedModels?: readonly string[];
}

/** The methods an `extensionUiRequest` may name; anything else is not a card this client draws. */
export const STRUCTURED_EXTENSION_UI_METHODS = [
  "select",
  "confirm",
  "input",
  "editor",
  "question",
  "set_editor_text",
] as const;

export type StructuredExtensionUiMethod = (typeof STRUCTURED_EXTENSION_UI_METHODS)[number];

/** `set_editor_text` asks the client to prefill its own draft; it is never a card. */
export function structuredExtensionUiPrefillsDraft(method: string): boolean {
  return method === "set_editor_text";
}

export function decodeStructuredCommands(body: unknown): StructuredSlashCommand[] {
  const rows = isRecord(body) ? body.commands : body;
  if (!Array.isArray(rows)) return [];
  const commands: StructuredSlashCommand[] = [];
  for (const row of rows) {
    if (!isRecord(row)) continue;
    const name = str(row, "name");
    if (name === undefined || name.trim().length === 0) continue;
    const description = optionalStr(row, "description");
    if (description === null) continue;
    commands.push({
      name,
      source: str(row, "source") ?? "builtin",
      syntax: str(row, "syntax") ?? "slash",
      ...(description === undefined ? {} : { description }),
    });
  }
  return commands;
}

/** A tree, or `null` when the body carries no `nodes` list at all (a malformed reply). */
export function decodeStructuredTree(value: unknown): StructuredSessionTree | null {
  if (!isRecord(value) || !Array.isArray(value.nodes)) return null;
  const nodes: StructuredSessionTreeNode[] = [];
  for (const row of value.nodes) {
    if (!isRecord(row)) continue;
    const id = str(row, "id");
    if (id === undefined) continue;
    const parentId = optionalStr(row, "parentId");
    const label = optionalStr(row, "label");
    if (parentId === null || label === null) continue;
    nodes.push({
      id,
      role: str(row, "role") ?? "assistant",
      preview: str(row, "preview") ?? "",
      active: row.active === true,
      ...(parentId === undefined ? {} : { parentId }),
      ...(label === undefined ? {} : { label }),
    });
  }
  const currentLeafId = optionalStr(value, "currentLeafId");
  return {
    nodes,
    ...(currentLeafId === undefined || currentLeafId === null ? {} : { currentLeafId }),
  };
}

/** Session stats; a body without the four counters is not one this client will draw. */
export function decodeStructuredStats(value: unknown): StructuredSessionStats | null {
  if (!isRecord(value)) return null;
  const turns = num(value, "turns");
  const totalTokens = num(value, "totalTokens");
  const inputTokens = num(value, "inputTokens");
  const outputTokens = num(value, "outputTokens");
  if (
    turns === undefined ||
    totalTokens === undefined ||
    inputTokens === undefined ||
    outputTokens === undefined
  ) {
    return null;
  }
  const name = optionalStr(value, "name");
  const contextWindow = optionalNum(value, "contextWindow");
  const costUsd = optionalNum(value, "costUsd");
  if (name === null || contextWindow === null || costUsd === null) return null;
  return {
    turns,
    totalTokens,
    inputTokens,
    outputTokens,
    ...(name === undefined ? {} : { name }),
    ...(contextWindow === undefined ? {} : { contextWindow }),
    ...(costUsd === undefined ? {} : { costUsd }),
  };
}

/** A media blob; a body without base64 bytes is not a picture the client may draw. */
export function decodeStructuredMedia(value: unknown): StructuredMediaBlob | null {
  if (!isRecord(value)) return null;
  const mimeType = str(value, "mimeType");
  const dataBase64 = str(value, "dataBase64");
  if (mimeType === undefined || dataBase64 === undefined) return null;
  return { mimeType, dataBase64 };
}

/** A fork/clone answer. `sessionId` is required: a result naming no session is unusable. */
export function decodeStructuredFork(value: unknown): StructuredForkResult | null {
  if (!isRecord(value)) return null;
  const sessionId = str(value, "sessionId");
  if (sessionId === undefined) return null;
  const threadId = optionalStr(value, "threadId");
  if (threadId === null) return null;
  return { sessionId, ...(threadId === undefined ? {} : { threadId }) };
}

/** One open `extensionUiRequest`, in stream order, each `requestId` named once. */
export interface StructuredExtensionUiRequest {
  readonly requestId: string;
  readonly method: string;
  readonly title?: string;
  readonly message?: string;
  readonly placeholder?: string;
  readonly prefill?: string;
  readonly options: readonly string[];
  readonly payload?: Readonly<Record<string, unknown>>;
  readonly seq: number;
  readonly tsMs: number;
}

/**
 * The extension requests still waiting on the reader, in stream order.
 *
 * `set_editor_text` is filtered out by the caller (it is a draft prefill, not a question), so
 * this returns every unanswered request and leaves that decision where the draft lives. A
 * request a later part marked answered is retired here even if its first row said otherwise,
 * for the same reason `structuredPendingApprovals` retires an approval.
 */
export function structuredOpenExtensionRequests(
  parts: readonly StructuredPart[],
): StructuredExtensionUiRequest[] {
  const answered = new Set<string>();
  for (const part of parts) {
    if (part.type === "extensionUiRequest" && part.answered) answered.add(part.requestId);
  }
  const seen = new Set<string>();
  const rows: StructuredExtensionUiRequest[] = [];
  for (const part of parts) {
    if (part.type !== "extensionUiRequest" || part.answered) continue;
    if (answered.has(part.requestId) || seen.has(part.requestId)) continue;
    seen.add(part.requestId);
    rows.push({
      requestId: part.requestId,
      method: part.method,
      options: part.options,
      seq: part.seq,
      tsMs: part.tsMs,
      ...(part.title === undefined ? {} : { title: part.title }),
      ...(part.message === undefined ? {} : { message: part.message }),
      ...(part.placeholder === undefined ? {} : { placeholder: part.placeholder }),
      ...(part.prefill === undefined ? {} : { prefill: part.prefill }),
      ...(part.payload === undefined ? {} : { payload: part.payload }),
    });
  }
  return rows;
}

/** The latest `set_editor_text` prefill still open, which the composer takes as a draft. */
export function structuredEditorPrefill(parts: readonly StructuredPart[]): string | null {
  const open = structuredOpenExtensionRequests(parts).filter((request) =>
    structuredExtensionUiPrefillsDraft(request.method),
  );
  const last = open[open.length - 1];
  if (last === undefined) return null;
  return last.prefill ?? last.message ?? null;
}

/** One `extensionStatus` pill, latest value per key (a key is re-stamped, never duplicated). */
export interface StructuredExtensionStatus {
  readonly statusKey: string;
  readonly statusText: string;
  readonly seq: number;
}

export function structuredExtensionStatuses(parts: readonly StructuredPart[]): StructuredExtensionStatus[] {
  const byKey = new Map<string, StructuredExtensionStatus>();
  for (const part of parts) {
    if (part.type !== "extensionStatus") continue;
    byKey.set(part.statusKey, {
      statusKey: part.statusKey,
      statusText: part.statusText ?? part.statusKey,
      seq: part.seq,
    });
  }
  return [...byKey.values()];
}

/** One `extensionWidget` block, latest value per key. */
export interface StructuredExtensionWidget {
  readonly widgetKey: string;
  readonly lines: readonly string[];
  readonly placement?: string;
  readonly seq: number;
}

export function structuredExtensionWidgets(parts: readonly StructuredPart[]): StructuredExtensionWidget[] {
  const byKey = new Map<string, StructuredExtensionWidget>();
  for (const part of parts) {
    if (part.type !== "extensionWidget") continue;
    byKey.set(part.widgetKey, {
      widgetKey: part.widgetKey,
      lines: part.lines,
      seq: part.seq,
      ...(part.placement === undefined ? {} : { placement: part.placement }),
    });
  }
  return [...byKey.values()];
}

/** One subagent's latest progress, keyed by `taskId`. */
export interface StructuredSubagentProgress {
  readonly taskId: string;
  readonly name: string;
  readonly status: string;
  readonly summary?: string;
  readonly seq: number;
}

export function structuredSubagentProgress(
  parts: readonly StructuredPart[],
): StructuredSubagentProgress[] {
  const byTask = new Map<string, StructuredSubagentProgress>();
  for (const part of parts) {
    if (part.type !== "subagentProgress") continue;
    byTask.set(part.taskId, {
      taskId: part.taskId,
      name: part.name,
      status: part.status,
      seq: part.seq,
      ...(part.summary === undefined ? {} : { summary: part.summary }),
    });
  }
  return [...byTask.values()];
}

/** One goal's latest state, keyed by `goalId`. */
export interface StructuredGoalUpdate {
  readonly goalId: string;
  readonly title: string;
  readonly status: string;
  readonly progress?: string;
  readonly seq: number;
}

export function structuredGoalUpdates(parts: readonly StructuredPart[]): StructuredGoalUpdate[] {
  const byGoal = new Map<string, StructuredGoalUpdate>();
  for (const part of parts) {
    if (part.type !== "goalUpdate") continue;
    byGoal.set(part.goalId, {
      goalId: part.goalId,
      title: part.title,
      status: part.status,
      seq: part.seq,
      ...(part.progress === undefined ? {} : { progress: part.progress }),
    });
  }
  return [...byGoal.values()];
}

/** One media item a tool result refers to; its bytes are a separate `media` read. */
export interface StructuredMediaRef {
  readonly toolCallId: string;
  readonly contentIndex: number;
  readonly mimeType: string;
  readonly alt?: string;
  readonly seq: number;
}

/** Every distinct media reference in the stream, in stream order. */
export function structuredMediaRefs(parts: readonly StructuredPart[]): StructuredMediaRef[] {
  const seen = new Set<string>();
  const rows: StructuredMediaRef[] = [];
  for (const part of parts) {
    if (part.type !== "mediaRef") continue;
    const key = `${part.toolCallId}:${part.contentIndex}`;
    if (seen.has(key)) continue;
    seen.add(key);
    rows.push({
      toolCallId: part.toolCallId,
      contentIndex: part.contentIndex,
      mimeType: part.mimeType,
      seq: part.seq,
      ...(part.alt === undefined ? {} : { alt: part.alt }),
    });
  }
  return rows;
}
