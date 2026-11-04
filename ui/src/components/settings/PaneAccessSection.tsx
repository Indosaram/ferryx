import { useCallback, useEffect, useState } from "react";
import { KeyRound, Trash2 } from "lucide-react";

import { Button } from "../ui/button";
import { Input } from "../ui/input";
import {
  addPaneGrant,
  listPaneGrants,
  removePaneGrant,
  toIpcError,
  type PaneGrant,
  type PaneGrantOp,
} from "../../lib/tauri";
import { SettingsGroup, SettingsHeading } from "./primitives";

const OPS: { op: PaneGrantOp; label: string }[] = [
  { op: "write", label: "Type" },
  { op: "control", label: "Control" },
  { op: "spawn", label: "Spawn" },
  { op: "admin", label: "Admin" },
];

/**
 * Settings > Pane access: which Ferryx pane may act on another pane (rex todo 5). Every process
 * in a pane runs as you, so the daemon refuses cross-pane input and owner actions from a pane
 * unless a grant made here (or by `ferryx-cli grants add` outside Ferryx) allows it.
 */
export function PaneAccessSection() {
  const [grants, setGrants] = useState<PaneGrant[] | null>(null);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [ops, setOps] = useState<PaneGrantOp[]>(["write"]);
  const [error, setError] = useState<string | null>(null);

  const run = useCallback(async (action: () => Promise<PaneGrant[] | null>) => {
    try {
      setGrants(await action());
      setError(null);
    } catch (failure) {
      setError(toIpcError(failure).message);
    }
  }, []);

  useEffect(() => {
    void run(listPaneGrants);
  }, [run]);

  const toggle = (op: PaneGrantOp) =>
    setOps((current) => (current.includes(op) ? current.filter((known) => known !== op) : [...current, op]));
  const admin = ops.includes("admin");
  const canAdd = from.trim() !== "" && to.trim() !== "" && ops.length > 0 && (!admin || to.trim() === "*");

  return (
    <div>
      <SettingsHeading
        icon={<KeyRound />}
        title="Pane access"
        description="A process inside one pane may not type into, control or spawn in another pane, or take owner actions (pairing, remote access, upgrades), unless you grant it here. Grants use session or workspace ids; * means every pane. Admin is granted on * only."
      />
      <SettingsGroup title="Grants">
        {grants === null ? (
          <p className="text-[12px] text-muted-foreground">Pane access is managed in the Ferryx desktop app.</p>
        ) : grants.length === 0 ? (
          <p className="text-[12px] text-muted-foreground">No pane may act on another pane.</p>
        ) : (
          <ul className="space-y-2" aria-label="Pane grants">
            {grants.map((grant) => (
              <li key={`${grant.from}->${grant.to}`} className="flex items-center justify-between gap-3 text-[12px]">
                <span className="font-mono">
                  {grant.from} {"->"} {grant.to}: {grant.ops.join(", ")}
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  aria-label={`Remove grant ${grant.from} to ${grant.to}`}
                  onClick={() => void run(() => removePaneGrant(grant.from, grant.to))}
                >
                  <Trash2 className="size-3.5" />
                </Button>
              </li>
            ))}
          </ul>
        )}
      </SettingsGroup>
      {grants !== null ? (
        <SettingsGroup title="Add a grant">
          <div className="flex flex-wrap items-center gap-2">
            <Input aria-label="From pane or workspace" placeholder="from" value={from} onChange={(event) => setFrom(event.target.value)} className="w-56 font-mono" />
            <Input aria-label="To pane, workspace or *" placeholder="to" value={to} onChange={(event) => setTo(event.target.value)} className="w-56 font-mono" />
            {OPS.map(({ op, label }) => (
              <label key={op} className="flex items-center gap-1 text-[12px]">
                <input type="checkbox" checked={ops.includes(op)} onChange={() => toggle(op)} />
                {label}
              </label>
            ))}
            <Button
              size="sm"
              disabled={!canAdd}
              onClick={() => void run(() => addPaneGrant({ from: from.trim(), to: to.trim(), ops }))}
            >
              Add
            </Button>
          </div>
        </SettingsGroup>
      ) : null}
      {error ? (
        <p role="alert" className="text-[12px] text-destructive">
          {error}
        </p>
      ) : null}
    </div>
  );
}
