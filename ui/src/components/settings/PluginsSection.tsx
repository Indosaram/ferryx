import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Switch } from "../ui/switch";
import { useTranslation } from "../../i18n";

type PluginStatus = { manifest: { name: string; version: string; command: string[]; capabilities: string[] }; enabled: boolean };

export function PluginsSection() {
  const { t } = useTranslation();
  const [plugins, setPlugins] = useState<PluginStatus[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let disposed = false;
    void invoke<PluginStatus[]>("cmd_plugins").then((value) => { if (!disposed) setPlugins(value); })
      .catch((cause: unknown) => { if (!disposed) setError(String(cause)); });
    return () => { disposed = true; };
  }, []);
  const toggle = async (name: string, enabled: boolean) => {
    setBusy(true);
    try { setPlugins(await invoke<PluginStatus[]>("cmd_plugins", { name, enabled })); setError(null); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  };
  return <section aria-label={t("plugins.title")}>
    <h3>{t("plugins.title")}</h3>
    <p>{t("plugins.consent")}</p>
    {error && <p role="alert">{error}</p>}
    {plugins.length === 0 && <p>{t("plugins.empty")}</p>}
    {plugins.map(({ manifest, enabled }) => <div key={manifest.name}>
      <label>{manifest.name} {manifest.version}
        <Switch aria-label={t("plugins.enable", { name: manifest.name })} checked={enabled} disabled={busy}
          onCheckedChange={(checked) => { void toggle(manifest.name, checked); }} />
      </label>
      <pre>{JSON.stringify(manifest.command)}</pre>
      <p>{t("plugins.capabilities", { capabilities: manifest.capabilities.join(", ") || t("plugins.none") })}</p>
    </div>)}
  </section>;
}
