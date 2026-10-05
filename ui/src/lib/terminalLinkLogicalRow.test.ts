import { describe, expect, it } from "vitest";

import { resolveTokenAtCol } from "./linkRouting";
import { readLinkLine } from "./terminalLinkTarget";

describe("physical-row terminal link receipts", () => {
  it("resolves a file from the clicked physical row and its viewport column", async () => {
    const logicalLine = "prefix: /tmp/some-long-directory/file.ts";
    const cols = 20;
    const rows = [logicalLine.slice(0, cols), logicalLine.slice(cols), ""];
    const clickedRow = 1;
    const clickedCol = 6;
    const invoke = async <T,>(_command: string, args?: Record<string, unknown>): Promise<T> => {
      const row = args?.row as number;
      return {
        text: rows[row] ?? "",
        col: row === clickedRow ? clickedCol : 0,
        row,
      } as T;
    };

    const receipt = await readLinkLine(invoke, "backend-session", clickedCol, clickedRow, rows.length, cols);

    expect(receipt).not.toBeNull();
    expect(resolveTokenAtCol(receipt!.text, receipt!.col)).toMatchObject({
      type: "file",
      path: "/tmp/some-long-directory/file.ts",
    });
  });
});
