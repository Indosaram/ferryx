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
import { clearBrowserHistory } from "../../lib/browserHistory";
import {
  focusBrowser,
  importBrowserCookies,
  listBrowsers,
  setBrowserZoom,
} from "../../lib/browserTauri";
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
import { SettingRow, SettingsHeading } from "./primitives";

export function BrowserSection() {
  const { settings, updateSettings, resetSettings } = useBrowserSettings();
  const [homeDraft, setHomeDraft] = useState(settings.homePage);
  const [homeError, setHomeError] = useState<string | null>(null);
  const [profileDraft, setProfileDraft] = useState("");
  const [importStatus, setImportStatus] = useState<Record<string, string>>({});
  const [activeBrowsers, setActiveBrowsers] = useState<BrowserSessionSummary[]>([]);
  const namedProfilesSupported = browserNamedProfilesSupported();
  const visibleProfiles = supportedBrowserProfiles(settings);

  useEffect(() => setHomeDraft(settings.homePage), [settings.homePage]);

  useEffect(() => {
    void listBrowsers().then(setActiveBrowsers, () => setActiveBrowsers([]));
  }, []);

  const update = async (patch: Partial<BrowserSettingsState>) => {
    const next = updateSettings(patch);
    if (patch.defaultZoom !== undefined) {
      try {
        const list = await listBrowsers();
        setActiveBrowsers(list);
        await Promise.all(list.map((browser) => setBrowserZoom(browser.browserId, next.defaultZoom / 100)));
      } catch {
        // Native browser state can disappear while a tab is closing.
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
      setHomeError(error instanceof Error ? error.message : "Invalid home page URL.");
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
    const profiles = settings.profiles.filter((profile) => profile.id !== profileId);
    void update({
      profiles,
      defaultProfileId: settings.defaultProfileId === profileId ? DEFAULT_BROWSER_PROFILE.id : settings.defaultProfileId,
    });
  };

  const importCookies = async (profileId: string) => {
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
        [profileId]: error instanceof Error ? error.message : "Cookie import failed",
      }));
    }
  };

  return (
    <section aria-labelledby="settings-browser-heading">
      <SettingsHeading
        icon={<Globe />}
        title="Browser"
        description="Configure navigation, link routing, browser sessions, and cookies."
      />
      <h2 id="settings-browser-heading" className="sr-only">Browser</h2>
      <div className="mb-5 flex items-center justify-between border-b border-border pb-3">
        <h3 className="text-[12px] font-semibold">Web & Navigation</h3>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => {
            const next = resetSettings();
            setHomeDraft(next.homePage);
            setHomeError(null);
            void Promise.all(activeBrowsers.map((browser) => setBrowserZoom(browser.browserId, next.defaultZoom / 100))).catch(() => undefined);
          }}
          className="no-drag h-7 shrink-0 gap-1.5 px-2 text-[11px] text-muted-foreground hover:text-foreground"
        >
          <RotateCcw className="size-3" />
          Reset to defaults
        </Button>
      </div>
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
                className="h-8 min-w-0 flex-1 text-xs"
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
            {homeError ? <div className="mt-1 text-[10px] text-destructive">{homeError}</div> : null}
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
              className="h-8 w-[180px] text-xs"
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
              className="h-8 w-[180px] text-xs"
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
        <SettingRow label="Remember browsing history" description="Keep up to 100 recently visited built-in browser pages across Ferryx restarts.">
          <Switch
            id="browser-remember-history"
            aria-label="Remember browsing history"
            checked={settings.rememberBrowsingHistory}
            onCheckedChange={(checked) => {
              if (!checked) clearBrowserHistory();
              void update({ rememberBrowsingHistory: checked });
            }}
          />
        </SettingRow>
      </div>

      <h3 className="mt-7 border-b border-border pb-2 text-[12px] font-semibold">Link Routing</h3>
      <div className="border-b border-border">
        <SettingRow label="Open links in built-in browser" description="HTTP(S) links opened inside Ferryx use a built-in browser tab by default.">
          <Switch
            id="browser-open-links-builtin"
            aria-label="Open links in built-in browser"
            checked={settings.openLinksInBuiltInBrowser}
            onCheckedChange={(checked) => void update({ openLinksInBuiltInBrowser: checked })}
          />
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
      </div>

      <div className="mt-7 flex items-end justify-between gap-4 border-b border-border pb-2">
        <div>
          <h3 className="text-[12px] font-semibold">Session & Cookies</h3>
          <p className="mt-0.5 max-w-xl text-[11px] text-muted-foreground">
            Default uses the normal browser store. Private is ephemeral. {namedProfilesSupported
              ? "Named profiles use separate persistent data directories."
              : "macOS WebKit does not expose persistent named data stores, so only Default and Private are available."}
          </p>
        </div>
        {namedProfilesSupported ? (
          <div className="flex items-center gap-1.5">
            <Input
              value={profileDraft}
              onChange={(event) => setProfileDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") addProfile();
              }}
              aria-label="New browser profile name"
              placeholder="Profile name"
              className="h-8 w-36 text-xs"
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
        ) : null}
      </div>

      <div className="divide-y divide-border">
        {visibleProfiles.map((profile) => {
          const isDefault = profile.id === settings.defaultProfileId;
          const isBuiltIn = isBuiltInBrowserProfileId(profile.id);
          return (
            <div key={profile.id} className="flex items-center gap-3 py-3">
              <Cookie className="size-4 shrink-0 text-muted-foreground" />
              <div className="min-w-0 flex-1">
                <input
                  key={`${profile.id}:${profile.name}`}
                  defaultValue={profile.name}
                  disabled={isBuiltIn}
                  aria-label={`Profile name ${profile.id}`}
                  onBlur={(event) => renameProfile(profile.id, event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") event.currentTarget.blur();
                  }}
                  className="h-7 w-full max-w-56 rounded border border-transparent bg-transparent px-1 text-xs font-medium outline-none hover:border-border focus:border-ring disabled:opacity-80"
                />
                <div className="px-1 font-mono text-[9px] text-muted-foreground">{profile.id}</div>
                {importStatus[profile.id] ? <div className="mt-0.5 px-1 text-[10px] text-muted-foreground">{importStatus[profile.id]}</div> : null}
              </div>
              {isDefault ? (
                <Badge variant="secondary" className="rounded-full px-2 py-0.5 text-[10px] font-medium">Default</Badge>
              ) : (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => void update({ defaultProfileId: profile.id })}
                  className="h-7 px-2 text-[11px]"
                >
                  Make Default
                </Button>
              )}
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => void importCookies(profile.id)}
                className="h-7 gap-1 px-2 text-[11px]"
              >
                <FolderOpen className="size-3" /> Import Cookies
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                disabled={isBuiltIn}
                aria-label={`Delete profile ${profile.name}`}
                onClick={() => deleteProfile(profile.id)}
                title={isBuiltIn ? "Built-in profiles cannot be deleted" : "Delete profile"}
                className="h-7 w-7 text-muted-foreground hover:text-foreground disabled:cursor-not-allowed disabled:opacity-30"
              >
                <Trash2 className="size-3.5" />
              </Button>
            </div>
          );
        })}
      </div>

      <div className="mt-7">
        <h3 className="border-b border-border pb-2 text-[12px] font-semibold">Active Browser Tabs</h3>
        <div className="divide-y divide-border">
          {activeBrowsers.length === 0 ? (
            <div className="py-4 text-center text-xs text-muted-foreground">No active browser tabs open.</div>
          ) : (
            activeBrowsers.map((browser) => (
              <div key={browser.browserId} className="flex items-center gap-3 py-2.5">
                <div className="min-w-0 flex-1">
                  <div className="truncate text-xs font-medium">{browser.title || browser.url || browser.browserId}</div>
                  <div className="truncate font-mono text-[9px] text-muted-foreground">{browser.url} · profile:{browser.profileId}</div>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  aria-label={`Focus browser tab ${browser.title || browser.browserId}`}
                  onClick={() => void focusBrowser(browser.browserId)}
                  className="h-7 px-2 text-[11px]"
                >
                  Focus
                </Button>
              </div>
            ))
          )}
        </div>
      </div>
    </section>
  );
}

export { BrowserSection as BrowserSettings, BrowserSection as BrowserSettingsPanel };
export default BrowserSection;
