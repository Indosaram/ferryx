import { useEffect, useState } from "react";
import { PlatformIcon } from "@/components/ui/PlatformIcons";

const FRAME_MS = 900;

function useCycle(length: number, intervalMs = FRAME_MS) {
  const [index, setIndex] = useState(0);
  useEffect(() => {
    const timer = window.setInterval(() => setIndex((value) => (value + 1) % length), intervalMs);
    return () => window.clearInterval(timer);
  }, [length, intervalMs]);
  return index;
}

function Chrome({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="overflow-hidden rounded-xl border border-line bg-[#0e0e10]">
      <div className="flex h-8 items-center gap-1.5 border-b border-white/10 px-3">
        <i className="h-2 w-2 rounded-full bg-white/25" />
        <i className="h-2 w-2 rounded-full bg-white/25" />
        <i className="h-2 w-2 rounded-full bg-white/25" />
        <span className="ml-1.5 font-mono text-[10px] text-white/40">{label}</span>
      </div>
      {/* Fixed body height: a growing visual would reflow the whole page on every frame. */}
      <div className="h-[168px] overflow-hidden">{children}</div>
    </div>
  );
}

export function GhosttyVisual() {
  const frame = useCycle(3);
  const rows = [
    "$ ferryx --version",
    "ferryx 0.1.0 (libghostty + wgpu)",
    "renderer: wgpu / Metal",
  ];
  return (
    <Chrome label="ghostty">
      <div className="flex h-full flex-col justify-center gap-2 px-4 font-mono text-[13px] leading-[1.6]">
        {rows.map((row, index) => (
          <div
            key={row}
            className={`${row.startsWith("$") ? "text-white/85" : "text-emerald-300/80"} ${index <= frame ? "opacity-100" : "opacity-0"} transition-opacity duration-300`}
          >
            {row}
          </div>
        ))}
      </div>
    </Chrome>
  );
}

export function AgentsVisual() {
  const active = useCycle(3, 1100);
  const agents = [
    { name: "claude", task: "refactor ipc layer" },
    { name: "codex", task: "add regression test" },
    { name: "gemini", task: "update docs" },
  ];
  return (
    <Chrome label="agents">
      <div className="flex h-full flex-col justify-center gap-3 px-4">
        {agents.map((agent, index) => (
          <div key={agent.name} className="flex items-center gap-3 font-mono text-[13px]">
            <span
              className={`h-2 w-2 shrink-0 rounded-full ${index === active ? "bg-emerald-400" : "bg-white/20"}`}
            />
            <span className="w-16 shrink-0 text-white/80">{agent.name}</span>
            <span className="truncate text-white/40">{agent.task}</span>
          </div>
        ))}
      </div>
    </Chrome>
  );
}

export function SplitVisual() {
  const vertical = useCycle(2, 1400) === 1;
  return (
    <Chrome label="split panes">
      <div className={`flex h-full gap-2 p-3 ${vertical ? "flex-col" : "flex-row"}`}>
        <div className="flex-1 rounded-md bg-white/[0.07] transition-all duration-500" />
        <div className="flex-1 rounded-md bg-white/[0.04] transition-all duration-500" />
      </div>
    </Chrome>
  );
}

export function MobileVisual() {
  const step = useCycle(2, 1500);
  return (
    <Chrome label="pairing">
      <div className="flex h-full items-center justify-center gap-6 px-4">
        <div className="grid grid-cols-4 gap-1">
          {Array.from({ length: 16 }).map((_, cell) => (
            <i
              key={cell}
              className={`h-3 w-3 rounded-[2px] ${(cell * 7 + step * 3) % 3 === 0 ? "bg-white/70" : "bg-white/15"}`}
            />
          ))}
        </div>
        <div className="font-mono text-[13px] text-white/60">
          <div className="text-[18px] text-white/85">4F2A-91</div>
          <div className="mt-1 text-[12px]">{step === 0 ? "waiting…" : "paired"}</div>
        </div>
      </div>
    </Chrome>
  );
}

export function ZeroElectronVisual() {
  const platforms = ["macos", "windows", "linux"] as const;
  const active = useCycle(platforms.length, 1200);
  return (
    <Chrome label="native builds">
      <div className="flex h-full items-center justify-center gap-10">
        {platforms.map((platform, index) => (
          <PlatformIcon
            key={platform}
            platform={platform}
            className={`h-10 w-10 transition-opacity duration-500 ${index === active ? "text-white/90" : "text-white/25"}`}
          />
        ))}
      </div>
    </Chrome>
  );
}

export function PersistenceVisual() {
  const step = useCycle(3, 1000);
  const stages = ["snapshot", "daemon alive", "reattached"];
  return (
    <Chrome label="persistence">
      <div className="flex h-full flex-col justify-center gap-3 px-4 font-mono text-[13px]">
        {stages.map((stage, index) => (
          <div key={stage} className="flex items-center gap-3">
            <span className={index <= step ? "text-emerald-300/80" : "text-white/20"}>
              {index <= step ? "✓" : "·"}
            </span>
            <span className={index <= step ? "text-white/75" : "text-white/25"}>{stage}</span>
          </div>
        ))}
      </div>
    </Chrome>
  );
}
