import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertCircle, ExternalLink, Loader2, Puzzle, RefreshCw, Settings2, Store } from "lucide-react";

import {
  CHROME_WEB_STORE_URL,
  BROWSER_ENGINE_CHANGED_EVENT,
  listBrowserExtensions,
  openBrowserExtensionPage,
  openBrowserUrlInNewTab,
  type BrowserExtensionListing,
  type BrowserExtensionPage,
  type InstalledExtension,
} from "../../lib/browserExtensions";
import {
  DEFAULT_BROWSER_PROFILE,
  PRIVATE_BROWSER_PROFILE,
  supportedBrowserProfiles,
  useBrowserSettings,
} from "../../lib/browserSettings";
import { extractBrowserErrorCode } from "../../lib/browserTauri";
import { isStructuredIpcError } from "../../lib/tauri";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { IconButton } from "../ui/IconButton";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { SettingRow, SettingsGroup } from "./primitives";

const MANAGE_TOOLTIP = "Enable, disable or remove in Chrome's extension page";

function failureMessage(failure: unknown): string {
  if (isStructuredIpcError(failure) && failure.message.trim()) return failure.message.trim();
  return "Extensions could not be loaded";
}

function focusEngineInstall(): void {
  const root = document.getElementById("browser-engine");
  if (!root) return;
  root.scrollIntoView({ behavior: "smooth", block: "center" });
  root.querySelector<HTMLElement>('[data-testid="browser-engine-action"]')?.focus();
}

function ExtensionRow({
  extension,
  onOpen,
}: {
  extension: InstalledExtension;
  onOpen: (page: BrowserExtensionPage) => void;
}) {
  return (
    <div
      className="flex items-center justify-between gap-3 border-b border-border/40 py-3 last:border-0"
      data-testid="browser-extension-row"
      data-extension-id={extension.id}
    >
      <div className="flex min-w-0 items-center gap-3">
        <div className="flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-muted/40">
          {extension.iconDataUrl ? (
            <img src={extension.iconDataUrl} alt="" className="size-5 rounded-sm" />
          ) : (
            <Puzzle className="size-4 text-muted-foreground" aria-hidden="true" />
          )}
        </div>
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2">
            <span className="truncate text-[13px] font-medium text-foreground" title={extension.name}>
              {extension.name}
            </span>
            <span className="shrink-0 text-[11px] text-muted-foreground">{extension.version}</span>
            <Badge
              variant={extension.enabled ? "secondary" : "outline"}
              className="shrink-0 text-[11px] font-medium"
              data-testid="browser-extension-status"
              data-enabled={extension.enabled}
            >
              {extension.enabled ? "Enabled" : "Disabled"}
            </Badge>
          </div>
          {extension.description ? (
            <p className="truncate text-[11px] text-muted-foreground" title={extension.description}>
              {extension.description}
            </p>
          ) : null}
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {extension.optionsUrl ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 px-2 text-[11px]"
            onClick={() => onOpen({ kind: "options", id: extension.id })}
            data-testid="browser-extension-options"
          >
            Options
          </Button>
        ) : null}
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="h-7 px-2 text-[11px]"
          title={MANAGE_TOOLTIP}
          onClick={() => onOpen({ kind: "details", id: extension.id })}
          data-testid="browser-extension-manage"
        >
          Manage
        </Button>
      </div>
    </div>
  );
}

/** Lists the extensions installed in a Chromium profile and opens Chrome's own pages to manage them. */
export function BrowserExtensionsGroup() {
  const { settings } = useBrowserSettings();
  const profiles = useMemo(() => {
    const list = supportedBrowserProfiles(settings).filter((profile) => profile.id !== PRIVATE_BROWSER_PROFILE.id);
    const defaultProfile = list.find((profile) => profile.id === DEFAULT_BROWSER_PROFILE.id) ?? DEFAULT_BROWSER_PROFILE;
    return [defaultProfile, ...list.filter((profile) => profile.id !== defaultProfile.id)];
  }, [settings]);
  const [profileId, setProfileId] = useState<string>(DEFAULT_BROWSER_PROFILE.id);
  const activeProfileId = profiles.some((profile) => profile.id === profileId) ? profileId : DEFAULT_BROWSER_PROFILE.id;

  const [listing, setListing] = useState<BrowserExtensionListing | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void listBrowserExtensions(activeProfileId).then(
      (next) => {
        if (cancelled) return;
        setListing(next);
        setLoading(false);
      },
      (failure: unknown) => {
        if (cancelled) return;
        setListing(null);
        setError(failureMessage(failure));
        setLoading(false);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [activeProfileId, reloadToken]);

  useEffect(() => {
    const reload = () => setReloadToken((token) => token + 1);
    window.addEventListener(BROWSER_ENGINE_CHANGED_EVENT, reload);
    return () => window.removeEventListener(BROWSER_ENGINE_CHANGED_EVENT, reload);
  }, []);

  const open = useCallback(
    (page: BrowserExtensionPage) => {
      setOpenError(null);
      void openBrowserExtensionPage(page, activeProfileId).catch((failure: unknown) => {
        setOpenError(
          isStructuredIpcError(failure) && failure.message.trim() ? failure.message.trim() : extractBrowserErrorCode(failure),
        );
      });
    },
    [activeProfileId],
  );

  const ready = listing != null && listing.supported && listing.engineInstalled;

  const actions = ready ? (
    <>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="h-7 gap-1.5 px-2 text-[11px]"
        onClick={() => openBrowserUrlInNewTab(CHROME_WEB_STORE_URL, activeProfileId)}
        data-testid="browser-extensions-store"
      >
        <Store className="size-3.5" aria-hidden="true" />
        Open Chrome Web Store
      </Button>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="h-7 gap-1.5 px-2 text-[11px]"
        onClick={() => open({ kind: "manage" })}
        data-testid="browser-extensions-manage-all"
      >
        <Settings2 className="size-3.5" aria-hidden="true" />
        Manage all
      </Button>
      <IconButton
        label="Refresh extensions"
        onClick={() => setReloadToken((token) => token + 1)}
        disabled={loading}
        data-testid="browser-extensions-refresh"
      >
        <RefreshCw className={loading ? "size-3.5 animate-spin" : "size-3.5"} aria-hidden="true" />
      </IconButton>
    </>
  ) : undefined;

  return (
    <SettingsGroup title="Extensions" action={actions}>
      {profiles.length > 1 ? (
        <SettingRow label="Profile" description="Extensions are installed per browser profile.">
          <Select value={activeProfileId} onValueChange={setProfileId}>
            <SelectTrigger
              id="browser-extensions-profile"
              aria-label="Extensions profile"
              className="h-8 w-[180px] text-[11px]"
              data-testid="browser-extensions-profile"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {profiles.map((profile) => (
                <SelectItem key={profile.id} value={profile.id}>
                  {profile.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingRow>
      ) : null}

      {error ? (
        <div
          role="alert"
          className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2"
        >
          <AlertCircle className="mt-0.5 size-3.5 shrink-0 text-destructive" aria-hidden="true" />
          <p className="min-w-0 text-[12px] text-foreground" data-testid="browser-extensions-error">{error}</p>
        </div>
      ) : null}

      {listing == null && loading ? (
        <div className="flex items-center gap-2 py-3 text-[11px] text-muted-foreground" data-testid="browser-extensions-loading">
          <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
          Loading extensions...
        </div>
      ) : null}

      {listing && !listing.supported ? (
        <p className="py-3 text-[12px] text-muted-foreground" data-testid="browser-extensions-unsupported">
          Extensions need the Chromium engine, which is not available on this platform yet.
        </p>
      ) : null}

      {listing && listing.supported && !listing.engineInstalled ? (
        <div
          className="flex items-center justify-between gap-3 py-3"
          data-testid="browser-extensions-not-installed"
        >
          <p className="min-w-0 text-[12px] text-foreground">Extensions need the full browser engine</p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 shrink-0 gap-1.5 px-2 text-[11px]"
            onClick={focusEngineInstall}
            data-testid="browser-extensions-install-engine"
          >
            <ExternalLink className="size-3.5" aria-hidden="true" />
            Install engine
          </Button>
        </div>
      ) : null}

      {ready ? (
        listing.extensions.length === 0 ? (
          <p className="py-3 text-[12px] text-muted-foreground" data-testid="browser-extensions-empty">
            No extensions installed in this profile yet.
          </p>
        ) : (
          <div>
            {listing.extensions.map((extension) => (
              <ExtensionRow key={extension.id} extension={extension} onOpen={open} />
            ))}
          </div>
        )
      ) : null}

      {openError ? (
        <div role="alert" className="mt-2 text-[11px] text-destructive" data-testid="browser-extensions-open-error">
          {openError}
        </div>
      ) : null}

      {ready ? (
        <p className="mt-2 text-[11px] text-muted-foreground">
          Changes made in Chrome's extension page show here after Refresh.
        </p>
      ) : null}
    </SettingsGroup>
  );
}
