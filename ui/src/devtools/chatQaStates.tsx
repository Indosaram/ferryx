// Seeded composer states for real-browser QA of the production chat paths: the
// ReferencePromptCard in the composer, held messages, staged files, a disconnected chat,
// the on-screen keyboard and the terminal key dock.
import React from "react";

import { AppearanceChatPreview } from "../components/settings/AppearanceChatPreview";
import { AppearanceSection, ThemeLibrary } from "../components/settings/AppearanceSection";
import { DEFAULT_APPEARANCE_SETTINGS, useAppearanceSettings } from "../lib/appearanceSettings";
import { APPEARANCE_SETTINGS_STORAGE_KEY } from "../lib/storageKeys";
import { RemoteAppearanceSheet } from "../remote/appearance/RemoteAppearanceSheet";

import { userFacingError } from "../remote/userFacingError";
import { ReferencePromptCard } from "../remote/chat/ReferencePromptCard";
import type { HeldMessage } from "../remote/chat/referenceQueue";
import type { FollowUpMessage } from "../remote/chat/referenceFollowUp";
import type { MobileChatMessageProps } from "../remote/chat/MobileChatMessage";
import type { ReferencePart } from "../remote/chat/referenceTypes";
import {
  promptAttachmentDraftKey,
  referenceAnswerAttachmentStore,
} from "../remote/chat/promptAttach/referencePromptAttach";
import type { ToolCallVisibility } from "../remote/chat/MobileChatWorkspace";
import type {
  ChipItem,
  ChipSources,
  CitationInsert,
  PermissionControl,
} from "../remote/chat/composer/chipTypes";
import type { ReferenceFileReceipt, ReferencePrompt } from "../remote/chat/referenceTypes";

export const CHAT_QA_STATES = [
  "prompt-approval",
  "prompt-multi",
  "prompt-custom",
  "held",
  "attach",
  "attach-error",
  "unsaved",
  "offline",
  "keyboard-open",
  "terminal-dock",
  "queued-list",
  "chips",
  "chips-menu",
  "chips-typed",
  "citation",
  "citation-seeded",
  "tool-groups-all",
  "tool-groups-hide-successful",
  "tool-groups-hide-all",
  "tool-groups-zen",
  "mermaid",
  "lightbox",
  "find-bar",
  "plan-card",
  "appearance-preview",
  "theme-import-error",
  "custom-css-active",
  "remote-appearance-sheet",
] as const;

export const APPEARANCE_QA_STATES = [
  "appearance-preview",
  "theme-import-error",
  "custom-css-active",
  "remote-appearance-sheet",
] as const satisfies readonly ChatQaState[];

export type AppearanceQaState = (typeof APPEARANCE_QA_STATES)[number];

export function isAppearanceQaState(state: string): state is AppearanceQaState {
  return (APPEARANCE_QA_STATES as readonly string[]).includes(state);
}

/** Seed only the canonical browser store; the production applier owns all CSS. */
export function seedAppearanceQaState(state: string, theme: "light" | "dark") {
  window.localStorage.setItem(APPEARANCE_SETTINGS_STORAGE_KEY, JSON.stringify({
    ...DEFAULT_APPEARANCE_SETTINGS,
    theme,
    accentColor: "rose",
    customThemes: state === "theme-import-error"
      ? [{ version: 2, id: "qa-invalid", name: "Rejected import", base: theme, tokens: { "--radius-lg": "2px" } }]
      : [{ version: 1, id: "qa-saved", name: "QA saved theme", base: theme, tokens: {} }],
    customCss: state === "custom-css-active"
      ? '[data-preview-row="user"] { border: 2px solid #be185d; border-radius: 16px; }'
      : "",
  }));
}

/** Real appearance surfaces, with fixed local data and no workspace/transport. */
export function AppearanceQaFixture({ state }: { state: string }) {
  const { settings, invalidThemes, updateSettings } = useAppearanceSettings();
  const [sheetOpen, setSheetOpen] = React.useState(true);
  if (!isAppearanceQaState(state)) {
    return <p role="status" data-testid="appearance-qa-fallback">Unknown appearance QA state.</p>;
  }
  return <main data-testid={`qa-${state}`} className="min-h-screen bg-background p-4 text-foreground sm:p-6">
    <div className="mx-auto max-w-4xl space-y-4">
      {state === "appearance-preview" ? <AppearanceSection /> : null}
      {state === "theme-import-error" ? <ThemeLibrary settings={settings} invalidThemes={invalidThemes}
        updateSettings={updateSettings} resolvedTheme={settings.theme === "light" ? "light" : "dark"} /> : null}
      {state === "custom-css-active" ? <>
        <h1 className="text-lg font-semibold">Custom CSS active</h1>
        <button type="button" className="min-h-11 rounded-md border border-border px-3"
          onClick={() => updateSettings({ customCss: "" })}>Remove custom CSS</button>
        <pre aria-label="Active custom CSS" className="whitespace-pre-wrap break-all text-sm">{settings.customCss}</pre>
        <AppearanceChatPreview />
      </> : null}
      {state === "remote-appearance-sheet" ? <>
        <h1 className="text-lg font-semibold">Remote appearance</h1>
        <button type="button" className="min-h-11 rounded-md border border-border px-3"
          onClick={() => setSheetOpen(true)}>Open appearance</button>
        <RemoteAppearanceSheet open={sheetOpen} onOpenChange={setSheetOpen} />
      </> : null}
    </div>
  </main>;
}

export type ChatQaState = (typeof CHAT_QA_STATES)[number];

export function isChatQaState(state: string): state is ChatQaState {
  return (CHAT_QA_STATES as readonly string[]).includes(state);
}

/** The on-screen keyboard height a 390x844 iPhone reports; the QA root shrinks by it. */
// px, not rem: the system keyboard does not grow with the page's text size.
export const QA_KEYBOARD_HEIGHT = "336px";

const APPROVAL_PROMPT: ReferencePrompt = {
  id: "qa-approval",
  agent: "codex",
  kind: "approval",
  title: "Run this command?",
  question: "Codex wants to run a command on your computer.",
  body: "bun run test src/remote/chat",
  options: [
    { label: "Yes, run it", description: null },
    { label: "No, tell Codex what to do instead", description: null },
  ],
  multiSelect: false,
  customOptionIndex: null,
  queued: null,
  steps: [],
  fallback: null,
};

const MULTI_PROMPT: ReferencePrompt = {
  id: "qa-multi",
  agent: "claude",
  kind: "question",
  title: "Question",
  question: "Which surfaces should the redesign cover?",
  body: null,
  options: [
    { label: "Phone home (Recommended)", description: "The Supervisor panel, with Worktrees behind the + button" },
    { label: "Session chat", description: "Messages, composer and prompt cards" },
    { label: "Terminal", description: "Tab strip, key dock and line mode" },
    { label: "Account and sign-in", description: null },
    { label: "Something else", description: null },
  ],
  multiSelect: true,
  customOptionIndex: 4,
  queued: "open",
  steps: [
    { label: "Scope", answered: true, current: false },
    { label: "Surfaces", answered: false, current: true },
    { label: "Confirm", answered: false, current: false },
  ],
  fallback: null,
};

const CUSTOM_PROMPT: ReferencePrompt = {
  id: "qa-custom",
  agent: "claude",
  kind: "menu",
  title: "Question",
  question: "What should the empty Inbox say?",
  body: null,
  options: [
    { label: "Nothing needs you right now", description: null },
    { label: "All caught up", description: null },
    { label: "Type something else", description: null },
  ],
  multiSelect: false,
  customOptionIndex: 2,
  queued: null,
  steps: [],
  fallback: null,
};

const PROMPTS: Partial<Record<ChatQaState, ReferencePrompt>> = {
  "prompt-approval": APPROVAL_PROMPT,
  "prompt-multi": MULTI_PROMPT,
  "prompt-custom": CUSTOM_PROMPT,
};

const HELD: readonly HeldMessage[] = [
  { id: "held-1", text: "Also check the light theme before you commit." },
  { id: "held-2", text: "And run the composer tests again once the token change lands, then summarise what moved." },
];

const STAGED: readonly ReferenceFileReceipt[] = [
  {
    receipt: { hostId: "qa-host", attachmentId: "att-1", sha256: "0".repeat(64), sizeBytes: 188_416, mediaType: "image/png" },
    displayName: "screenshot-390x844.png",
    mentionText: "@screenshot-390x844.png",
  },
  {
    receipt: { hostId: "qa-host", attachmentId: "att-2", sha256: "1".repeat(64), sizeBytes: 12_697, mediaType: "text/plain" },
    displayName: "transcript-frames.log",
    mentionText: "@transcript-frames.log",
  },
];

const UNSAVED_DRAFT = "Rename the Inbox empty state and keep the";

/** Props for `MobileChatWorkspace` that put the composer into one seeded state. */
export interface ChatQaComposerProps {
  readonly promptCard?: React.ReactNode;
  readonly heldMessages?: readonly HeldMessage[];
  readonly stagedFiles?: readonly ReferenceFileReceipt[];
  readonly attachError?: string | null;
  readonly draft?: string;
  readonly draftUnsaved?: boolean;
  readonly composerWarning?: string | null;
  readonly disabled?: boolean;
  readonly queuedMessages?: readonly FollowUpMessage[];
  readonly chipSources?: ChipSources;
  readonly permissionControl?: PermissionControl;
  readonly onCiteSelection?: (citation: CitationInsert) => void;
  readonly toolCallVisibility?: ToolCallVisibility;
}

export function chatQaComposerProps(state: string): ChatQaComposerProps {
  if (!isChatQaState(state)) return {};
  const prompt = PROMPTS[state];
  if (prompt) {
    return {
      promptCard: (
        <ReferencePromptCard
          key={prompt.id}
          prompt={prompt}
          screenRevision={`${prompt.id}-r1`}
          onAnswer={() => Promise.resolve(true)}
          /* G6: a question that accepts a typed answer offers the attach row; the seeded
             attachments below land in this card's own set. */
          onAttachAnswerFiles={qaStageAnswerFiles}
          answerScope={QA_ANSWER_SCOPE}
        />
      ),
    };
  }
  switch (state) {
    case "held":
      return { heldMessages: HELD };
    case "attach":
      return { stagedFiles: STAGED };
    case "attach-error":
      return { stagedFiles: STAGED.slice(0, 1), attachError: userFacingError({ code: "TIMEOUT" }, { action: "attach this file" }) };
    case "unsaved":
      return { draft: UNSAVED_DRAFT, draftUnsaved: true };
    case "offline":
      return { composerWarning: userFacingError({ code: "STREAM_CLOSED" }), disabled: true };
    case "keyboard-open":
      return { draft: UNSAVED_DRAFT };
    case "queued-list":
      return { queuedMessages: QUEUED };
    case "chips":
    case "chips-menu":
    case "chips-typed":
      // A draft that already carries every chip kind the composer writes: a model, a pull
      // request, a skill, a file, and a slash command the agent advertises. `chips-menu` seeds
      // the same sources but no draft, so the menu has to open on text the editor owns itself;
      // `chips-typed` keeps the link-bearing draft, which is the half that is broken.
      return {
        chipSources: CHIP_SOURCES,
        permissionControl: PERMISSION_CONTROL,
        ...(state === "chips-menu" ? {} : { draft: CHIP_DRAFT }),
      };
    case "citation":
      return {
        draft: CITATION_DRAFT,
        onCiteSelection: (citation) => window.console.info("qa cite", citation),
      };
    case "citation-seeded":
      // The same draft with no interaction at all: what the chip row shows the moment the pane
      // mounts decides whether a comment survives the rich editor's own round-trip.
      return { draft: CITATION_DRAFT };
    case "tool-groups-all":
      return { toolCallVisibility: "all" };
    case "tool-groups-hide-successful":
      return { toolCallVisibility: "hide-successful" };
    case "tool-groups-hide-all":
      return { toolCallVisibility: "hide-all" };
    case "tool-groups-zen":
      return { toolCallVisibility: "zen" };
    // t3code items whose renderer belongs to the composer/messages lanes (G3/G4): the state
    // and its seed content exist here so the QA lane screenshots the same URL once those land.
    case "mermaid":
    case "lightbox":
    case "find-bar":
    case "plan-card":
    case "prompt-approval":
    case "prompt-multi":
    case "prompt-custom":
    case "terminal-dock":
    case "appearance-preview":
    case "theme-import-error":
    case "custom-css-active":
    case "remote-appearance-sheet":
      return {};
  }
}

/**
 * Two queued rows, the second one refused: a refused delivery is never silent, so the row stays
 * marked "Not confirmed" (referenceFollowUp's `unknown`) until it is retried or removed.
 */
const QUEUED: readonly FollowUpMessage[] = [
  { id: "queued-1", text: "Then re-run the failing suite and summarise the failures.", ownerId: "qa-owner", epoch: "1", phase: "queued", requestId: null },
  { id: "queued-2", text: "And draft the release note from the diff.", ownerId: "qa-owner", epoch: "1", phase: "unknown", requestId: null },
];

/** Chip markdown is what the composer stores in the draft; the editor draws each as a link. */
const CHIP_DRAFT = [
  "Delegate to [Opus](ferryx://chip/model/opus) and pick up [#42](ferryx://chip/pr/42)",
  "with [run-tests](ferryx://chip/skill/run-tests) on [DESIGN.md](ferryx://chip/file/DESIGN.md),",
  "then run /compact once the turn ends.",
].join(" ");

const CITATION_DRAFT =
  "Keep this note: [the work-row fade mask has to stay composited](ferryx://cite/assistant-2 \"why the mask stays composited\") and quote it in the answer.";

const QA_ANSWER_SCOPE = "qa-pane";

/** The staged media types the contract allows; anything else is staged as plain text. */
const QA_ANSWER_MEDIA_TYPES = ["image/png", "image/jpeg", "image/webp", "text/plain", "application/pdf"] as const;
type QaAnswerMediaType = (typeof QA_ANSWER_MEDIA_TYPES)[number];

function qaAnswerMediaType(type: string): QaAnswerMediaType {
  return (QA_ANSWER_MEDIA_TYPES as readonly string[]).includes(type)
    ? (type as QaAnswerMediaType)
    : "text/plain";
}

/** The card's own upload seam: a staged receipt per file, as the pane's host would answer. */
const qaStageAnswerFiles = async (files: readonly File[]) =>
  files.map((file, index) => ({
    receipt: {
      hostId: "qa-host",
      attachmentId: `qa-answer-${index}`,
      sha256: String(index).repeat(64),
      sizeBytes: file.size,
      mediaType: qaAnswerMediaType(file.type),
    },
    displayName: file.name,
    mentionText: `@${file.name}`,
  }));

/**
 * Seed one ready answer attachment for the prompt states, so the card's attach row shows its
 * upload state instead of an empty button. The set is keyed exactly as the card keys it
 * (`promptAttachmentDraftKey(prompt.id, answerScope)`), and the seed runs before the card's
 * first render because the harness calls it while rendering its parent.
 */
export function seedChatQaAnswerAttachments(state: string): void {
  const prompt = PROMPTS[state as ChatQaState];
  if (!prompt) return;
  const key = promptAttachmentDraftKey(prompt.id, QA_ANSWER_SCOPE);
  if (referenceAnswerAttachmentStore.get(key).length > 0) return;
  const file = new File(["qa screenshot bytes"], "screenshot-390x844.png", { type: "image/png" });
  const { added } = referenceAnswerAttachmentStore.add(key, [file]);
  if (added.length === 0) return;
  referenceAnswerAttachmentStore.markReady(key, added.map((each) => each.id), [
    {
      receipt: {
        hostId: "qa-host",
        attachmentId: "qa-answer-seed",
        sha256: "e".repeat(64),
        sizeBytes: file.size,
        mediaType: "image/png",
      },
      displayName: file.name,
      mentionText: `@${file.name}`,
    },
  ]);
}

const CHIP_MODELS: readonly ChipItem[] = [
  { id: "model-opus", kind: "model", label: "Opus", detail: "reasoning" },
  { id: "model-sonnet", kind: "model", label: "Sonnet", detail: "balanced" },
];
const CHIP_SKILLS: readonly ChipItem[] = [
  { id: "skill-test", kind: "skill", label: "run-tests", detail: "Run the project test suite" },
];
const CHIP_SOURCES: ChipSources = {
  models: CHIP_MODELS,
  skills: CHIP_SKILLS,
  threads: async (query) => [
    { id: "thread-1", kind: "thread", label: `feat/mobile-chat ${query}`.trim(), detail: "12 messages" },
  ],
  pullRequests: async () => [
    { id: "pr-42", kind: "pr", label: "#42 Chat parity", detail: "open" },
  ],
};

const PERMISSION_CONTROL: PermissionControl = {
  modes: ["supervised", "auto-accept-edits", "auto", "full-access"],
  current: "supervised",
  onChange: () => undefined,
};

const TOOL_GROUP_ITEMS: MobileChatMessageProps["toolCalls"] = [
  { toolName: "grep", summary: "Searched for the composer props", command: "rg -n 'chipSources' src/remote", output: "2 files", status: "success", workKey: "qa-t1" },
  { toolName: "read", summary: "Read ui/src/remote/chat/MobileChatComposer.tsx", output: "312 lines", status: "success", workKey: "qa-t2" },
  { toolName: "bash", summary: "Ran the chat tests", command: "bun run test src/remote/chat", output: "44 passed", status: "running", workKey: "qa-t3" },
  { toolName: "edit", summary: "Edited MobileChatWorkspace.tsx", output: "permission denied", status: "error", workKey: "qa-t4" },
];

const TOOL_GROUP_MESSAGES: readonly MobileChatMessageProps[] = [
  { id: "qa-user", role: "user", content: "Find every composer prop and run the chat tests.", timestamp: 1_700_000_000_000 },
  {
    id: "qa-assistant-tools",
    role: "assistant",
    content: "I searched the composer props and started the chat test suite.",
    timestamp: 1_700_000_060_000,
    toolCalls: TOOL_GROUP_ITEMS,
  },
];

/**
 * A proposed plan is a rich part, not prose: `isPlanPart` is a runtime guard over a kind the
 * frozen union does not name yet, so the fixture builds the wire shape and casts once.
 */
const PLAN_PART = {
  kind: "plan",
  markdown: [
    "## Proposed plan",
    "",
    "1. Land the workspace seams (this lane).",
    "2. Land the composer rich text (composer lane).",
    "3. Land the message renderer upgrades (messages lane).",
    "4. Integrate and QA at both sizes.",
  ].join("\n"),
} as unknown as ReferencePart;

const PLAN_CARD_MESSAGES: readonly MobileChatMessageProps[] = [
  { id: "qa-user-plan", role: "user", content: "Plan the chat parity rollout.", timestamp: 1_700_000_000_000 },
  {
    id: "qa-assistant-plan",
    role: "assistant",
    content: "",
    timestamp: 1_700_000_060_000,
    referenceParts: [PLAN_PART],
  },
];

const MERMAID_MESSAGES: readonly MobileChatMessageProps[] = [
  {
    id: "qa-assistant-mermaid",
    role: "assistant",
    content: [
      "The submit path looks like this:",
      "",
      "```mermaid",
      "graph LR;",
      "  A[Composer] --> B[Submit lane];",
      "  B --> C[Owning host];",
      "```",
    ].join("\n"),
    timestamp: 1_700_000_060_000,
  },
];

const LIGHTBOX_MESSAGES: readonly MobileChatMessageProps[] = [
  {
    id: "qa-assistant-image",
    role: "assistant",
    content: "Here is the screenshot at phone width.",
    timestamp: 1_700_000_060_000,
    attachments: [{ id: "qa-img-1", name: "screenshot-390x844.png", type: "image", url: "/ferryx-icon.png", size: "184 KB" }],
  },
];

const FIND_BAR_MESSAGES: readonly MobileChatMessageProps[] = [
  { id: "qa-user-find", role: "user", content: "Where does the anchor logic live?", timestamp: 1_700_000_000_000 },
  {
    id: "qa-assistant-find",
    role: "assistant",
    content: "The anchor logic lives in workspace/timelineScrollAnchoring.ts, and the visit model in workspace/timelineWatermark.ts.",
    timestamp: 1_700_000_060_000,
  },
];

/** Message-stream seeds for the QA states that show timeline content. */
export function chatQaMessages(state: string): readonly MobileChatMessageProps[] | null {
  switch (state) {
    case "tool-groups-all":
    case "tool-groups-hide-successful":
    case "tool-groups-hide-all":
    case "tool-groups-zen":
      return TOOL_GROUP_MESSAGES;
    case "plan-card":
      return PLAN_CARD_MESSAGES;
    case "mermaid":
      return MERMAID_MESSAGES;
    case "lightbox":
      return LIGHTBOX_MESSAGES;
    case "find-bar":
      return FIND_BAR_MESSAGES;
    default:
      return null;
  }
}

/** A neutral stand-in for the terminal grid: the real grid needs a live socket. */
export const QaTerminalSurface: React.FC = () => (
  <div
    data-testid="qa-terminal-placeholder"
    aria-hidden="true"
    className="min-h-0 w-full flex-1 bg-terminal"
  />
);
