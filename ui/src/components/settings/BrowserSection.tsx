import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Cookie, FolderOpen, Globe, Plus, RotateCcw, Trash2 } from "lucide-react";

import {
  BROWSER_ZOOM_LEVELS,
  DEFAULT_BROWSER_PROFILE,
  browserNamedProfilesSupported,
  isBuiltInBrowserProfileId,
  makeBrowserProfileId,
  normalizeHomePageInput,
  supportedBrowserProfiles,
  useBrowserSettings,
  type BrowserProfile,
  type BrowserSettingsState,
} from "../../lib/browserSettings";
import { clearAllBrowserHistory } from "../../lib/browserLibrary";
import {
  extractBrowserErrorCode,
  focusBrowser,
  formatBrowserCookieImportError,
  importBrowserCookies,
  importInstalledBrowserCookies,
  listBrowsers,
  setBrowserZoom,
} from "../../lib/browserTauri";
import { isInstalledBrowserCookieImportSupported, isMacHost } from "../../lib/platform";
import {
  loadFileOpenTargets,
  saveFileOpenTargets,
  type FileOpenCategory,
  type FileOpenTarget,
} from "../../lib/fileOpenTargets";
import type { BrowserSessionSummary } from "../../lib/types";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";
import { Switch } from "../ui/switch";
import { BrowserEngineGroup } from "./BrowserEngineGroup";
import { SiteSettingsGroup } from "./SiteSettingsGroup";
import { BrowserExtensionsGroup } from "./BrowserExtensionsGroup";
import { BrowserAgentAccessGroup } from "./BrowserAgentAccessGroup";
import { DefaultBrowserGroup } from "./DefaultBrowserGroup";
import { BrowserPasswordsGroup } from "./BrowserPasswordsGroup";
import { SettingRow, SettingsGroup, SettingsHeading } from "./primitives";
import { ImportBrowserDataDialog } from "./ImportBrowserDataDialog";

export function BrowserSection() {
  const { settings, updateSettings, resetSettings } = useBrowserSettings();
  const [homeDraft, setHomeDraft] = useState(settings.homePage);
  const [homeError, setHomeError] = useState<string | null>(null);
  const [tabActionError, setTabActionError] = useState<string | null>(null);
  const [profileDraft, setProfileDraft] = useState("");
  const [importStatus, setImportStatus] = useState<Record<string, string>>({});
  const [importingProfiles, setImportingProfiles] = useState<ReadonlySet<string>>(new Set());
  const [pendingDeleteProfileId, setPendingDeleteProfileId] = useState<string | null>(null);
  const [activeBrowsers, setActiveBrowsers] = useState<BrowserSessionSummary[]>([]);
  // Section-level failures (listing tabs, applying the default zoom) have no field of their own, so
  // they surface once at the top of the section; per-row actions (Focus) report inside their row.
  const [actionError, setActionError] = useState<string | null>(null);
  const [focusErrors, setFocusErrors] = useState<Record<string, string>>({});
  const namedProfilesSupported = browserNamedProfilesSupported();
  const visibleProfiles = supportedBrowserProfiles(settings);
  const [fileOpenTargets, setFileOpenTargets] = useState(loadFileOpenTargets);

  const updateFileOpenTarget = (category: FileOpenCategory, target: FileOpenTarget) => {
    const next = { ...fileOpenTargets, [category]: target };
    setFileOpenTargets(next);
    saveFileOpenTargets(next);
  };

  useEffect(() => setHomeDraft(settings.homePage), [settings.homePage]);

  useEffect(() => {
    void listBrowsers().then(setActiveBrowsers, (error: unknown) => {
      // An empty list asserts "no browser tabs are open"; a failed probe is not that claim.
      setActiveBrowsers([]);
      setActionError(`List browser tabs failed: ${extractBrowserErrorCode(error)}`);
    });
  }, []);

  const update = async (patch: Partial<BrowserSettingsState>) => {
    const next = updateSettings(patch);
    if (patch.defaultZoom !== undefined) {
      try {
        const list = await listBrowsers();
        setActiveBrowsers(list);
        await Promise.all(list.map((browser) => setBrowserZoom(browser.browserId, next.defaultZoom / 100)));
      } catch (error) {
        // Native browser state can disappear while a tab is closing, but a partially applied zoom
        // must be visible instead of assumed.
        setActionError(`Apply default zoom failed: ${extractBrowserErrorCode(error)}`);
      }
    }
    return next;
  };

  const saveHomePage = () => {
    try {
      const homePage = normalizeHomePageInput(homeDraft);
      setHomeDraft(homePage);
      setHomeError(null);
      void update({ homePage });
    } catch (error) {
      setHomeError(extractBrowserErrorCode(error));
    }
  };

  const addProfile = () => {
    if (!namedProfilesSupported) return;
    const name = profileDraft.trim();
    if (!name) return;
    const profile: BrowserProfile = { id: makeBrowserProfileId(name, settings.profiles), name: name.slice(0, 80) };
    setProfileDraft("");
    void update({ profiles: [...settings.profiles, profile] });
  };

  const renameProfile = (profileId: string, name: string) => {
    if (isBuiltInBrowserProfileId(profileId)) return;
    const nextName = name.trim();
    if (!nextName) return;
    void update({
      profiles: settings.profiles.map((profile) => profile.id === profileId ? { ...profile, name: nextName.slice(0, 80) } : profile),
    });
  };

  const deleteProfile = (profileId: string) => {
    if (isBuiltInBrowserProfileId(profileId)) return;
    setPendingDeleteProfileId(null);
    const profiles = settings.profiles.filter((profile) => profile.id !== profileId);
    void update({
      profiles,
      defaultProfileId: settings.defaultProfileId === profileId ? DEFAULT_BROWSER_PROFILE.id : settings.defaultProfileId,
    });
  };

  const setImporting = (profileId: string, importing: boolean) => {
    setImportingProfiles((prev) => {
      if (prev.has(profileId) === importing) return prev;
      const next = new Set(prev);
      if (importing) next.add(profileId);
      else next.delete(profileId);
      return next;
    });
  };

  const importCookies = async (profileId: string) => {
    if (importingProfiles.has(profileId)) return;
    setImporting(profileId, true);
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [
          { name: "Cookie files", extensions: ["json", "txt", "cookies"] },
        ],
      });
      const path = Array.isArray(selected) ? selected[0] : selected;
      if (!path) return;
      setImportStatus((prev) => ({ ...prev, [profileId]: "Importing…" }));
      const count = await importBrowserCookies(profileId, path);
      setImportStatus((prev) => ({ ...prev, [profileId]: `Imported ${count} cookie${count === 1 ? "" : "s"}` }));
    } catch (error) {
      setImportStatus((prev) => ({
        ...prev,
        [profileId]: `Cookie import failed: ${formatBrowserCookieImportError(error)}`,
      }));
    } finally {
      setImporting(profileId, false);
    }
  };

  const importInstalledCookies = async (profileId: string, source: "chrome" | "edge") => {
    if (importingProfiles.has(profileId)) return;
    setImporting(profileId, true);
    try {
      setImportStatus((prev) => ({
        ...prev,
        [profileId]: `Importing from ${source === "chrome" ? "Chrome" : "Edge"}…`,
      }));
      const { importedCount, skippedCount } = await importInstalledBrowserCookies(profileId, source);
      const skippedText = skippedCount > 0 ? ` (${skippedCount} skipped)` : "";
      setImportStatus((prev) => ({
        ...prev,
        [profileId]: `Imported ${importedCount} cookies${skippedText}`,
      }));
    } catch (error) {
      setImportStatus((prev) => ({
        ...prev,
        [profileId]: `Cookie import failed: ${formatBrowserCookieImportError(error)}`,
      }));
    } finally {
      setImporting(profileId, false);
    }
  };

  const installedImportSupported = isInstalledBrowserCookieImportSupported();
  const isMac = isMacHost();

  return (
    <section aria-labelledby="settings-browser-heading">
      <SettingsHeading
        icon={<Globe />}
        title="Browser"
        description="Configure navigation, link routing, browser sessions, and cookies."
      />
      <h2 id="settings-browser-heading" className="sr-only">Browser</h2>
      {actionError ? (
        <div
          role="alert"
          data-testid="browser-settings-error"
          className="mb-4 rounded border border-destructive/30 bg-destructive/10 px-2 py-1 text-[11px] text-destructive"
        >
          {actionError}
        </div>
      ) : null}
      <SettingsGroup
        title="Web & Navigation"
        action={
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => {
              const next = resetSettings();
              setHomeDraft(next.homePage);
              setHomeError(null);
              setTabActionError(null);
              // Re-list first, as update() does: the mount-time list misses tabs opened since.
              void listBrowsers()
                .then((list) => {
                  setActiveBrowsers(list);
                  return Promise.all(list.map((browser) => setBrowserZoom(browser.browserId, next.defaultZoom / 100)));
                })
                .catch((error: unknown) => {
                  setTabActionError(
                    `Settings were reset, but the default zoom could not be applied to every open tab: ${extractBrowserErrorCode(error)}`,
                  );
                });
            }}
            className="no-drag h-7 shrink-0 gap-1.5 px-2 text-[11px] text-muted-foreground hover:text-foreground"
          >
            <RotateCcw className="size-3" />
            Reset to defaults
          </Button>
        }
      >
        <div className="border-b border-border">
          <SettingRow label="Default Home Page" description="New browser tabs open this URL. Leave it blank to open a blank tab.">
            <div className="w-[330px]">
              <div className="flex gap-1.5">
                <Input
                  value={homeDraft}
                  onChange={(event) => setHomeDraft(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") saveHomePage();
                  }}
                  placeholder="https://example.com or blank"
                  aria-label="Default Home Page"
                  aria-invalid={homeError ? true : undefined}
                  aria-describedby={homeError ? "browser-home-error" : undefined}
                  className="h-8 min-w-0 flex-1 text-[11px]"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={saveHomePage}
                  className="h-8 px-3 text-[11px]"
                >
                  Save
                </Button>
              </div>
              {homeError ? <div id="browser-home-error" role="alert" className="mt-1 text-[11px] text-destructive">{homeError}</div> : null}
            </div>
          </SettingRow>

          <SettingRow label="Default Search Engine" description="Used by both the address bar and new-tab search input.">
            <Select
              value={settings.searchEngine}
              onValueChange={(value) => void update({ searchEngine: value as BrowserSettingsState["searchEngine"] })}
            >
              <SelectTrigger
                id="browser-search-engine"
                aria-label="Default search engine"
                className="h-8 w-[180px] text-[11px]"
              >
                <SelectValue placeholder="Default search engine" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="google">Google</SelectItem>
                <SelectItem value="bing">Bing</SelectItem>
                <SelectItem value="duckduckgo">DuckDuckGo</SelectItem>
                <SelectItem value="brave">Brave Search</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>

          <SettingRow label="Default Zoom" description="Applied to new, restored, and currently open built-in browser tabs.">
            <Select
              value={settings.defaultZoom.toString()}
              onValueChange={(value) => void update({ defaultZoom: Number(value) })}
            >
              <SelectTrigger
                id="browser-default-zoom"
                aria-label="Default zoom level"
                className="h-8 w-[180px] text-[11px]"
              >
                <SelectValue placeholder="Default zoom level" />
              </SelectTrigger>
              <SelectContent>
                {BROWSER_ZOOM_LEVELS.map((level) => (
                  <SelectItem key={level} value={level.toString()}>
                    {level}%
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </SettingRow>

          <SettingRow label="Restore browser tabs on launch" description="Persist built-in browser tabs as part of the workspace session.">
            <Switch
              id="browser-restore-tabs"
              aria-label="Restore tabs on launch"
              checked={settings.restoreTabsOnLaunch}
              onCheckedChange={(checked) => void update({ restoreTabsOnLaunch: checked })}
            />
          </SettingRow>
          <SettingRow label="Remember browsing history" description="Keep the pages you visit in the built-in browser (never private tabs). Turning this off clears it.">
            <Switch
              id="browser-remember-history"
              aria-label="Remember browsing history"
              checked={settings.rememberBrowsingHistory}
              onCheckedChange={(checked) => {
                if (!checked) {
                  void clearAllBrowserHistory().catch((error: unknown) => {
                    setActionError(`Clear browsing history failed: ${extractBrowserErrorCode(error)}`);
                  });
                }
                void update({ rememberBrowsingHistory: checked });
              }}
            />
          </SettingRow>
          <SettingRow label="Show bookmarks bar" description="Show your bookmarks below the address bar.">
            <Switch
              id="browser-show-bookmark-bar"
              aria-label="Show bookmarks bar"
              checked={settings.showBookmarkBar}
              onCheckedChange={(checked) => void update({ showBookmarkBar: checked })}
            />
          </SettingRow>
          <SettingRow label="Search suggestions" description="Send what you type in the address bar to your search engine for suggestions. Off: only bookmarks, history and open tabs.">
            <Switch
              id="browser-search-suggestions"
              aria-label="Search suggestions"
              checked={settings.searchSuggestions}
              onCheckedChange={(checked) => void update({ searchSuggestions: checked })}
            />
          </SettingRow>
        </div>
      </SettingsGroup>

      <DefaultBrowserGroup />

      <BrowserEngineGroup />
      <BrowserExtensionsGroup />

      <SiteSettingsGroup />

      <BrowserAgentAccessGroup />
      <BrowserPasswordsGroup />

      <SettingsGroup title="Link Routing">
        <div className="border-b border-border">
          <SettingRow label="Click a link" description="Plain click in the embedded browser opens the chosen browser.">
            <Select
              value={settings.linkClickTarget}
              onValueChange={(value) => {
                const linkClickTarget = value === "external" ? "external" : "builtin";
                void update({
                  linkClickTarget,
                  openLinksInBuiltInBrowser: linkClickTarget === "builtin",
                });
              }}
            >
              <SelectTrigger id="browser-link-click-target" aria-label="Click a link" className="h-8 w-[180px] text-[11px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="builtin">Built-in browser</SelectItem>
                <SelectItem value="external">System browser</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>
          <SettingRow label="Command/Ctrl-click a link" description="On macOS this is Command-click. On Windows and Linux it is Ctrl-click.">
            <Select
              value={settings.modifierClickTarget}
              onValueChange={(value) => void update({ modifierClickTarget: value === "builtin" ? "builtin" : "external" })}
            >
              <SelectTrigger id="browser-modifier-click-target" aria-label="Command or Ctrl-click a link" className="h-8 w-[180px] text-[11px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="builtin">Built-in browser</SelectItem>
                <SelectItem value="external">System browser</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>
          <SettingRow label="Hold Shift to open in your web browser" description="Shift-click bypasses the built-in browser and opens the system default web browser.">
            <Switch
              id="browser-shift-opens-system"
              aria-label="Hold Shift to open in your web browser"
              checked={settings.shiftOpensSystemBrowser}
              onCheckedChange={(checked) => void update({ shiftOpensSystemBrowser: checked })}
            />
          </SettingRow>
          <SettingRow label="Show terminal link actions" description="Clicking a terminal URL shows explicit built-in and system-browser actions. Turn off for immediate default routing.">
            <Switch
              id="browser-show-terminal-actions"
              aria-label="Show terminal link actions"
              checked={settings.showTerminalLinkActions}
              onCheckedChange={(checked) => void update({ showTerminalLinkActions: checked })}
            />
          </SettingRow>
          <SettingRow label="Localhost Worktree Labels" description="Append the source worktree/branch to localhost and 127.0.0.1 browser tab labels.">
            <Switch
              id="browser-localhost-worktree-labels"
              aria-label="Localhost Worktree Labels"
              checked={settings.localhostWorktreeLabels}
              onCheckedChange={(checked) => void update({ localhostWorktreeLabels: checked })}
            />
          </SettingRow>
          <SettingRow label="Ask where to save downloads" description="Choose a folder for each download instead of saving it to the Downloads folder.">
            <Switch
              id="browser-ask-where-to-save"
              aria-label="Ask where to save downloads"
              checked={settings.askWhereToSave}
              onCheckedChange={(checked) => void update({ askWhereToSave: checked })}
            />
          </SettingRow>
        </div>
      </SettingsGroup>

      <SettingsGroup title="File Links">
        <div className="border-b border-border">
          <SettingRow label="Markdown" description="Where Command/Ctrl-click on a terminal file path opens.">
            <Select
              value={fileOpenTargets.markdown}
              onValueChange={(value) => updateFileOpenTarget("markdown", value as FileOpenTarget)}
            >
              <SelectTrigger id="file-open-target-markdown" aria-label="Open Markdown files in" className="h-8 w-[180px] text-[11px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="in-app">Ferryx tab</SelectItem>
                <SelectItem value="external">External app</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>
          <SettingRow label="Images">
            <Select
              value={fileOpenTargets.image}
              onValueChange={(value) => updateFileOpenTarget("image", value as FileOpenTarget)}
            >
              <SelectTrigger id="file-open-target-image" aria-label="Open Image files in" className="h-8 w-[180px] text-[11px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="in-app">Ferryx tab</SelectItem>
                <SelectItem value="external">External app</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>
          <SettingRow label="PDF">
            <Select
              value={fileOpenTargets.pdf}
              onValueChange={(value) => updateFileOpenTarget("pdf", value as FileOpenTarget)}
            >
              <SelectTrigger id="file-open-target-pdf" aria-label="Open PDF files in" className="h-8 w-[180px] text-[11px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="in-app">Ferryx tab</SelectItem>
                <SelectItem value="external">External app</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>
          <SettingRow label="Video & audio">
            <Select
              value={fileOpenTargets.media}
              onValueChange={(value) => updateFileOpenTarget("media", value as FileOpenTarget)}
            >
              <SelectTrigger id="file-open-target-media" aria-label="Open Video & audio files in" className="h-8 w-[180px] text-[11px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="in-app">Ferryx tab</SelectItem>
                <SelectItem value="external">External app</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>
          <SettingRow label="Other text files">
            <Select
              value={fileOpenTargets.text}
              onValueChange={(value) => updateFileOpenTarget("text", value as FileOpenTarget)}
            >
              <SelectTrigger id="file-open-target-text" aria-label="Open Other text files in" className="h-8 w-[180px] text-[11px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="in-app">Ferryx tab</SelectItem>
                <SelectItem value="external">External app</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>
        </div>
      </SettingsGroup>

      <SettingsGroup
        title="Session & Cookies"
        description={`Default uses the normal browser store. Private is ephemeral. ${
          namedProfilesSupported
            ? "Named profiles use separate persistent data directories."
            : "macOS WebKit does not expose persistent named data stores, so only Default and Private are available."
        }`}
        action={
          namedProfilesSupported ? (
            <div className="flex items-center gap-1.5">
              <Input
                value={profileDraft}
                onChange={(event) => setProfileDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") addProfile();
                }}
                aria-label="New browser profile name"
                placeholder="Profile name"
                className="h-8 w-36 text-[11px]"
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={addProfile}
                className="h-8 gap-1 px-2 text-[11px]"
              >
                <Plus className="size-3" /> Add Profile
              </Button>
            </div>
          ) : null
        }
      >

      <div className="divide-y divide-border">
        {visibleProfiles.map((profile) => {
          const isDefault = profile.id === settings.defaultProfileId;
          const isBuiltIn = isBuiltInBrowserProfileId(profile.id);
          return (
            <div key={profile.id} className="flex flex-wrap items-center gap-x-3 gap-y-2 py-3" data-testid={`browser-profile-row-${profile.id}`}>
              <Cookie className="size-4 shrink-0 text-muted-foreground" />
              {/* The name column keeps a readable minimum; the actions wrap below it instead of
                  squeezing it, so the pill and buttons never overlap the name or id. */}
              <div className="min-w-[10rem] flex-1 basis-40">
                <input
                  key={`${profile.id}:${profile.name}`}
                  defaultValue={profile.name}
                  disabled={isBuiltIn}
                  aria-label={`Profile name ${profile.id}`}
                  onBlur={(event) => {
                    // A blank name is rejected; restore the visible text so it matches state.
                    if (!event.target.value.trim()) {
                      event.target.value = profile.name;
                      return;
                    }
                    renameProfile(profile.id, event.target.value);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") event.currentTarget.blur();
                  }}
                  className="h-7 w-full max-w-56 rounded border border-transparent bg-transparent px-1 text-[11px] font-medium outline-none hover:border-border focus:border-ring disabled:opacity-80"
                />
                <div className="truncate px-1 font-mono text-[11px] text-muted-foreground" title={profile.id}>{profile.id}</div>
                {importStatus[profile.id] ? <div className="mt-0.5 px-1 text-[11px] text-muted-foreground">{importStatus[profile.id]}</div> : null}
                {!installedImportSupported ? (
                  <div className="mt-0.5 px-1 text-[11px] text-muted-foreground" data-testid="browser-profile-import-hint">
                    To bring cookies from Chrome or Edge, use Import browser data… under Import below.
                  </div>
                ) : isMac ? (
                  <div className="mt-0.5 px-1 text-[11px] text-muted-foreground">macOS will ask for Keychain access</div>
                ) : null}
              </div>
              <div className="flex flex-wrap items-center justify-end gap-1.5">
              {isDefault ? (
                <Badge variant="secondary" className="shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium">Default</Badge>
              ) : (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => void update({ defaultProfileId: profile.id })}
                  className="h-7 shrink-0 px-2 text-[11px]"
                >
                  Make Default
                </Button>
              )}
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={importingProfiles.has(profile.id)}
                onClick={() => void importCookies(profile.id)}
                className="h-7 shrink-0 gap-1 px-2 text-[11px]"
              >
                <FolderOpen className="size-3" /> Import Cookies
              </Button>
              {installedImportSupported ? (
                <>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={importingProfiles.has(profile.id)}
                      onClick={() => void importInstalledCookies(profile.id, "chrome")}
                      className="h-7 shrink-0 px-2 text-[11px]"
                    >
                      Import from Chrome
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={importingProfiles.has(profile.id)}
                      onClick={() => void importInstalledCookies(profile.id, "edge")}
                      className="h-7 shrink-0 px-2 text-[11px]"
                    >
                      Import from Edge
                    </Button>
                </>
              ) : null}
              {pendingDeleteProfileId === profile.id ? (
                <div role="group" aria-label={`Confirm delete profile ${profile.name}`} className="flex items-center gap-1.5">
                  <Button
                    type="button"
                    variant="destructive"
                    size="sm"
                    aria-label={`Confirm delete profile ${profile.name}`}
                    onClick={() => deleteProfile(profile.id)}
                    className="h-7 px-2 text-[11px]"
                  >
                    Confirm Delete
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => setPendingDeleteProfileId(null)}
                    className="h-7 px-2 text-[11px]"
                  >
                    Cancel
                  </Button>
                </div>
              ) : (
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  disabled={isBuiltIn}
                  aria-label={`Delete profile ${profile.name}`}
                  onClick={() => setPendingDeleteProfileId(profile.id)}
                  title={isBuiltIn ? "Built-in profiles cannot be deleted" : "Delete profile"}
                  className="h-7 w-7 text-muted-foreground hover:text-foreground disabled:cursor-not-allowed disabled:opacity-30"
                >
                  <Trash2 className="size-3.5" />
                </Button>
              )}
              </div>
            </div>
          );
        })}
      </div>
      </SettingsGroup>

      <SettingsGroup title="Active Browser Tabs">
        <div className="divide-y divide-border">
          {activeBrowsers.length === 0 ? (
            <div className="py-4 text-center text-[12px] text-muted-foreground">No active browser tabs open.</div>
          ) : (
            activeBrowsers.map((browser) => (
              <div key={browser.browserId} className="flex items-center gap-3 py-2.5">
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13px] font-medium">{browser.title || browser.url || browser.browserId}</div>
                  <div className="truncate font-mono text-[11px] text-muted-foreground">{browser.url} · profile:{browser.profileId}</div>
                  {focusErrors[browser.browserId] ? (
                    <div role="alert" className="mt-0.5 truncate text-[11px] text-destructive">
                      {focusErrors[browser.browserId]}
                    </div>
                  ) : null}
                </div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  aria-label={`Focus browser tab ${browser.title || browser.browserId}`}
                  onClick={() => {
                    // cmd_browser_focus reports a missing webview instead of succeeding silently,
                    // and this list can outlive a browser that is closing.
                    void focusBrowser(browser.browserId)
                      .then(() => setFocusErrors((prev) => {
                        if (!(browser.browserId in prev)) return prev;
                        const remaining = { ...prev };
                        delete remaining[browser.browserId];
                        return remaining;
                      }))
                      .catch((error: unknown) => setFocusErrors((prev) => ({
                        ...prev,
                        [browser.browserId]: `Focus failed: ${extractBrowserErrorCode(error)}`,
                      })));
                  }}
                  className="h-7 px-2 text-[11px]"
                >
                  Focus
                </Button>
              </div>
            ))
          )}
        </div>
        {tabActionError ? (
          <div role="alert" className="mt-2 text-[11px] text-destructive">
            {tabActionError}
          </div>
        ) : null}
      </SettingsGroup>
      <ImportBrowserDataGroup
        profiles={visibleProfiles.filter((profile) => profile.id !== "private")}
        defaultProfileId={settings.defaultProfileId === "private" ? "default" : settings.defaultProfileId}
      />
    </section>
  );
}

function ImportBrowserDataGroup({
  profiles,
  defaultProfileId,
}: {
  profiles: { id: string; name: string }[];
  defaultProfileId: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <SettingsGroup
      title="Import"
      description="Bring bookmarks, history, cookies and passwords from another browser or a password manager export."
      action={
        <Button type="button" variant="outline" size="sm" onClick={() => setOpen(true)} className="h-8 px-2 text-[11px]">
          Import browser data…
        </Button>
      }
    >
      {open ? (
        <ImportBrowserDataDialog open={open} onOpenChange={setOpen} profiles={profiles} defaultProfileId={defaultProfileId} />
      ) : null}
    </SettingsGroup>
  );
}

export { BrowserSection as BrowserSettings, BrowserSection as BrowserSettingsPanel };
export default BrowserSection;
