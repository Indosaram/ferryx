import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MarkdownTable, serializeTable, tableGridFromElement, type TableCopyFormat } from "./markdownTable";

afterEach(cleanup);

const TABLE_MARKDOWN = ["| Tool | Uses |", "| --- | --- |", "| grep | 12 |", "| glob | 3 |"].join("\n");

/** The chat's own markdown pipeline, so the table is drawn by the shipped component. */
async function renderTable(markdown = TABLE_MARKDOWN) {
  const { ChatProseMarkdown } = await import("../MobileChatComponents");
  return render(<ChatProseMarkdown text={markdown} />);
}

describe("markdown table", () => {
  it("serializes a rendered table to markdown, csv, and html", async () => {
    await renderTable();
    const table = document.querySelector("table");
    const grid = tableGridFromElement(table);
    expect(grid).not.toBeNull();
    expect(serializeTable(grid!, "markdown")).toBe(
      ["| Tool | Uses |", "| --- | --- |", "| grep | 12 |", "| glob | 3 |"].join("\n"),
    );
    expect(serializeTable(grid!, "csv")).toBe(["Tool,Uses", "grep,12", "glob,3"].join("\n"));
    expect(serializeTable(grid!, "html")).toContain("<th>Tool</th>");
    expect(serializeTable(grid!, "html")).toContain("<td>grep</td>");
  });

  it("escapes a cell that would break the target format", () => {
    const grid = { header: ["A|B"], rows: [[`say "hi", ok`]] };
    expect(serializeTable(grid, "markdown")).toContain("A\\|B");
    expect(serializeTable(grid, "csv")).toContain(`"say ""hi"", ok"`);
    expect(serializeTable(grid, "html")).toContain("&quot;");
  });

  it("copies the table as markdown and offers csv/html from the menu", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    await renderTable();

    fireEvent.click(screen.getByTestId("table-copy-markdown"));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(TABLE_MARKDOWN));

    fireEvent.click(screen.getByTestId("table-copy-more"));
    const formats: TableCopyFormat[] = ["csv", "html"];
    for (const format of formats) {
      fireEvent.click(screen.getByTestId(`table-copy-${format}`));
      await waitFor(() =>
        expect(writeText).toHaveBeenCalledWith(expect.stringContaining(format === "csv" ? "Tool,Uses" : "<table>")),
      );
      if (format !== formats[formats.length - 1]) fireEvent.click(screen.getByTestId("table-copy-more"));
    }
  });

  it("closes the format menu with Escape", async () => {
    await renderTable();
    fireEvent.click(screen.getByTestId("table-copy-more"));
    expect(screen.getByTestId("table-copy-menu")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("table-copy-menu")).not.toBeInTheDocument();
  });

  it("scrolls long tables inside the chat lane rather than the page", () => {
    render(
      <MarkdownTable>
        <tbody>
          <tr>
            <td>only</td>
          </tr>
        </tbody>
      </MarkdownTable>,
    );
    expect(screen.getByTestId("markdown-table").querySelector(".overflow-x-auto")).not.toBeNull();
  });
});
