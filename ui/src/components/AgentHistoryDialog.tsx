import {
  AlertCircle,
  AlertTriangle,
  History,
  Loader2,
  MessageSquare,
  Play,
  Search,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import {
  readAgentHistory,
  searchAgentHistory,
  type AgentHistoryEntry,
  type AgentHistoryMessage,
  type AgentHistoryProvider,
} from "../lib/agentHistory";
import { cn } from "../lib/cn";
import { toIpcError } from "../lib/tauri";
import { type StructuredIpcError } from "../lib/types";

export type AgentHistoryServices = {
  search: typeof searchAgentHistory;
  read: typeof readAgentHistory;
};

export interface AgentHistoryDialogProps {
  workspaceId: string;
  projectName: string;
  cwd: string | null;
  onClose: () => void;
  onResume: (entry: AgentHistoryEntry) => void;
  services?: AgentHistoryServices;
}

const DEFAULT_PAGE_LIMIT = 20;

function createDefaultServices(): AgentHistoryServices {
  return {
    search: searchAgentHistory,
    read: readAgentHistory,
  };
}

export function AgentHistoryDialog({
  projectName,
  cwd,
  onClose,
  onResume,
  services: injectedServices,
}: AgentHistoryDialogProps) {
  const servicesRef = useRef<AgentHistoryServices>(injectedServices ?? createDefaultServices());
  useEffect(() => {
    if (injectedServices) servicesRef.current = injectedServices;
  }, [injectedServices]);

  const searchGeneration = useRef(0);
  const readGeneration = useRef(0);

  const [provider, setProvider] = useState<AgentHistoryProvider>("claude");
  const [query, setQuery] = useState("");
  const [isSearching, setIsSearching] = useState(false);
  const [isLoadingMoreEntries, setIsLoadingMoreEntries] = useState(false);
  const [searchError, setSearchError] = useState<StructuredIpcError | null>(null);
  const [hasSearched, setHasSearched] = useState(false);

  const [entries, setEntries] = useState<AgentHistoryEntry[]>([]);
  const [nextEntryCursor, setNextEntryCursor] = useState<string | null>(null);
  const [entriesPartial, setEntriesPartial] = useState(false);
  const [entriesWarnings, setEntriesWarnings] = useState<string[]>([]);

  const [selectedEntry, setSelectedEntry] = useState<AgentHistoryEntry | null>(null);
  const [messages, setMessages] = useState<AgentHistoryMessage[]>([]);
  const [nextMessageCursor, setNextMessageCursor] = useState<string | null>(null);
  const [isLoadingMessages, setIsLoadingMessages] = useState(false);
  const [isLoadingMoreMessages, setIsLoadingMoreMessages] = useState(false);
  const [messageError, setMessageError] = useState<StructuredIpcError | null>(null);
  const [messagesPartial, setMessagesPartial] = useState(false);
  const [messagesWarnings, setMessagesWarnings] = useState<string[]>([]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  useEffect(() => {
    searchGeneration.current += 1;
    readGeneration.current += 1;
    setEntries([]);
    setNextEntryCursor(null);
    setEntriesWarnings([]);
    setSearchError(null);
    setIsSearching(false);
    setSelectedEntry(null);
    setMessages([]);
    setNextMessageCursor(null);
    setMessagesWarnings([]);
    setMessageError(null);
    setIsLoadingMessages(false);
    // Abandoning an in-flight read also drops its pagination spinner: the stale handler cannot
    // clear it (its generation is gone), and a stuck flag would disable all later pagination.
    setIsLoadingMoreMessages(false);
  }, [provider]);

  const handleSearch = useCallback(
    async (resetCursor = true) => {
      const gen = ++searchGeneration.current;
      setIsSearching(true);
      setSearchError(null);
      setHasSearched(true);
      if (resetCursor) {
        readGeneration.current += 1;
        setEntries([]);
        setSelectedEntry(null);
        setMessages([]);
        setNextEntryCursor(null);
        setNextMessageCursor(null);
        setEntriesWarnings([]);
        setMessagesWarnings([]);
        setMessageError(null);
        setIsLoadingMessages(false);
        setIsLoadingMoreMessages(false);
      }

      try {
        const page = await servicesRef.current.search({
          provider,
          cwd,
          query: query.trim(),
          cursor: resetCursor ? null : nextEntryCursor,
          limit: DEFAULT_PAGE_LIMIT,
        });

        if (searchGeneration.current !== gen) return;

        if (resetCursor) {
          setEntries(page.items);
        } else {
          setEntries((prev) => [...prev, ...page.items]);
        }
        setNextEntryCursor(page.nextCursor);
        setEntriesPartial(page.partial);
        setEntriesWarnings(page.warnings ?? []);
      } catch (err: unknown) {
        if (searchGeneration.current !== gen) return;
        setSearchError(toIpcError(err));
      } finally {
        if (searchGeneration.current === gen) {
          setIsSearching(false);
          setIsLoadingMoreEntries(false);
        }
      }
    },
    [cwd, nextEntryCursor, provider, query],
  );

  const handleLoadMoreEntries = () => {
    if (!nextEntryCursor || isSearching || isLoadingMoreEntries) return;
    setIsLoadingMoreEntries(true);
    void handleSearch(false);
  };

  const handleSelectEntry = async (entry: AgentHistoryEntry) => {
    if (selectedEntry?.entryKey === entry.entryKey) return;
    const gen = ++readGeneration.current;
    setSelectedEntry(entry);
    setMessages([]);
    setNextMessageCursor(null);
    setMessagesWarnings([]);
    setMessageError(null);
    setIsLoadingMessages(true);
    setIsLoadingMoreMessages(false);

    try {
      const page = await servicesRef.current.read({
        entryKey: entry.entryKey,
        cursor: null,
        limit: DEFAULT_PAGE_LIMIT,
      });
      if (readGeneration.current !== gen) return;
      setMessages(page.items);
      setNextMessageCursor(page.nextCursor);
      setMessagesPartial(page.partial);
      setMessagesWarnings(page.warnings ?? []);
    } catch (err: unknown) {
      if (readGeneration.current !== gen) return;
      setMessageError(toIpcError(err));
    } finally {
      if (readGeneration.current === gen) {
        setIsLoadingMessages(false);
      }
    }
  };

  const handleLoadMoreMessages = async () => {
    if (!selectedEntry || !nextMessageCursor || isLoadingMessages || isLoadingMoreMessages) return;
    const gen = ++readGeneration.current;
    setIsLoadingMoreMessages(true);
    try {
      const page = await servicesRef.current.read({
        entryKey: selectedEntry.entryKey,
        cursor: nextMessageCursor,
        limit: DEFAULT_PAGE_LIMIT,
      });
      if (readGeneration.current !== gen) return;
      setMessages((prev) => [...prev, ...page.items]);
      setNextMessageCursor(page.nextCursor);
      setMessagesPartial(page.partial);
      setMessagesWarnings(page.warnings ?? []);
    } catch (err: unknown) {
      if (readGeneration.current !== gen) return;
      setMessageError(toIpcError(err));
    } finally {
      if (readGeneration.current === gen) {
        setIsLoadingMoreMessages(false);
      }
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onMouseDown={onClose}
    >
      <div
        role="dialog"
        aria-label="Past conversations"
        aria-modal="true"
        className="flex max-h-[85vh] w-full max-w-5xl flex-col animate-enter overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex items-center justify-between border-b border-border px-5 py-3.5">
          <div className="flex items-center gap-2.5 text-sm font-semibold text-foreground">
            <History className="size-4 text-primary" />
            <span>Past Conversations</span>
            {projectName ? (
              <span className="rounded-md bg-muted px-2 py-0.5 text-xs font-normal text-muted-foreground">
                {projectName}
              </span>
            ) : null}
            {cwd ? (
              <span className="max-w-xs truncate font-mono text-[11px] text-muted-foreground" title={cwd}>
                {cwd}
              </span>
            ) : null}
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onClose}
              aria-label="Close dialog"
              className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
            >
              <X className="size-4" />
            </button>
          </div>
        </header>

        <div className="border-b border-border/80 bg-muted/20 px-5 py-3">
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void handleSearch(true);
            }}
            className="flex flex-wrap items-center gap-2.5 text-xs"
          >
            <div className="flex items-center gap-1.5">
              <label htmlFor="agent-history-provider" className="text-[11px] font-medium text-muted-foreground">
                Provider:
              </label>
              <select
                id="agent-history-provider"
                value={provider}
                onChange={(e) => {
                  setProvider(e.target.value as AgentHistoryProvider);
                  setHasSearched(false);
                  readGeneration.current += 1;
                  setEntries([]);
                  setSelectedEntry(null);
                  setMessages([]);
                  setNextMessageCursor(null);
                  setMessagesWarnings([]);
                  setMessageError(null);
                  setIsLoadingMessages(false);
                }}
                className="h-8 rounded-md border border-input bg-background px-2.5 text-xs text-foreground outline-none focus:border-ring"
              >
                <option value="claude">Claude</option>
                <option value="codex">Codex</option>
              </select>
            </div>

            <div className="relative flex-1 min-w-[200px]">
              <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <input
                type="text"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search past conversations..."
                aria-label="Search past conversations"
                className="h-8 w-full rounded-md border border-input bg-background pl-8 pr-3 text-xs text-foreground placeholder:text-muted-foreground outline-none focus:border-ring"
              />
            </div>

            <button
              type="submit"
              disabled={isSearching}
              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-primary px-3.5 text-xs font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
            >
              {isSearching ? <Loader2 className="size-3.5 animate-spin" /> : <Search className="size-3.5" />}
              <span>Search</span>
            </button>
          </form>
        </div>

        <div className="selectable flex min-h-0 flex-1 overflow-hidden text-xs">
          <div className="flex w-1/2 flex-col border-r border-border overflow-y-auto p-4">
            {searchError ? (
              <div role="alert" className="mb-4 rounded-lg border border-destructive/35 bg-destructive/10 p-3 text-destructive">
                <div className="flex items-start gap-2">
                  <AlertCircle className="mt-0.5 size-4 shrink-0" />
                  <div className="flex-1">
                    <div className="font-semibold">Search failed</div>
                    <div className="text-[11px] leading-relaxed text-destructive/90">{searchError.message}</div>
                  </div>
                </div>
              </div>
            ) : null}

            {entriesPartial ? (
              <div className="mb-3 flex items-center gap-1.5 rounded-md border border-status-warning/30 bg-status-warning/10 px-2.5 py-1.5 text-[11px] text-status-warning">
                <AlertTriangle className="size-3.5 shrink-0" />
                <span>Partial results returned</span>
              </div>
            ) : null}

            {entriesWarnings.length > 0 ? (
              <div className="mb-3 space-y-1 rounded-md border border-status-warning/30 bg-status-warning/10 p-2.5 text-[11px] text-status-warning">
                {entriesWarnings.map((warn, i) => (
                  <div key={i} className="flex items-center gap-1.5">
                    <AlertTriangle className="size-3.5 shrink-0" />
                    <span>{warn}</span>
                  </div>
                ))}
              </div>
            ) : null}

            {hasSearched && !isSearching && entries.length === 0 && !searchError ? (
              <div className="flex flex-1 flex-col items-center justify-center rounded-lg border border-dashed border-border py-12 text-center text-muted-foreground">
                <MessageSquare className="mb-2 size-8 text-muted-foreground/40" />
                <div className="text-sm font-medium text-foreground">No conversations found</div>
                <p className="mt-1 text-xs">Try adjusting your search query or provider.</p>
              </div>
            ) : !hasSearched && entries.length === 0 ? (
              <div className="flex flex-1 flex-col items-center justify-center py-12 text-center text-muted-foreground">
                <Search className="mb-2 size-8 text-muted-foreground/30" />
                <p className="text-xs">Enter a search query or press Search to browse past conversations.</p>
              </div>
            ) : null}

            {entries.length > 0 ? (
              <div className="space-y-2">
                {entries.map((entry) => {
                  const isSelected = selectedEntry?.entryKey === entry.entryKey;
                  return (
                    <div
                      key={entry.entryKey}
                      onClick={() => void handleSelectEntry(entry)}
                      className={cn(
                        "group flex cursor-pointer flex-col gap-1.5 rounded-lg border p-3 transition-colors",
                        isSelected
                          ? "border-primary bg-primary/5 text-foreground"
                          : "border-border bg-background/40 hover:bg-accent/40",
                      )}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <div className="flex items-center gap-2 min-w-0">
                          <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[10px] uppercase text-muted-foreground">
                            {entry.provider}
                          </span>
                          <span
                            className="truncate font-mono text-xs font-semibold text-foreground"
                            title={entry.providerSession.id}
                          >
                            {entry.providerSession.id}
                          </span>
                        </div>
                        <button
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            onResume(entry);
                          }}
                          className="inline-flex shrink-0 items-center gap-1 rounded-md bg-primary/10 px-2 py-1 text-[11px] font-medium text-primary hover:bg-primary hover:text-primary-foreground transition-colors"
                        >
                          <Play className="size-3" />
                          <span>Resume</span>
                        </button>
                      </div>

                      <div className="flex flex-col gap-0.5 text-[11px] text-muted-foreground">
                        <div className="truncate font-mono" title={entry.cwd}>
                          {entry.cwd}
                        </div>
                        {entry.version ? <div>Version: {entry.version}</div> : null}
                      </div>
                    </div>
                  );
                })}

                {nextEntryCursor ? (
                  <div className="pt-2 text-center">
                    <button
                      type="button"
                      disabled={isLoadingMoreEntries}
                      onClick={handleLoadMoreEntries}
                      className="inline-flex items-center gap-1.5 rounded-md border border-border px-3 py-1.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
                    >
                      {isLoadingMoreEntries ? <Loader2 className="size-3.5 animate-spin" /> : null}
                      <span>Load more</span>
                    </button>
                  </div>
                ) : null}
              </div>
            ) : null}
          </div>

          <div className="flex w-1/2 flex-col overflow-y-auto p-4 bg-muted/10">
            {selectedEntry ? (
              <div className="flex flex-col h-full">
                <div className="mb-3 border-b border-border/60 pb-2">
                  <div className="flex items-center justify-between">
                    <span className="font-semibold text-foreground">Conversation Preview</span>
                    <span className="font-mono text-[10px] text-muted-foreground">{selectedEntry.entryKey}</span>
                  </div>
                </div>

                {messageError ? (
                  <div role="alert" className="mb-3 rounded-lg border border-destructive/35 bg-destructive/10 p-2.5 text-destructive">
                    <div className="flex items-start gap-2 text-xs">
                      <AlertCircle className="mt-0.5 size-3.5 shrink-0" />
                      <span>{messageError.message}</span>
                    </div>
                  </div>
                ) : null}

                {messagesPartial ? (
                  <div className="mb-2 flex items-center gap-1.5 text-[11px] text-status-warning">
                    <AlertTriangle className="size-3.5 shrink-0" />
                    <span>Partial messages</span>
                  </div>
                ) : null}
                {messagesWarnings.length > 0 ? (
                  <div className="mb-2 space-y-1 text-[11px] text-status-warning">
                    {messagesWarnings.map((warn, i) => (
                      <div key={i} className="flex items-center gap-1.5">
                        <AlertTriangle className="size-3.5 shrink-0" />
                        <span>{warn}</span>
                      </div>
                    ))}
                  </div>
                ) : null}

                {isLoadingMessages ? (
                  <div className="flex flex-1 items-center justify-center text-muted-foreground">
                    <Loader2 className="mr-2 size-4 animate-spin text-status-working" />
                    <span>Loading messages…</span>
                  </div>
                ) : messages.length === 0 && !messageError ? (
                  <div className="flex flex-1 items-center justify-center text-muted-foreground">
                    <span>No messages found in this conversation.</span>
                  </div>
                ) : (
                  <div className="flex-1 space-y-3 overflow-y-auto pr-1">
                    {messages.map((msg, index) => (
                      <div
                        key={msg.id ?? `${msg.ordinal}-${index}`}
                        className="rounded-lg border border-border/60 bg-background/50 p-2.5"
                      >
                        <div className="mb-1 flex items-center justify-between text-[10px] text-muted-foreground">
                          <span className="font-semibold uppercase tracking-wider text-foreground/80">
                            {msg.role}
                          </span>
                          <span>#{msg.ordinal}</span>
                        </div>
                        <div className="whitespace-pre-wrap font-mono text-[11px] leading-relaxed text-foreground/90">
                          {msg.text}
                        </div>
                      </div>
                    ))}

                    {nextMessageCursor ? (
                      <div className="pt-2 text-center">
                        <button
                          type="button"
                          disabled={isLoadingMoreMessages}
                          onClick={() => void handleLoadMoreMessages()}
                          className="inline-flex items-center gap-1.5 rounded-md border border-border px-3 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
                        >
                          {isLoadingMoreMessages ? <Loader2 className="size-3.5 animate-spin" /> : null}
                          <span>Load more</span>
                        </button>
                      </div>
                    ) : null}
                  </div>
                )}
              </div>
            ) : (
              <div className="flex flex-1 flex-col items-center justify-center text-center text-muted-foreground">
                <MessageSquare className="mb-2 size-8 text-muted-foreground/30" />
                <p className="text-xs">Select a conversation to view its messages.</p>
              </div>
            )}
          </div>
        </div>

        <footer className="flex items-center justify-end border-t border-border px-5 py-3">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md border border-border bg-background px-4 py-2 text-xs font-medium text-foreground hover:bg-accent"
          >
            Close
          </button>
        </footer>
      </div>
    </div>
  );
}
