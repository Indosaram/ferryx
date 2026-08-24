import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Cookie, FolderOpen, Globe, Plus, RotateCcw, Trash2 } from "lucide-react";

import {
  BROWSER_ZOOM_LEVELS,
  DEFAULT_BROWSER_PROFILE,
  makeBrowserProfileId,
  normalizeHomePageInput,
  useBrowserSettings,
  type BrowserProfile,
  type BrowserSettingsState,
} from "../lib/browserSettings";
import { clearBrowserHistory } from "../lib/browserHistory";
import {
  focusBrowser,
  importBrowserCookies,
  listBrowsers,
  setBrowserZoom,
} from "../lib/browserTauri";
import type { BrowserSessionSummary } from "../lib/types";

function SettingRow({
  label,
  description,
  children,
}: {
  label: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-h-14 items-center justify-between gap-5 border-b border-border py-3 last:border-b-0">
      <div className="min-w-0">
        <div className="text-xs font-medium text-foreground">{label}</div>
        <div className="mt-0.5 max-w-xl text-[11px] leading-4 text-muted-foreground">{description}</div>
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (checked: boolean) => void; label: string }) {
  return (
    <input
      type="checkbox"
      aria-label={label}
      checked={checked}
      onChange={(event) => onChange(event.target.checked)}
      className="size-4 accent-foreground"
    />
  );
}

export function BrowserSettingsPanel() {
  const { settings, updateSettings, resetSettings } = useBrowserSettings();
  const [homeDraft, setHomeDraft] = useState(settings.homePage);
  const [homeError, setHomeError] = useState<string | null>(null);
  const [profileDraft, setProfileDraft] = useState("");
  const [importStatus, setImportStatus] = useState<Record<string, string>>({});
  const [activeBrowsers, setActiveBrowsers] = useState<BrowserSessionSummary[]>([]);

  useEffect(() => setHomeDraft(settings.homePage), [settings.homePage]);

  const refreshBrowsers = async () => {
    try {
      setActiveBrowsers(await listBrowsers());
    } catch {
      setActiveBrowsers([]);
    }
  };

  useEffect(() => {
    void refreshBrowsers();
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
    const name = profileDraft.trim();
    if (!name) return;
    const profile: BrowserProfile = { id: makeBrowserProfileId(name, settings.profiles), name: name.slice(0, 80) };
    setProfileDraft("");
    void update({ profiles: [...settings.profiles, profile] });
  };

  const renameProfile = (profileId: string, name: string) => {
    const nextName = name.trim();
    if (!nextName) return;
    void update({
      profiles: settings.profiles.map((profile) => profile.id === profileId ? { ...profile, name: nextName.slice(0, 80) } : profile),
    });
  };

  const deleteProfile = (profileId: string) => {
    if (profileId === DEFAULT_BROWSER_PROFILE.id) return;
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
      <div className="mb-5 flex items-start gap-3">
        <Globe className="mt-0.5 size-5 text-muted-foreground" />
        <div className="min-w-0 flex-1">
          <h2 id="settings-browser-heading" className="text-sm font-semibold">Browser</h2>
          <p className="mt-1 text-xs text-muted-foreground">Configure navigation, link routing, isolated sessions, and cookies.</p>
        </div>
        <button
          type="button"
          onClick={() => {
            const next = resetSettings();
            setHomeDraft(next.homePage);
            setHomeError(null);
            void Promise.all(activeBrowsers.map((browser) => setBrowserZoom(browser.browserId, next.defaultZoom / 100))).catch(() => undefined);
          }}
          className="flex h-7 items-center gap-1.5 rounded-md border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
        >
          <RotateCcw className="size-3" />
          Reset to defaults
        </button>
      </div>

      <h3 className="border-b border-border pb-2 text-[12px] font-semibold">Web & Navigation</h3>
      <div className="border-b border-border">
        <SettingRow label="Default Home Page" description="New browser tabs open this URL. Leave it blank to open a blank tab.">
          <div className="w-[330px]">
            <div className="flex gap-1.5">
              <input
                value={homeDraft}
                onChange={(event) => setHomeDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") saveHomePage();
                }}
                placeholder="https://example.com or blank"
                aria-label="Default Home Page"
                className="h-8 min-w-0 flex-1 rounded-md border border-input bg-background px-2 text-xs outline-none focus:border-ring"
              />
              <button type="button" onClick={saveHomePage} className="h-8 rounded-md border border-border px-3 text-[11px] hover:bg-accent">Save</button>
            </div>
            {homeError ? <div className="mt-1 text-[10px] text-destructive">{homeError}</div> : null}
          </div>
        </SettingRow>

        <SettingRow label="Default Search Engine" description="Used by both the address bar and new-tab search input.">
          <select
            aria-label="Default search engine"
            value={settings.searchEngine}
            onChange={(event) => void update({ searchEngine: event.target.value as BrowserSettingsState["searchEngine"] })}
            className="h-8 rounded-md border border-input bg-background px-2 text-xs"
          >
            <option value="google">Google</option>
            <option value="bing">Bing</option>
            <option value="duckduckgo">DuckDuckGo</option>
            <option value="brave">Brave Search</option>
          </select>
        </SettingRow>

        <SettingRow label="Default Zoom" description="Applied to new, restored, and currently open built-in browser tabs.">
          <select
            aria-label="Default zoom level"
            value={settings.defaultZoom}
            onChange={(event) => void update({ defaultZoom: Number(event.target.value) })}
            className="h-8 rounded-md border border-input bg-background px-2 text-xs"
          >
            {BROWSER_ZOOM_LEVELS.map((level) => <option key={level} value={level}>{level}%</option>)}
          </select>
        </SettingRow>

        <SettingRow label="Restore browser tabs on launch" description="Persist built-in browser tabs as part of the workspace session.">
          <Toggle label="Restore tabs on launch" checked={settings.restoreTabsOnLaunch} onChange={(value) => void update({ restoreTabsOnLaunch: value })} />
        </SettingRow>
        <SettingRow label="Remember browsing history" description="Keep up to 100 recently visited built-in browser pages across Ferryx restarts.">
          <Toggle
            label="Remember browsing history"
            checked={settings.rememberBrowsingHistory}
            onChange={(value) => {
              if (!value) clearBrowserHistory();
              void update({ rememberBrowsingHistory: value });
            }}
          />
        </SettingRow>
      </div>

      <h3 className="mt-7 border-b border-border pb-2 text-[12px] font-semibold">Link Routing</h3>
      <div className="border-b border-border">
        <SettingRow label="Open links in built-in browser" description="HTTP(S) links opened inside Ferryx use a built-in browser tab by default.">
          <Toggle label="Open links in built-in browser" checked={settings.openLinksInBuiltInBrowser} onChange={(value) => void update({ openLinksInBuiltInBrowser: value })} />
        </SettingRow>
        <SettingRow label="Hold Shift to open in your web browser" description="Shift-click bypasses the built-in browser and opens the system default web browser.">
          <Toggle label="Hold Shift to open in your web browser" checked={settings.shiftOpensSystemBrowser} onChange={(value) => void update({ shiftOpensSystemBrowser: value })} />
        </SettingRow>
        <SettingRow label="Show terminal link actions" description="Clicking a terminal URL shows explicit built-in and system-browser actions. Turn off for immediate default routing.">
          <Toggle label="Show terminal link actions" checked={settings.showTerminalLinkActions} onChange={(value) => void update({ showTerminalLinkActions: value })} />
        </SettingRow>
        <SettingRow label="Localhost Worktree Labels" description="Append the source worktree/branch to localhost and 127.0.0.1 browser tab labels.">
          <Toggle label="Localhost Worktree Labels" checked={settings.localhostWorktreeLabels} onChange={(value) => void update({ localhostWorktreeLabels: value })} />
        </SettingRow>
      </div>

      <div className="mt-7 flex items-end justify-between gap-4 border-b border-border pb-2">
        <div>
          <h3 className="text-[12px] font-semibold">Session & Cookies</h3>
          <p className="mt-0.5 text-[11px] text-muted-foreground">Each profile uses a separate WebView data store for cookies and site sessions.</p>
        </div>
        <div className="flex items-center gap-1.5">
          <input
            value={profileDraft}
            onChange={(event) => setProfileDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") addProfile();
            }}
            aria-label="New browser profile name"
            placeholder="Profile name"
            className="h-8 w-36 rounded-md border border-input bg-background px-2 text-xs outline-none focus:border-ring"
          />
          <button type="button" onClick={addProfile} className="flex h-8 items-center gap-1 rounded-md border border-border px-2 text-[11px] hover:bg-accent">
            <Plus className="size-3" /> Add Profile
          </button>
        </div>
      </div>

      <div className="divide-y divide-border">
        {settings.profiles.map((profile) => {
          const isDefault = profile.id === settings.defaultProfileId;
          return (
            <div key={profile.id} className="flex items-center gap-3 py-3">
              <Cookie className="size-4 shrink-0 text-muted-foreground" />
              <div className="min-w-0 flex-1">
                <input
                  key={`${profile.id}:${profile.name}`}
                  defaultValue={profile.name}
                  aria-label={`Profile name ${profile.id}`}
                  onBlur={(event) => renameProfile(profile.id, event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") event.currentTarget.blur();
                  }}
                  className="h-7 w-full max-w-56 rounded border border-transparent bg-transparent px-1 text-xs font-medium outline-none hover:border-border focus:border-ring"
                />
                <div className="px-1 font-mono text-[9px] text-muted-foreground">{profile.id}</div>
                {importStatus[profile.id] ? <div className="mt-0.5 px-1 text-[10px] text-muted-foreground">{importStatus[profile.id]}</div> : null}
              </div>
              {isDefault ? (
                <span className="rounded-full bg-accent px-2 py-0.5 text-[10px] font-medium">Default</span>
              ) : (
                <button type="button" onClick={() => void update({ defaultProfileId: profile.id })} className="h-7 rounded border border-border px-2 text-[10px] hover:bg-accent">Make Default</button>
              )}
              <button
                type="button"
                onClick={() => void importCookies(profile.id)}
                className="flex h-7 items-center gap-1 rounded border border-border px-2 text-[10px] hover:bg-accent"
              >
                <FolderOpen className="size-3" /> Import Cookies
              </button>
              <button
                type="button"
                disabled={profile.id === DEFAULT_BROWSER_PROFILE.id}
                aria-label={`Delete profile ${profile.name}`}
                onClick={() => deleteProfile(profile.id)}
                title={profile.id === DEFAULT_BROWSER_PROFILE.id ? "The built-in default profile cannot be deleted" : "Delete profile"}
                className="rounded p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-30"
              >
                <Trash2 className="size-3.5" />
              </button>
            </div>
          );
        })}
      </div>

      <div className="mt-7">
        <h3 className="border-b border-border pb-2 text-[12px] font-semibold">Active Browser Tabs</h3>
        <div className="divide-y divide-border">
          {activeBrowsers.length === 0 ? (
            <div className="py-4 text-center text-xs text-muted-foreground">No active browser tabs open.</div>
          ) : activeBrowsers.map((browser) => (
            <div key={browser.browserId} className="flex items-center gap-3 py-2.5">
              <div className="min-w-0 flex-1">
                <div className="truncate text-xs font-medium">{browser.title || browser.url || browser.browserId}</div>
                <div className="truncate font-mono text-[9px] text-muted-foreground">{browser.url} · profile:{browser.profileId}</div>
              </div>
              <button type="button" onClick={() => void focusBrowser(browser.browserId)} className="h-7 rounded border border-border px-2 text-[10px] hover:bg-accent">Focus</button>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
