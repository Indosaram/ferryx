import {
  GitBranch,
  Laptop,
  RefreshCw,
  Terminal as TerminalIcon,
} from "lucide-react";
import React from "react";

export type RemoteSessionItem = {
  sessionId: string;
  title?: string | null;
  workspaceId?: string | null;
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
    <div className="max-w-xl mx-auto p-4 space-y-6">
      {/* Workspace Header Overview */}
      <div className="flex items-center justify-between pb-3 border-b border-[#262833]">
        <div className="flex items-center gap-2.5">
          <div className="w-8 h-8 rounded-lg bg-blue-600/10 border border-blue-500/20 flex items-center justify-center text-blue-400">
            <Laptop size={18} />
          </div>
          <div>
            <h2 className="text-sm font-semibold text-white tracking-tight">
              Desktop Workspace
            </h2>
            <p className="text-[11px] text-[#8b949e]">
              Connected to Ferryx native engine
            </p>
          </div>
        </div>
        <button
          type="button"
          onClick={onRefresh}
          className="flex items-center gap-1.5 px-2.5 py-1.5 bg-[#1a1d26] hover:bg-[#222734] active:scale-95 text-[#c9d1d9] border border-[#2c3140] rounded text-xs font-medium transition-all"
        >
          <RefreshCw size={13} />
          Refresh
        </button>
      </div>

      {/* Terminal Sessions Section */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <TerminalIcon size={15} className="text-[#8b949e]" />
            <h3 className="text-xs font-semibold uppercase tracking-wider text-[#8b949e]">
              Active Terminals ({sessions.length})
            </h3>
          </div>
        </div>

        {sessions.length === 0 ? (
          <div className="text-center py-12 px-4 bg-[#12141a] border border-[#262833] rounded-xl space-y-3">
            <div className="w-10 h-10 mx-auto rounded-full bg-[#1c202c] flex items-center justify-center text-[#6e7681]">
              <TerminalIcon size={20} />
            </div>
            <div className="space-y-1">
              <p className="text-sm font-medium text-[#c9d1d9]">No active terminals</p>
              <p className="text-xs text-[#8b949e]">
                Open a terminal in your desktop Ferryx app to attach.
              </p>
            </div>
          </div>
        ) : (
          <div className="grid gap-2.5">
            {sessions.map((s) => (
              <div
                key={s.sessionId}
                onClick={() => onSelect(s.sessionId)}
                className="group relative flex items-center justify-between p-3.5 bg-[#12141a] border border-[#262833] hover:border-blue-500/50 hover:bg-[#161922] rounded-xl cursor-pointer transition-all active:scale-[0.99]"
              >
                <div className="flex items-start gap-3 min-w-0">
                  <div className="w-8 h-8 rounded-lg bg-[#1c202c] border border-[#2c3140] flex items-center justify-center text-[#8b949e] group-hover:text-blue-400 group-hover:border-blue-500/30 transition-colors shrink-0 mt-0.5">
                    <TerminalIcon size={16} />
                  </div>
                  <div className="min-w-0 space-y-0.5">
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-xs font-semibold text-[#fafafa] truncate">
                        {s.title || `Terminal (${s.sessionId.substring(0, 8)})`}
                      </span>
                      <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium bg-emerald-500/10 text-emerald-400 border border-emerald-500/20">
                        <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
                        Running
                      </span>
                    </div>
                    <div className="flex items-center gap-2 text-[11px] text-[#8b949e]">
                      <span className="flex items-center gap-1 font-mono">
                        <GitBranch size={12} />
                        {s.worktreeLabel || "main"}
                      </span>
                      <span>•</span>
                      <span className="font-mono text-[10px] text-[#6e7681]">
                        ID: {s.sessionId.substring(0, 8)}
                      </span>
                    </div>
                  </div>
                </div>

                <div className="shrink-0 pl-3">
                  <span className="inline-flex items-center justify-center px-3 py-1.5 bg-blue-600 group-hover:bg-blue-500 text-white rounded-lg text-xs font-medium shadow-sm transition-colors">
                    Attach
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};
