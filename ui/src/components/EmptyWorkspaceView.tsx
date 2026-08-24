import { Globe, Terminal } from "lucide-react";

export interface EmptyWorkspaceViewProps {
  onNewTerminal: () => void;
  onNewBrowserTab: () => void;
}

export function EmptyWorkspaceView({ onNewTerminal, onNewBrowserTab }: EmptyWorkspaceViewProps) {
  return (
    <div
      data-testid="empty-workspace-view"
      className="flex h-full flex-1 flex-col items-center justify-center gap-4 bg-background"
    >
      <div className="flex flex-col items-center gap-1 text-center">
        <p className="text-sm font-medium text-muted-foreground">No open tabs</p>
        <p className="text-xs text-muted-foreground/70">Open a terminal or browser tab to get started.</p>
      </div>
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={onNewTerminal}
          className="flex items-center gap-2 rounded-md bg-accent px-3 py-1.5 text-xs font-medium hover:bg-accent/80"
        >
          <Terminal className="size-4" />
          <span>New Terminal</span>
        </button>
        <button
          type="button"
          onClick={onNewBrowserTab}
          className="flex items-center gap-2 rounded-md border border-border px-3 py-1.5 text-xs font-medium hover:bg-accent/50"
        >
          <Globe className="size-4" />
          <span>New Browser Tab</span>
        </button>
      </div>
    </div>
  );
}
