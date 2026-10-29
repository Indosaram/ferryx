// The phone's one Inbox on the home: the same rows the session sheet shows (every waiting or
// finished session on the host), each needs-you row answerable in place.
import { Inbox } from "lucide-react";

import { AttentionInbox } from "../../features/ferryx/attention/AttentionInbox";
import type { AttentionRow } from "../../features/ferryx/attention/attentionModel";
import type { AttentionPrompts } from "../../features/ferryx/attention/attentionPrompts";

export type InboxHomeProps = {
  readonly rows: readonly AttentionRow[];
  readonly prompts?: AttentionPrompts;
  readonly now: number;
  readonly loading: boolean;
  readonly onOpen: (row: AttentionRow) => void;
};

export function InboxHome({ rows, prompts, now, loading, onOpen }: InboxHomeProps) {
  return (
    <div data-testid="inbox-home" className="flex min-h-0 flex-1 flex-col overflow-y-auto bg-chat-screen scrollbar-sleek">
      <div className="flex w-full flex-1 flex-col gap-3 px-4 pb-6 pt-4">
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
          <AttentionInbox compact rows={rows} prompts={prompts} now={now} onOpen={onOpen} />
        )}
      </div>
    </div>
  );
}
