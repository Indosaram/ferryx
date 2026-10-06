// Seeded mobile chat states for real-browser QA: open /mobile-chat-qa.html?state=empty|stream|running|long (dev server only).

import React, { useCallback, useMemo, useState } from "react";

import { MobileChatWorkspace } from "../remote/chat/MobileChatWorkspace";
import type { MobileChatMessageProps } from "../remote/chat/MobileChatMessage";
import type { ChatAttachment, ChatWorkItem } from "../remote/chat/MobileChatComponents";

const STATE = (() => {
  if (typeof window === "undefined") return "stream";
  return new URLSearchParams(window.location.search).get("state") ?? "stream";
})();

const WORK_ITEMS: ChatWorkItem[] = [
  {
    kind: "thinking",
    text: "I should check how the remote chat frames are decoded before touching the composer, because the envelope parser is the only thing standing between raw escape bytes and a bubble.",
    workKey: "t1",
    source: "prose",
  },
  {
    toolName: "grep",
    summary: "Searched the remote chat modules for the composer props",
    command: "rg -n 'quickActions|onSelectQuickAction' src/remote",
    output: "src/remote/chat/MobileChatComposer.tsx:67:  readonly quickActions?: readonly unknown[];\n0 consumers found",
    status: "success",
    workKey: "t2",
  },
  {
    toolName: "read",
    summary: "Read ui/src/lib/clipboard.ts",
    command: "sed -n '1,40p' ui/src/lib/clipboard.ts",
    output: "export async function writeText(text: string): Promise<boolean> {\n  try {\n    await navigator.clipboard.writeText(text);\n    return true;\n  } catch {\n    return legacyCopy(text);\n  }\n}",
    status: "success",
    workKey: "t3",
  },
  {
    toolName: "bash",
    summary: "Ran bun run test src/remote/chat",
    command: "bun run test src/remote/chat",
    output: "Test Files  4 passed (4)\n     Tests  44 passed (44)",
    status: "success",
    workKey: "t4",
  },
  {
    toolName: "edit",
    summary: "Edited MobileChatWorkspace.tsx",
    output: "write failed: EACCES: permission denied, open 'MobileChatWorkspace.tsx'",
    status: "error",
    workKey: "t5",
  },
  {
    toolName: "write",
    summary: "Wrote the token layer into index.css",
    output: "ok",
    status: "running",
    workKey: "t6",
  },
];

const ATTACHMENTS: ChatAttachment[] = [
  { id: "a1", name: "screenshot-390x844.png", type: "image", url: "/ferryx-icon.png", size: "184 KB" },
  { id: "a2", name: "transcript-frames.log", type: "file", size: "12.4 KB" },
];

const LONG_PROSE = [
  "The transcript poll re-renders the whole turn list every three seconds, which is why the work-row fade mask has to stay composited.",
  "",
  "```ts",
  "const frames = stripControlSequences(raw);",
  "if (!frames) return;",
  "appendBounded(bubble, frames, 12_000);",
  "```",
  "",
  "- The envelope parser drops `replayGap` frames.",
  "- The optimistic user turn is deduplicated by content.",
  "",
  "> Long tool output stays inside a 15rem scroll window instead of pushing the composer off screen.",
].join("\n");

function buildMessages(state: string): MobileChatMessageProps[] {
  const base: MobileChatMessageProps[] = [
    {
      id: "user-1",
      role: "user",
      content: "Polish the mobile chat view: tokens, a11y, motion. Review it until you find nothing left.",
      timestamp: Date.now() - 240_000,
    },
    {
      id: "assistant-2",
      role: "assistant",
      content: [
        "I audited the phone surface and found the defects in three groups.",
        "",
        "1. **Token layer** — every chat file hardcodes the T3 palette while the neighbouring host drawer already uses semantic tokens.",
        "2. **Accessibility** — the `+`, send and stop controls carry no accessible name.",
        "3. **Motion** — the drawer animates its height and the scroll pill bounces forever.",
        "",
        "```md",
        "DESIGN.md §1: no color, size, spacing, or motion value may appear",
        "in a component unless it traces back to a token named here.",
        "```",
      ].join("\n"),
      timestamp: Date.now() - 200_000,
      durationLabel: "1m 30s",
      toolCalls: WORK_ITEMS.slice(0, 4),
      attachments: [ATTACHMENTS[1]],
    },
    {
      id: "assistant-3",
      role: "assistant",
      content: "Approval is required before I rewrite the shared token layer, since the desktop shell reads the same `:root` block.",
      timestamp: Date.now() - 120_000,
      approvalAction: {
        description: "Add 16 chat tokens to ui/src/index.css :root and map them in tailwind.config.js.",
        confirmLabel: "Approve",
        declineLabel: "Decline",
        onAccept: () => undefined,
        onDecline: () => undefined,
      },
    },
  ];

  if (state === "empty") return [];
  if (state === "running") {
    return [
      ...base.slice(0, 2),
      {
        id: "assistant-4",
        role: "assistant",
        content: "",
        timestamp: Date.now() - 8_000,
        durationLabel: "12s",
        toolCalls: WORK_ITEMS.slice(4),
        activityState: "running_tool",
      },
    ];
  }
  if (state === "long") {
    return [
      base[0],
      {
        id: "assistant-5",
        role: "assistant",
        content: `${LONG_PROSE}\n\n${LONG_PROSE}`,
        timestamp: Date.now() - 60_000,
        durationLabel: "4m 12s",
        toolCalls: [...WORK_ITEMS, ...WORK_ITEMS, ...WORK_ITEMS.slice(0, 2)].map((item, index) => ({
          ...item,
          workKey: `${(item as { workKey?: string }).workKey ?? "w"}-${index}`,
        })),
        attachments: ATTACHMENTS,
      },
    ];
  }
  return base;
}

export const MobileChatQaHarness: React.FC = () => {
  const seeded = useMemo(() => buildMessages(STATE), []);
  const [messages, setMessages] = useState<MobileChatMessageProps[]>(seeded);
  const [isRunning, setIsRunning] = useState(STATE === "running");

  const handleSend = useCallback((text: string) => {
    setMessages((prev) => [
      ...prev,
      { id: `user-${Date.now()}`, role: "user" as const, content: text, timestamp: Date.now() },
    ]);
  }, []);

  return (
    <div className="h-[100dvh] w-full bg-chat-screen">
      <MobileChatWorkspace
        headerTitle="ferryx"
        headerSubtitle="maho-workspace · feature/mobile-chat"
        workspaceLabel="maho-workspace"
        worktreeLabel="feature/mobile-chat"
        onBack={() => undefined}
        messages={messages}
        warnings={
          STATE === "stream"
            ? ["older history is not available for paired-host sessions; showing the most recent messages"]
            : undefined
        }
        isRunning={isRunning}
        onSendMessage={handleSend}
        onStopExecution={() => setIsRunning(false)}
      />
    </div>
  );
};
