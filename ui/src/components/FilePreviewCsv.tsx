import { useMemo, useState } from "react";
import { detectDelimiter, parseDelimited } from "../lib/csvParse";

const MAX_DISPLAY_ROWS = 5000;

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
  const bodyRows = rows.length > 1 ? rows.slice(1, 1 + MAX_DISPLAY_ROWS) : [];

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

      <div className="min-h-0 flex-1 overflow-auto">
        {showRaw ? (
          <pre
            data-testid="file-preview-csv-raw"
            className="selectable p-3 font-mono text-[12px] text-foreground"
          >
            {text}
          </pre>
        ) : (
          <div className="p-3">
            <table
              data-testid="file-preview-csv"
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
              <tbody>
                {bodyRows.map((row, rowIndex) => (
                  <tr
                    key={rowIndex}
                    className="border-b border-border/50 hover:bg-accent/40"
                  >
                    <td className="px-3 py-1.5 text-right font-mono text-muted-foreground select-none">
                      {rowIndex + 1}
                    </td>
                    {row.map((cell, cellIndex) => (
                      <td
                        key={cellIndex}
                        className="px-3 py-1.5 text-foreground whitespace-pre-wrap"
                      >
                        {cell}
                      </td>
                    ))}
                  </tr>
                ))}
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
        )}
      </div>
    </div>
  );
}
