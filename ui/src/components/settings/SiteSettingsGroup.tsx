import { useCallback, useEffect, useState } from "react";

import {
  listSitePermissions,
  onSitePermissionsChanged,
  revokeSitePermission,
  SITE_PERMISSION_LABELS,
  sitePermissionErrorCode,
  type SitePermission,
} from "../../lib/browserSitePermissions";
import { Button } from "../ui/button";
import { SettingRow, SettingsGroup } from "./primitives";

interface SiteEntry {
  profile: string;
  origin: string;
  permissions: SitePermission[];
}

/** Groups remembered decisions by profile and site, sites in alphabetical order. */
export function groupSitePermissions(entries: SitePermission[]): SiteEntry[] {
  const sites = new Map<string, SiteEntry>();
  for (const entry of entries) {
    const key = `${entry.profile}\u0000${entry.origin}`;
    const site = sites.get(key) ?? { profile: entry.profile, origin: entry.origin, permissions: [] };
    site.permissions.push(entry);
    sites.set(key, site);
  }
  return [...sites.values()].sort(
    (a, b) => a.origin.localeCompare(b.origin) || a.profile.localeCompare(b.profile),
  );
}

/** Settings > Browser > Site settings: review and revoke remembered site permissions. */
export function SiteSettingsGroup() {
  const [entries, setEntries] = useState<SitePermission[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    void listSitePermissions().then(
      (next) => {
        setEntries(next);
        setError(null);
      },
      (failure: unknown) => setError(sitePermissionErrorCode(failure)),
    );
  }, []);

  useEffect(() => {
    refresh();
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void onSitePermissionsChanged(refresh).then(
      (unlisten) => {
        if (disposed) unlisten();
        else cleanup = unlisten;
      },
      (failure: unknown) => {
        if (!disposed) setError(sitePermissionErrorCode(failure));
      },
    );
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [refresh]);

  const revoke = (permission: Pick<SitePermission, "profile" | "origin"> & { kind?: SitePermission["kind"] }) => {
    void revokeSitePermission(permission.profile, permission.origin, permission.kind).then(refresh, (failure: unknown) =>
      setError(sitePermissionErrorCode(failure)),
    );
  };

  const sites = groupSitePermissions(entries ?? []);
  return (
    <SettingsGroup
      title="Site settings"
      description="Camera, microphone, screen sharing, notifications, location and clipboard decisions you asked Ferryx to remember. Sites without a decision are asked every time."
    >
      {error ? (
        <div role="alert" className="px-3 py-2 text-[11px] text-destructive">
          {error}
        </div>
      ) : null}
      {entries !== null && sites.length === 0 ? (
        <div className="px-3 py-2 text-[11px] text-muted-foreground" data-testid="site-settings-empty">
          No remembered site permissions.
        </div>
      ) : null}
      {sites.map((site) => (
        <div key={`${site.profile}:${site.origin}`} className="border-b border-border" data-testid="site-settings-site">
          <SettingRow
            label={site.origin}
            description={
              <span className="flex flex-wrap gap-1">
                {site.profile !== "default" ? <span className="text-muted-foreground">Profile {site.profile}:</span> : null}
                {site.permissions.map((permission) => (
                  <button
                    key={permission.kind}
                    type="button"
                    onClick={() => revoke(permission)}
                    title="Forget this decision"
                    aria-label={`Forget ${SITE_PERMISSION_LABELS[permission.kind]} for ${site.origin}`}
                    className="rounded border border-border px-1.5 py-0.5 text-[10px] hover:bg-accent"
                  >
                    {SITE_PERMISSION_LABELS[permission.kind]}: {permission.decision === "allow" ? "Allowed" : "Blocked"} ×
                  </button>
                ))}
              </span>
            }
          >
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => revoke({ profile: site.profile, origin: site.origin })}
              className="h-7 px-2 text-[11px]"
              aria-label={`Reset permissions for ${site.origin}`}
            >
              Reset
            </Button>
          </SettingRow>
        </div>
      ))}
    </SettingsGroup>
  );
}
