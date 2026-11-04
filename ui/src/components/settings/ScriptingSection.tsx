import { useEffect, useState } from "react";
import { useTranslation } from "../../i18n";
import { scriptControl, type ScriptAction, type ScriptStatus } from "../../lib/scripting";
import { toIpcError } from "../../lib/tauri";
import { Button } from "../ui/button";

export function ScriptingSection() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<ScriptStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function run(action: ScriptAction) {
    setBusy(true);
    try { setStatus(await scriptControl(action)); setError(null); }
    catch (failure) { setError(toIpcError(failure).message); }
    finally { setBusy(false); }
  }
  useEffect(() => { void run({ action: "status" }); }, []);
  return <section className="space-y-4">
    <h2>{t("scripting.title")}</h2>
    <p>{t("scripting.description")}</p>
    <label className="flex gap-2">
      <input type="checkbox" checked={status?.enabled ?? false} disabled={busy || status === null}
        onChange={(event) => void run({ action: "setEnabled", enabled: event.target.checked })} />
      {t("scripting.enabled")}
    </label>
    <Button disabled={busy || !status?.enabled} onClick={() => void run({ action: "reload" })}>{t("scripting.reload")}</Button>
    <Button disabled={busy} onClick={() => void run({ action: "status" })}>{t("scripting.refresh")}</Button>
    {error && <p role="alert">{error}</p>}
    <pre aria-label={t("scripting.log")} role="log" className="max-h-80 overflow-auto whitespace-pre-wrap text-xs">{status?.log ?? ""}</pre>
  </section>;
}
