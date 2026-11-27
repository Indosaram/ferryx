import React, { useEffect, useRef, useState } from "react";
import { Check, Copy } from "lucide-react";
import { copyTextToClipboard } from "../../../lib/clipboard";
import { cn } from "../../../lib/cn";

/**
 * Markdown tables (t3code MarkdownTable). The table itself scrolls sideways
 * inside the chat lane; a copy control beside it hands the table back as
 * markdown, CSV, or HTML, because a phone has no way to select a table's cells
 * cleanly.
 */

export type TableCopyFormat = "markdown" | "csv" | "html";

const TABLE_SELECTOR = "table";

interface Grid {
  readonly header: readonly string[];
  readonly rows: readonly (readonly string[])[];
}

/** A rendered table's text as a grid, read from the DOM it is drawn from. */
export function tableGridFromElement(table: Element | null): Grid | null {
  if (!table) return null;
  const rows = Array.from(table.querySelectorAll("tr")).map((row) =>
    Array.from(row.querySelectorAll("th,td")).map((cell) => (cell.textContent ?? "").trim()),
  );
  if (rows.length === 0) return null;
  const headerRow = table.querySelector("thead tr");
  const header = headerRow ? rows[0]! : [];
  return { header, rows: headerRow ? rows.slice(1) : rows };
}

const escapeMarkdownCell = (cell: string) => cell.replace(/\|/g, "\\|").replace(/\r?\n/g, " ");
const escapeHtml = (text: string) =>
  text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

function csvCell(cell: string): string {
  return /[",\r\n]/.test(cell) ? `"${cell.replace(/"/g, '""')}"` : cell;
}

export function serializeTable(grid: Grid, format: TableCopyFormat): string {
  const { header, rows } = grid;
  const width = rows.reduce((max, row) => Math.max(max, row.length), header.length);
  const pad = (row: readonly string[]) =>
    Array.from({ length: width }, (_, index) => row[index] ?? "");
  if (format === "csv") {
    return [header, ...rows]
      .map((row) => pad(row).map(csvCell).join(","))
      .join("\n");
  }
  if (format === "html") {
    const head = header.length > 0 ? `<thead><tr>${pad(header).map((cell) => `<th>${escapeHtml(cell)}</th>`).join("")}</tr></thead>` : "";
    const body = `<tbody>${rows
      .map((row) => `<tr>${pad(row).map((cell) => `<td>${escapeHtml(cell)}</td>`).join("")}</tr>`)
      .join("")}</tbody>`;
    return `<table>${head}${body}</table>`;
  }
  const headerLine = pad(header);
  const divider = Array.from({ length: width }, () => "---");
  return [
    `| ${headerLine.map(escapeMarkdownCell).join(" | ")} |`,
    `| ${divider.join(" | ")} |`,
    ...rows.map((row) => `| ${pad(row).map(escapeMarkdownCell).join(" | ")} |`),
  ].join("\n");
}

/** One button that copies markdown, plus a menu for the other two formats. */
export const MarkdownTable: React.FC<{
  children?: React.ReactNode;
  className?: string;
}> = ({ children, className }) => {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [copied, setCopied] = useState<TableCopyFormat | null>(null);

  // A menu that cannot be dismissed is a trap: Escape and a tap outside close it.
  useEffect(() => {
    if (!menuOpen) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        setMenuOpen(false);
      }
    };
    const onPointerDown = (event: PointerEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) setMenuOpen(false);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointerDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointerDown);
    };
  }, [menuOpen]);

  const copy = async (format: TableCopyFormat) => {
    const grid = tableGridFromElement(containerRef.current?.querySelector(TABLE_SELECTOR) ?? null);
    if (!grid) return;
    const ok = await copyTextToClipboard(serializeTable(grid, format));
    setCopied(ok ? format : null);
    setMenuOpen(false);
  };

  return (
    <div
      ref={containerRef}
      data-testid="markdown-table"
      className={cn("my-2 w-full", className)}
    >
      <div className="w-full overflow-x-auto">
        <table className="w-full border-collapse text-sm">{children}</table>
      </div>
      <div className="mt-0.5 flex items-center justify-end select-none">
        <button
          type="button"
          data-testid="table-copy-markdown"
          aria-label={copied === "markdown" ? "Copied" : "Copy table as markdown"}
          onClick={() => void copy("markdown")}
          className="flex min-h-9 items-center gap-1 px-2 text-xs text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          {copied === "markdown" ? (
            <Check aria-hidden="true" className="size-3" />
          ) : (
            <Copy aria-hidden="true" className="size-3" />
          )}
          <span>{copied === "markdown" ? "Copied" : "Copy table"}</span>
        </button>
        <button
          type="button"
          data-testid="table-copy-more"
          aria-label="More table formats"
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          onClick={() => setMenuOpen((prev) => !prev)}
          className="flex min-h-9 items-center px-2 text-xs text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          …
        </button>
      </div>
      {menuOpen && (
        <div
          ref={menuRef}
          role="menu"
          data-testid="table-copy-menu"
          className="ml-auto flex w-40 flex-col overflow-hidden rounded-lg border border-chat-border bg-chat-surface shadow-xl"
        >
          {(["csv", "html"] as const).map((format) => (
            <button
              key={format}
              type="button"
              role="menuitem"
              data-testid={`table-copy-${format}`}
              onClick={() => void copy(format)}
              className="flex min-h-11 items-center px-3 text-sm text-chat-foreground transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              {`Copy as ${format.toUpperCase()}`}
            </button>
          ))}
        </div>
      )}
    </div>
  );
};
