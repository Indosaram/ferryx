import { ArrowUp, Folder, Home, LoaderCircle, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { cn } from "../lib/cn";
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
  const [prefix, setPrefix] = useState("");
  const [open, setOpen] = useState(true);
  const [active, setActive] = useState(-1);
  const [showHidden, setShowHidden] = useState(false);
  const cache = useRef(new Map<string | null, Promise<RemoteDirectoryListing>>());
  const requestId = useRef(0);
  const requested = useRef({ path: null as string | null, prefix: "", replaceInput: true, suffix: "" });
  const pathField = useRef<HTMLInputElement>(null);
  const listId = useId();
  const composing = useRef(false);
  const windows = listing ? /^[A-Za-z]:|^\\\\/.test(listing.homePath) : /^[A-Za-z]:|^\\\\/.test(pathInput);
  const pathKey = (path: string) => windows ? path.replace(/\\/g, "/").toLowerCase() : path;

  const load = useCallback(async (
    path: string | null, nextPrefix = "", replaceInput = false, suffix = "", refresh = false,
  ) => {
    const id = ++requestId.current;
    requested.current = { path, prefix: nextPrefix, replaceInput, suffix };
    if (replaceInput) setPathInput(path ?? "~");
    onSelect(null);
    setLoading(true);
    setError(null);
    setPrefix(nextPrefix);
    setActive(-1);
    setOpen(true);
    let pending: Promise<RemoteDirectoryListing> | undefined;
    try {
      if (refresh) cache.current.clear();
      pending = cache.current.get(path);
      if (!pending) {
        pending = listRemoteDirectories(hostId, path);
        if (cache.current.size >= 32) cache.current.clear();
        cache.current.set(path, pending);
      }
      const result = await pending;
      if (id !== requestId.current) return;
      cache.current.set(result.path, pending);
      setListing(result);
      if (replaceInput) {
        setPathInput(suffix && !result.path.endsWith(suffix) ? result.path + suffix : result.path);
      }
      setLoading(false);
      onSelect(nextPrefix ? null : result.path);
    } catch (cause) {
      if (cache.current.get(path) === pending) cache.current.delete(path);
      if (id !== requestId.current) return;
      setError(toIpcError(cause).message);
      setLoading(false);
    }
  }, [hostId, onSelect]);

  useEffect(() => {
    cache.current.clear();
    setListing(null);
    void load(null, "", true);
    return () => { requestId.current += 1; };
  }, [load]);

  const navigate = (path: string | null, complete = false) => {
    const suffix = complete ? (windows && path?.includes("\\") ? "\\" : "/") : "";
    void load(path, "", true, suffix);
  };

  const refresh = () => {
    const request = requested.current;
    void load(request.path, request.prefix, request.replaceInput, request.suffix, true);
  };

  const updatePath = (value: string) => {
    setPathInput(value);
    onSelect(null);
    setActive(-1);
    setOpen(true);
    if (composing.current || !value) {
      requestId.current += 1;
      setLoading(false);
      setOpen(false);
      setError(null);
      return;
    }
    if (value === "~" || (listing && pathKey(value) === pathKey(listing.path))) {
      void load(value === "~" ? null : listing?.path ?? value);
      return;
    }
    const normalized = windows ? value.replace(/\\/g, "/") : value;
    const lastSlash = normalized.lastIndexOf("/");
    const parent = lastSlash < 0 ? null
      : lastSlash === 0 ? "/"
      : windows && lastSlash === 2 && normalized[1] === ":" ? normalized.slice(0, 3)
      : normalized.slice(0, lastSlash);
    const fragment = value.slice(lastSlash + 1);
    const directory = parent && listing && pathKey(parent) === pathKey(listing.path) ? listing.path : parent;
    void load(directory, fragment);
  };

  const entries = !pathInput || composing.current || loading || error ? [] : listing?.entries.filter((entry) =>
    (showHidden || prefix.startsWith(".") || !entry.hidden)
    && entry.name.toLocaleLowerCase().startsWith(prefix.toLocaleLowerCase()),
  ) ?? [];

  useEffect(() => {
    if (open && active >= 0) document.getElementById(`${listId}-${active}`)?.scrollIntoView?.({ block: "nearest" });
  }, [active, open, listId]);

  return (
    <fieldset disabled={disabled} className="min-w-0 space-y-2" aria-label="Remote folders"
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
      }}>
      <div className="space-y-1">
        <label htmlFor="remote-repo-path" className="text-[11px] text-muted-foreground">Remote folder</label>
        <div className="flex gap-2">
          <Input
            ref={pathField}
            autoFocus
            role="combobox"
            aria-expanded={open}
            aria-controls={open ? listId : undefined}
            aria-autocomplete="list"
            aria-activedescendant={open && entries[active] ? `${listId}-${active}` : undefined}
            id="remote-repo-path"
            aria-label="Remote repository path"
            data-testid="remote-repo-path-input"
            className="h-8 min-w-0 font-mono text-xs"
            value={pathInput}
            placeholder="~"
            autoComplete="off"
            spellCheck={false}
            onFocus={() => setOpen(true)}
            onClick={() => setOpen(true)}
            onCompositionStart={() => { composing.current = true; }}
            onCompositionEnd={(event) => {
              composing.current = false;
              updatePath(event.currentTarget.value);
            }}
            onChange={(event) => updatePath(event.target.value)}
            onKeyDown={(event) => {
              if (event.nativeEvent.isComposing || composing.current) {
                if (event.key === "Enter") event.stopPropagation();
                return;
              }
              if (event.key === "Escape" && open) {
                event.preventDefault();
                event.stopPropagation();
                setOpen(false);
                return;
              }
              if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                event.preventDefault();
                event.stopPropagation();
                setOpen(true);
                setActive((index) => entries.length === 0 ? -1 : event.key === "ArrowDown"
                  ? (index + 1) % entries.length : (index <= 0 ? entries.length : index) - 1);
                return;
              }
              if (event.key === "Tab" && !event.shiftKey && open && entries.length > 0) {
                event.preventDefault();
                const entry = entries[active < 0 ? 0 : active];
                if (entry) navigate(entry.path, true);
                return;
              }
              if (event.key === "Enter") {
                event.preventDefault();
                event.stopPropagation();
                const entry = open ? entries[active] ?? (prefix ? entries[0] : undefined) : undefined;
                if (entry) navigate(entry.path, true);
                else if (pathInput) navigate(pathInput);
              }
            }}
          />
        </div>
      </div>
      {open ? (
        <div className="max-h-56 overflow-y-auto rounded-md border border-border bg-popover shadow-md"
          aria-busy={loading} aria-label="Folder contents">
          <ul id={listId} role="listbox" aria-label="Remote folder suggestions" className="p-1">
            {entries.map((entry, index) => (
              <li key={entry.path} role="option" id={`${listId}-${index}`}
                aria-label={entry.name} aria-selected={active === index} aria-disabled={disabled}
                className={cn("flex h-8 cursor-pointer items-center gap-2 rounded-sm px-2 text-xs hover:bg-accent",
                  active === index && "bg-accent text-foreground")}
                title={entry.path}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => {
                  if (disabled) return;
                  navigate(entry.path, true);
                  pathField.current?.focus();
                }}>
                <Folder className="size-3.5 shrink-0 text-muted-foreground" />
                <span className="truncate">{entry.name}</span>
                <span aria-hidden="true" className="ml-auto text-muted-foreground">/</span>
              </li>
            ))}
          </ul>
          {loading ? (
            <div role="status" className="flex items-center justify-center gap-2 p-4 text-xs text-muted-foreground">
              <LoaderCircle className="size-3.5 animate-spin motion-reduce:animate-none" />Loading folders...
            </div>
          ) : error ? (
            <div role="alert" className="space-y-2 break-words p-3 text-xs text-destructive">
              <p>{error}</p>
              <Button type="button" size="sm" variant="outline"
                onClick={refresh}>Retry</Button>
            </div>
          ) : entries.length === 0 ? (
            <p role="status" className="p-4 text-center text-xs text-muted-foreground">
              {prefix ? "No folders match this path." : "No visible subfolders."}
            </p>
          ) : null}
          {!loading && !error && listing?.truncated ? (
            <p role="status" className="p-3 text-[11px] text-muted-foreground">
              Some folders are not shown. Enter a full path to open a folder outside this list.
            </p>
          ) : null}
        </div>
      ) : null}
      <div className="flex flex-wrap items-center gap-2">
        <Button type="button" size="sm" variant="ghost" aria-label="Remote home" title="Remote home"
          onClick={() => navigate(null)}><Home className="size-3.5" /></Button>
        <Button type="button" size="sm" variant="ghost" aria-label="Parent folder" title="Parent folder"
          disabled={loading || !listing?.parentPath}
          onClick={() => { if (listing?.parentPath) navigate(listing.parentPath); }}>
          <ArrowUp className="size-3.5" />
        </Button>
        <Button type="button" size="sm" variant="ghost" aria-label="Refresh folders" title="Refresh folders"
          disabled={!pathInput || composing.current}
          onClick={refresh}><RefreshCw className="size-3.5" /></Button>
        <div className="ml-auto flex items-center gap-2">
          <label htmlFor="remote-show-hidden" className="text-[11px] text-muted-foreground">Hidden folders</label>
          <Switch id="remote-show-hidden" checked={showHidden} onCheckedChange={(checked) => {
            setShowHidden(checked); setActive(-1); setOpen(true);
          }} />
        </div>
      </div>
    </fieldset>
  );
}
