// Cross-machine needs-you list: every session waiting on the person, then finished ones.
import { Inbox } from "lucide-react";

import { isMonochromeAgentLogo, resolveAgentLogo } from "../../lib/agentIcon";
import { cn } from "../../lib/cn";
import { INBOX_WHY, relativeTime, type InboxRow } from "./phoneHomeModel";

export type InboxHomeProps = {
  readonly rows: readonly InboxRow[];
  readonly now: number;
  readonly loading: boolean;
  readonly onOpen: (row: InboxRow) => void;
};

export function InboxHome({ rows, now, loading, onOpen }: InboxHomeProps) {
  return (
    <div data-testid="inbox-home" className="flex min-h-0 flex-1 flex-col overflow-y-auto bg-chat-screen scrollbar-sleek">
      <div className="flex w-full flex-col gap-3 px-4 pb-6 pt-4">
        <h1 className="px-1 text-[17px] font-semibold text-chat-foreground">Inbox</h1>
        {rows.length === 0 ? (
          <div className="flex flex-col items-center gap-2 rounded-2xl bg-chat-surface px-4 py-8 text-center">
            <Inbox aria-hidden="true" className="size-5 text-chat-foreground-secondary" />
            <p className="text-[14px] font-medium text-chat-foreground">{loading ? "Checking your computers…" : "Nothing needs you"}</p>
            {loading ? null : (
              <p className="text-[13px] leading-snug text-chat-foreground-secondary">
                Sessions that wait for an answer or finish on any computer show up here.
              </p>
            )}
          </div>
        ) : (
          <ul className="flex flex-col gap-2">
            {rows.map((row) => {
              const logo = resolveAgentLogo(row.agentType);
              return (
                <li key={row.key}>
                  <button
                    type="button"
                    data-testid="inbox-row"
                    data-machine-id={row.machineId}
                    data-session-id={row.sessionId}
                    data-kind={row.kind}
                    onClick={() => onOpen(row)}
                    className="flex min-h-11 w-full min-w-0 items-start gap-3 rounded-xl bg-chat-surface px-3 py-3 text-left transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <span
                      aria-hidden="true"
                      className={cn(
                        "mt-1.5 size-2 shrink-0 rounded-full",
                        row.kind === "done" ? "bg-status-success" : "bg-status-warning",
                      )}
                    />
                    <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span className="flex min-w-0 items-center gap-1.5">
                        {logo ? (
                          <img src={logo} alt="" className={cn("size-4 shrink-0", isMonochromeAgentLogo(row.agentType) && "agent-tab-logo--monochrome opacity-80")} />
                        ) : null}
                        <span className="min-w-0 flex-1 truncate text-[15px] font-medium text-chat-foreground">{row.title}</span>
                        {row.observedAt ? (
                          <span className="shrink-0 text-[12px] tabular-nums text-chat-foreground-secondary">{relativeTime(row.observedAt, now)}</span>
                        ) : null}
                      </span>
                      <span className="min-w-0 truncate text-[13px] text-chat-foreground-secondary">{row.where}</span>
                      <span className={cn("text-[13px]", row.kind === "done" ? "text-chat-foreground-secondary" : "text-status-warning")}>
                        {INBOX_WHY[row.kind]}
                      </span>
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}
