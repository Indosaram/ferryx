import { LoaderCircle, Send, Sparkles, Square, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";

import { desktopUnifiedClient } from "../../../lib/supervisor";
import {
  newRequestId,
  parseSubmitReceipt,
  submitEnvelope,
  type UnifiedSupervisorClient,
} from "../../../lib/unifiedSupervisor";
import type { BrowserStoreTab } from "../../../state/browserStore";
import type { BrowserAgentActivity } from "./MahoAgentControl";
import { displayHost } from "./mahoBrowserTabActions";

export type PageAgentEntry = {
  id: string;
  prompt: string;
  status: "submitting" | "admitted" | "awaiting_owner" | "conversational" | "needs_choice" | "error";
  summary: string;
  goalId?: string;
  errorCode?: string;
};

export function buildBrowserPageAgentInstruction(tab: BrowserStoreTab | null, prompt: string): string {
  const trimmed = prompt.trim();
  if (!tab) return trimmed;
  const title = tab.title.trim();
  const lines = [
    `[Browser page context: browserId=${tab.browserId}, url=${tab.url}${title ? `, title=${title}` : ""}]`,
    "Operate on this browser tab using the Ferryx browser agent tools.",
    "",
    trimmed,
  ];
  return lines.join("\n");
}

export type MahoPageAgentPanelProps = {
  tab: BrowserStoreTab | null;
  activity: BrowserAgentActivity;
  pendingPrompt?: { id: number; text: string } | null;
  onPendingPromptHandled?: () => void;
  onClose: () => void;
  client?: UnifiedSupervisorClient;
};

export function MahoPageAgentPanel({
  tab,
  activity,
  pendingPrompt = null,
  onPendingPromptHandled,
  onClose,
  client = desktopUnifiedClient,
}: MahoPageAgentPanelProps) {
  const [draft, setDraft] = useState("");
  const [entries, setEntries] = useState<PageAgentEntry[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const handledPromptIdRef = useRef<number | null>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const isControlled = Boolean(tab && activity.controlledBrowserIds.includes(tab.browserId));
  const controllerLabel = tab ? activity.controllerLabelFor(tab.browserId) : null;
  const activeReceipt = tab ? activity.receiptFor(tab.browserId) : null;

  const submitPrompt = useCallback(
    async (rawPrompt: string) => {
      const prompt = rawPrompt.trim();
      if (!prompt || submitting) return;
      const entryId = newRequestId("desktop", "submit");
      setEntries((current) => [
        ...current,
        { id: entryId, prompt, status: "submitting", summary: "Attaching agent to page…" },
      ]);
      setSubmitting(true);
      try {
        const instruction = buildBrowserPageAgentInstruction(tab, prompt);
        const envelope = submitEnvelope(entryId, null, {
          text: instruction,
          doneWhen: prompt,
          verifier: "",
        });
        const result = await client.request(envelope);
        if (result.kind === "error") {
          setEntries((current) =>
            current.map((item) =>
              item.id === entryId
                ? {
                    ...item,
                    status: "error",
                    errorCode: result.code,
                    summary: result.message || result.code,
                  }
                : item,
            ),
          );
          return;
        }
        client.refresh();
        const parsed = parseSubmitReceipt(result.value);
        setEntries((current) =>
          current.map((item) => {
            if (item.id !== entryId) return item;
            if (parsed.outcome === "admitted" || parsed.outcome === "awaiting_owner") {
              return {
                ...item,
                status: parsed.outcome,
                goalId: parsed.goalId,
                summary:
                  parsed.acknowledgment
                  || (parsed.outcome === "awaiting_owner"
                    ? "Waiting for your approval."
                    : "Agent attached and working on this page."),
              };
            }
            if (parsed.outcome === "conversational") {
              return { ...item, status: "conversational", summary: parsed.reason || "Answered." };
            }
            if (parsed.outcome === "needs_choice") {
              return { ...item, status: "needs_choice", summary: parsed.detail || parsed.missing };
            }
            return { ...item, status: "admitted", summary: "Sent to agent." };
          }),
        );
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        setEntries((current) =>
          current.map((item) =>
            item.id === entryId
              ? { ...item, status: "error", errorCode: "INTERNAL_ERROR", summary: message }
              : item,
          ),
        );
      } finally {
        setSubmitting(false);
      }
    },
    [client, submitting, tab],
  );

  useEffect(() => {
    if (!pendingPrompt || handledPromptIdRef.current === pendingPrompt.id) return;
    handledPromptIdRef.current = pendingPrompt.id;
    onPendingPromptHandled?.();
    void submitPrompt(pendingPrompt.text);
  }, [onPendingPromptHandled, pendingPrompt, submitPrompt]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const handleSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (!draft.trim() || submitting) return;
    const text = draft;
    setDraft("");
    void submitPrompt(text);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      if (!draft.trim() || submitting) return;
      const text = draft;
      setDraft("");
      void submitPrompt(text);
    }
  };

  const hostLabel = tab ? displayHost(tab.url) : "No active page";

  return (
    <section
      data-testid="maho-page-agent-panel"
      aria-label="Page agent"
      className="flex h-full min-h-0 flex-col bg-card text-foreground"
    >
      <header className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-3 py-2">
        <div className="flex min-w-0 items-center gap-1.5">
          <Sparkles className="size-3.5 shrink-0 text-primary" aria-hidden="true" />
          <span className="truncate text-xs font-semibold">Ask Agent</span>
        </div>
        <button
          type="button"
          aria-label="Close page agent"
          onClick={onClose}
          className="inline-flex size-6 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
        >
          <X className="size-3.5" aria-hidden="true" />
        </button>
      </header>

      <div className="shrink-0 border-b border-border/70 bg-muted/40 px-3 py-2 text-[11px]">
        <div className="flex items-center justify-between gap-2">
          <span className="truncate font-medium text-foreground" data-testid="maho-page-agent-context">
            {tab ? tab.title.trim() || hostLabel : "No active tab"}
          </span>
          {tab ? <span className="shrink-0 text-muted-foreground">{hostLabel}</span> : null}
        </div>
        {isControlled && tab ? (
          <div className="mt-1.5 flex items-center justify-between gap-2 text-primary">
            <span className="truncate">
              {controllerLabel ? `${controllerLabel} is operating on this page` : "Agent operating on this page"}
            </span>
            <button
              type="button"
              onClick={() => void activity.takeOver(tab.browserId)}
              className="inline-flex shrink-0 items-center gap-1 rounded border border-border bg-background px-1.5 py-0.5 text-[10px] font-medium text-foreground hover:bg-accent"
            >
              <Square className="size-2.5" aria-hidden="true" />
              Stop
            </button>
          </div>
        ) : null}
        {activeReceipt ? (
          <p className="mt-1 truncate text-[10px] text-muted-foreground">{activeReceipt}</p>
        ) : null}
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-3 py-3 text-xs">
        {entries.length === 0 ? (
          <p className="text-muted-foreground">
            {tab
              ? "Tell the agent what to inspect, click, fill, or automate on this page."
              : "Open a page first, or ask the agent to open and act on a site."}
          </p>
        ) : (
          entries.map((entry) => (
            <div
              key={entry.id}
              data-testid="maho-page-agent-entry"
              data-status={entry.status}
              className="space-y-1.5 rounded-lg border border-border bg-background p-2.5"
            >
              <p className="font-medium text-foreground">{entry.prompt}</p>
              <p
                role={entry.status === "error" ? "alert" : "status"}
                className={entry.status === "error" ? "text-[11px] text-destructive" : "text-[11px] text-muted-foreground"}
              >
                {entry.status === "submitting" ? (
                  <span className="inline-flex items-center gap-1">
                    <LoaderCircle className="size-3 animate-spin" aria-hidden="true" />
                    {entry.summary}
                  </span>
                ) : entry.errorCode ? (
                  `${entry.errorCode}: ${entry.summary}`
                ) : (
                  entry.summary
                )}
              </p>
            </div>
          ))
        )}
      </div>

      <form onSubmit={handleSubmit} className="shrink-0 border-t border-border p-2.5">
        <div className="flex items-end gap-1.5 rounded-lg border border-border bg-background px-2.5 py-1.5 focus-within:ring-1 focus-within:ring-ring">
          <textarea
            ref={inputRef}
            rows={2}
            value={draft}
            disabled={submitting}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={handleKeyDown}
            aria-label="Ask agent on this page"
            placeholder={tab ? `Ask agent on ${hostLabel}…` : "Ask agent…"}
            className="min-h-[36px] flex-1 resize-none bg-transparent text-xs text-foreground placeholder:text-muted-foreground focus:outline-none"
          />
          <button
            type="submit"
            disabled={!draft.trim() || submitting}
            aria-label="Send to page agent"
            className="inline-flex size-7 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
          >
            <Send className="size-3.5" aria-hidden="true" />
          </button>
        </div>
      </form>
    </section>
  );
}
