import { ArrowUp, Folder, Home, LoaderCircle, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { cn } from "../lib/cn";
import { listRemoteDirectories, type RemoteDirectoryListing, type DirectorySource } from "../lib/remoteDirectories";
import { toIpcError } from "../lib/tauri";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Switch } from "./ui/switch";

interface RemoteDirectoryPickerProps {
  readonly hostId?: string;
  readonly source?: DirectorySource;
  readonly disabled: boolean;
  readonly onSelect: (path: string | null) => void;
}

/** Codes meaning the remote refused the path itself, as opposed to a transient connection/IO failure. */
const REJECTED_PATH_CODES: ReadonlySet<string> = new Set([
  "NOT_FOUND", "DIRECTORY_NOT_FOUND", "PERMISSION_DENIED", "INVALID_PATH", "PATH_OUTSIDE_WORKSPACE",
]);

/** Completions are a background nicety: typing must not flood the remote host with RPCs. */
const COMPLETION_DEBOUNCE_MS = 180;

/**
 * A typed value split into the absolute path it denotes and the directory whose children complete
 * the trailing fragment. `absolute` is what the backend accepts; `null` means "not resolvable yet"
 * (a bare fragment, or `~` before the remote home is known). An empty `fragment` means the value
 * ended with a separator, so the whole path is the folder to browse.
 */
interface TypedPath {
  readonly absolute: string | null;
  readonly directory: string | null;
  readonly fragment: string;
  readonly browseHome: boolean;
}

/**
 * A browse the picker can replay. `navigate` replaces the field and owns the selection; `complete`
 * only lists children behind a typed fragment. A completion with no fragment lists the folder the
 * user just finished typing and adopts its canonical spelling; one with a fragment re-emits the
 * direct path the user typed (`select`) once the remote confirms the parent, and never `null` for a
 * resolvable path.
 */
type BrowseRequest =
  | { readonly kind: "navigate"; readonly path: string | null; readonly suffix: string }
  | { readonly kind: "complete"; readonly path: string | null; readonly prefix: string; readonly select: string | null };

const HOME: BrowseRequest = { kind: "navigate", path: null, suffix: "" };
const UNRESOLVED_HOME: TypedPath = { absolute: null, directory: null, fragment: "", browseHome: true };

/** A drive path (`C:\dir`) or a UNC path naming its share (`\\server\share`); `\\server` alone is not. */
function isWindowsAbsolute(value: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(value) || /^\\\\[^\\/]+[\\/]+[^\\/]+/.test(value);
}

function splitTyped(value: string, windows: boolean): TypedPath {
  const last = windows ? Math.max(value.lastIndexOf("/"), value.lastIndexOf("\\")) : value.lastIndexOf("/");
  // A drive root (`C:\`) and a share root (`\\server\share`) are browsable whole; their parent is not.
  const atRoot = windows && ((last === 2 && value[1] === ":" && value.length === 3)
    || /^\\\\[^\\/]+[\\/]+[^\\/]+$/.test(value));
  if (atRoot) return { absolute: value, directory: value, fragment: "", browseHome: false };
  if (last < 0) return { absolute: value, directory: null, fragment: value, browseHome: false };
  const driveRoot = windows && last === 2 && value[1] === ":" ? value.slice(0, 3) : null;
  const directory = driveRoot ?? (last === 0 ? value.slice(0, 1) : value.slice(0, last));
  return { absolute: value, directory, fragment: value.slice(last + 1), browseHome: false };
}

/** Expand `~` against the remote home so `validate_path` always receives an absolute path. */
function resolveTypedPath(value: string, home: string | null): TypedPath {
  if (value === "~") return { absolute: home, directory: null, fragment: "", browseHome: true };
  const tildeRelative = value.startsWith("~/") || value.startsWith("~\\");
  if (tildeRelative) {
    if (home === null) return UNRESOLVED_HOME;
    const joiner = isWindowsAbsolute(home) ? "\\" : "/";
    const joined = `${home.replace(/[\\/]+$/, "")}${joiner}${value.slice(2)}`;
    return splitTyped(joined, isWindowsAbsolute(joined));
  }
  if (isWindowsAbsolute(value)) return splitTyped(value, true);
  if (value.startsWith("/")) return splitTyped(value, false);
  return { absolute: null, directory: null, fragment: value, browseHome: false };
}

export function RemoteDirectoryPicker({ hostId, source, disabled, onSelect }: RemoteDirectoryPickerProps) {
  const [pathInput, setPathInput] = useState("");
  const [listing, setListing] = useState<RemoteDirectoryListing | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [prefix, setPrefix] = useState("");
  const [open, setOpen] = useState(true);
  const [active, setActive] = useState(-1);
  const [showHidden, setShowHidden] = useState(false);
  const remoteHidden = source ? showHidden : false;
  const previousSource = useRef(source?.key);
  const cache = useRef(new Map<string, Promise<RemoteDirectoryListing>>());
  const requestId = useRef(0);
  const debounce = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingTilde = useRef<string | null>(null);
  const requested = useRef<BrowseRequest>(HOME);
  const pathField = useRef<HTMLInputElement>(null);
  const listId = useId();
  const composing = useRef(false);
  const home = listing?.homePath ?? null;
  const windows = listing ? isWindowsAbsolute(listing.homePath) : isWindowsAbsolute(pathInput);
  const pathKey = (path: string) => windows ? path.replace(/\\/g, "/").toLowerCase() : path;

  /** Any input, source, or lifecycle change discards queued and in-flight work. */
  const invalidate = useCallback(() => {
    requestId.current += 1;
    pendingTilde.current = null;
    if (debounce.current !== null) {
      clearTimeout(debounce.current);
      debounce.current = null;
    }
  }, []);

  /**
   * Browse. A navigation clears the selection while its folder loads. A completion never clears it:
   * with a fragment it re-emits the direct path that was typed, so a reply for the trailing fragment
   * cannot leave a valid path deselected. Typing itself only invalidates (see `updatePath`).
   */
  const load = useCallback(async (request: BrowseRequest, refresh = false) => {
    const id = ++requestId.current;
    requested.current = request;
    const requestPrefix = request.kind === "complete" ? request.prefix : "";
    if (request.kind === "navigate") {
      setPathInput(request.path ?? "~");
      onSelect(null);
    }
    setLoading(true);
    setError(null);
    setPrefix(requestPrefix);
    setActive(-1);
    setOpen(true);
    const includeHidden = remoteHidden || requestPrefix.startsWith(".");
    const cacheKey = (value: string | null) => JSON.stringify([source?.key ?? hostId, value, source ? includeHidden : false]);
    let pending: Promise<RemoteDirectoryListing> | undefined;
    try {
      if (refresh) cache.current.clear();
      pending = cache.current.get(cacheKey(request.path));
      if (!pending) {
        pending = source ? source.directories(request.path, includeHidden)
          : listRemoteDirectories(hostId!, request.path);
        if (cache.current.size >= 32) cache.current.clear();
        cache.current.set(cacheKey(request.path), pending);
      }
      const result = await pending;
      if (id !== requestId.current) return;
      cache.current.set(cacheKey(result.path), pending);
      setListing(result);
      setLoading(false);
      if (request.kind === "navigate") {
        const { suffix } = request;
        setPathInput(suffix && !result.path.endsWith(suffix) ? result.path + suffix : result.path);
        onSelect(result.path);
        return;
      }
      if (pendingTilde.current !== null) {
        // A `~` typed before the first browse settled: emit it expanded now that the home is known.
        const typed = pendingTilde.current;
        pendingTilde.current = null;
        onSelect(resolveTypedPath(typed, result.homePath).absolute);
        return;
      }
      onSelect(request.prefix === "" ? result.path : request.select);
    } catch (cause) {
      if (cache.current.get(cacheKey(request.path)) === pending) cache.current.delete(cacheKey(request.path));
      if (id !== requestId.current) return;
      const failure = toIpcError(cause);
      setError(failure.message);
      setLoading(false);
      // A path the remote explicitly rejected must not stay submittable; transient errors keep it.
      if (REJECTED_PATH_CODES.has(failure.code)) onSelect(null);
    }
  }, [hostId, source, remoteHidden, onSelect]);

  useEffect(() => {
    cache.current.clear();
    setListing(null);
    invalidate();
    const previous = requested.current;
    const retain = source !== undefined && previousSource.current === source.key;
    previousSource.current = source?.key;
    void load(retain ? previous : HOME);
    return invalidate;
  }, [load, invalidate]);

  const navigate = (path: string | null, complete = false) => {
    invalidate();
    const suffix = complete ? (windows && path?.includes("\\") ? "\\" : "/") : "";
    void load({ kind: "navigate", path, suffix });
  };

  const refresh = () => {
    invalidate();
    void load(requested.current, true);
  };

  const updatePath = (value: string) => {
    // Every keystroke invalidates queued and in-flight work, so a late reply can never adopt.
    invalidate();
    setPathInput(value);
    setActive(-1);
    setOpen(true);
    setError(null);
    if (composing.current) {
      setLoading(false);
      setOpen(false);
      return;
    }
    const trimmed = value.trim();
    if (!trimmed) {
      setPrefix("");
      setLoading(false);
      setOpen(false);
      onSelect(null);
      return;
    }
    const typed = resolveTypedPath(trimmed, home);
    if (typed.browseHome && typed.absolute === null) {
      // `~` before the remote home is known: never emit the unexpanded form, browse and emit after.
      onSelect(null);
      pendingTilde.current = trimmed;
      void load({ kind: "complete", path: null, prefix: "", select: null });
      return;
    }
    // A resolvable path is emitted as typed, so editing never leaves a valid path deselected. The
    // settled completion emits the same value, so this is not a flicker. Only a bare fragment,
    // which no directory can resolve, clears the selection.
    onSelect(typed.absolute);
    const listed = listing !== null && typed.directory !== null && pathKey(typed.directory) === pathKey(listing.path);
    const directory = listed && listing !== null ? listing.path : typed.directory;
    if (typed.fragment === "") {
      // A separator ends the path: list that folder now, without rewriting what the user typed.
      void load({ kind: "complete", path: directory, prefix: "", select: null });
      return;
    }
    // Recorded before the debounce so Refresh replays the typed completion, not the previous folder.
    const request: BrowseRequest = { kind: "complete", path: directory, prefix: typed.fragment, select: typed.absolute };
    requested.current = request;
    setPrefix(typed.fragment);
    // Children of the listed folder are already here, so the filter is immediate and exact.
    if (!listed) setLoading(true);
    debounce.current = setTimeout(() => {
      debounce.current = null;
      void load(request);
    }, COMPLETION_DEBOUNCE_MS);
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
            onCompositionStart={() => {
              composing.current = true;
              invalidate();
            }}
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
