import { ChevronDown, ChevronUp, Search, X } from "lucide-react";
import React, { useCallback, useEffect, useRef, useState } from "react";

import type { TerminalInstance } from "../lib/terminalHostManager";
import { IconButton } from "./ui/IconButton";

export type TerminalSearchOverlayProps = {
  instance?: TerminalInstance;
  onClose: () => void;
};

export function TerminalSearchOverlay({ instance, onClose }: TerminalSearchOverlayProps) {
  const [query, setQuery] = useState("");
  const [resultIndex, setResultIndex] = useState<number | null>(null);
  const [resultCount, setResultCount] = useState<number | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  useEffect(() => {
    if (!instance?.searchAddon?.onDidChangeResults) return;
    const disposable = instance.searchAddon.onDidChangeResults((e) => {
      setResultIndex(e.resultIndex);
      setResultCount(e.resultCount);
    });
    return () => {
      disposable?.dispose?.();
    };
  }, [instance]);

  const handleFindNext = useCallback(() => {
    if (!query || !instance?.searchAddon) return;
    instance.searchAddon.findNext(query, { incremental: false });
  }, [instance, query]);

  const handleFindPrevious = useCallback(() => {
    if (!query || !instance?.searchAddon) return;
    instance.searchAddon.findPrevious(query, { incremental: false });
  }, [instance, query]);

  const handleQueryChange = (newQuery: string) => {
    setQuery(newQuery);
    if (!newQuery) {
      instance?.searchAddon?.clearDecorations();
      setResultIndex(null);
      setResultCount(null);
    } else {
      instance?.searchAddon?.findNext(newQuery, { incremental: true });
    }
  };

  const handleClose = useCallback(() => {
    instance?.searchAddon?.clearDecorations();
    onClose();
    instance?.terminal?.focus();
  }, [instance, onClose]);

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
      className="no-drag absolute top-2 right-4 z-40 flex items-center gap-1 rounded border border-border bg-card/95 p-1 text-foreground shadow-lg backdrop-blur-sm"
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
