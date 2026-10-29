import { AlertCircle } from "lucide-react";
import { useState } from "react";

import {
  canRestartPreviousTerminals,
  daemonUpdatePendingFrom,
  pendingUpdateMessage,
  rereadDaemonUpdatePending,
  restartConfirmMessage,
  restartPreviousTerminals,
  type DaemonUpdatePending,
} from "../lib/updaterDaemonPending";
import { Button } from "./ui/button";

export type ProjectConnectionError = {
  code: string;
  message: string;
  details?: Record<string, unknown>;
};

function UpdatePendingActions(props: {
  initial: DaemonUpdatePending;
  onRetry: () => void;
}): JSX.Element {
  const [pending, setPending] = useState(props.initial);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const enabled = canRestartPreviousTerminals(pending) && !busy;

  const confirm = async (): Promise<void> => {
    setBusy(true);
    setNotice(null);
    try {
      const outcome = await restartPreviousTerminals(pending);
      setConfirming(false);
      if (outcome.outcome === "accepted") {
        props.onRetry();
      } else if (outcome.outcome === "countChanged") {
        setPending(await rereadDaemonUpdatePending());
        setNotice("The number of running terminals changed. Nothing was closed; check the count and confirm again.");
      } else if (outcome.outcome === "refused") {
        setNotice(`The previous version refused to restart (${outcome.code}). Nothing was closed.`);
      } else {
        setPending({ ...pending, liveSessions: null });
      }
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String((error as { message?: string })?.message ?? error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mt-2 flex flex-col gap-2" data-testid="daemon-update-pending">
      <div className="text-[12px] text-muted-foreground" data-testid="daemon-update-pending-text">
        {pendingUpdateMessage(pending)}
      </div>
      {notice ? <div className="text-[12px] text-status-warning">{notice}</div> : null}
      {confirming && pending.liveSessions !== null ? (
        <div className="flex items-center gap-2">
          <span className="text-[12px]" data-testid="daemon-restart-confirm-text">
            {restartConfirmMessage(pending.liveSessions)}
          </span>
          <Button type="button" size="sm" variant="destructive" className="h-7 text-[11px]"
            data-testid="daemon-restart-confirm" disabled={busy} onClick={() => void confirm()}>
            Restart {pending.liveSessions}
          </Button>
          <Button type="button" size="sm" variant="secondary" className="h-7 text-[11px]"
            data-testid="daemon-restart-cancel" disabled={busy} onClick={() => setConfirming(false)}>
            Cancel
          </Button>
        </div>
      ) : (
        <div>
          <Button type="button" size="sm" variant="secondary" className="h-7 text-[11px]"
            data-testid="daemon-restart-terminals" disabled={!enabled} onClick={() => setConfirming(true)}>
            Restart terminals now
          </Button>
        </div>
      )}
    </div>
  );
}

export function DaemonConnectionBanner(props: {
  error: ProjectConnectionError;
  onRetry: () => void;
}): JSX.Element {
  const { error, onRetry } = props;

  let title = "Couldn't open this project";
  let body = error.message;
  const updatePending = daemonUpdatePendingFrom(error);

  if (error.code === "DAEMON_UNAVAILABLE") {
    title = "Ferryx can't reach its background service";
    body =
      "Your saved tabs and panes are kept. Retry, or quit and reopen Ferryx if this keeps happening.";
  } else if (error.code === "DAEMON_PROTOCOL_MISMATCH") {
    title = "The background service is from a different Ferryx version";
    body =
      "Running terminals are kept. Quit and reopen Ferryx to finish the update.";
  } else if (updatePending?.restartRequired) {
    title = "Update installed";
    body =
      "Terminals are still running on the previous version. Restart them to finish updating; nothing closes until you confirm.";
  } else if (updatePending) {
    title = "Update installed";
    body =
      "The previous version keeps its terminals and stops by itself when the last one closes.";
  }

  return (
    <div
      role="alert"
      data-testid="daemon-connection-banner"
      className="flex items-start gap-3 border-b border-border bg-card px-4 py-3"
    >
      <AlertCircle
        aria-hidden="true"
        className="mt-0.5 size-4 shrink-0 text-status-warning"
      />

      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium text-foreground">{title}</div>
        <div className="mt-0.5 text-[12px] leading-relaxed text-muted-foreground">
          {body}
        </div>
        {updatePending ? (
          <UpdatePendingActions initial={updatePending} onRetry={onRetry} />
        ) : null}
        <div className="mt-1 font-mono text-[11px] text-muted-foreground">
          {error.code}
        </div>
      </div>

      <Button
        type="button"
        variant="secondary"
        size="sm"
        className="h-7 shrink-0 text-[11px]"
        data-testid="daemon-connection-retry"
        onClick={onRetry}
      >
        Retry
      </Button>
    </div>
  );
}
