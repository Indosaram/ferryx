import React from "react";

export type RemoteSessionItem = {
  sessionId: string;
  title?: string | null;
  worktreeLabel?: string | null;
  running: boolean;
};

type RemoteSessionListProps = {
  sessions: RemoteSessionItem[];
  onSelect: (sessionId: string) => void;
  onRefresh: () => void;
};

export const RemoteSessionList: React.FC<RemoteSessionListProps> = ({
  sessions,
  onSelect,
  onRefresh,
}) => {
  return (
    <div className="max-w-md mx-auto p-4 space-y-4">
      <div className="flex justify-between items-center">
        <h2 className="text-sm font-semibold uppercase tracking-wider text-neutral-400">
          Desktop Terminals
        </h2>
        <button
          onClick={onRefresh}
          className="text-xs px-2 py-1 bg-neutral-800 hover:bg-neutral-700 text-neutral-300 rounded"
        >
          Refresh
        </button>
      </div>

      {sessions.length === 0 ? (
        <div className="text-center p-8 bg-neutral-900 border border-neutral-800 rounded-lg text-neutral-500 text-sm">
          No active terminals found on desktop.
        </div>
      ) : (
        <div className="space-y-2">
          {sessions.map((s) => (
            <div
              key={s.sessionId}
              onClick={() => onSelect(s.sessionId)}
              className="flex justify-between items-center p-3 bg-neutral-900 border border-neutral-800 hover:border-blue-500 rounded-lg cursor-pointer transition-colors"
            >
              <div>
                <div className="font-mono text-sm text-neutral-200">
                  {s.sessionId.substring(0, 8)}
                </div>
                <div className="text-xs text-neutral-500">
                  {s.worktreeLabel || "Default workspace"}
                </div>
              </div>
              <span className="text-xs font-medium px-2 py-1 bg-blue-950 text-blue-300 border border-blue-800 rounded">
                Attach
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};
