import { useMemo, useState } from "react";
import { detectDelimiter, parseDelimited } from "../lib/csvParse";
import { cn } from "../lib/cn";
import { useLineWindow } from "../lib/useLineWindow";

const MAX_DISPLAY_ROWS = 5000;
/** Tables with more body rows than this mount only the visible rows (plus overscan). */
export const CSV_TABLE_WINDOW_THRESHOLD = 500;
/** Raw text with more lines than this mounts only the visible lines (plus overscan). */
export const CSV_RAW_WINDOW_THRESHOLD = 2000;
/**
 * Estimated pitch of a windowed table row: a one-line (h-5) cell, py-1.5 padding and a
 * 1px border. The real pitch is measured from the mounted rows once layout exists.
 */
const CSV_TABLE_ROW_HEIGHT = 33;
/** Pitch of one raw line (`h-5`). */
const CSV_RAW_LINE_HEIGHT = 20;

export function FilePreviewCsv({
  text,
  displayName,
}: {
  text: string;
  displayName: string;
}) {
  const [showRaw, setShowRaw] = useState(false);

  const delimiter = useMemo(() => detectDelimiter(displayName, text), [displayName, text]);
  const rows = useMemo(() => parseDelimited(text, delimiter), [text, delimiter]);

  const headerRow = rows.length > 0 ? rows[0] : null;
  const totalBodyRows = rows.length > 1 ? rows.length - 1 : 0;
  const isTruncated = totalBodyRows > MAX_DISPLAY_ROWS;
  const bodyRows = useMemo(() => (rows.length > 1 ? rows.slice(1, 1 + MAX_DISPLAY_ROWS) : []), [rows]);
  const columnCount = 1 + Math.max(headerRow?.length ?? 0, ...bodyRows.slice(0, 1).map((row) => row.length));

  const rawLines = useMemo(() => (showRaw ? text.split(/\r\n|\r|\n/) : []), [showRaw, text]);

  // Windowing assumes a fixed row pitch, so windowed table cells show one line each
  // (the full value stays in the cell's title and in the Raw view).
  const tableWindow = useLineWindow({
    count: bodyRows.length,
    enabled: !showRaw && bodyRows.length > CSV_TABLE_WINDOW_THRESHOLD,
    estimatedRowHeight: CSV_TABLE_ROW_HEIGHT,
  });
  const rawWindow = useLineWindow({
    count: rawLines.length,
    enabled: showRaw && rawLines.length > CSV_RAW_WINDOW_THRESHOLD,
    estimatedRowHeight: CSV_RAW_LINE_HEIGHT,
  });
  const tableWindowed = tableWindow.active;

  return (
    <div className="flex h-full min-h-0 w-full flex-col bg-card text-foreground">
      <div className="flex h-9 shrink-0 items-center justify-between border-b border-border px-3">
        <span className="truncate text-[12px] font-medium text-foreground">
          {displayName}
        </span>
        <button
          type="button"
          data-testid="file-preview-csv-raw-toggle"
          onClick={() => setShowRaw((prev) => !prev)}
          className="inline-flex items-center rounded-md border border-border bg-background px-2 py-1 text-[12px] font-medium text-foreground hover:bg-accent"
        >
          {showRaw ? "Table" : "Raw"}
        </button>
      </div>

      {showRaw ? (
        <div key="raw" ref={rawWindow.scrollRef} className="min-h-0 flex-1 overflow-auto">
          {rawWindow.active ? (
            <pre
              data-testid="file-preview-csv-raw"
              data-windowed="true"
              className="selectable p-3 font-mono text-[12px] leading-5 text-foreground"
            >
              {/* Block spans (phrasing content, valid inside <pre>) give one fixed-height row per line. */}
              <span ref={rawWindow.listRef} className="block">
                {rawWindow.padTop > 0 ? (
                  <span aria-hidden className="block" style={{ height: rawWindow.padTop }} />
                ) : null}
                {rawLines.slice(rawWindow.start, rawWindow.end).map((line, offset) => (
                  <span
                    key={rawWindow.start + offset}
                    data-line-window-row=""
                    data-testid={`file-preview-csv-raw-line-${rawWindow.start + offset + 1}`}
                    className="block h-5"
                  >
                    {line}
                  </span>
                ))}
                {rawWindow.padBottom > 0 ? (
                  <span aria-hidden className="block" style={{ height: rawWindow.padBottom }} />
                ) : null}
              </span>
            </pre>
          ) : (
            <pre
              data-testid="file-preview-csv-raw"
              className="selectable p-3 font-mono text-[12px] text-foreground"
            >
              {text}
            </pre>
          )}
        </div>
      ) : (
        <div key="table" ref={tableWindow.scrollRef} className="min-h-0 flex-1 overflow-auto">
          <div className="p-3">
            <table
              data-testid="file-preview-csv"
              data-windowed={tableWindowed ? "true" : undefined}
              className="selectable w-full border-collapse text-left text-[12px]"
            >
              {headerRow && (
                <thead>
                  <tr className="border-b border-border">
                    <th className="sticky top-0 z-10 border-b border-border bg-card px-3 py-2 text-right font-medium text-muted-foreground select-none">
                      #
                    </th>
                    {headerRow.map((cell, index) => (
                      <th
                        key={index}
                        className="sticky top-0 z-10 border-b border-border bg-card px-3 py-2 font-medium text-foreground"
                      >
                        {cell}
                      </th>
                    ))}
                  </tr>
                </thead>
              )}
              <tbody ref={tableWindow.listRef}>
                {tableWindow.padTop > 0 ? (
                  <tr aria-hidden data-testid="file-preview-csv-spacer-top" style={{ height: tableWindow.padTop }}>
                    <td colSpan={columnCount} className="p-0" />
                  </tr>
                ) : null}
                {bodyRows.slice(tableWindow.start, tableWindow.end).map((row, offset) => {
                  const rowIndex = tableWindow.start + offset;
                  return (
                    <tr
                      key={rowIndex}
                      data-line-window-row=""
                      className="border-b border-border/50 hover:bg-accent/40"
                    >
                      <td
                        className={cn(
                          "px-3 py-1.5 text-right font-mono text-muted-foreground select-none",
                          tableWindowed && "leading-5",
                        )}
                      >
                        {rowIndex + 1}
                      </td>
                      {row.map((cell, cellIndex) =>
                        tableWindowed ? (
                          <td key={cellIndex} title={cell} className="px-3 py-1.5 text-foreground">
                            <div className="h-5 max-w-[48ch] overflow-hidden text-ellipsis whitespace-pre leading-5">
                              {cell}
                            </div>
                          </td>
                        ) : (
                          <td
                            key={cellIndex}
                            className="px-3 py-1.5 text-foreground whitespace-pre-wrap"
                          >
                            {cell}
                          </td>
                        ),
                      )}
                    </tr>
                  );
                })}
                {tableWindow.padBottom > 0 ? (
                  <tr aria-hidden data-testid="file-preview-csv-spacer-bottom" style={{ height: tableWindow.padBottom }}>
                    <td colSpan={columnCount} className="p-0" />
                  </tr>
                ) : null}
              </tbody>
            </table>

            {isTruncated && (
              <p
                data-testid="file-preview-csv-truncated"
                className="mt-3 text-center text-[12px] text-muted-foreground"
              >
                Showing 5000 of {totalBodyRows} rows
              </p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
