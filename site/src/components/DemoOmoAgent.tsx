import { useEffect, useRef, useState } from "react";

const MODEL = "gpt-5.6-terra:xhigh";
const TICK_MS = 45;
const CHARS_PER_TICK = 2;
const HOLD_TICKS = 60;

type Segment = {
  readonly kind: "prompt" | "reasoning" | "tool" | "text";
  readonly text: string;
};

const SCRIPT: readonly Segment[] = [
  { kind: "prompt", text: "how does the PTY daemon keep sessions alive?" },
  { kind: "reasoning", text: "Reading src-tauri/src/daemon/" },
  { kind: "tool", text: "grep  BoundedBuffer  src-tauri/src/terminal/" },
  { kind: "text", text: "Sessions live in the headless daemon, not the GUI, so" },
  { kind: "text", text: "closing the window never touches the PTYs." },
  { kind: "text", text: "Output buffers in a 512 KiB ring with monotonic sequence" },
  { kind: "text", text: "numbers, and reattach replays from the last sequence." },
];

const TOTAL_CHARS = SCRIPT.reduce((sum, segment) => sum + segment.text.length, 0);

function usePlayback() {
  const [revealed, setRevealed] = useState(0);
  useEffect(() => {
    let holding = 0;
    const timer = window.setInterval(() => {
      setRevealed((current) => {
        if (current < TOTAL_CHARS) return Math.min(TOTAL_CHARS, current + CHARS_PER_TICK);
        holding += 1;
        if (holding <= HOLD_TICKS) return current;
        holding = 0;
        return 0;
      });
    }, TICK_MS);
    return () => window.clearInterval(timer);
  }, []);
  return revealed;
}

function segmentSlices(revealed: number) {
  let consumed = 0;
  return SCRIPT.map((segment) => {
    const start = consumed;
    consumed += segment.text.length;
    const shown = Math.max(0, Math.min(segment.text.length, revealed - start));
    return { segment, shown, streaming: revealed > start && revealed < consumed };
  });
}

function SegmentLine({ segment, shown }: { segment: Segment; shown: number }) {
  const text = segment.text.slice(0, shown);
  if (segment.kind === "prompt") {
    return (
      <div className="flex gap-2">
        <span className="shrink-0 text-emerald-400">❯</span>
        <span className="text-zinc-100">{text}</span>
      </div>
    );
  }
  if (segment.kind === "reasoning") {
    return <div className="text-zinc-500 italic">{text}</div>;
  }
  if (segment.kind === "tool") {
    return (
      <div className="flex gap-2">
        <span className="shrink-0 text-sky-400/80">⏺</span>
        <span className="text-zinc-400">{text}</span>
      </div>
    );
  }
  return <div className="text-zinc-300">{text}</div>;
}

export function DemoOmoAgent() {
  const revealed = usePlayback();
  const transcriptRef = useRef<HTMLDivElement>(null);
  const slices = segmentSlices(revealed);
  const streaming = revealed > 0 && revealed < TOTAL_CHARS;
  const tokens = Math.round(revealed * 0.31);

  useEffect(() => {
    const node = transcriptRef.current;
    if (node) node.scrollTop = node.scrollHeight;
  }, [revealed]);

  return (
    <div className="absolute inset-0 flex flex-col bg-[#0a0a0a] font-mono text-[12px] leading-[1.55] text-zinc-300">
      <div ref={transcriptRef} className="min-h-0 flex-1 space-y-1 overflow-hidden px-3 py-2">
        <div className="text-zinc-500">omo (omo-ai beta 5.0.0-0.beta.21)</div>
        <div className="text-zinc-500">Optimized system prompt applied: gpt-5.6</div>
        <div className="pb-1 pt-0.5 text-zinc-400">
          <span className="bg-zinc-800 px-1 text-zinc-300">Settings: settings.json (JSON)</span>
        </div>
        {slices.map(({ segment, shown }, index) =>
          shown > 0 ? <SegmentLine key={index} segment={segment} shown={shown} /> : null,
        )}
        {streaming ? (
          <span className="inline-block h-[13px] w-[6px] animate-pulse bg-zinc-300/80 align-middle" />
        ) : null}
      </div>

      <div className="shrink-0 px-3 pb-2">
        <div className="border-t border-zinc-800" />
        <div className="flex items-center gap-2 py-1.5">
          <span className="text-emerald-400">❯</span>
          {streaming ? (
            <span className="text-zinc-500">esc to interrupt</span>
          ) : (
            <span className="inline-block h-[14px] w-[7px] animate-pulse bg-zinc-400/80" />
          )}
        </div>
        <div className="border-t border-zinc-800" />
        <div className="flex items-center justify-between pt-1.5 text-[11px] text-zinc-500">
          <span>
            ~/code/ferryx • {tokens}/400K ({((tokens / 400000) * 100).toFixed(1)}%) (auto)
          </span>
          <span className="truncate pl-2">(quotio) {MODEL}</span>
        </div>
        <div className="pt-0.5 text-[11px] text-zinc-600">(😺 OmO Native) mem:ferryx-73b13e40</div>
      </div>
    </div>
  );
}
