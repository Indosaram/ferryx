import { useEffect, useState } from "react";
import { Plug, Plus, Server } from "lucide-react";
import { loadSshHosts, subscribeSshHosts } from "../../lib/sshHosts";
import { testSshConnection } from "../../lib/tauri";
import type { SshHost, SshTargetSummary } from "../../lib/types";
import { SshHostDialog } from "../SshHostDialog";
import { Button } from "../ui/button";
import { SettingsHeading } from "./primitives";

export function SshSection() {
  const [hosts, setHosts] = useState<SshHost[]>([]);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [testStates, setTestStates] = useState<Record<string, { pending?: boolean; summary?: SshTargetSummary }>>({});

  useEffect(() => {
    let alive = true;
    const unsubscribe = subscribeSshHosts((next) => {
      if (alive) setHosts(next);
    });
    void loadSshHosts()
      .then((next) => {
        if (alive) setHosts(next);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      unsubscribe();
    };
  }, []);

  const test = async (host: SshHost) => {
    setTestStates((prev) => ({ ...prev, [host.id]: { pending: true } }));
    try {
      const summary = await testSshConnection(host);
      setTestStates((prev) => ({ ...prev, [host.id]: { summary } }));
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setTestStates((prev) => ({
        ...prev,
        [host.id]: { summary: { host, reachable: false, lastError: message, checkedAt: Date.now() } },
      }));
    }
  };

  return (
    <div className="space-y-4" data-testid="settings-ssh-section">
      <SettingsHeading
        icon={<Server />}
        title="SSH Hosts"
        description="Remote machines you can open terminals and agent sessions on."
      />

      <div className="flex items-center justify-end">
        <Button size="sm" variant="outline" data-testid="settings-ssh-add" onClick={() => setDialogOpen(true)}>
          <Plus className="mr-1 size-3.5" />
          Add host
        </Button>
      </div>

      {hosts.length > 0 ? (
        <ul className="space-y-1.5">
          {hosts.map((host) => {
            const state = testStates[host.id];
            return (
              <li
                key={host.id}
                data-testid={`settings-ssh-row-${host.id}`}
                className="flex items-center gap-3 rounded-md border border-border px-3 py-2"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-[12px] font-medium">{host.label}</span>
                    <span className="rounded-full bg-neutral-700/60 px-1.5 py-px text-[9px] font-medium uppercase leading-none text-neutral-300">
                      {host.source}
                    </span>
                  </div>
                  <div className="truncate text-[11px] text-muted-foreground">
                    {host.username ? `${host.username}@${host.hostname}` : host.hostname}
                    {host.port != null && host.port !== 22 ? `:${host.port}` : ""}
                  </div>
                </div>
                {state?.pending ? <span className="text-[11px] text-muted-foreground">Testing…</span> : null}
                {state?.summary?.reachable ? (
                  <span className="text-[11px] text-status-working">Reachable</span>
                ) : null}
                {state?.summary && !state.summary.reachable ? (
                  <span className="max-w-[50%] truncate text-[11px] text-destructive" title={state.summary.lastError ?? ""}>
                    {state.summary.lastError ?? "Unreachable"}
                  </span>
                ) : null}
                <Button
                  size="sm"
                  variant="ghost"
                  data-testid={`settings-ssh-test-${host.id}`}
                  onClick={() => void test(host)}
                >
                  <Plug className="mr-1 size-3.5" />
                  Test
                </Button>
              </li>
            );
          })}
        </ul>
      ) : (
        <p className="text-[12px] text-muted-foreground">
          No SSH hosts configured. Import from ~/.ssh/config or add one manually.
        </p>
      )}

      {dialogOpen ? <SshHostDialog onClose={() => setDialogOpen(false)} /> : null}
    </div>
  );
}
