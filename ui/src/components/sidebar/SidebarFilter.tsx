import { cn } from "../../lib/cn";
import { Input } from "../ui/input";
import type { SidebarDensity } from "./SidebarWorktreeRow";

export type SidebarFilterProps = {
  readonly value: string;
  readonly onChange: (value: string) => void;
  readonly density?: SidebarDensity;
};

/** Session-only filter box; Escape clears a non-empty query without leaking the key further. */
export function SidebarFilter({ value, onChange, density = "compact" }: SidebarFilterProps) {
  return (
    <div className="shrink-0 px-2 pt-1">
      <Input
        type="search"
        value={value}
        placeholder="Filter worktrees"
        aria-label="Filter worktrees"
        className={cn(density === "touch" ? "h-11 text-base md:text-base" : "h-7 text-[12px] md:text-[12px]")}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key !== "Escape" || value === "") return;
          event.preventDefault();
          event.stopPropagation();
          onChange("");
        }}
      />
    </div>
  );
}
