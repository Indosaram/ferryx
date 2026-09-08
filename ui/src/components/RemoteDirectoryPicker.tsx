import { ArrowUp, Folder, Home, LoaderCircle, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { listRemoteDirectories, type RemoteDirectoryListing } from "../lib/remoteDirectories";
import { toIpcError } from "../lib/tauri";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Switch } from "./ui/switch";

interface RemoteDirectoryPickerProps {
  readonly hostId: string;
  readonly disabled: boolean;
  readonly onSelect: (path: string | null) => void;
}

export function RemoteDirectoryPicker({ hostId, disabled, onSelect }: RemoteDirectoryPickerProps) {
  const [pathInput, setPathInput] = useState("");
  const [listing, setListing] = useState<RemoteDirectoryListing | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [showHidden, setShowHidden] = useState(false);
  const cache = useRef(new Map<string | null, RemoteDirectoryListing>());
  const requestId = useRef(0);
  const requestedPath = useRef<string | null>(null);
  const pathField = useRef<HTMLInputElement>(null);

  const navigate = useCallback(async (path: string | null, refresh = false) => {
    const id = ++requestId.current;
    requestedPath.current = path;
    setPathInput(path ?? "~");
    onSelect(null);
    setLoading(true);
    setError(null);
    setFilter("");
    try {
      if (refresh) cache.current.clear();
      const result = cache.current.get(path) ?? await listRemoteDirectories(hostId, path);
      if (id !== requestId.current) return;
      if (cache.current.size >= 32) cache.current.clear();
      cache.current.set(path, result);
      cache.current.set(result.path, result);
      setListing(result);
      setPathInput(result.path);
      setLoading(false);
      onSelect(result.path);
    } catch (cause) {
      if (id !== requestId.current) return;
      setError(toIpcError(cause).message);
      setLoading(false);
    }
  }, [hostId, onSelect]);

  useEffect(() => {
    void navigate(null);
    return () => { requestId.current += 1; };
  }, [navigate]);

  const entries = listing?.entries.filter((entry) =>
    (showHidden || !entry.hidden) && entry.name.toLocaleLowerCase().includes(filter.toLocaleLowerCase()),
  ) ?? [];

  return (
    <fieldset disabled={disabled} className="min-w-0 space-y-3" aria-label="Remote folders">
      <div className="space-y-1">
        <label htmlFor="remote-repo-path" className="text-[11px] text-muted-foreground">Remote folder</label>
        <div className="flex gap-2">
          <Input
            ref={pathField}
            id="remote-repo-path"
            aria-label="Remote repository path"
            data-testid="remote-repo-path-input"
            className="h-8 min-w-0 font-mono text-xs"
            value={pathInput}
            placeholder="~"
            autoComplete="off"
            spellCheck={false}
            onChange={(event) => {
              requestId.current += 1;
              setLoading(false);
              setError(null);
              setPathInput(event.target.value);
              onSelect(null);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                event.stopPropagation();
                if (!event.nativeEvent.isComposing && pathInput) void navigate(pathInput);
              }
            }}
          />
          <Button type="button" size="sm" variant="outline" disabled={!pathInput}
            onClick={() => void navigate(pathInput)}>Go</Button>
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <Button type="button" size="sm" variant="ghost" aria-label="Remote home" title="Remote home"
          onClick={() => void navigate(null)}><Home className="size-3.5" /></Button>
        <Button type="button" size="sm" variant="ghost" aria-label="Parent folder" title="Parent folder"
          disabled={!listing?.parentPath}
          onClick={() => { if (listing?.parentPath) void navigate(listing.parentPath); }}>
          <ArrowUp className="size-3.5" />
        </Button>
        <Button type="button" size="sm" variant="ghost" aria-label="Refresh folders" title="Refresh folders"
          onClick={() => void navigate(requestedPath.current, true)}><RefreshCw className="size-3.5" /></Button>
        <div className="ml-auto flex items-center gap-2">
          <label htmlFor="remote-show-hidden" className="text-[11px] text-muted-foreground">Hidden folders</label>
          <Switch id="remote-show-hidden" checked={showHidden} onCheckedChange={setShowHidden} />
        </div>
      </div>
      <Input aria-label="Filter folders" placeholder="Filter folders" className="h-8 text-xs"
        value={filter} onChange={(event) => setFilter(event.target.value)}
        onKeyDown={(event) => { if (event.key === "Enter") event.preventDefault(); }} />
      <div className="h-56 overflow-y-auto rounded-md border border-border bg-background"
        aria-busy={loading} aria-label="Folder contents">
        {loading ? (
          <div role="status" className="flex items-center justify-center gap-2 p-6 text-xs text-muted-foreground">
            <LoaderCircle className="size-3.5 animate-spin motion-reduce:animate-none" />Loading folders...
          </div>
        ) : error ? (
          <div role="alert" className="space-y-3 break-words p-3 text-xs text-destructive">
            <p>{error}</p>
            <Button type="button" size="sm" variant="outline"
              onClick={() => void navigate(requestedPath.current, true)}>Retry</Button>
          </div>
        ) : entries.length === 0 ? (
          <p role="status" className="p-6 text-center text-xs text-muted-foreground">
            {filter ? "No folders match this filter." : "No visible subfolders."}
          </p>
        ) : (
          <ul className="p-1">
            {entries.map((entry) => (
              <li key={entry.path}>
                <button type="button" className="flex h-8 w-full items-center gap-2 rounded-sm px-2 text-left text-xs hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  title={entry.path} onClick={() => {
                    void navigate(entry.path);
                    pathField.current?.focus();
                  }}>
                  <Folder className="size-3.5 shrink-0 text-muted-foreground" />
                  <span className="truncate">{entry.name}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
      {!loading && !error && listing?.truncated ? (
        <p role="status" className="text-[11px] text-muted-foreground">
          Some folders are not shown. Enter a full path to open a folder outside this list.
        </p>
      ) : null}
    </fieldset>
  );
}
