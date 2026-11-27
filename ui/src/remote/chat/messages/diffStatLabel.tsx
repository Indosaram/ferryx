import React from "react";
import { cn } from "../../../lib/cn";

/**
 * The +/- line-change stat beside a diff (t3code DiffStatLabel). Counts are
 * compacted the way the reference compacts them (1.2k), the group carries one
 * readable label for a screen reader, and the visual halves stay aria-hidden
 * so the numbers are not read twice.
 */

export function formatCompactDiffCount(value: number): string {
  const count = Math.abs(Math.trunc(value));
  if (count < 1000) return String(count);
  if (count < 999_500) {
    const k = count / 1000;
    return `${k < 10 ? k.toFixed(1).replace(/\.0$/, "") : Math.round(k)}k`;
  }
  if (count < 999_500_000) {
    const m = count / 1_000_000;
    return `${m < 10 ? m.toFixed(1).replace(/\.0$/, "") : Math.round(m)}m`;
  }
  const b = count / 1_000_000_000;
  return `${b < 10 ? b.toFixed(1).replace(/\.0$/, "") : Math.round(b)}b`;
}

/** "12 additions, 3 deletions", the sentence a screen reader hears. */
export function diffStatLabelText(additions: number, deletions: number): string {
  const additionsPart = `${additions} addition${additions === 1 ? "" : "s"}`;
  const deletionsPart = `${deletions} deletion${deletions === 1 ? "" : "s"}`;
  return `${additionsPart}, ${deletionsPart}`;
}

export const DiffStatLabel: React.FC<{
  additions: number;
  deletions: number;
  className?: string;
  layout?: "aligned" | "inline";
}> = ({ additions, deletions, className, layout = "aligned" }) => (
  <span
    data-testid="diff-stat-label"
    role="group"
    aria-label={diffStatLabelText(additions, deletions)}
    className={cn(
      "shrink-0 font-mono text-xs tabular-nums",
      layout === "inline" ? "inline-flex items-center gap-1 align-middle" : "inline-grid grid-cols-[4ch_4ch] gap-2 text-right align-middle",
      className,
    )}
  >
    <span aria-hidden="true" className="text-status-success">
      +{formatCompactDiffCount(additions)}
    </span>
    <span aria-hidden="true" className="text-chat-danger">
      -{formatCompactDiffCount(deletions)}
    </span>
  </span>
);
