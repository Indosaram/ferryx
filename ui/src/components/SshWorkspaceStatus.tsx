import { Loader2 } from "lucide-react";
import { Button } from "./ui/button";

type SshWorkspaceStatusProps = {
  readonly hostLabel: string;
  readonly message: string;
  readonly error?: string;
  readonly onRetry: () => void;
};

export function SshWorkspaceStatus({ hostLabel, message, error, onRetry }: SshWorkspaceStatusProps) {
  return (
    <div
      data-testid="ssh-workspace-status"
      role="status"
      aria-live="polite"
      aria-busy={!error}
      className="flex h-full min-w-0 flex-1 flex-col items-center justify-center gap-3 overflow-auto bg-background p-6 text-center text-muted-foreground"
    >
      {!error && <Loader2 aria-hidden="true" className="size-5 shrink-0 animate-spin motion-reduce:animate-none" />}
      <p className="max-w-full break-words text-sm font-medium">{error ? "SSH connection failed" : message}</p>
      <p className="max-w-full break-words text-xs">{hostLabel}</p>
      {error && (
        <>
          <p className="max-w-full select-text break-words text-xs">{error}</p>
          <Button size="sm" variant="secondary" onClick={onRetry}>Retry connection</Button>
        </>
      )}
    </div>
  );
}
