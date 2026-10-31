import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { FilePreviewCsv } from "./FilePreviewCsv";

afterEach(() => {
  cleanup();
});

describe("FilePreviewCsv", () => {
  it("opens the complete multiline value from a keyboard-focusable cell", () => {
    const show = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, "showModal");
    const close = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, "close");
    Object.defineProperties(HTMLDialogElement.prototype, {
      showModal: { configurable: true, value(this: HTMLDialogElement) { this.open = true; } },
      close: { configurable: true, value(this: HTMLDialogElement) { this.open = false; this.dispatchEvent(new Event("close")); } },
    });
    try {
    render(<FilePreviewCsv text={'value\n"first\nlast"'} displayName="multiline.csv" />);
    const cell = screen.getByRole("button", { name: "View row 1, column 1" });
    cell.focus();
    expect(cell).toHaveFocus();
    fireEvent.click(cell);
    const dialog = screen.getByRole("dialog", { name: "Cell contents" });
    expect(dialog.querySelector("pre")?.textContent).toBe("first\nlast");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    } finally {
      if (show) Object.defineProperty(HTMLDialogElement.prototype, "showModal", show);
      else Reflect.deleteProperty(HTMLDialogElement.prototype, "showModal");
      if (close) Object.defineProperty(HTMLDialogElement.prototype, "close", close);
      else Reflect.deleteProperty(HTMLDialogElement.prototype, "close");
    }
  });

  it("renders header row as th elements and body rows with row numbers", () => {
    const csv = "Name,Age,Role\nAlice,30,Engineer\nBob,25,Designer";
    render(<FilePreviewCsv text={csv} displayName="users.csv" />);

    const table = screen.getByTestId("file-preview-csv");
    expect(table).toBeDefined();

    const headers = table.querySelectorAll("th");
    expect(headers).toHaveLength(4);
    expect(headers[0].textContent).toBe("#");
    expect(headers[1].textContent).toBe("Name");
    expect(headers[2].textContent).toBe("Age");
    expect(headers[3].textContent).toBe("Role");

    const rows = table.querySelectorAll("tbody tr");
    expect(rows).toHaveLength(2);

    const firstRowCells = rows[0].querySelectorAll("td");
    expect(firstRowCells[0].textContent).toBe("1");
    expect(firstRowCells[1].textContent).toBe("Alice");
    expect(firstRowCells[2].textContent).toBe("30");
    expect(firstRowCells[3].textContent).toBe("Engineer");

    const secondRowCells = rows[1].querySelectorAll("td");
    expect(secondRowCells[0].textContent).toBe("2");
    expect(secondRowCells[1].textContent).toBe("Bob");
    expect(secondRowCells[2].textContent).toBe("25");
    expect(secondRowCells[3].textContent).toBe("Designer");
  });

  it("handles TSV format by detecting .tsv extension or delimiter", () => {
    const tsv = "Header1\tHeader2\nValue1\tValue2";
    render(<FilePreviewCsv text={tsv} displayName="report.tsv" />);

    const table = screen.getByTestId("file-preview-csv");
    const headers = table.querySelectorAll("th");
    expect(headers[1].textContent).toBe("Header1");
    expect(headers[2].textContent).toBe("Header2");

    const cells = table.querySelectorAll("tbody td");
    expect(cells[0].textContent).toBe("1");
    expect(cells[1].textContent).toBe("Value1");
    expect(cells[2].textContent).toBe("Value2");
  });

  it("handles quoted commas and embedded newlines in table cells", () => {
    const csv = 'Title,Desc\nBook,"A, B, and C"\nMulti,"Line 1\nLine 2"';
    render(<FilePreviewCsv text={csv} displayName="books.csv" />);

    const rows = screen.getByTestId("file-preview-csv").querySelectorAll("tbody tr");
    expect(rows).toHaveLength(2);

    const row1 = rows[0].querySelectorAll("td");
    expect(row1[1].textContent).toBe("Book");
    expect(row1[2].textContent).toBe("A, B, and C");

    const row2 = rows[1].querySelectorAll("td");
    expect(row2[1].textContent).toBe("Multi");
    expect(row2[2].textContent).toBe("Line 1\nLine 2");
  });

  it("displays truncation footer when body row count exceeds 5000", () => {
    const lines = ["id,value"];
    for (let i = 1; i <= 5001; i++) {
      lines.push(`${i},val${i}`);
    }
    const csv = lines.join("\n");

    render(<FilePreviewCsv text={csv} displayName="big.csv" />);

    const truncationMessage = screen.getByTestId("file-preview-csv-truncated");
    expect(truncationMessage).toBeDefined();
    expect(truncationMessage.textContent).toBe("Showing 5000 of 5001 rows");

    // The 5000 displayed rows are windowed: only the visible slice is mounted.
    const renderedRows = screen.getByTestId("file-preview-csv").querySelectorAll("tbody tr[data-line-window-row]");
    expect(renderedRows.length).toBeGreaterThan(0);
    expect(renderedRows.length).toBeLessThan(200);
  });

  it("toggles between table view and raw text view", () => {
    const csv = "col1,col2\nval1,val2";
    render(<FilePreviewCsv text={csv} displayName="test.csv" />);

    expect(screen.getByTestId("file-preview-csv")).toBeDefined();
    expect(screen.queryByTestId("file-preview-csv-raw")).toBeNull();

    const toggleBtn = screen.getByTestId("file-preview-csv-raw-toggle");
    expect(toggleBtn.textContent).toBe("Raw");

    fireEvent.click(toggleBtn);

    expect(screen.queryByTestId("file-preview-csv")).toBeNull();
    const rawView = screen.getByTestId("file-preview-csv-raw");
    expect(rawView).toBeDefined();
    expect(rawView.textContent).toBe(csv);
    expect(toggleBtn.textContent).toBe("Table");

    fireEvent.click(toggleBtn);

    expect(screen.getByTestId("file-preview-csv")).toBeDefined();
    expect(screen.queryByTestId("file-preview-csv-raw")).toBeNull();
    expect(toggleBtn.textContent).toBe("Raw");
  });

  it("keeps both the table and raw views text-selectable", () => {
    render(<FilePreviewCsv text={"a,b\n1,2"} displayName="t.csv" />);
    expect(screen.getByTestId("file-preview-csv")).toHaveClass("selectable");
    fireEvent.click(screen.getByTestId("file-preview-csv-raw-toggle"));
    expect(screen.getByTestId("file-preview-csv-raw")).toHaveClass("selectable");
  });

  it("renders a bounded number of rows for a 5,000-row CSV and keeps the header", () => {
    const csv = ["id,value", ...Array.from({ length: 5000 }, (_, i) => `${i + 1},val${i + 1}`)].join("\n");
    render(<FilePreviewCsv text={csv} displayName="big.csv" />);

    const table = screen.getByTestId("file-preview-csv");
    expect(table).toHaveAttribute("data-windowed", "true");
    expect(table.querySelectorAll("thead th")).toHaveLength(3);
    expect(table.querySelectorAll("tr").length).toBeLessThan(200);
    const rows = table.querySelectorAll("tbody tr[data-line-window-row]");
    expect(rows[0].querySelectorAll("td")[0].textContent).toBe("1");
    expect(screen.queryByTestId("file-preview-csv-spacer-top")).toBeNull();
    const bottom = screen.getByTestId("file-preview-csv-spacer-bottom");
    expect(parseFloat(bottom.style.height)).toBeGreaterThan(0);
  });

  it("moves the table window when the table scrolls", () => {
    const csv = ["id,value", ...Array.from({ length: 5000 }, (_, i) => `${i + 1},val${i + 1}`)].join("\n");
    render(<FilePreviewCsv text={csv} displayName="big.csv" />);
    const table = screen.getByTestId("file-preview-csv");
    const scroller = table.parentElement!.parentElement!;

    scroller.scrollTop = 4000 * 33;
    fireEvent.scroll(scroller);

    const rows = table.querySelectorAll("tbody tr[data-line-window-row]");
    expect(rows.length).toBeLessThan(200);
    const numbers = Array.from(rows, (row) => Number(row.querySelector("td")!.textContent));
    expect(numbers).toContain(4001);
    expect(numbers).not.toContain(1);
    expect(screen.getByTestId("file-preview-csv-spacer-top")).toBeInTheDocument();
  });

  it("windows the raw view of a large CSV instead of one huge pre", () => {
    const csv = ["id,value", ...Array.from({ length: 5000 }, (_, i) => `${i + 1},val${i + 1}`)].join("\n");
    render(<FilePreviewCsv text={csv} displayName="big.csv" />);
    fireEvent.click(screen.getByTestId("file-preview-csv-raw-toggle"));

    const raw = screen.getByTestId("file-preview-csv-raw");
    expect(raw).toHaveAttribute("data-windowed", "true");
    expect(raw).toHaveClass("selectable");
    const lines = raw.querySelectorAll("[data-line-window-row]");
    expect(lines.length).toBeGreaterThan(0);
    expect(lines.length).toBeLessThan(200);
    expect(screen.getByTestId("file-preview-csv-raw-line-1")).toHaveTextContent("id,value");
    expect(raw.textContent!.length).toBeLessThan(csv.length / 10);

    const scroller = raw.parentElement!;
    scroller.scrollTop = 4500 * 20;
    fireEvent.scroll(scroller);
    expect(screen.getByTestId("file-preview-csv-raw-line-4501")).toHaveTextContent("4500,val4500");
    expect(screen.queryByTestId("file-preview-csv-raw-line-1")).toBeNull();
  });
});
