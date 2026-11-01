import { useEffect, useMemo, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

import {
  browserImportTypeLabel,
  cancelBrowserImport,
  describeBrowserImportResult,
  formatBrowserImportError,
  importPasswordFile,
  listBrowserImportSources,
  PASSWORD_FILE_FORMATS,
  runBrowserImport,
  type BrowserImportSource,
  type BrowserImportType,
  type BrowserImportTypeResult,
  type PasswordFileFormat,
} from "../../lib/browserImport";
import { Button } from "../ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "../ui/dialog";

type Profile = { id: string; name: string };

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Ferryx profiles that keep data (private is excluded by the caller). */
  profiles: Profile[];
  defaultProfileId: string;
};

const PASSWORD_FILE_SOURCE = "password-file";
const selectClass =
  "h-8 w-full rounded-md border border-border bg-background px-2 text-[12px] text-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60";

export function ImportBrowserDataDialog({ open, onOpenChange, profiles, defaultProfileId }: Props) {
  const [sources, setSources] = useState<BrowserImportSource[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [sourceId, setSourceId] = useState("");
  const [profileId, setProfileId] = useState(defaultProfileId);
  const [types, setTypes] = useState<ReadonlySet<BrowserImportType>>(new Set());
  const [passwordFormat, setPasswordFormat] = useState<PasswordFileFormat>("onePasswordPux");
  const [passwordFile, setPasswordFile] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<BrowserImportTypeResult[] | null>(null);
  const [runError, setRunError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let live = true;
    setSources(null);
    setLoadError(null);
    setResults(null);
    setRunError(null);
    listBrowserImportSources().then(
      (found) => {
        if (!live) return;
        setSources(found);
        setSourceId((current) => current || (found[0]?.id ?? PASSWORD_FILE_SOURCE));
      },
      (error: unknown) => {
        if (!live) return;
        setSources([]);
        setSourceId(PASSWORD_FILE_SOURCE);
        setLoadError(formatBrowserImportError(error));
      },
    );
    return () => {
      live = false;
    };
  }, [open]);

  const source = useMemo(
    () => sources?.find((candidate) => candidate.id === sourceId) ?? null,
    [sources, sourceId],
  );

  // Offer every type the source has, checked by default.
  useEffect(() => {
    setTypes(new Set(source?.types ?? []));
    setPasswordFile(null);
  }, [source]);

  const fromPasswordFile = sourceId === PASSWORD_FILE_SOURCE;
  const needsPasswordCsv = Boolean(source?.passwordsNeedFile && types.has("passwords"));
  const canRun =
    !running &&
    Boolean(profileId) &&
    (fromPasswordFile ? Boolean(passwordFile) : types.size > 0 && (!needsPasswordCsv || Boolean(passwordFile)));

  const chooseFile = async (extensions: string[]) => {
    const picked = await openFileDialog({ multiple: false, filters: [{ name: "Password export", extensions }] });
    if (typeof picked === "string") setPasswordFile(picked);
  };

  const toggle = (type: BrowserImportType) => {
    setTypes((current) => {
      const next = new Set(current);
      if (next.has(type)) next.delete(type);
      else next.add(type);
      return next;
    });
  };

  const run = async () => {
    setRunning(true);
    setResults(null);
    setRunError(null);
    try {
      if (fromPasswordFile) {
        if (!passwordFile) return;
        setResults([await importPasswordFile({ format: passwordFormat, path: passwordFile, profileId })]);
      } else {
        const report = await runBrowserImport({
          sourceId,
          profileId,
          types: [...types],
          ...(needsPasswordCsv && passwordFile ? { passwordCsvPath: passwordFile } : {}),
        });
        setResults(report.results);
      }
    } catch (error) {
      setRunError(formatBrowserImportError(error));
    } finally {
      setRunning(false);
    }
  };

  const formatEntry = PASSWORD_FILE_FORMATS.find((entry) => entry.format === passwordFormat);

  return (
    <Dialog open={open} onOpenChange={(next) => (!running ? onOpenChange(next) : undefined)}>
      <DialogContent closeDisabled={running} aria-describedby="import-browser-data-description">
        <DialogTitle>Import browser data</DialogTitle>
        <DialogDescription id="import-browser-data-description">
          Bring bookmarks, history, cookies and passwords from another browser or a password manager.
        </DialogDescription>

        <div className="grid gap-3">
          <label className="grid gap-1 text-[12px]">
            <span className="text-muted-foreground">Import from</span>
            <select
              aria-label="Import from"
              className={selectClass}
              value={sourceId}
              disabled={running || sources === null}
              onChange={(event) => setSourceId(event.target.value)}
            >
              {sources === null ? <option value="">Looking for browsers…</option> : null}
              {sources?.map((candidate) => (
                <option key={candidate.id} value={candidate.id}>
                  {candidate.name}
                </option>
              ))}
              <option value={PASSWORD_FILE_SOURCE}>Password manager export file</option>
            </select>
          </label>
          {loadError ? (
            <div role="alert" className="text-[11px] text-destructive">
              Finding browsers failed: {loadError}
            </div>
          ) : null}
          {sources !== null && sources.length === 0 && !loadError ? (
            <div className="text-[11px] text-muted-foreground">No other browsers were found on this computer.</div>
          ) : null}

          <label className="grid gap-1 text-[12px]">
            <span className="text-muted-foreground">Into Ferryx profile</span>
            <select
              aria-label="Into Ferryx profile"
              className={selectClass}
              value={profileId}
              disabled={running}
              onChange={(event) => setProfileId(event.target.value)}
            >
              {profiles.map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.name}
                </option>
              ))}
            </select>
          </label>

          {fromPasswordFile ? (
            <div className="grid gap-2">
              <label className="grid gap-1 text-[12px]">
                <span className="text-muted-foreground">File format</span>
                <select
                  aria-label="File format"
                  className={selectClass}
                  value={passwordFormat}
                  disabled={running}
                  onChange={(event) => {
                    setPasswordFormat(event.target.value as PasswordFileFormat);
                    setPasswordFile(null);
                  }}
                >
                  {PASSWORD_FILE_FORMATS.map((entry) => (
                    <option key={entry.format} value={entry.format}>
                      {entry.label}
                    </option>
                  ))}
                </select>
              </label>
              <FilePicker
                file={passwordFile}
                disabled={running}
                onChoose={() => void chooseFile(formatEntry?.extensions ?? ["csv"])}
              />
            </div>
          ) : source ? (
            <fieldset className="grid gap-1.5" disabled={running}>
              <legend className="mb-1 text-[12px] text-muted-foreground">Data</legend>
              {source.types.map((type) => (
                <label key={type} className="flex items-center gap-2 text-[12px]">
                  <input type="checkbox" checked={types.has(type)} onChange={() => toggle(type)} />
                  {browserImportTypeLabel(type)}
                </label>
              ))}
              {source.requiresFullDiskAccess ? (
                <div className="text-[11px] text-muted-foreground">
                  macOS asks for Full Disk Access before Ferryx can read {source.name}.
                </div>
              ) : null}
              {needsPasswordCsv ? (
                <div className="grid gap-1">
                  <div className="text-[11px] text-muted-foreground">
                    {source.name} passwords: export them to a CSV file first (File › Export › Passwords).
                  </div>
                  <FilePicker file={passwordFile} disabled={running} onChoose={() => void chooseFile(["csv"])} />
                </div>
              ) : null}
            </fieldset>
          ) : null}
        </div>

        {results ? (
          <ul aria-label="Import results" className="grid gap-1 text-[12px]">
            {results.map((result) => (
              <li key={result.type} className={result.error ? "text-destructive" : "text-foreground"}>
                {describeBrowserImportResult(result)}
                {result.notes.map((note) => (
                  <div key={note} className="text-[11px] text-muted-foreground">
                    {note}
                  </div>
                ))}
              </li>
            ))}
          </ul>
        ) : null}
        {runError ? (
          <div role="alert" className="text-[12px] text-destructive">
            {runError}
          </div>
        ) : null}

        <div className="flex justify-end gap-2">
          {running && !fromPasswordFile ? (
            <Button type="button" variant="outline" size="sm" onClick={() => void cancelBrowserImport()}>
              Stop
            </Button>
          ) : (
            <Button type="button" variant="outline" size="sm" onClick={() => onOpenChange(false)}>
              {results ? "Done" : "Cancel"}
            </Button>
          )}
          <Button type="button" size="sm" disabled={!canRun} onClick={() => void run()}>
            {running ? "Importing…" : "Import"}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function FilePicker({ file, disabled, onChoose }: { file: string | null; disabled: boolean; onChoose: () => void }) {
  return (
    <div className="flex items-center gap-2">
      <Button type="button" variant="outline" size="sm" disabled={disabled} onClick={onChoose}>
        Choose file…
      </Button>
      <span className="min-w-0 truncate font-mono text-[11px] text-muted-foreground">{file ?? "No file chosen"}</span>
    </div>
  );
}
