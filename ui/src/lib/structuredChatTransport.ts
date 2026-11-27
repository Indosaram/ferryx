/**
 * Structured agent chat transport (lane G7, chat-t3-parity plan).
 *
 * One typed contract over the two surfaces lane G1 published: the Tauri command
 * `cmd_structured_chat` (desktop) and the `/api/v1/structured-chat` routes (remote). Both take
 * the same `{op, sessionId?, body?}` request, so the framing lives here once and each adapter
 * only differs in how the bytes leave the process.
 *
 * Nothing throws on a wire result: a refusal is a typed `{ok:false, code, message}`, and a
 * mutation that threw without an answer is reported as `OPERATION_OUTCOME_UNKNOWN` because it
 * may already have landed.
 */
import {
  STRUCTURED_CHAT_OUTCOME_UNKNOWN,
  STRUCTURED_CHAT_ROUTE_PREFIX,
  STRUCTURED_PERMISSION_MODES,
  decodeStructuredCapabilities,
  decodeStructuredCommands,
  decodeStructuredFork,
  decodeStructuredMedia,
  decodeStructuredPage,
  decodeStructuredSeq,
  decodeStructuredSession,
  decodeStructuredSessions,
  decodeStructuredSettings,
  decodeStructuredStats,
  decodeStructuredTree,
  groupStructuredTurns,
  mergeStructuredParts,
  structuredHighestSeq,
  structuredTurnsBackTo,
  type StructuredCapabilities,
  type StructuredCreateSessionBody,
  type StructuredExtensionUiResponse,
  type StructuredForkResult,
  type StructuredMediaBlob,
  type StructuredModelPreferences,
  type StructuredPart,
  type StructuredPermissionMode,
  type StructuredSessionSettings,
  type StructuredSessionStats,
  type StructuredSessionSummary,
  type StructuredSessionTree,
  type StructuredSlashCommand,
} from "./structuredChatTypes";

/**
 * The failure codes a caller switches on. They are defined with the wire types and re-exported
 * here because a caller of this module should not have to know which half of the contract a code
 * lives in.
 */
export {
  STRUCTURED_CHAT_INVALID_REQUEST,
  STRUCTURED_CHAT_NOT_FOUND,
  STRUCTURED_CHAT_CONFLICT,
  STRUCTURED_CHAT_STEER_UNSUPPORTED,
  STRUCTURED_CHAT_COMPACT_UNSUPPORTED,
  STRUCTURED_CHAT_PROVIDER_FAILED,
  STRUCTURED_CHAT_FORBIDDEN,
  STRUCTURED_CHAT_OUTCOME_UNKNOWN,
  structuredChatOutcomeUnknown,
} from "./structuredChatTypes";

/** Every operation the routes and the IPC command mirror, op for op. */
export type StructuredChatOp =
  | "createSession"
  | "listSessions"
  | "turn"
  | "history"
  | "events"
  | "steer"
  | "interrupt"
  | "rollback"
  | "approve"
  | "settings"
  | "capabilities"
  | "clear"
  | "compact"
  | "resume"
  | "closeSession"
  | "commands"
  | "answerExtensionUi"
  | "tree"
  | "navigateTree"
  | "fork"
  | "clone"
  | "editMessage"
  | "rename"
  | "stats"
  | "reload"
  | "media"
  | "clearQueue"
  | "modelPreferences";

/** Ops that act on one existing session. */
export type StructuredChatSessionOp = Exclude<StructuredChatOp, "createSession" | "listSessions">;

export interface StructuredChatAttachment {
  readonly path: string;
}

export interface StructuredChatRequests {
  readonly createSession: {
    readonly body: StructuredCreateSessionBody;
    readonly data: StructuredSessionSummary;
  };
  readonly listSessions: {
    readonly body: Record<string, never>;
    readonly data: readonly StructuredSessionSummary[];
  };
  readonly turn: {
    readonly body: { readonly text: string; readonly attachments?: readonly StructuredChatAttachment[] };
    readonly data: number;
  };
  readonly history: {
    readonly body: { readonly after: number; readonly before?: number; readonly limit?: number };
    readonly data: StructuredPartPage;
  };
  readonly events: { readonly body: { readonly after: number }; readonly data: StructuredPartPage };
  readonly steer: { readonly body: { readonly text: string }; readonly data: number };
  readonly interrupt: { readonly body: Record<string, never>; readonly data: void };
  readonly rollback: {
    /** `revertFiles` is sent only when the agent advertises it (`Capabilities.revertFiles`). */
    readonly body: { readonly turns: number; readonly revertFiles?: boolean };
    readonly data: void;
  };
  readonly approve: {
    readonly body: { readonly requestId: string; readonly decision: StructuredChatApprovalDecision };
    readonly data: void;
  };
  readonly settings: { readonly body: StructuredSessionSettings; readonly data: StructuredSessionSettings };
  readonly capabilities: { readonly body: Record<string, never>; readonly data: StructuredCapabilities };
  readonly clear: { readonly body: Record<string, never>; readonly data: number };
  readonly compact: {
    /** `customInstructions` rides only where the agent advertises it. */
    readonly body: { readonly customInstructions?: string };
    readonly data: void;
  };
  readonly resume: { readonly body: Record<string, never>; readonly data: void };
  readonly closeSession: { readonly body: Record<string, never>; readonly data: void };
  readonly commands: { readonly body: Record<string, never>; readonly data: readonly StructuredSlashCommand[] };
  readonly answerExtensionUi: {
    readonly body: { readonly requestId: string } & StructuredExtensionUiResponse;
    readonly data: void;
  };
  readonly tree: { readonly body: Record<string, never>; readonly data: StructuredSessionTree };
  readonly navigateTree: {
    readonly body: {
      readonly targetId: string;
      readonly summarize?: boolean;
      readonly customInstructions?: string;
    };
    readonly data: void;
  };
  readonly fork: { readonly body: { readonly entryId: string }; readonly data: StructuredForkResult };
  readonly clone: { readonly body: Record<string, never>; readonly data: StructuredForkResult };
  readonly editMessage: {
    readonly body: { readonly entryId: string; readonly role: string; readonly text: string };
    readonly data: void;
  };
  readonly rename: { readonly body: { readonly name: string }; readonly data: void };
  readonly stats: { readonly body: Record<string, never>; readonly data: StructuredSessionStats };
  readonly reload: { readonly body: Record<string, never>; readonly data: void };
  readonly media: {
    readonly body: { readonly toolCallId: string; readonly contentIndex: number };
    readonly data: StructuredMediaBlob;
  };
  readonly clearQueue: { readonly body: Record<string, never>; readonly data: void };
  readonly modelPreferences: {
    readonly body: StructuredModelPreferences;
    readonly data: void;
  };
}

export type StructuredChatApprovalDecision = "accept" | "acceptForSession" | "decline";

/** One `{parts}` read plus the cursor past it; see `decodeStructuredPage`. */
export interface StructuredPartPage {
  readonly parts: readonly StructuredPart[];
  readonly nextSeq: number;
  /** Older parts exist below a bounded `history` page; `nextBefore` walks back to them. */
  readonly hasOlder?: boolean;
  readonly nextBefore?: number | null;
}

export type StructuredChatResult<T> =
  | { readonly ok: true; readonly data: T }
  | { readonly ok: false; readonly code: string; readonly message: string };

export interface StructuredChatRequest<K extends StructuredChatOp = StructuredChatOp> {
  readonly op: K;
  /** Absent only for `createSession` and `listSessions`. */
  readonly sessionId?: string;
  readonly body: StructuredChatRequests[K]["body"];
}

export type StructuredChatResponse<K extends StructuredChatOp> = Promise<
  StructuredChatResult<StructuredChatRequests[K]["data"]>
>;

/**
 * One typed async call: the op names both the body it takes and the data it answers with, so a
 * caller never reads a reply through a cast. A caller that picks the op at runtime (a read that
 * is either `history` or `events`) infers the union and gets the parts both ops answer with.
 */
export interface StructuredChatTransport {
  <K extends StructuredChatOp>(request: StructuredChatRequest<K>, signal: AbortSignal): StructuredChatResponse<K>;
}

/** The wire a raw adapter produces before this module validates it. */
export type StructuredChatWireBody = unknown;

/** One request as it leaves the process, before any adapter-specific framing. */
export interface StructuredChatWireRequest {
  readonly op: StructuredChatOp;
  readonly sessionId?: string;
  readonly body: unknown;
}

/** A raw adapter answers one request; it validates nothing and throws only on transport loss. */
export type StructuredChatWire = (
  request: StructuredChatWireRequest,
  signal: AbortSignal,
) => Promise<{ readonly status: number; readonly body: StructuredChatWireBody }>;

const SESSION_OPS: readonly StructuredChatOp[] = [
  "turn",
  "history",
  "events",
  "steer",
  "interrupt",
  "rollback",
  "approve",
  "settings",
  "capabilities",
  "clear",
  "compact",
  "resume",
  "closeSession",
  "commands",
  "answerExtensionUi",
  "tree",
  "navigateTree",
  "fork",
  "clone",
  "editMessage",
  "rename",
  "stats",
  "reload",
  "media",
  "clearQueue",
  "modelPreferences",
];

/** Does this op need a `sessionId` on the wire? */
export function structuredChatOpNeedsSession(op: StructuredChatOp): boolean {
  return SESSION_OPS.includes(op);
}

/** The HTTP method and path one op maps to, matching the route table lane G1 published. */
export function structuredChatRequestRoute(
  op: StructuredChatOp,
  sessionId?: string,
): { readonly method: "GET" | "POST" | "PUT" | "DELETE"; readonly path: string } {
  const base = `${STRUCTURED_CHAT_ROUTE_PREFIX}/sessions`;
  const one = (suffix = ""): string =>
    `${base}/${encodeURIComponent(sessionId ?? "")}${suffix}`;
  switch (op) {
    case "createSession":
      return { method: "POST", path: base };
    case "listSessions":
      return { method: "GET", path: base };
    case "turn":
      return { method: "POST", path: one("/turns") };
    case "history":
      return { method: "GET", path: one("/history") };
    case "events":
      return { method: "GET", path: one("/events") };
    case "steer":
      return { method: "POST", path: one("/steer") };
    case "interrupt":
      return { method: "POST", path: one("/interrupt") };
    case "rollback":
      return { method: "POST", path: one("/rollback") };
    case "approve":
      return { method: "POST", path: one("/approvals") };
    case "settings":
      return { method: "PUT", path: one("/settings") };
    case "capabilities":
      return { method: "GET", path: one("/capabilities") };
    case "clear":
      return { method: "POST", path: one("/clear") };
    case "compact":
      return { method: "POST", path: one("/compact") };
    case "resume":
      return { method: "POST", path: one("/resume") };
    case "commands":
      return { method: "GET", path: one("/commands") };
    case "answerExtensionUi":
      return { method: "POST", path: one("/extension-ui") };
    case "tree":
      return { method: "GET", path: one("/tree") };
    case "navigateTree":
      return { method: "POST", path: one("/navigate-tree") };
    case "fork":
      return { method: "POST", path: one("/fork") };
    case "clone":
      return { method: "POST", path: one("/clone") };
    case "editMessage":
      return { method: "POST", path: one("/edit-message") };
    case "rename":
      return { method: "PUT", path: one("/name") };
    case "stats":
      return { method: "GET", path: one("/stats") };
    case "reload":
      return { method: "POST", path: one("/reload") };
    case "media":
      return { method: "GET", path: one("/media") };
    case "clearQueue":
      return { method: "POST", path: one("/queue/clear") };
    case "modelPreferences":
      return { method: "PUT", path: one("/models/preferences") };
    case "closeSession":
      return { method: "DELETE", path: one() };
  }
}

function isRecord(value: unknown): value is Readonly<Record<string, unknown>> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * The machine code and message a refusal carried. The HTTP routes answer
 * `{error:{code,message}}`; the IPC command rejects with `{code, message}` where `code` is an
 * `IpcErrorCode` name (PascalCase) and the structured code rides the message's prefix.
 *
 * The rule: a SCREAMING_SNAKE `code` wins (that is the frozen wire code); an IPC-style name
 * falls through to the message prefix (`STEER_UNSUPPORTED: …` survives the IPC mapping); with
 * neither, the HTTP status names the class of failure.
 */
export function structuredChatFailure(body: unknown, status: number): { code: string; message: string } {
  const envelope = isRecord(body) && isRecord(body.error) ? body.error : isRecord(body) ? body : null;
  const message =
    envelope !== null && typeof envelope.message === "string"
      ? envelope.message
      : `Structured chat request failed (${status})`;
  const named = /^([A-Z][A-Z_]+):/.exec(message);
  const wire = envelope !== null && typeof envelope.code === "string" ? envelope.code : undefined;
  const snake = wire !== undefined && /^[A-Z][A-Z_]+$/.test(wire) ? wire : undefined;
  const code = snake ?? (named === null ? undefined : named[1]) ?? statusToCode(status);
  return { code, message };
}

function statusToCode(status: number): string {
  if (status === 400 || status === 422) return "INVALID_REQUEST";
  if (status === 403) return "FORBIDDEN";
  if (status === 404) return "NOT_FOUND";
  if (status === 409) return "CONFLICT";
  if (status === 502) return "PROVIDER_FAILED";
  return "REQUEST_FAILED";
}

function okBody(body: unknown): unknown {
  if (isRecord(body) && body.ok === true && "data" in body) return body.data;
  return body;
}

type DecodedBody = { readonly ok: true; readonly data: unknown } | { readonly ok: false };

const MALFORMED: DecodedBody = { ok: false };

/**
 * Validate the success body of one op. `MALFORMED` means a field the contract requires is gone.
 *
 * `after` is the cursor the read asked from: a page does not carry it, so an empty page's
 * `nextSeq` is the cursor it was asked with. A poll that comes back empty therefore keeps asking
 * the same question instead of restarting from zero.
 */
function decodeOp(op: StructuredChatOp, body: unknown, after: number): DecodedBody {
  const value = okBody(body);
  switch (op) {
    case "createSession": {
      const session = decodeStructuredSession(value);
      return session === null ? MALFORMED : { ok: true, data: session };
    }
    case "listSessions":
      // A read that cannot fail is worse than one that does: a truncated 200 (`readJson` answers
      // null) must not render as "no sessions" and a body without the list is not an empty list.
      return isRecord(value) && Array.isArray(value.sessions)
        ? { ok: true, data: decodeStructuredSessions(value) }
        : MALFORMED;
    case "turn":
    case "steer":
    case "clear": {
      const seq = decodeStructuredSeq(value);
      return seq === null ? MALFORMED : { ok: true, data: seq };
    }
    case "history":
    case "events":
      // Same rule for the transcript: `{parts: []}` is an empty page, a missing or non-array
      // `parts` (or a body that did not parse) is a malformed answer the caller must report.
      return isRecord(value) && Array.isArray(value.parts)
        ? { ok: true, data: decodeStructuredPage(value, after) }
        : MALFORMED;
    case "settings":
      return { ok: true, data: decodeStructuredSettings(value) };
    case "capabilities": {
      const capabilities = decodeStructuredCapabilities(value);
      return capabilities === null ? MALFORMED : { ok: true, data: capabilities };
    }
    case "commands": {
      const commands = decodeStructuredCommands(value);
      // A body without the list is not an empty list: the chip menu would go quiet instead of
      // reporting that the gateway answered with a shape this client does not understand.
      return isRecord(value) && Array.isArray(value.commands)
        ? { ok: true, data: commands }
        : MALFORMED;
    }
    case "tree": {
      const tree = decodeStructuredTree(value);
      return tree === null ? MALFORMED : { ok: true, data: tree };
    }
    case "stats": {
      const stats = decodeStructuredStats(value);
      return stats === null ? MALFORMED : { ok: true, data: stats };
    }
    case "media": {
      const media = decodeStructuredMedia(value);
      return media === null ? MALFORMED : { ok: true, data: media };
    }
    case "fork":
    case "clone": {
      const result = decodeStructuredFork(value);
      return result === null ? MALFORMED : { ok: true, data: result };
    }
    default:
      return { ok: true, data: undefined };
  }
}

/**
 * Validate one raw reply for `op`. A body that does not match the frozen contract is a
 * `MALFORMED_RESPONSE` refusal, never a half-populated value the UI would draw.
 */
export function decodeStructuredReply<K extends StructuredChatOp>(
  op: K,
  status: number,
  body: unknown,
  after = 0,
): StructuredChatResult<StructuredChatRequests[K]["data"]> {
  if (status < 200 || status >= 300) {
    const failure = structuredChatFailure(body, status);
    return { ok: false, code: failure.code, message: failure.message };
  }
  if (isRecord(body) && isRecord(body.error)) {
    const failure = structuredChatFailure(body, status);
    return { ok: false, code: failure.code, message: failure.message };
  }
  const decoded = decodeOp(op, body, after);
  if (!decoded.ok) {
    return { ok: false, code: "MALFORMED_RESPONSE", message: "The gateway answered with a shape this client does not understand." };
  }
  return { ok: true, data: decoded.data as StructuredChatRequests[K]["data"] };
}

/** The `after` a read carried, or `0` for an op that has no cursor. */
function readCursor(request: { readonly op: StructuredChatOp; readonly body: unknown }): number {
  if (request.op !== "history" && request.op !== "events") return 0;
  const after = (request.body as { after?: unknown }).after;
  return typeof after === "number" && Number.isFinite(after) ? after : 0;
}

/** Wrap a raw wire as a typed transport. Reads honour `signal`; mutations are never aborted. */
export function structuredChatTransportFrom(wire: StructuredChatWire): StructuredChatTransport {
  async function dispatch<K extends StructuredChatOp>(
    request: StructuredChatRequest<K>,
    signal: AbortSignal,
  ): Promise<StructuredChatResult<StructuredChatRequests[K]["data"]>> {
    signal.throwIfAborted();
    let reply: { status: number; body: unknown };
    try {
      reply = await wire({ op: request.op, ...(request.sessionId === undefined ? {} : { sessionId: request.sessionId }), body: request.body }, signal);
    } catch (error) {
      signal.throwIfAborted();
      return {
        ok: false,
        code: STRUCTURED_CHAT_OUTCOME_UNKNOWN,
        message: error instanceof Error ? error.message : String(error),
      };
    }
    signal.throwIfAborted();
    return decodeStructuredReply(request.op, reply.status, reply.body, readCursor(request));
  }
  function call<K extends StructuredChatOp>(
    request: StructuredChatRequest<K>,
    signal: AbortSignal,
  ): StructuredChatResponse<K> {
    return dispatch(request, signal);
  }
  return call;
}

/** What an HTTP adapter needs; `fetchImpl` is the test seam, production calls `remoteFetch`. */
export interface StructuredChatHttpDeps {
  readonly baseUrl: string;
  readonly token: string;
  readonly fetchImpl?: StructuredChatPathFetch;
}

/** One fetch bound to a base URL: the path-only shape the adapter and every test seam share. */
export type StructuredChatPathFetch = (path: string, init: RequestInit) => Promise<Response>;

async function readJson(response: Response): Promise<unknown> {
  try {
    return await response.json();
  } catch {
    return null;
  }
}

/**
 * The path segments a route names beyond its own suffix: `approve` and `answerExtensionUi`
 * address one request by id, and `media` addresses one content item of one tool call.
 */
function routeIdSegments(op: StructuredChatOp, body: Readonly<Record<string, unknown>>): string {
  const segment = (value: unknown): string => encodeURIComponent(String(value ?? ""));
  switch (op) {
    case "approve":
    case "answerExtensionUi":
      return `/${segment(body.requestId)}`;
    case "media":
      return `/${segment(body.toolCallId)}/${segment(body.contentIndex ?? 0)}`;
    default:
      return "";
  }
}

/**
 * The remote surface: the same route table over HTTP with a bearer token. `approve`,
 * `answerExtensionUi` and `media` carry their ids in the bottom path segments the route table
 * names.
 *
 * `fetchPath` is a path-only fetch: the caller binds the base URL once, so this module never has
 * to know how the app reaches a host. `deps.fetchImpl` overrides it for a test.
 */
export function structuredChatHttpWire(
  deps: StructuredChatHttpDeps,
  fetchPath: StructuredChatPathFetch,
): StructuredChatWire {
  const send = deps.fetchImpl ?? fetchPath;
  return async (request, signal) => {
    const route = structuredChatRequestRoute(request.op, request.sessionId);
    const body = request.body as Record<string, unknown>;
    const query =
      request.op === "history" || request.op === "events"
        ? `?after=${encodeURIComponent(String(Number(body.after ?? 0)))}${historyQueryTail(body)}`
        : "";
    const path = `${route.path}${routeIdSegments(request.op, body)}${query}`;
    const init: RequestInit = {
      method: route.method,
      headers: { Authorization: `Bearer ${deps.token}`, "content-type": "application/json" },
      signal,
      ...(route.method === "GET" || route.method === "DELETE" ? {} : { body: JSON.stringify(body) }),
    };
    const response = await send(path, init);
    return { status: response.status, body: await readJson(response) };
  };
}

/**
 * The extra `history` query a backwards page carries: `before=<seq>` walks to the page below and
 * `limit` bounds the window. `events` has neither, so a poll asks exactly one question.
 */
function historyQueryTail(body: Readonly<Record<string, unknown>>): string {
  const bits: string[] = [];
  if (typeof body.before === "number" && Number.isFinite(body.before)) {
    bits.push(`before=${encodeURIComponent(String(body.before))}`);
  }
  if (typeof body.limit === "number" && Number.isFinite(body.limit)) {
    bits.push(`limit=${encodeURIComponent(String(body.limit))}`);
  }
  return bits.length === 0 ? "" : `&${bits.join("&")}`;
}

/** The desktop surface: one Tauri command that mirrors the route table op for op. */
export function structuredChatIpcWire(
  invoke: (request: StructuredChatWireRequest) => Promise<unknown>,
): StructuredChatWire {
  return async (request) => {
    try {
      return { status: 200, body: await invoke(request) };
    } catch (error) {
      const failure = isRecord(error) ? error : { message: String(error) };
      return { status: statusForIpc(failure), body: { error: failure } };
    }
  };
}

function statusForIpc(failure: Readonly<Record<string, unknown>>): number {
  switch (failure.code) {
    case "SessionNotFound":
      return 404;
    case "ControlConflict":
      return 409;
    case "InvalidArgument":
      return 400;
    case "PermissionDenied":
      return 403;
    default:
      return 500;
  }
}

/**
 * The incremental read half of the transport: one cursor, one long-poll at a time.
 *
 * `history` seeds the stream; `events` then asks only for what is past the cursor, and the page
 * is merged by `seq`, so a replayed or overlapping page never duplicates a row. A failed read
 * leaves the cursor where it was: the next call asks the same question again, which is how a
 * gateway restart (sessions come back `ended`) and a dropped tunnel recover without losing a part.
 */
export class StructuredChatStream {
  private parts: readonly StructuredPart[] = [];
  private cursor = 0;
  private generation = 0;

  constructor(
    private readonly transport: StructuredChatTransport,
    private readonly sessionId: string,
  ) {}

  /** Every part read so far, ascending. */
  read(): readonly StructuredPart[] {
    return this.parts;
  }

  /** The `seq` the next read asks `after=`; `0` asks for the whole journal. */
  after(): number {
    return this.cursor;
  }

  /** Replace the stream with a fresh page (a target switch, a resync). */
  reset(): void {
    this.parts = [];
    this.cursor = 0;
    this.generation += 1;
  }

  /** Read the whole journal once. */
  async load(signal: AbortSignal): Promise<StructuredChatResult<readonly StructuredPart[]>> {
    return this.pull("history", 0, signal);
  }

  /** Long-poll the parts past the cursor. Returns `[]` on a bounded-poll timeout. */
  async poll(signal: AbortSignal): Promise<StructuredChatResult<readonly StructuredPart[]>> {
    return this.pull("events", this.cursor, signal);
  }

  private async pull(
    op: "history" | "events",
    after: number,
    signal: AbortSignal,
  ): Promise<StructuredChatResult<readonly StructuredPart[]>> {
    const generation = this.generation;
    const result =
      op === "history"
        ? await this.transport({ op: "history", sessionId: this.sessionId, body: { after } }, signal)
        : await this.transport({ op: "events", sessionId: this.sessionId, body: { after } }, signal);
    if (!result.ok) return result;
    if (generation !== this.generation) return { ok: true, data: this.parts };
    const merged = op === "history" ? mergeStructuredParts([], result.data.parts) : mergeStructuredParts(this.parts, result.data.parts);
    this.parts = merged;
    this.cursor = structuredHighestSeq(merged, after);
    return { ok: true, data: result.data.parts };
  }
}

/**
 * How a send goes out, following t3code's composer rules: an idle session starts a turn; a
 * running one steers when the provider advertises it and otherwise queues the text for the
 * next turn. Queueing is never a silent drop - the caller holds the row and delivers it.
 */
export type StructuredSendMode = "turn" | "steer" | "queue";

export function structuredChatSendMode(input: {
  readonly running: boolean;
  readonly steer: boolean;
  readonly text: string;
}): StructuredSendMode | null {
  if (input.text.trim().length === 0) return null;
  if (!input.running) return "turn";
  return input.steer ? "steer" : "queue";
}

/**
 * The lane command a composer message is, or `null` for prose and for provider slash commands.
 *
 * `/clear` and `/compact` are this client's own operations, not text the agent reads, so they are
 * recognised here and never sent as a turn. Only a message that is the command by itself counts:
 * t3code reads `/clear` "on its own", and `/clearly a bug` is prose.
 */
export function structuredChatLaneCommand(text: string): "clear" | "compact" | null {
  const bare = text.trim().toLowerCase();
  if (bare === "/clear") return "clear";
  if (bare === "/compact") return "compact";
  return null;
}

/**
 * Is this message a slash command the agent itself advertises?
 *
 * t3code (composer.md:72-74): a slash command such as `/compact` or `/context` waits for the
 * current turn to finish, because a command steered into a turn would replace the agent's task.
 * The lane's own commands are recognised by `structuredChatLaneCommand`; everything else is the
 * provider's, and the only authority on what it accepts is `capabilities.slashCommands`.
 */
export function structuredChatAdvertisedCommand(
  text: string,
  capabilities: StructuredCapabilities | null,
): string | null {
  if (capabilities === null) return null;
  const name = text.trim().split(/\s+/, 1)[0] ?? "";
  if (!name.startsWith("/")) return null;
  return capabilities.slashCommands.includes(name) ? name : null;
}

/**
 * The prompt text a rollback of `turns` whole turns takes back, in the order the user wrote it.
 * Reads the merged stream, so the composer can put it back in the draft before the rollback lands.
 *
 * Turns, not user parts: a steer lands a second user message inside the turn that was already
 * running, and rolling back that one turn must take both back. Parts outside a turn (the
 * `newSession` divider) are never a rollback unit, so they are not counted.
 */
export function structuredChatRollbackPrompts(parts: readonly StructuredPart[], turns: number): readonly string[] {
  if (turns < 1) return [];
  const turnGroups = groupStructuredTurns(parts).filter((group) => group.turnId !== null);
  const tail = turnGroups.slice(Math.max(0, turnGroups.length - turns));
  return tail.flatMap((group) =>
    group.parts
      .filter((part): part is Extract<StructuredPart, { type: "userMessage" }> => part.type === "userMessage")
      .map((part) => part.text),
  );
}

/** The `turns` argument an edit-from-here needs for one `seq`, and the text it restores. */
export function structuredChatEditFrom(input: {
  readonly parts: readonly StructuredPart[];
  readonly seq: number;
}): { readonly turns: number; readonly text: string } | null {
  const turns = structuredTurnsBackTo(input.parts, input.seq);
  if (turns < 1) return null;
  const prompts = structuredChatRollbackPrompts(input.parts, turns);
  const text = prompts.join("\n\n").trim();
  if (text.length === 0) return null;
  return { turns, text };
}

/**
 * `rollback` cannot revert working-tree files: the provider reverts its thread only. The UI
 * reports this instead of offering a "revert files too" switch it cannot honour.
 */
export const STRUCTURED_CHAT_REVERT_FILES_UNSUPPORTED =
  "Reverting edited files is not supported for this agent; only the conversation is rolled back.";

/**
 * Does this agent advertise reverting edited files along with the conversation?
 *
 * The frozen `Capabilities` contract carries `steer`/`rollback`/`permissionModes`/`models`/
 * `slashCommands` and no file-revert bit, so no agent does today. A gateway that adds one is
 * honoured without a client change; until then the UI reports the refusal rather than offering
 * a switch it cannot honour.
 */
export function structuredChatRevertFilesSupported(capabilities: StructuredCapabilities | null): boolean {
  if (capabilities === null) return false;
  return (capabilities as unknown as Readonly<Record<string, unknown>>).revertFiles === true;
}

/** The permission modes an agent advertises, in grant order and de-duplicated. */
export function structuredChatPermissionModes(
  capabilities: StructuredCapabilities | null,
): readonly StructuredPermissionMode[] {
  // No capability means no claim: a mode the agent never advertised is not offered (the same
  // rule referenceTypes.ts:302 states for models/options/slash entries).
  if (capabilities === null) return [];
  if (capabilities.permissionModes.length === 0) return [];
  return STRUCTURED_PERMISSION_MODES.filter((mode) => capabilities.permissionModes.includes(mode));
}
