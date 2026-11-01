// Seeded composer states for real-browser QA of the production chat paths: the
// ReferencePromptCard in the composer, held messages, staged files, a disconnected chat,
// the on-screen keyboard and the terminal key dock.
import React from "react";

import { userFacingError } from "../remote/userFacingError";
import { ReferencePromptCard } from "../remote/chat/ReferencePromptCard";
import type { HeldMessage } from "../remote/chat/referenceQueue";
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
] as const;

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
    { label: "Phone home (Recommended)", description: "Supervisor, Inbox and Worktrees tabs" },
    { label: "Session chat", description: "Messages, composer and prompt cards" },
    { label: "Terminal", description: "Tab strip, key dock and line mode" },
    { label: "Account and sign-in", description: null },
  ],
  multiSelect: true,
  customOptionIndex: null,
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
    case "prompt-approval":
    case "prompt-multi":
    case "prompt-custom":
    case "terminal-dock":
      return {};
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
