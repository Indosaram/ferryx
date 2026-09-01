import { useEffect, useState } from "react";
import { ChevronRight, Plus, Server } from "lucide-react";
import { loadSshHosts, subscribeSshHosts } from "../lib/sshHosts";
import { listRemoteWorktrees } from "../lib/tauri";
import type { RemoteWorktree, SshHost } from "../lib/types";
import { SshHostDialog } from "./SshHostDialog";
import { cn } from "../lib/cn";

type RemoteListingState = {
  loading?: boolean;
  error?: string;
  worktrees?: RemoteWorktree[];
};

export function RemoteHostsSection({
  onOpenRemoteWorktree,
}: {
  onOpenRemoteWorktree: (
    host: SshHost,
    remote: { path: string | null; head?: string | null; branch?: string | null },
  ) => void;
}) {
  const [hosts, setHosts] = useState<SshHost[]>([]);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [expandedHostId, setExpandedHostId] = useState<string | null>(null);
  const [listings, setListings] = useState<Record<string, RemoteListingState>>({});

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

  const toggleHost = async (host: SshHost) => {
    const next = expandedHostId === host.id ? null : host.id;
    setExpandedHostId(next);
    if (next !== host.id || listings[host.id]) return;
    setListings((prev) => ({ ...prev, [host.id]: { loading: true } }));
    try {
      const worktrees = await listRemoteWorktrees(host);
      setListings((prev) => ({ ...prev, [host.id]: { worktrees } }));
    } catch (cause) {
      setListings((prev) => ({
        ...prev,
        [host.id]: { error: cause instanceof Error ? cause.message : String(cause) },
      }));
    }
  };

  return (
    <div className="mt-1" data-testid="remote-hosts-section">
      <div className="flex h-7 items-center justify-between pl-5 pr-1">
        <span className="text-[10px] font-semibold uppercase tracking-[0.08em] text-muted-foreground/55">
          SSH Hosts
        </span>
        <button
          type="button"
          data-testid="sidebar-add-ssh-host"
          onClick={() => setDialogOpen(true)}
          className="flex size-5 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          aria-label="Add SSH host"
        >
          <Plus className="size-3" />
        </button>
      </div>

      {hosts.map((host) => {
        const expanded = expandedHostId === host.id;
        const listing = listings[host.id];
        return (
          <div key={host.id}>
            <button
              type="button"
              data-testid={`remote-host-row-${host.id}`}
              onClick={() => void toggleHost(host)}
              aria-expanded={expanded}
              className="group/remote-host flex h-7 w-full items-center gap-0.5 rounded-md pl-5 pr-1 text-worktree-sidebar-foreground/65 transition-colors hover:bg-worktree-sidebar-accent/60 hover:text-worktree-sidebar-foreground"
            >
              <ChevronRight
                aria-hidden="true"
                className={cn("size-3 shrink-0 transition-transform", expanded && "rotate-90")}
              />
              <Server className="size-3.5 shrink-0" />
              <span className="min-w-0 flex-1 truncate text-left text-[12px]">{host.label}</span>
              <span
                data-testid={`remote-host-title-${host.id}`}
                className="shrink-0 text-[10px] text-muted-foreground/70"
              >
                {host.username ? `${host.username}@${host.hostname}` : host.hostname}
              </span>
            </button>
            {expanded ? (
              <div className="pb-1 pl-10 pr-2">
                {listing?.loading ? (
                  <div className="py-1 text-[11px] text-muted-foreground">Listing remote worktrees…</div>
                ) : null}
                {listing?.error ? (
                  <div
                    data-testid={`remote-host-error-${host.id}`}
                    className="py-1 text-[11px] text-destructive"
                    title={listing.error}
                  >
                    {listing.error}
                  </div>
                ) : null}
                {listing?.worktrees?.map((worktree) => (
                  <button
                    key={worktree.path}
                    type="button"
                    data-testid={`remote-worktree-${worktree.path}`}
                    onClick={() =>
                      onOpenRemoteWorktree(host, {
                        path: worktree.path,
                        head: worktree.head ?? null,
                        branch: worktree.branch ?? null,
                      })
                    }
                    className="flex w-full items-center gap-1.5 rounded-sm py-1 text-left text-[11px] text-worktree-sidebar-foreground/70 transition-colors hover:text-worktree-sidebar-foreground"
                  >
                    <span className="min-w-0 flex-1 truncate">{worktree.branch ?? worktree.path}</span>
                  </button>
                ))}
                {listing?.worktrees && listing.worktrees.length === 0 ? (
                  <div className="py-1 text-[11px] text-muted-foreground">No remote worktrees</div>
                ) : null}
              </div>
            ) : null}
          </div>
        );
      })}

      {hosts.length === 0 ? (
        <div className="pb-1 pl-5 pr-2 text-[11px] text-muted-foreground/60">No SSH hosts yet</div>
      ) : null}

      {dialogOpen ? <SshHostDialog onClose={() => setDialogOpen(false)} /> : null}
    </div>
  );
}
