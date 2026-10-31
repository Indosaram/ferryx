import { describe, expect, it } from "vitest";

import { parseNotebook, stripAnsi } from "./ipynbParse";

describe("ipynbParse", () => {
  it("returns null for invalid JSON", () => {
    expect(parseNotebook("")).toBeNull();
    expect(parseNotebook("not a json string")).toBeNull();
    expect(parseNotebook("{")).toBeNull();
    expect(parseNotebook("123")).toBeNull();
  });

  it("returns null for JSON without cells array", () => {
    expect(parseNotebook("{}")).toBeNull();
    expect(parseNotebook('{"metadata": {}}')).toBeNull();
    expect(parseNotebook('{"cells": "not an array"}')).toBeNull();
  });

  it("parses empty cells array and default language python", () => {
    const raw = JSON.stringify({ cells: [] });
    const parsed = parseNotebook(raw);
    expect(parsed).toEqual({
      cells: [],
      language: "python",
    });
  });

  it("extracts language from kernelspec or language_info", () => {
    const rawKernelspec = JSON.stringify({
      cells: [],
      metadata: { kernelspec: { language: "julia" } },
    });
    expect(parseNotebook(rawKernelspec)?.language).toBe("julia");

    const rawLangInfo = JSON.stringify({
      cells: [],
      metadata: { language_info: { name: "r" } },
    });
    expect(parseNotebook(rawLangInfo)?.language).toBe("r");
  });

  it("handles source as string vs array of strings", () => {
    const notebookStr = JSON.stringify({
      cells: [
        {
          cell_type: "markdown",
          source: "# Heading 1\nLine 2",
        },
        {
          cell_type: "code",
          source: ["import os\n", "print(os.getcwd())"],
          execution_count: 1,
          outputs: [],
        },
      ],
    });

    const parsed = parseNotebook(notebookStr);
    expect(parsed).not.toBeNull();
    expect(parsed?.cells).toHaveLength(2);
    expect(parsed?.cells[0].source).toBe("# Heading 1\nLine 2");
    expect(parsed?.cells[1].source).toBe("import os\nprint(os.getcwd())");
  });

  it("handles stream text outputs", () => {
    const notebook = JSON.stringify({
      cells: [
        {
          cell_type: "code",
          source: "print('hello')",
          execution_count: 2,
          outputs: [
            {
              output_type: "stream",
              name: "stdout",
              text: ["hello\n", "world"],
            },
          ],
        },
      ],
    });

    const parsed = parseNotebook(notebook);
    expect(parsed?.cells[0].outputs).toEqual([
      {
        kind: "text",
        text: "hello\nworld",
      },
    ]);
  });

  it("handles image/png and image/jpeg output preferring png, jpeg over text", () => {
    const notebook = JSON.stringify({
      cells: [
        {
          cell_type: "code",
          source: "plot()",
          execution_count: 3,
          outputs: [
            {
              output_type: "display_data",
              data: {
                "text/plain": "<Figure size 640x480 with 1 Axes>",
                "image/png": "iVBORw0KGgoAAAANSUhEUgAAAAUA\nAAAFCAYAAACNbybl",
              },
            },
            {
              output_type: "execute_result",
              data: {
                "text/plain": "<Image>",
                "image/jpeg": "/9j/4AAQSkZJRg==",
              },
            },
          ],
        },
      ],
    });

    const parsed = parseNotebook(notebook);
    expect(parsed?.cells[0].outputs[0]).toEqual({
      kind: "image",
      mime: "image/png",
      base64: "iVBORw0KGgoAAAANSUhEUgAAAAUAAAAFCAYAAACNbybl",
    });
    expect(parsed?.cells[0].outputs[1]).toEqual({
      kind: "image",
      mime: "image/jpeg",
      base64: "/9j/4AAQSkZJRg==",
    });
  });

  it("ignores text/html output and unsupported mime types", () => {
    const notebook = JSON.stringify({
      cells: [
        {
          cell_type: "code",
          source: "df",
          execution_count: 4,
          outputs: [
            {
              output_type: "execute_result",
              data: {
                "text/html": "<table><tr><td>1</td></tr></table>",
                "application/json": { key: "val" },
              },
            },
          ],
        },
      ],
    });

    const parsed = parseNotebook(notebook);
    expect(parsed?.cells[0].outputs).toEqual([]);
  });

  it("handles error output and strips ANSI escapes from traceback and evalue", () => {
    const notebook = JSON.stringify({
      cells: [
        {
          cell_type: "code",
          source: "1 / 0",
          execution_count: 5,
          outputs: [
            {
              output_type: "error",
              ename: "\u001b[0;31mZeroDivisionError\u001b[0m",
              evalue: "division by zero",
              traceback: [
                "\u001b[0;31m---------------------------------------------------------------------------\u001b[0m",
                "\u001b[0;31mZeroDivisionError\u001b[0m: division by zero",
              ],
            },
          ],
        },
      ],
    });

    const parsed = parseNotebook(notebook);
    expect(parsed?.cells[0].outputs[0]).toEqual({
      kind: "error",
      name: "ZeroDivisionError",
      message: "---------------------------------------------------------------------------\nZeroDivisionError: division by zero",
    });
  });

  it("strips ANSI escapes with stripAnsi helper", () => {
    const withAnsi = "\u001b[31mRed\u001b[0m and \u001b[1;32mGreen\u001b[0m";
    expect(stripAnsi(withAnsi)).toBe("Red and Green");
  });

  it("reads nbformat v3 cells from worksheets[0]", () => {
    const notebook = JSON.stringify({
      nbformat: 3,
      worksheets: [{ cells: [{ cell_type: "markdown", source: ["# v3"] }] }],
    });
    const parsed = parseNotebook(notebook);
    expect(parsed?.cells).toHaveLength(1);
    expect(parsed?.cells[0].source).toBe("# v3");
  });
});
