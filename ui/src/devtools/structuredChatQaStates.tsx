// Seeded structured-chat states for real-browser QA of `StructuredChatPane` (lane G7):
// open /mobile-chat-qa.html?state=<structured state> (dev server only).
//
// The transport is a fake wire, not a stub component: the pane mounts for real and its own hook
// reads `history`/`events`, so a screenshot shows the production renderer for the parts below.
// A state whose control the product does not draw yet is rendered anyway and carries the
// `pending-gaps` mark, so the final QA run re-checks it instead of quietly passing.
import React from "react";

import { StructuredChatPane } from "../remote/chat/structured/StructuredChatPane";
import type { StructuredChatOp, StructuredChatTransport } from "../lib/structuredChatTransport";
import type {
  StructuredCapabilities,
  StructuredPart,
  StructuredSessionSummary,
} from "../lib/structuredChatTypes";

export const STRUCTURED_CHAT_QA_STATES = [
  "structured-streaming",
  "structured-approval",
  "structured-followup",
  "structured-held-queue",
  "structured-commands",
  "structured-rollback",
  "structured-edit-from",
  "structured-older",
  "structured-mhc-extensions",
] as const;

export type StructuredChatQaState = (typeof STRUCTURED_CHAT_QA_STATES)[number];

export function isStructuredChatQaState(state: string): state is StructuredChatQaState {
  return (STRUCTURED_CHAT_QA_STATES as readonly string[]).includes(state);
}

/**
 * Controls the GAPS lane still owes this pane. A state that needs one renders the fixture and
 * says so; the capture script asserts the mark instead of a control that is not there.
 */
export const STRUCTURED_CHAT_PENDING_GAPS: Readonly<Partial<Record<StructuredChatQaState, readonly string[]>>> = {};

const SESSION: StructuredSessionSummary = {
  sessionId: "qa-structured",
  provider: "codex",
  cwd: "/workspace/example",
  state: "running",
  settings: { model: "gpt-5", effort: "high", permissionMode: "supervised" },
};

/** Every capability the pane can draw, so no control is missing for a reason other than GAPS. */
const CAPABILITIES: StructuredCapabilities = {
  steer: true,
  rollback: true,
  permissionModes: ["supervised", "auto-accept-edits", "auto", "full-access"],
  models: [
    { id: "gpt-5", label: "GPT-5", efforts: ["low", "medium", "high"] },
    { id: "gpt-5-mini", label: "GPT-5 mini", efforts: ["low", "high"] },
  ],
  slashCommands: ["/clear", "/compact", "/context"],
};

function at(seq: number, extra: Record<string, unknown>): StructuredPart {
  return { seq, tsMs: 1_700_000_000_000 + seq * 1_000, ...extra } as StructuredPart;
}

const TURN_ONE: readonly StructuredPart[] = [
  at(1, { turnId: "t1", type: "userMessage", text: "Audit the structured lane and run its suite." }),
  at(2, { turnId: "t1", type: "reasoning", text: "The pane draws through the existing workspace, so the composer wiring is the first thing to check." }),
  at(3, { turnId: "t1", type: "assistantMessage", text: "I read the composer wiring and started the suite." }),
  at(4, { turnId: "t1", type: "toolCall", id: "call-1", name: "bash", status: "completed", output: "Test Files  6 passed (6)", exitCode: 0, durationMs: 1_840 }),
];

const TURN_TWO: readonly StructuredPart[] = [
  at(5, { turnId: "t2", type: "userMessage", text: "Now check the approval path." }),
  at(6, { turnId: "t2", type: "assistantMessage", text: "Checking the approval path next." }),
];

/** A live turn: the last part is still in progress, which is what makes the pane "running". */
const STREAMING_PARTS: readonly StructuredPart[] = [
  ...TURN_ONE,
  ...TURN_TWO,
  at(7, { turnId: "t2", type: "reasoning", text: "The approval card is the reference prompt card, so one answer path serves both surfaces." }),
  at(8, { turnId: "t2", type: "assistantMessage", text: "Running the approval suite now; the transcript below is still streaming." }),
  at(9, { turnId: "t2", type: "toolCall", id: "call-2", name: "bash", status: "inProgress", output: "RUN  v3.2.4 /workspace/example\n ✓ approval answers with the picked decision\n ..." }),
];

const APPROVAL_PARTS: readonly StructuredPart[] = [
  ...TURN_ONE,
  at(5, { turnId: "t2", type: "userMessage", text: "Clean up the generated fixtures." }),
  at(6, { turnId: "t2", type: "toolCall", id: "call-3", name: "bash", status: "completed", output: "3 files removed", exitCode: 0 }),
  at(7, {
    turnId: "t2",
    type: "approvalRequest",
    requestId: "req-qa-1",
    kind: "command",
    summary: "Codex wants to run a command on your computer.",
    reason: "bun run test src/lib/structuredChat",
    answered: false,
  }),
];

const SETTLED_PARTS: readonly StructuredPart[] = [
  ...TURN_ONE,
  ...TURN_TWO,
];

const MHC_CAPABILITIES: StructuredCapabilities = {
  ...CAPABILITIES,
  executionModes: ["default", "plan"],
  agents: ["default", "reviewer"],
  thinkingLevels: ["off", "minimal", "low", "medium", "high", "xhigh", "max"],
  fastMode: true,
  additionalDirs: true,
  queueModes: true,
  compactCustomInstructions: true,
  treeNavigation: true,
  fork: true,
  cloneSession: true,
  editUserMessage: true,
  editAssistantMessage: true,
  sessionRename: true,
  sessionStats: true,
  sessionSwitch: true,
  reload: true,
  media: true,
  extensionUi: true,
  goals: true,
  favoriteModels: true,
  scopedModels: true,
};

const MHC_EXTENSION_PARTS: readonly StructuredPart[] = [
  at(1, { turnId: "t1", type: "extensionStatus", statusKey: "senpi", statusText: "mhc RPC ready" }),
  at(2, { turnId: "t1", type: "extensionWidget", widgetKey: "goal", lines: ["Goal: Multi-harness chat parity"], placement: "aboveEditor" }),
  at(3, { turnId: "t1", type: "subagentProgress", taskId: "st-1", name: "falsify-backend", status: "completed", summary: "All wire checks verified" }),
  at(4, { turnId: "t1", type: "goalUpdate", goalId: "g-1", title: "Chat Harness Parity", status: "active", progress: "10/13" }),
  at(5, { turnId: "t1", type: "userMessage", text: "Verify multi-harness mhc RPC controls and extension UI." }),
  at(6, { turnId: "t1", type: "assistantMessage", text: "All mhc RPC controls, session tree, and extension UI prompts are active." }),
  at(7, {
    turnId: "t1",
    type: "extensionUiRequest",
    requestId: "ext-select-1",
    method: "select",
    title: "Select verification target",
    message: "Choose the suite to run next",
    options: ["Full UI + Rust gate", "Headless Chrome QA"],
    answered: false,
  }),
];

function partsFor(state: StructuredChatQaState): readonly StructuredPart[] {
  switch (state) {
    case "structured-approval":
      return APPROVAL_PARTS;
    case "structured-mhc-extensions":
      return MHC_EXTENSION_PARTS;
    case "structured-rollback":
    case "structured-edit-from":
    case "structured-older":
      return SETTLED_PARTS;
    case "structured-streaming":
    case "structured-followup":
    case "structured-held-queue":
    case "structured-commands":
      return STREAMING_PARTS;
  }
}

export function structuredChatQaTransport(
  parts: readonly StructuredPart[],
  options: { readonly hasOlder?: boolean; readonly capabilities?: StructuredCapabilities } = {},
): StructuredChatTransport {
  const hasOlder = options.hasOlder ?? false;
  const caps = options.capabilities ?? CAPABILITIES;
  const wire = async (request: { op: StructuredChatOp; sessionId?: string; body: unknown }) => {
    switch (request.op) {
      case "listSessions":
        return { ok: true as const, data: [SESSION] };
      case "capabilities":
        return { ok: true as const, data: caps };
      case "commands":
        return {
          ok: true as const,
          data: [
            { name: "compact", description: "Compact context", source: "builtin", syntax: "slash" },
            { name: "tree", description: "Session tree", source: "builtin", syntax: "slash" },
            { name: "visual-qa", description: "Visual QA skill", source: "skill", syntax: "dollar" },
          ],
        };
      case "tree":
        return {
          ok: true as const,
          data: {
            currentLeafId: "e2",
            nodes: [
              { id: "e1", parentId: null, role: "user", preview: "Verify controls", active: false },
              { id: "e2", parentId: "e1", role: "assistant", preview: "All active", active: true },
            ],
          },
        };
      case "stats":
        return {
          ok: true as const,
          data: { name: "QA Session", turns: 2, totalTokens: 1500, inputTokens: 1000, outputTokens: 500, contextWindow: 200000 },
        };
      case "history":
        return {
          ok: true as const,
          data: hasOlder
            ? { parts, nextSeq: parts.length, hasOlder: true, nextBefore: 1 }
            : { parts, nextSeq: parts.length, hasOlder: false },
        };
      case "events":
        return { ok: true as const, data: { parts: [], nextSeq: parts.length } };
      case "turn":
      case "steer":
      case "clear":
        return { ok: true as const, data: parts.length + 1 };
      default:
        return { ok: true as const, data: undefined };
    }
  };
  return wire as unknown as StructuredChatTransport;
}

export function structuredChatQaParts(state: StructuredChatQaState): readonly StructuredPart[] {
  return partsFor(state);
}

/** The controls this state is waiting on, if any; the fixture renders them as a visible note. */
export function structuredChatQaPendingGaps(state: StructuredChatQaState): readonly string[] {
  return STRUCTURED_CHAT_PENDING_GAPS[state] ?? [];
}

export const StructuredChatQaFixture: React.FC<{ state: StructuredChatQaState }> = ({ state }) => {
  const parts = structuredChatQaParts(state);
  const transport = React.useMemo(
    () =>
      structuredChatQaTransport(parts, {
        hasOlder: state === "structured-older",
        capabilities: state === "structured-mhc-extensions" ? MHC_CAPABILITIES : CAPABILITIES,
      }),
    [parts, state],
  );
  const pending = structuredChatQaPendingGaps(state);
  return (
    <div className="remote-app-root flex h-[100dvh] w-full flex-col bg-chat-screen">
      <div className="flex-1 min-h-0 w-full">
        <StructuredChatPane
          transport={transport}
          sessionId={SESSION.sessionId}
          pollMs={0}
          workspaceLabel="maho-workspace"
          headerTitle="codex · structured"
          headerSubtitle={`maho-workspace · example · ${state}`}
          showHeader
        />
      </div>
      {pending.length > 0 && (
        <div
          data-testid="qa-pending-gaps"
          data-qa-state={state}
          className="border-t border-chat-border bg-chat-surface-raised px-3 py-2 text-xs text-chat-foreground-secondary"
        >
          <span className="font-medium text-chat-foreground">pending-gaps</span>
          {": "}
          {pending.join("; ")}
        </div>
      )}
    </div>
  );
};
