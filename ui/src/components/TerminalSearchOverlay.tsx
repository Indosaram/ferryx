import { ChevronDown, ChevronUp, Search, X } from "lucide-react";
import React, { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

import { scrollNativeTerminal } from "../lib/tauri";
import { TERMINAL_SEARCH_DOCK_HEIGHT } from "../lib/terminalSearchLayout";
import { IconButton } from "./ui/IconButton";

export type NativeTerminalSearchMatch = {
  row: number;
  startCol: number;
  endCol: number;
};

export type NativeTerminalSearchResult = {
  matches: NativeTerminalSearchMatch[];
  totalMatches: number;
};

export type TerminalSearchOverlayProps = {
  /** Search occupies its own 48px strip outside native surface bounds. */
  docked?: boolean;
  sessionId?: string;
  onClose: () => void;
  onFocusTerminal?: () => void;
};

function parseSearchResponse(raw: unknown): NativeTerminalSearchMatch[] {
  if (!raw) return [];
  if (Array.isArray(raw)) {
    return raw.map((item) => {
      if (Array.isArray(item)) {
        return {
          row: Number(item[0] ?? 0),
          startCol: Number(item[1] ?? 0),
          endCol: Number(item[2] ?? 0),
        };
      }
      if (item && typeof item === "object") {
        const obj = item as Record<string, unknown>;
        return {
          row: Number(obj.row ?? 0),
          startCol: Number(obj.startCol ?? obj.start_col ?? 0),
          endCol: Number(obj.endCol ?? obj.end_col ?? 0),
        };
      }
      return { row: 0, startCol: 0, endCol: 0 };
    });
  }
  if (typeof raw === "object") {
    const obj = raw as Record<string, unknown>;
    if (Array.isArray(obj.matches)) {
      return parseSearchResponse(obj.matches);
    }
  }
  return [];
}

export function TerminalSearchOverlay({
  docked = false,
  sessionId,
  onClose,
  onFocusTerminal,
}: TerminalSearchOverlayProps) {
  const [query, setQuery] = useState("");
  const [resultIndex, setResultIndex] = useState<number | null>(null);
  const [resultCount, setResultCount] = useState<number | null>(null);
  const [nativeMatches, setNativeMatches] = useState<NativeTerminalSearchMatch[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);
  // Monotonic id of the most recent search request. Any in-flight promise whose
  // captured id no longer matches is stale (superseded by a newer request, a
  // cleared query, a session change, or unmount) and must not touch state.
  const requestSeq = useRef(0);
  const queryRef = useRef(query);
  queryRef.current = query;

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const performNativeSearch = useCallback((searchQuery: string) => {
    const seq = ++requestSeq.current;
    if (!sessionId || !isTauri() || !searchQuery) {
      setResultIndex(null);
      setResultCount(searchQuery ? 0 : null);
      setNativeMatches([]);
      return;
    }

    void invoke<NativeTerminalSearchResult | Array<[number, number, number]>>("cmd_native_terminal_search", {
      sessionId,
      query: searchQuery,
      caseSensitive: false,
    })
      .then((response) => {
        if (seq !== requestSeq.current) return;
        const matches = parseSearchResponse(response);
        if (matches.length > 0) {
          setNativeMatches(matches);
          setResultCount(matches.length);
          setResultIndex(0);
          const first = matches[0];
          if (first) {
            void scrollNativeTerminal(sessionId, { type: "row", offset: first.row }).catch(() => {});
          }
        } else {
          setNativeMatches([]);
          setResultCount(0);
          setResultIndex(null);
        }
      })
      .catch(() => {
        if (seq !== requestSeq.current) return;
        setNativeMatches([]);
        setResultCount(0);
        setResultIndex(null);
      });
  }, [sessionId]);

  // On session change (and initial mount) drop any counts/matches tied to the
  // previous session, invalidate its in-flight request, and re-run the current
  // query against the new session so the counter never shows stale results.
  useEffect(() => {
    requestSeq.current += 1;
    setResultIndex(null);
    setResultCount(null);
    setNativeMatches([]);
    const currentQuery = queryRef.current;
    if (currentQuery) {
      performNativeSearch(currentQuery);
    }
  }, [sessionId, performNativeSearch]);

  // Invalidate any pending search on unmount so a late resolution cannot set
  // state on an unmounted component.
  useEffect(() => () => {
    requestSeq.current += 1;
  }, []);

  const handleFindNext = useCallback(() => {
    if (!query) return;
    if (sessionId) {
      if (nativeMatches.length > 0) {
        setResultIndex((prev) => {
          const current = prev ?? 0;
          const next = (current + 1) % nativeMatches.length;
          const target = nativeMatches[next];
          if (target) {
            void scrollNativeTerminal(sessionId, { type: "row", offset: target.row }).catch(() => {});
          }
          return next;
        });
      } else {
        performNativeSearch(query);
      }
    }
  }, [nativeMatches, performNativeSearch, query, sessionId]);

  const handleFindPrevious = useCallback(() => {
    if (!query) return;
    if (sessionId) {
      if (nativeMatches.length > 0) {
        setResultIndex((prev) => {
          const current = prev ?? 0;
          const prevIndex = (current - 1 + nativeMatches.length) % nativeMatches.length;
          const target = nativeMatches[prevIndex];
          if (target) {
            void scrollNativeTerminal(sessionId, { type: "row", offset: target.row }).catch(() => {});
          }
          return prevIndex;
        });
      } else {
        performNativeSearch(query);
      }
    }
  }, [nativeMatches, performNativeSearch, query, sessionId]);

  const handleQueryChange = (newQuery: string) => {
    setQuery(newQuery);
    if (!newQuery) {
      // Clearing the query supersedes any in-flight search so its late result
      // cannot repopulate counts/matches.
      requestSeq.current += 1;
      setResultIndex(null);
      setResultCount(null);
      setNativeMatches([]);
    } else if (sessionId) {
      performNativeSearch(newQuery);
    }
  };

  const handleClose = useCallback(() => {
    onClose();
    onFocusTerminal?.();
  }, [onClose, onFocusTerminal]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      if (e.shiftKey) {
        handleFindPrevious();
      } else {
        handleFindNext();
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      handleClose();
    }
  };

  return (
    <div
      role="search"
      aria-label="Terminal search"
      data-native-terminal-keep-visible={docked ? "true" : undefined}
      data-native-terminal-search-layout={docked ? "docked" : "floating"}
      style={docked ? { height: TERMINAL_SEARCH_DOCK_HEIGHT } : undefined}
      className={`no-drag absolute z-40 flex items-center gap-1 border border-border bg-card/95 p-1 text-foreground shadow-lg backdrop-blur-sm ${docked
        ? "inset-x-0 top-0 justify-end border-x-0 border-t-0 px-4"
        : "top-2 right-4 rounded"}`}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          handleClose();
        }
      }}
    >
      <div className="relative flex items-center">
        <Search className="pointer-events-none absolute left-2 size-3 text-muted-foreground" />
        <input
          ref={inputRef}
          type="text"
          aria-label="Find in terminal"
          data-testid="terminal-search-input"
          placeholder="Find..."
          value={query}
          onChange={(e) => handleQueryChange(e.target.value)}
          onKeyDown={handleKeyDown}
          className="h-6 w-44 rounded border border-border bg-background pl-6 pr-2 text-xs text-foreground placeholder:text-muted-foreground/60 focus:border-primary focus:outline-none"
        />
      </div>
      {resultCount !== null && query.length > 0 ? (
        <span className="px-1 font-mono text-[10px] text-muted-foreground">
          {resultCount === 0 ? "0/0" : `${(resultIndex ?? 0) + 1}/${resultCount}`}
        </span>
      ) : null}
      <IconButton
        label="Previous match"
        aria-label="Previous match"
        size="sm"
        className="size-5 rounded p-0 text-muted-foreground hover:text-foreground"
        onClick={handleFindPrevious}
      >
        <ChevronUp className="size-3" />
      </IconButton>
      <IconButton
        label="Next match"
        aria-label="Next match"
        size="sm"
        className="size-5 rounded p-0 text-muted-foreground hover:text-foreground"
        onClick={handleFindNext}
      >
        <ChevronDown className="size-3" />
      </IconButton>
      <IconButton
        label="Close search"
        aria-label="Close search"
        size="sm"
        className="size-5 rounded p-0 text-muted-foreground hover:text-foreground"
        onClick={handleClose}
      >
        <X className="size-3" />
      </IconButton>
    </div>
  );
}
