import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "../../i18n";
import type { ResolvedAgent } from "../../lib/agentsSettings";
import { Button } from "../ui/button";

export function SkillsStep({ agents, onInstalled }: {
  agents: readonly ResolvedAgent[];
  onInstalled: () => void;
}): JSX.Element {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const detected = agents.filter((agent) => agent.available && !agent.custom);
  return <div className="space-y-3">
    <p>{detected.map((agent) => agent.name).join(", ") || t("onboarding.step.skills.none")}</p>
    {error ? <p role="alert">{error}</p> : null}
    <Button disabled={busy || detected.length === 0} onClick={async () => {
      setBusy(true);
      setError(null);
      try {
        await invoke("cmd_agent_integration_set", { enabled: true, agents: detected.map((agent) => agent.name) });
        onInstalled();
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
      } finally { setBusy(false); }
    }}>{t("onboarding.step.skills.install")}</Button>
  </div>;
}
