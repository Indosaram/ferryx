import { useCallback, useEffect, useState } from "react";
import { AlertCircle, Check, Cloud, KeyRound } from "lucide-react";
import { Badge } from "../../ui/badge";
import { Button } from "../../ui/button";
import { Input } from "../../ui/input";
import {
  listCloudProviders,
  setCloudProviderSecret,
  type CloudProviderStatus,
} from "../../../lib/cloudHost";

const PLAIN_TEXT_KEYS = new Set(["BL_WORKSPACE", "FERRYX_CODESPACE_REPO", "FERRYX_CODESPACE_MACHINE"]);

export interface CloudProvidersSectionProps {
  providers?: CloudProviderStatus[];
  onUpdated?: (providers: CloudProviderStatus[]) => void;
}

export function CloudProvidersSection({ providers: initialProviders, onUpdated }: CloudProvidersSectionProps) {
  const [providers, setProviders] = useState<CloudProviderStatus[]>(initialProviders ?? []);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [savingKey, setSavingKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (initialProviders) return;
    const rows = await listCloudProviders();
    setProviders(rows);
  }, [initialProviders]);

  useEffect(() => {
    if (initialProviders) {
      setProviders(initialProviders);
      return;
    }
    void load();
  }, [initialProviders, load]);

  if (providers.length === 0) return null;

  const readyCount = providers.filter((p) => p.configured).length;

  const handleSave = async (provider: CloudProviderStatus) => {
    setError(null);
    setSavingKey(provider.id);
    try {
      let latest = providers;
      for (const key of provider.secretKeys) {
        const val = drafts[key]?.trim();
        if (val) {
          latest = await setCloudProviderSecret(key, val);
        }
      }
      setProviders(latest);
      onUpdated?.(latest);
      setEditingId(null);
      setDrafts((prev) => {
        const next = { ...prev };
        for (const key of provider.secretKeys) delete next[key];
        return next;
      });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSavingKey(null);
    }
  };

  const handleClear = async (provider: CloudProviderStatus) => {
    setError(null);
    setSavingKey(provider.id);
    try {
      let latest = providers;
      for (const key of provider.secretKeys) {
        latest = await setCloudProviderSecret(key, null);
      }
      setProviders(latest);
      onUpdated?.(latest);
      setEditingId(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSavingKey(null);
    }
  };

  return (
    <section aria-labelledby="remote-cloud-providers" className="space-y-3">
      <header className="flex flex-col gap-1 border-b border-border pb-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 id="remote-cloud-providers" className="flex items-center gap-1.5 text-[13px] font-semibold text-foreground">
            <Cloud className="size-3.5 text-muted-foreground" />
            Cloud providers
          </h2>
          <p className="text-[12px] text-muted-foreground">
            Configure once. Cloud tasks pick a ready provider automatically and remove disposable boxes when done.
          </p>
        </div>
        <p className="text-[11px] text-muted-foreground">
          {readyCount} of {providers.length} ready
        </p>
      </header>

      {error ? (
        <div role="alert" className="flex items-center gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-2.5 text-[12px] text-destructive">
          <AlertCircle className="size-4 shrink-0" />
          <span>{error}</span>
        </div>
      ) : null}

      <div role="list" aria-label="Cloud providers" className="space-y-2">
        {providers.map((provider) => {
          const isEditing = editingId === provider.id;
          const isSaving = savingKey === provider.id;
          const hasKeys = provider.secretKeys.length > 0;
          const canSave = provider.secretKeys.some((k) => Boolean(drafts[k]?.trim()));
          return (
            <div
              key={provider.id}
              role="listitem"
              data-cloud-provider-config={provider.id}
              className="space-y-2 rounded-lg border border-border bg-card/50 p-3"
            >
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="min-w-0 space-y-0.5">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-[13px] font-medium text-foreground">{provider.label}</span>
                    <Badge
                      variant={provider.configured ? "default" : "outline"}
                      className="h-4 px-1.5 py-0 text-[10px]"
                    >
                      {provider.configured ? (
                        <span className="inline-flex items-center gap-0.5">
                          <Check className="size-2.5" />
                          Ready
                        </span>
                      ) : (
                        "Not configured"
                      )}
                    </Badge>
                    <span className="rounded border border-border/70 bg-muted/40 px-1.5 text-[10px] text-muted-foreground">
                      {provider.transport}
                    </span>
                    <span className="text-[11px] text-muted-foreground">{provider.freeQuota}</span>
                  </div>
                  <p className="text-[11px] text-muted-foreground">{provider.note}</p>
                </div>

                {hasKeys ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    disabled={isSaving}
                    onClick={() => {
                      setError(null);
                      setEditingId(isEditing ? null : provider.id);
                    }}
                  >
                    <KeyRound className="mr-1 size-3" />
                    {isEditing ? "Cancel" : provider.configured ? "Update" : "Set key"}
                  </Button>
                ) : null}
              </div>

              {isEditing ? (
                <div className="space-y-2 border-t border-border/60 pt-2">
                  {provider.secretKeys.map((keyName) => (
                    <label key={keyName} className="block space-y-1 text-[11px] text-muted-foreground">
                      <span className="font-mono">{keyName}</span>
                      <Input
                        type={PLAIN_TEXT_KEYS.has(keyName) ? "text" : "password"}
                        aria-label={keyName}
                        placeholder={
                          provider.missingKeys.includes(keyName)
                            ? `Enter ${keyName}`
                            : `Configured (enter to replace)`
                        }
                        value={drafts[keyName] ?? ""}
                        onChange={(e) =>
                          setDrafts((prev) => ({ ...prev, [keyName]: e.target.value }))
                        }
                        className="h-8 text-[12px]"
                        autoComplete="off"
                      />
                    </label>
                  ))}
                  <div className="flex items-center justify-end gap-2">
                    {provider.configured ? (
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        disabled={isSaving}
                        className="text-destructive hover:text-destructive"
                        onClick={() => void handleClear(provider)}
                      >
                        Clear key
                      </Button>
                    ) : null}
                    <Button
                      type="button"
                      size="sm"
                      disabled={isSaving || !canSave}
                      onClick={() => void handleSave(provider)}
                    >
                      {isSaving ? "Saving..." : "Save"}
                    </Button>
                  </div>
                </div>
              ) : null}
            </div>
          );
        })}
      </div>
    </section>
  );
}
